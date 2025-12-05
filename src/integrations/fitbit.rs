//! Fitbit Integration
//!
//! OAuth 2.0 integration with Fitbit API for:
//! - Activity data (steps, calories)
//! - Sleep data
//! - Heart rate data

use super::*;
use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

/// Fitbit API integration
pub struct FitbitIntegration {
    client: Client,
    config: FitbitConfig,
    tokens: RwLock<Option<FitbitTokens>>,
}

/// Configuration for Fitbit integration
#[derive(Debug, Clone)]
pub struct FitbitConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// OAuth tokens for Fitbit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitbitTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
}

impl FitbitIntegration {
    /// Create a new Fitbit integration
    pub fn new(config: FitbitConfig) -> Self {
        Self {
            client: Client::new(),
            config,
            tokens: RwLock::new(None),
        }
    }

    /// Generate OAuth authorization URL
    pub fn oauth_url(&self, state: &str) -> String {
        format!(
            "https://www.fitbit.com/oauth2/authorize?\
             response_type=code&\
             client_id={}&\
             redirect_uri={}&\
             scope=activity+heartrate+sleep+weight&\
             state={}",
            self.config.client_id,
            urlencoding::encode(&self.config.redirect_uri),
            state
        )
    }

    /// Set tokens (for restoring from storage)
    pub fn set_tokens(&self, tokens: FitbitTokens) {
        *self.tokens.write().unwrap() = Some(tokens);
    }

    /// Get current tokens
    pub fn get_tokens(&self) -> Option<FitbitTokens> {
        self.tokens.read().unwrap().clone()
    }

    /// Refresh token if needed
    async fn refresh_token_if_needed(&self) -> Result<(), IntegrationError> {
        let needs_refresh = {
            let tokens = self.tokens.read().unwrap();
            match tokens.as_ref() {
                Some(t) => t.expires_at <= Utc::now() + Duration::minutes(5),
                None => return Err(IntegrationError::NotAuthenticated),
            }
        };

        if !needs_refresh {
            return Ok(());
        }

        let refresh_token = {
            let tokens = self.tokens.read().unwrap();
            tokens.as_ref().unwrap().refresh_token.clone()
        };

        let response = self
            .client
            .post("https://api.fitbit.com/oauth2/token")
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_token),
            ])
            .send()
            .await
            .map_err(|e| IntegrationError::ApiError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(IntegrationError::AuthFailed("Token refresh failed".into()));
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
        }

        let token_resp: TokenResponse = response
            .json()
            .await
            .map_err(|e| IntegrationError::ParseError(e.to_string()))?;

        *self.tokens.write().unwrap() = Some(FitbitTokens {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            expires_at: Utc::now() + Duration::seconds(token_resp.expires_in),
        });

        Ok(())
    }

    /// Fetch activity data for a specific date
    async fn fetch_activities(&self, date: &str) -> Result<Vec<DataPoint>, IntegrationError> {
        self.refresh_token_if_needed().await?;

        let access_token = {
            let tokens = self.tokens.read().unwrap();
            tokens.as_ref().unwrap().access_token.clone()
        };

        let response = self
            .client
            .get(&format!(
                "https://api.fitbit.com/1/user/-/activities/date/{}.json",
                date
            ))
            .bearer_auth(&access_token)
            .send()
            .await
            .map_err(|e| IntegrationError::ApiError(e.to_string()))?;

        if response.status() == 429 {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600);
            return Err(IntegrationError::RateLimited(retry_after));
        }

        if !response.status().is_success() {
            return Err(IntegrationError::ApiError(format!(
                "API returned {}",
                response.status()
            )));
        }

        #[derive(Deserialize)]
        struct ActivitiesResponse {
            summary: ActivitySummary,
        }

        #[derive(Deserialize)]
        struct ActivitySummary {
            steps: u64,
            #[serde(rename = "caloriesOut")]
            calories_out: u64,
            #[serde(rename = "veryActiveMinutes")]
            very_active_minutes: Option<u64>,
            #[serde(rename = "fairlyActiveMinutes")]
            fairly_active_minutes: Option<u64>,
        }

        let data: ActivitiesResponse = response
            .json()
            .await
            .map_err(|e| IntegrationError::ParseError(e.to_string()))?;

        let date_ts = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|e| IntegrationError::ParseError(e.to_string()))?
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();

        let mut points = vec![
            DataPoint::new(0, data.summary.steps as f64).timestamp(date_ts),
            DataPoint::new(0, data.summary.calories_out as f64).timestamp(date_ts),
        ];

        // Calculate total active minutes
        let active_minutes = data.summary.very_active_minutes.unwrap_or(0)
            + data.summary.fairly_active_minutes.unwrap_or(0);
        points.push(DataPoint::new(0, active_minutes as f64).timestamp(date_ts));

        Ok(points)
    }

    /// Fetch sleep data for a specific date
    async fn fetch_sleep(&self, date: &str) -> Result<Option<DataPoint>, IntegrationError> {
        self.refresh_token_if_needed().await?;

        let access_token = {
            let tokens = self.tokens.read().unwrap();
            tokens.as_ref().unwrap().access_token.clone()
        };

        let response = self
            .client
            .get(&format!(
                "https://api.fitbit.com/1.2/user/-/sleep/date/{}.json",
                date
            ))
            .bearer_auth(&access_token)
            .send()
            .await
            .map_err(|e| IntegrationError::ApiError(e.to_string()))?;

        if response.status() == 429 {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600);
            return Err(IntegrationError::RateLimited(retry_after));
        }

        #[derive(Deserialize)]
        struct SleepResponse {
            summary: SleepSummary,
        }

        #[derive(Deserialize)]
        struct SleepSummary {
            #[serde(rename = "totalMinutesAsleep")]
            total_minutes_asleep: u64,
        }

        let data: SleepResponse = response
            .json()
            .await
            .map_err(|e| IntegrationError::ParseError(e.to_string()))?;

        if data.summary.total_minutes_asleep == 0 {
            return Ok(None);
        }

        let date_ts = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|e| IntegrationError::ParseError(e.to_string()))?
            .and_hms_opt(6, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();

        let sleep_hours = data.summary.total_minutes_asleep as f64 / 60.0;
        Ok(Some(
            DataPoint::new(0, sleep_hours).timestamp(date_ts),
        ))
    }
}

