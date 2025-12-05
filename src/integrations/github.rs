//! GitHub Integration
//!
//! Fetches developer productivity metrics from GitHub:
//! - Commits per day
//! - PRs opened/merged
//! - Issues closed

use super::*;
use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

/// GitHub API integration
pub struct GitHubIntegration {
    client: Client,
    config: GitHubConfig,
}

/// Configuration for GitHub integration
#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub token: String,
    pub username: String,
}

impl GitHubIntegration {
    /// Create a new GitHub integration
    pub fn new(config: GitHubConfig) -> Self {
        Self {
            client: Client::builder()
                .user_agent("Chronicle/0.1")
                .build()
                .unwrap(),
            config,
        }
    }

    /// Fetch events for the authenticated user
    async fn fetch_events(&self, page: u32) -> Result<Vec<GitHubEvent>, IntegrationError> {
        let response = self
            .client
            .get(&format!(
                "https://api.github.com/users/{}/events?page={}&per_page=100",
                self.config.username, page
            ))
            .bearer_auth(&self.config.token)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
            .map_err(|e| IntegrationError::ApiError(e.to_string()))?;

        if response.status() == 403 {
            // Rate limited
            let reset = response
                .headers()
                .get("X-RateLimit-Reset")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok())
                .map(|ts| ts - Utc::now().timestamp())
                .unwrap_or(3600) as u64;
            return Err(IntegrationError::RateLimited(reset));
        }

        if response.status() == 401 {
            return Err(IntegrationError::AuthFailed("Invalid GitHub token".into()));
        }

        if !response.status().is_success() {
            return Err(IntegrationError::ApiError(format!(
                "GitHub API returned {}",
                response.status()
            )));
        }

        let events: Vec<GitHubEvent> = response
            .json()
            .await
            .map_err(|e| IntegrationError::ParseError(e.to_string()))?;

