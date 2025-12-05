//! Chronicle API Server
//!
//! Run with: cargo run --bin chronicle-api
//!
//! # Configuration
//!
//! Environment variables:
//! - `CHRONICLE_HOST`: Host to bind to (default: 0.0.0.0)
//! - `CHRONICLE_PORT`: Port to listen on (default: 8082)
//! - `CHRONICLE_DATA_DIR`: Data directory (default: chronicle_data)
//! - `CHRONICLE_AUTO_CREATE_METRICS`: Auto-create metrics (default: true)
//! - `MEMMACHINE_URL`: MemMachine API URL (optional, enables AI insights)
//! - `MEMMACHINE_USER_ID`: User ID for MemMachine (default: default-user)
//! - `MEMMACHINE_SYNC_ENABLED`: Enable background sync (default: true if MEMMACHINE_URL set)
//! - `RUST_LOG`: Log level (default: info)

use chronicle::api::{serve, ApiConfig, AppState};
use chronicle::memmachine::{
    CorrelationEngine, InsightEngine, MemMachineClient, MemMachineConfig, SyncConfig, SyncManager,
};
use chronicle::query::QueryExecutor;
use chronicle::storage::{StorageConfig, StorageEngine};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "chronicle=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Chronicle API server v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration from environment
    let api_config = load_api_config();
    let storage_config = load_storage_config();
    let memmachine_config = load_memmachine_config();

    tracing::info!("Data directory: {:?}", storage_config.data_dir);
    tracing::info!("Auto-create metrics: {}", api_config.auto_create_metrics);

    // Initialize storage engine
    tracing::info!("Initializing storage engine...");
    let storage = Arc::new(StorageEngine::new(storage_config).await?);
    tracing::info!("Storage engine initialized");

    // Initialize query executor
    let executor = Arc::new(QueryExecutor::new(Arc::clone(&storage)));

    // Create app state (with or without MemMachine)
    let state = if let Some(mm_config) = memmachine_config {
        tracing::info!("MemMachine integration enabled: {}", mm_config.base_url);

        // Initialize MemMachine components
        let mm_client = Arc::new(MemMachineClient::new(mm_config));

        // Check MemMachine availability
        match mm_client.health_check().await {
            Ok(_) => tracing::info!("MemMachine connection verified"),
            Err(e) => tracing::warn!("MemMachine not available: {} (insights will be limited)", e),
        }

        let sync_config = load_sync_config();
        let sync_manager = Arc::new(SyncManager::new(
            Arc::clone(&mm_client),
            Arc::clone(&storage),
            Arc::clone(&executor),
            sync_config,
        ));

        let insight_engine = Arc::new(InsightEngine::new(
            Arc::clone(&mm_client),
            Arc::clone(&storage),
            Arc::clone(&executor),
        ));

        let correlation_engine = Arc::new(CorrelationEngine::new(
            Arc::clone(&storage),
            Arc::clone(&executor),
            Arc::clone(&mm_client),
        ));

        // Start background sync if enabled
        if sync_manager.is_enabled() {
            tracing::info!("Starting background sync to MemMachine");
            Arc::clone(&sync_manager).start_background_sync();
        }

        AppState::with_memmachine(
            Arc::clone(&storage),
            executor,
            api_config.clone(),
            insight_engine,
            correlation_engine,
            sync_manager,
        )
    } else {
        tracing::info!("MemMachine integration disabled (set MEMMACHINE_URL to enable)");
        AppState::new(Arc::clone(&storage), executor, api_config.clone())
    };

    // Run server
    tracing::info!("Starting server on {}:{}", api_config.host, api_config.port);
    serve(state, &api_config).await?;

    // Graceful shutdown
    tracing::info!("Shutting down storage engine...");
    storage.shutdown().await?;
    tracing::info!("Chronicle API server stopped");

    Ok(())
}

/// Load API configuration from environment
fn load_api_config() -> ApiConfig {
    let host = std::env::var("CHRONICLE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let port = std::env::var("CHRONICLE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8082);

    let auto_create_metrics = std::env::var("CHRONICLE_AUTO_CREATE_METRICS")
        .map(|s| s.to_lowercase() != "false" && s != "0")
        .unwrap_or(true);

    ApiConfig {
        host,
        port,
        auto_create_metrics,
        ..Default::default()
    }
}

/// Load storage configuration from environment
fn load_storage_config() -> StorageConfig {
    let data_dir = std::env::var("CHRONICLE_DATA_DIR")
        .unwrap_or_else(|_| "chronicle_data".to_string());

    StorageConfig::new(data_dir)
}

/// Load MemMachine configuration from environment
/// Returns None if MEMMACHINE_URL is not set and local MemMachine is not available
fn load_memmachine_config() -> Option<MemMachineConfig> {
    // Try env var first
    let base_url = std::env::var("MEMMACHINE_URL").ok().or_else(|| {
        // Fallback: check if local MemMachine is available at default port
        let default_url = "http://localhost:8080".to_string();
        // We'll verify connectivity later, but assume local dev setup
        tracing::info!("MEMMACHINE_URL not set, trying default: {}", default_url);
        Some(default_url)
    })?;

    let user_id = std::env::var("MEMMACHINE_USER_ID")
        .unwrap_or_else(|_| "default-user".to_string());

    let group_id = std::env::var("MEMMACHINE_GROUP")
        .unwrap_or_else(|_| "chronicle".to_string());

    Some(MemMachineConfig {
        base_url,
        group_id,
        agent_id: "chronicle-engine".to_string(),
        user_id,
        request_timeout_ms: 5000,
        max_retries: 3,
    })
}

/// Load sync configuration from environment
fn load_sync_config() -> SyncConfig {
    let enabled = std::env::var("MEMMACHINE_SYNC_ENABLED")
        .map(|s| s.to_lowercase() != "false" && s != "0")
        .unwrap_or(true);

    let sync_interval_hours = std::env::var("MEMMACHINE_SYNC_INTERVAL_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    SyncConfig {
        sync_interval_hours,
        batch_size: 100,
        enabled,
    }
}
