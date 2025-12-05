//! # Chronicle
//!
//! Personal Time-Series Intelligence - A full-stack Rust application for storing,
//! querying, and analyzing personal time-series data.
//!
//! ## Features
//!
//! - **High-performance storage**: Append-only log with LZ4 compression
//! - **Efficient queries**: B-tree indexes for fast time-range queries
//! - **Durability**: Write-ahead log ensures no data loss
//! - **Real-time**: WebSocket support for live dashboards
//! - **Pattern detection**: MemMachine integration for insights
//!
//! ## Modules
//!
//! - [`storage`]: Core time-series storage engine
//! - [`index`]: Index structures for efficient queries
//! - [`query`]: Query language parser and executor
//! - [`api`]: REST API server with Axum
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use chronicle::storage::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize storage
//!     let engine = StorageEngine::new(StorageConfig::default()).await?;
//!
//!     // Register metrics
//!     let mood_id = engine.register_metric(
//!         Metric::new("mood", "1-10", Category::Mood, AggregationType::Average)
//!             .description("Daily mood rating")
//!             .range(1.0, 10.0)
//!     ).await?;
//!
//!     // Write data points
//!     engine.write(DataPoint::new(mood_id, 7.5).tag("source", "manual")).await?;
//!
//!     // Query last 7 days
//!     let range = TimeRange::last_days(7);
//!     let points = engine.query_metric("mood", range).await?;
//!
//!     println!("Found {} mood entries", points.len());
//!
//!     // Graceful shutdown
//!     engine.shutdown().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod config;
pub mod index;
pub mod integrations;
pub mod memmachine;
pub mod query;
pub mod storage;
pub mod websocket;

// Re-export top-level types for convenience
pub use storage::{
    AggregationType, Category, DataPoint, Metric, QueryFilter, StorageConfig, StorageEngine,
    StorageError, StorageResult, StorageStats, TimeRange,
};

pub use index::{DataLocation, IndexManager, IndexStats};

pub use query::{
    AggregationFunc, GroupByInterval, Query, QueryError, QueryExecutor, QueryResultData, ResultRow,
};

pub use api::{build_router, serve, ApiConfig, ApiError, AppState};

pub use memmachine::{
    Correlation, CorrelationEngine, InsightEngine, InsightError, InsightResponse,
    MemMachineClient, MemMachineConfig, MemMachineError, SyncConfig, SyncManager, SyncState,
    SyncStatus,
};

pub use websocket::{
    ClientMessage, ConnectionHub, HubConfig, HubError, ServerMessage, WsEvent,
    websocket_handler,
};

pub use config::{
    Config, ConfigError, StorageConfig as ConfigStorageConfig, ApiConfig as ConfigApiConfig,
    MemMachineConfig as ConfigMemMachineConfig, LoggingConfig, IntegrationsConfig,
};

pub use integrations::{
    Integration, IntegrationError, MetricDefinition, AuthCredentials, SyncResult,
    FitbitIntegration, GitHubIntegration, CsvImporter, IntegrationScheduler,
    IntegrationStatus, ScheduleConfig,
};
