//! Integration Scheduler
//!
//! Manages periodic syncing of integrations.

use super::*;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages scheduled syncing of integrations
pub struct IntegrationScheduler {
    integrations: Arc<RwLock<HashMap<String, Box<dyn Integration>>>>,
    schedules: Arc<RwLock<HashMap<String, ScheduleConfig>>>,
    running: Arc<RwLock<bool>>,
}

/// Configuration for integration scheduling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    pub enabled: bool,
    pub interval_hours: u64,
    pub last_sync: Option<DateTime<Utc>>,
    pub last_status: Option<SyncStatus>,
    pub next_sync: Option<DateTime<Utc>>,
    pub error_count: u32,
}

/// Status of the last sync attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncStatus {
    Success { points_synced: usize },
    Failed { error: String },
    RateLimited { retry_after: u64 },
}

/// Current status of an integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub name: String,
    pub description: String,
    pub authenticated: bool,
    pub enabled: bool,
    pub last_sync: Option<DateTime<Utc>>,
    pub last_status: Option<SyncStatus>,
    pub next_sync: Option<DateTime<Utc>>,
    pub error_count: u32,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 6,
            last_sync: None,
            last_status: None,
            next_sync: None,
            error_count: 0,
        }
    }
}

impl IntegrationScheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        Self {
            integrations: Arc::new(RwLock::new(HashMap::new())),
            schedules: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Register an integration with a schedule
    pub async fn register(
        &self,
        integration: Box<dyn Integration>,
        schedule: ScheduleConfig,
    ) {
        let name = integration.name().to_string();

        // Calculate next sync time
        let mut schedule = schedule;
        if schedule.enabled && schedule.next_sync.is_none() {
            schedule.next_sync = Some(Utc::now());
        }

        self.integrations.write().await.insert(name.clone(), integration);
        self.schedules.write().await.insert(name, schedule);
    }

    /// Get the status of all integrations
    pub async fn get_status(&self) -> Vec<IntegrationStatus> {
        let integrations = self.integrations.read().await;
        let schedules = self.schedules.read().await;
        let mut status = Vec::new();

        for (name, integration) in integrations.iter() {
            let schedule = schedules.get(name);

            status.push(IntegrationStatus {
                name: name.clone(),
                description: integration.description().to_string(),
                authenticated: integration.is_authenticated(),
                enabled: schedule.map(|s| s.enabled).unwrap_or(false),
                last_sync: schedule.and_then(|s| s.last_sync),
                last_status: schedule.and_then(|s| s.last_status.clone()),
                next_sync: schedule.and_then(|s| s.next_sync),
                error_count: schedule.map(|s| s.error_count).unwrap_or(0),
            });
        }

        status
    }

    /// Get status of a specific integration
    pub async fn get_integration_status(&self, name: &str) -> Option<IntegrationStatus> {
        let integrations = self.integrations.read().await;
        let schedules = self.schedules.read().await;

        integrations.get(name).map(|integration| {
            let schedule = schedules.get(name);

            IntegrationStatus {
                name: name.to_string(),
                description: integration.description().to_string(),
                authenticated: integration.is_authenticated(),
                enabled: schedule.map(|s| s.enabled).unwrap_or(false),
                last_sync: schedule.and_then(|s| s.last_sync),
                last_status: schedule.and_then(|s| s.last_status.clone()),
                next_sync: schedule.and_then(|s| s.next_sync),
                error_count: schedule.map(|s| s.error_count).unwrap_or(0),
            }
        })
    }

    /// Manually trigger a sync for an integration
    pub async fn trigger_sync(&self, name: &str) -> Result<SyncResult, IntegrationError> {
        let integrations = self.integrations.read().await;
        let integration = integrations
            .get(name)
            .ok_or_else(|| IntegrationError::ApiError(format!("Integration {} not found", name)))?;

        if !integration.is_authenticated() {
            return Err(IntegrationError::NotAuthenticated);
        }

        let since = {
            let schedules = self.schedules.read().await;
            schedules.get(name).and_then(|s| s.last_sync)
        };

        let result = integration.sync(since).await;

        // Update schedule
        {
            let mut schedules = self.schedules.write().await;
            if let Some(schedule) = schedules.get_mut(name) {
                schedule.last_sync = Some(Utc::now());
                match &result {
                    Ok(r) => {
                        schedule.last_status = Some(SyncStatus::Success {
                            points_synced: r.points.len(),
                        });
                        schedule.error_count = 0;
                        schedule.next_sync =
                            Some(Utc::now() + Duration::hours(schedule.interval_hours as i64));
                    }
                    Err(IntegrationError::RateLimited(secs)) => {
                        schedule.last_status = Some(SyncStatus::RateLimited {
                            retry_after: *secs,
                        });
                        schedule.next_sync = Some(Utc::now() + Duration::seconds(*secs as i64));
                    }
                    Err(e) => {
                        schedule.last_status = Some(SyncStatus::Failed {
                            error: e.to_string(),
                        });
                        schedule.error_count += 1;
                        // Exponential backoff on errors
                        let backoff = std::cmp::min(schedule.error_count as i64 * 15, 60);
                        schedule.next_sync = Some(Utc::now() + Duration::minutes(backoff));
                    }
                }
            }
        }

        result
    }

    /// Start the scheduler background task
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let scheduler = self.clone();

        tokio::spawn(async move {
            *scheduler.running.write().await = true;

            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));

            loop {
                interval.tick().await;

                if !*scheduler.running.read().await {
                    break;
                }

                scheduler.check_and_run_syncs().await;
            }
        })
    }

    /// Stop the scheduler
    pub async fn stop(&self) {
        *self.running.write().await = false;
    }

    /// Check all integrations and run syncs that are due
    async fn check_and_run_syncs(&self) {
        let now = Utc::now();
        let due_integrations: Vec<String> = {
            let schedules = self.schedules.read().await;
            schedules
                .iter()
                .filter(|(_, schedule)| {
                    schedule.enabled
                        && schedule
                            .next_sync
                            .map(|next| now >= next)
                            .unwrap_or(true)
                })
                .map(|(name, _)| name.clone())
                .collect()
        };

        for name in due_integrations {
            tracing::info!("Running scheduled sync for {}", name);

            match self.trigger_sync(&name).await {
                Ok(result) => {
                    tracing::info!(
                        "Integration {} synced {} points",
                        name,
                        result.points.len()
                    );
                }
                Err(e) => {
                    tracing::error!("Integration {} sync failed: {}", name, e);
                }
            }
        }
    }

    /// Enable or disable an integration
    pub async fn set_enabled(&self, name: &str, enabled: bool) {
        let mut schedules = self.schedules.write().await;
        if let Some(schedule) = schedules.get_mut(name) {
            schedule.enabled = enabled;
            if enabled && schedule.next_sync.is_none() {
                schedule.next_sync = Some(Utc::now());
            }
        }
    }

    /// Update the sync interval for an integration
    pub async fn set_interval(&self, name: &str, interval_hours: u64) {
        let mut schedules = self.schedules.write().await;
        if let Some(schedule) = schedules.get_mut(name) {
            schedule.interval_hours = interval_hours;
        }
    }
}

impl Default for IntegrationScheduler {
    fn default() -> Self {
        Self::new()
    }
}
