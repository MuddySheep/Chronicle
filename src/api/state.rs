//! Application State
//!
//! Shared state accessible by all API handlers.
//! Wrapped in Arc for thread-safe sharing across async tasks.

use crate::memmachine::{CorrelationEngine, InsightEngine, SyncManager};
use crate::query::QueryExecutor;
use crate::storage::StorageEngine;
use crate::websocket::{ConnectionHub, HubConfig};
use std::sync::Arc;
use std::time::Instant;

/// Shared application state for all handlers
#[derive(Clone)]
pub struct AppState {
    /// Storage engine for reading/writing time-series data
    pub storage: Arc<StorageEngine>,
    /// Query executor for running queries
    pub executor: Arc<QueryExecutor>,
    /// API configuration
    pub config: Arc<ApiConfig>,
    /// Server start time for uptime tracking
    pub start_time: Instant,
    /// WebSocket connection hub for real-time streaming
    pub ws_hub: Arc<ConnectionHub>,
    /// Insight engine for MemMachine integration (optional)
    pub insight_engine: Option<Arc<InsightEngine>>,
    /// Correlation engine for MemMachine integration (optional)
    pub correlation_engine: Option<Arc<CorrelationEngine>>,
    /// Sync manager for MemMachine integration (optional)
    pub sync_manager: Option<Arc<SyncManager>>,
}

impl AppState {
    /// Create a new AppState without MemMachine integration
    pub fn new(
        storage: Arc<StorageEngine>,
        executor: Arc<QueryExecutor>,
        config: ApiConfig,
    ) -> Self {
        Self {
            storage,
            executor,
            config: Arc::new(config),
            start_time: Instant::now(),
            ws_hub: Arc::new(ConnectionHub::new(HubConfig::default())),
            insight_engine: None,
            correlation_engine: None,
            sync_manager: None,
        }
    }

    /// Create AppState with MemMachine integration
    pub fn with_memmachine(
        storage: Arc<StorageEngine>,
        executor: Arc<QueryExecutor>,
        config: ApiConfig,
        insight_engine: Arc<InsightEngine>,
        correlation_engine: Arc<CorrelationEngine>,
        sync_manager: Arc<SyncManager>,
    ) -> Self {
        Self {
            storage,
            executor,
            config: Arc::new(config),
            start_time: Instant::now(),
            ws_hub: Arc::new(ConnectionHub::new(HubConfig::default())),
            insight_engine: Some(insight_engine),
            correlation_engine: Some(correlation_engine),
            sync_manager: Some(sync_manager),
        }
    }

    /// Create AppState with custom WebSocket hub configuration
    pub fn with_ws_config(
        storage: Arc<StorageEngine>,
        executor: Arc<QueryExecutor>,
        config: ApiConfig,
        hub_config: HubConfig,
    ) -> Self {
        Self {
            storage,
            executor,
            config: Arc::new(config),
            start_time: Instant::now(),
            ws_hub: Arc::new(ConnectionHub::new(hub_config)),
            insight_engine: None,
            correlation_engine: None,
            sync_manager: None,
        }
    }

    /// Get server uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Check if MemMachine integration is available
    pub fn has_memmachine(&self) -> bool {
        self.insight_engine.is_some()
    }

    /// Get WebSocket connection count
    pub async fn ws_connection_count(&self) -> usize {
        self.ws_hub.connection_count().await
    }
}

/// API server configuration
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Host to bind to
    pub host: String,
    /// Port to listen on
    pub port: u16,
    /// Request timeout in milliseconds
    pub request_timeout_ms: u64,
    /// Maximum request body size in bytes
    pub max_body_size: usize,
    /// Auto-create metrics when ingesting unknown metric names
    pub auto_create_metrics: bool,
    /// Enable data export endpoint
    pub enable_export: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8082,
            request_timeout_ms: 30_000,
            max_body_size: 10 * 1024 * 1024, // 10MB
            auto_create_metrics: true,
            enable_export: true,
        }
    }
}

impl ApiConfig {
    /// Create config with custom host and port
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    /// Get the socket address string
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