#[async_trait]
impl Integration for FitbitIntegration {
    fn name(&self) -> &str {
        "fitbit"
    }

    fn description(&self) -> &str {
        "Fitbit activity, sleep, and heart rate data"
    }

    fn metrics_provided(&self) -> Vec<MetricDefinition> {
        vec![
            MetricDefinition {
                name: "steps".into(),
                unit: "steps".into(),
                category: "health".into(),
                aggregation: "sum".into(),
            },
            MetricDefinition {
                name: "calories_burned".into(),
                unit: "kcal".into(),
                category: "health".into(),
                aggregation: "sum".into(),
            },
            MetricDefinition {
                name: "active_minutes".into(),
                unit: "minutes".into(),
                category: "health".into(),
                aggregation: "sum".into(),
            },
            MetricDefinition {
                name: "sleep_hours".into(),
                unit: "hours".into(),
                category: "health".into(),
                aggregation: "average".into(),
            },
        ]
    }

    fn is_authenticated(&self) -> bool {
        self.tokens.read().unwrap().is_some()
    }

    async fn authenticate(&mut self, credentials: AuthCredentials) -> Result<(), IntegrationError> {
        let code = match credentials {
            AuthCredentials::OAuth { code, .. } => code,
            _ => return Err(IntegrationError::AuthFailed("OAuth required for Fitbit".into())),
        };

        let response = self
            .client
            .post("https://api.fitbit.com/oauth2/token")
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", &self.config.redirect_uri),
            ])
            .send()
            .await
            .map_err(|e| IntegrationError::ApiError(e.to_string()))?;

        if !response.status().is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(IntegrationError::AuthFailed(text));
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
        }

        let token_resp: TokenResponse = response
            .json()
            .await
            .map_err(|e| IntegrationError::ParseError(e.to_string()))?;

        *self.tokens.write().unwrap() = Some(FitbitTokens {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            expires_at: Utc::now() + Duration::seconds(token_resp.expires_in),
        });

        Ok(())
    }

    async fn sync(&self, since: Option<DateTime<Utc>>) -> Result<SyncResult, IntegrationError> {
        if !self.is_authenticated() {
            return Err(IntegrationError::NotAuthenticated);
        }

        let start_date = since.unwrap_or_else(|| Utc::now() - Duration::days(30));
        let end_date = Utc::now();

        let mut all_points = Vec::new();
        let mut current = start_date.date_naive();
        let end = end_date.date_naive();

        while current <= end {
            let date_str = current.format("%Y-%m-%d").to_string();

            // Fetch activities
            match self.fetch_activities(&date_str).await {
                Ok(points) => all_points.extend(points),
                Err(IntegrationError::RateLimited(secs)) => {
                    tracing::warn!("Rate limited, stopping sync. Retry after {} seconds", secs);
                    break;
                }
                Err(e) => tracing::warn!("Failed to fetch activities for {}: {}", date_str, e),
            }

            // Fetch sleep
            match self.fetch_sleep(&date_str).await {
                Ok(Some(point)) => all_points.push(point),
                Ok(None) => {}
                Err(IntegrationError::RateLimited(secs)) => {
                    tracing::warn!("Rate limited, stopping sync. Retry after {} seconds", secs);
                    break;
                }
                Err(e) => tracing::warn!("Failed to fetch sleep for {}: {}", date_str, e),
            }

            current += Duration::days(1);
        }

        Ok(SyncResult {
            points: all_points,
            metrics_synced: vec![
                "steps".into(),
                "calories_burned".into(),
                "active_minutes".into(),
                "sleep_hours".into(),
            ],
            earliest: Some(start_date),
            latest: Some(end_date),
        })
    }
}