        Ok(events)
    }

    /// Aggregate events by day and type
    fn aggregate_events(
        &self,
        events: &[GitHubEvent],
        since: DateTime<Utc>,
    ) -> HashMap<NaiveDate, DailyStats> {
        let mut daily: HashMap<NaiveDate, DailyStats> = HashMap::new();

        for event in events {
            let event_time = match DateTime::parse_from_rfc3339(&event.created_at) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => continue,
            };

            if event_time < since {
                continue;
            }

            let date = event_time.date_naive();
            let stats = daily.entry(date).or_insert_with(DailyStats::default);

            match event.event_type.as_str() {
                "PushEvent" => {
                    // Count commits in the push
                    if let Some(payload) = &event.payload {
                        if let Some(commits) = &payload.commits {
                            stats.commits += commits.len() as u32;
                        }
                    }
                }
                "PullRequestEvent" => {
                    if let Some(payload) = &event.payload {
                        if let Some(action) = &payload.action {
                            match action.as_str() {
                                "opened" => stats.prs_opened += 1,
                                "closed" => {
                                    if payload.pull_request.as_ref().map(|pr| pr.merged).unwrap_or(false) {
                                        stats.prs_merged += 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "IssuesEvent" => {
                    if let Some(payload) = &event.payload {
                        if payload.action.as_deref() == Some("closed") {
                            stats.issues_closed += 1;
                        }
                    }
                }
                "PullRequestReviewEvent" => {
                    stats.reviews += 1;
                }
                _ => {}
            }
        }

        daily
    }
}

#[derive(Debug, Deserialize)]
struct GitHubEvent {
    #[serde(rename = "type")]
    event_type: String,
    created_at: String,
    payload: Option<EventPayload>,
}

#[derive(Debug, Deserialize)]
struct EventPayload {
    action: Option<String>,
    commits: Option<Vec<Commit>>,
    pull_request: Option<PullRequest>,
}

#[derive(Debug, Deserialize)]
struct Commit {
    #[allow(dead_code)]
    sha: String,
}

#[derive(Debug, Deserialize)]
struct PullRequest {
    #[serde(default)]
    merged: bool,
}

#[derive(Debug, Default)]
struct DailyStats {
    commits: u32,
    prs_opened: u32,
    prs_merged: u32,
    issues_closed: u32,
    reviews: u32,
}

#[async_trait]
impl Integration for GitHubIntegration {
    fn name(&self) -> &str {
        "github"
    }

    fn description(&self) -> &str {
        "GitHub commits, PRs, and issues"
    }

    fn metrics_provided(&self) -> Vec<MetricDefinition> {
        vec![
            MetricDefinition {
                name: "commits".into(),
                unit: "count".into(),
                category: "productivity".into(),
                aggregation: "sum".into(),
            },
            MetricDefinition {
                name: "prs_opened".into(),
                unit: "count".into(),
                category: "productivity".into(),
                aggregation: "sum".into(),
            },
            MetricDefinition {
                name: "prs_merged".into(),
                unit: "count".into(),
                category: "productivity".into(),
                aggregation: "sum".into(),
            },
            MetricDefinition {
                name: "issues_closed".into(),
                unit: "count".into(),
                category: "productivity".into(),
                aggregation: "sum".into(),
            },
            MetricDefinition {
                name: "code_reviews".into(),
                unit: "count".into(),
                category: "productivity".into(),
                aggregation: "sum".into(),
            },
        ]
    }

    fn is_authenticated(&self) -> bool {
        !self.config.token.is_empty()
    }

    async fn authenticate(&mut self, credentials: AuthCredentials) -> Result<(), IntegrationError> {
        match credentials {
            AuthCredentials::Token { token } => {
                // Validate the token by making a test request
                let response = self
                    .client
                    .get("https://api.github.com/user")
                    .bearer_auth(&token)
                    .header("Accept", "application/vnd.github.v3+json")
                    .send()
                    .await
                    .map_err(|e| IntegrationError::ApiError(e.to_string()))?;

                if !response.status().is_success() {
                    return Err(IntegrationError::AuthFailed("Invalid GitHub token".into()));
                }

                #[derive(Deserialize)]
                struct User {
                    login: String,
                }

                let user: User = response
                    .json()
                    .await
                    .map_err(|e| IntegrationError::ParseError(e.to_string()))?;

                self.config.token = token;
                self.config.username = user.login;

                Ok(())
            }
            _ => Err(IntegrationError::AuthFailed(
                "Token required for GitHub".into(),
            )),
        }
    }

    async fn sync(&self, since: Option<DateTime<Utc>>) -> Result<SyncResult, IntegrationError> {
        if !self.is_authenticated() {
            return Err(IntegrationError::NotAuthenticated);
        }

        let since_date = since.unwrap_or_else(|| Utc::now() - Duration::days(30));

        // Fetch up to 3 pages of events (300 events max)
        let mut all_events = Vec::new();
        for page in 1..=3 {
            match self.fetch_events(page).await {
                Ok(events) => {
                    if events.is_empty() {
                        break;
                    }
                    all_events.extend(events);
                }
                Err(IntegrationError::RateLimited(secs)) => {
                    tracing::warn!("GitHub rate limited, stopping. Retry after {} seconds", secs);
                    break;
                }
                Err(e) => {
                    tracing::error!("Failed to fetch GitHub events page {}: {}", page, e);
                    break;
                }
            }
        }

        // Aggregate by day
        let daily_stats = self.aggregate_events(&all_events, since_date);

        // Convert to DataPoints
        let mut points = Vec::new();
        for (date, stats) in daily_stats {
            let ts = date.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp_millis();

            if stats.commits > 0 {
                points.push(DataPoint::new(0, stats.commits as f64).timestamp(ts));
            }
            if stats.prs_opened > 0 {
                points.push(DataPoint::new(0, stats.prs_opened as f64).timestamp(ts));
            }
            if stats.prs_merged > 0 {
                points.push(DataPoint::new(0, stats.prs_merged as f64).timestamp(ts));
            }
            if stats.issues_closed > 0 {
                points.push(DataPoint::new(0, stats.issues_closed as f64).timestamp(ts));
            }
            if stats.reviews > 0 {
                points.push(DataPoint::new(0, stats.reviews as f64).timestamp(ts));
            }
        }

        Ok(SyncResult {
            points,
            metrics_synced: vec![
                "commits".into(),
                "prs_opened".into(),
                "prs_merged".into(),
                "issues_closed".into(),
                "code_reviews".into(),
            ],
            earliest: Some(since_date),
            latest: Some(Utc::now()),
        })
    }
}
