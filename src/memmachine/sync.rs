//! Sync Manager
//!
//! Manages periodic synchronization of Chronicle data to MemMachine.
//! Generates daily summaries and sends them as episodic memories.

use crate::memmachine::client::{MemMachineClient, MemMachineError};
use crate::query::{AggregationFunc, Query, QueryExecutor};
use crate::storage::{StorageEngine, TimeRange};
use chrono::{TimeZone, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages periodic sync to MemMachine
pub struct SyncManager {
    client: Arc<MemMachineClient>,
    storage: Arc<StorageEngine>,
    executor: Arc<QueryExecutor>,
    state: Arc<RwLock<SyncState>>,
    config: SyncConfig,
}

/// Configuration for sync behavior
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// How often to sync (in hours)
    pub sync_interval_hours: u64,
    /// Maximum items per sync batch
    pub batch_size: usize,
    /// Whether sync is enabled
    pub enabled: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            sync_interval_hours: 1,
            batch_size: 100,
            enabled: true,
        }
    }
}

/// Current state of the sync manager
#[derive(Debug, Clone, Default)]
pub struct SyncState {
    /// Timestamp of last successful sync
    pub last_sync_timestamp: i64,
    /// Whether there's data waiting to be synced
    pub pending_sync: bool,
    /// Status of the last sync attempt
    pub last_sync_status: Option<SyncStatus>,
}

/// Status of a sync operation
#[derive(Debug, Clone)]
pub struct SyncStatus {
    /// When the sync completed
    pub timestamp: i64,
    /// Number of items synced
    pub items_synced: u32,
    /// How long the sync took
    pub duration_ms: u64,
    /// Whether it succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new(
        client: Arc<MemMachineClient>,
        storage: Arc<StorageEngine>,
        executor: Arc<QueryExecutor>,
        config: SyncConfig,
    ) -> Self {
        Self {
            client,
            storage,
            executor,
            state: Arc::new(RwLock::new(SyncState::default())),
            config,
        }
    }

    /// Start background sync task
    ///
    /// Spawns a tokio task that runs sync on the configured interval.
    pub fn start_background_sync(self: Arc<Self>) {
        if !self.config.enabled {
            tracing::info!("MemMachine sync disabled");
            return;
        }

        tracing::info!(
            interval_hours = self.config.sync_interval_hours,
            "Starting MemMachine background sync"
        );

        tokio::spawn(async move {
            let interval =
                std::time::Duration::from_secs(self.config.sync_interval_hours * 3600);
            let mut ticker = tokio::time::interval(interval);

            // Skip the first immediate tick
            ticker.tick().await;

            loop {
                ticker.tick().await;

                tracing::debug!("Running scheduled MemMachine sync");
                match self.sync().await {
                    Ok(status) => {
                        tracing::info!(
                            items = status.items_synced,
                            duration_ms = status.duration_ms,
                            "MemMachine sync completed"
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "MemMachine sync failed");
                    }
                }
            }
        });
    }

    /// Perform sync to MemMachine
    ///
    /// Generates daily summaries for all data since the last sync
    /// and sends them to MemMachine as episodic memories.
    pub async fn sync(&self) -> Result<SyncStatus, MemMachineError> {
        let start = std::time::Instant::now();

        // Check if MemMachine is available
        if let Err(e) = self.client.health_check().await {
            tracing::warn!(error = %e, "MemMachine unavailable, skipping sync");
            return Err(e);
        }

        let state = self.state.read().await;
        let since = if state.last_sync_timestamp > 0 {
            state.last_sync_timestamp
        } else {
            // First sync: last 7 days
            Utc::now().timestamp_millis() - (7 * 24 * 3600 * 1000)
        };
        drop(state);

        // Generate daily summaries
        let summaries = self.generate_daily_summaries(since).await;

        // Send each summary to MemMachine
        let mut items_synced = 0;
        let session_id = format!("sync-{}", Utc::now().format("%Y%m%d-%H%M%S"));

        for summary in &summaries {
            match self
                .client
                .add_episodic_memory(
                    &session_id,
                    &summary.content,
                    "daily_summary",
                    summary.metadata.clone(),
                )
                .await
            {
                Ok(_) => {
                    items_synced += 1;
                    tracing::debug!(date = %summary.metadata.get("date").unwrap_or(&String::new()), "Synced daily summary");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to sync daily summary");
                    // Continue with other summaries
                }
            }
        }

        // Update sync state
        let mut state = self.state.write().await;
        state.last_sync_timestamp = Utc::now().timestamp_millis();
        state.pending_sync = false;

        let status = SyncStatus {
            timestamp: state.last_sync_timestamp,
            items_synced,
            duration_ms: start.elapsed().as_millis() as u64,
            success: true,
            error: None,
        };

        state.last_sync_status = Some(status.clone());

        Ok(status)
    }

    /// Generate daily summary text for each day since `since`
    async fn generate_daily_summaries(&self, since: i64) -> Vec<DailySummary> {
        let metrics = self.storage.get_metrics().await;
        let mut summaries = Vec::new();

        if metrics.is_empty() {
            return summaries;
        }

        let now = Utc::now().timestamp_millis();
        let mut current_day = since;

        // Process each day
        while current_day < now {
            let day_end = current_day + (24 * 3600 * 1000);
            let date_str = Utc
                .timestamp_millis_opt(current_day)
                .single()
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();

            if date_str.is_empty() {
                current_day = day_end;
                continue;
            }

            let mut content_parts = vec![format!("Daily summary for {}:", date_str)];
            let mut metadata = HashMap::new();
            metadata.insert("date".to_string(), date_str.clone());

            let mut metric_names = Vec::new();
            let mut data_points = 0;

            for metric in &metrics {
                // Query this metric for this day with average aggregation
                let query = Query::select(&[metric.name.as_str()])
                    .time_range(TimeRange::new(current_day, day_end))
                    .with_aggregation(AggregationFunc::Avg)
                    .build();

                if let Ok(result) = self.executor.execute(query).await {
                    if let Some(row) = result.rows.first() {
                        if let Some(&value) = row.values.get(&metric.name) {
                            content_parts.push(format!(
                                "- {}: {:.1} {}",
                                metric.name, value, metric.unit
                            ));
                            metric_names.push(metric.name.clone());
                            data_points += result.rows.len();
                        }
                    }
                }
            }

            // Only include days with actual data
            if content_parts.len() > 1 {
                metadata.insert("metrics".to_string(), metric_names.join(","));
                metadata.insert("data_points".to_string(), data_points.to_string());

                summaries.push(DailySummary {
                    content: content_parts.join("\n"),
                    metadata,
                });
            }

            current_day = day_end;
        }

        // Limit to batch size
        if summaries.len() > self.config.batch_size {
            summaries.truncate(self.config.batch_size);
        }

        summaries
    }

    /// Get current sync status
    pub async fn get_status(&self) -> SyncState {
        self.state.read().await.clone()
    }

    /// Mark that data has changed and should be synced
    pub async fn mark_pending(&self) {
        let mut state = self.state.write().await;
        state.pending_sync = true;
    }

    /// Check if sync is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Internal struct for daily summary data
#[derive(Debug)]
struct DailySummary {
    content: String,
    metadata: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SyncConfig::default();
        assert_eq!(config.sync_interval_hours, 1);
        assert_eq!(config.batch_size, 100);
        assert!(config.enabled);
    }

    #[test]
    fn test_sync_state_default() {
        let state = SyncState::default();
        assert_eq!(state.last_sync_timestamp, 0);
        assert!(!state.pending_sync);
        assert!(state.last_sync_status.is_none());
    }
}
