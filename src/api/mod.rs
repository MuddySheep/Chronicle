//! Chronicle REST API
//!
//! HTTP API layer for Chronicle, built with Axum.
//!
//! # Endpoints
//!
//! ## Ingest
//! - `POST /api/v1/ingest` - Single data point
//! - `POST /api/v1/ingest/batch` - Batch of data points
//!
//! ## Query
//! - `POST /api/v1/query` - Execute a query
//!
//! ## Metrics
//! - `GET /api/v1/metrics` - List all metrics
//! - `POST /api/v1/metrics` - Create a metric
//! - `GET /api/v1/metrics/:id` - Get a metric
//! - `PUT /api/v1/metrics/:id` - Update a metric
//! - `DELETE /api/v1/metrics/:id` - Delete a metric
//!
//! ## Export
//! - `GET /api/v1/export` - Export data
//!
//! ## Insights (MemMachine Integration)
//! - `POST /api/v1/insights` - Ask questions about your data
//! - `GET /api/v1/correlations` - Get metric correlations
//! - `POST /api/v1/sync` - Trigger MemMachine sync
//! - `GET /api/v1/sync/status` - Get sync status
//!
//! ## Health
//! - `GET /health/live` - Liveness probe
//! - `GET /health/ready` - Readiness probe
//! - `GET /health` - Full health status
//!
//! ## WebSocket
//! - `GET /ws` - Real-time streaming connection
//!
//! # Example
//!
//! ```rust,ignore
//! use chronicle::api::{build_router, serve, ApiConfig, AppState};
//! use chronicle::storage::{StorageConfig, StorageEngine};
//! use chronicle::query::QueryExecutor;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let storage = Arc::new(StorageEngine::new(StorageConfig::default()).await?);
//!     let executor = Arc::new(QueryExecutor::new(Arc::clone(&storage)));
//!     let config = ApiConfig::default();
//!
//!     let state = AppState::new(storage, executor, config.clone());
//!     serve(state, &config).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod dto;
pub mod error;
pub mod routes;
pub mod state;

pub use error::{ApiError, ApiResult};
pub use state::{ApiConfig, AppState};

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::websocket::websocket_handler;

/// Build the API router with all routes and middleware
pub fn build_router(state: AppState) -> Router {
    let api_routes = Router::new()
        // Ingest routes
        .route("/ingest", post(routes::ingest::ingest_single))
        .route("/ingest/batch", post(routes::ingest::ingest_batch))
        // Query routes
        .route("/query", post(routes::query::execute_query))
        // Metric routes
        .route("/metrics", get(routes::metrics::list_metrics))
        .route("/metrics", post(routes::metrics::create_metric))
        .route("/metrics/:id", get(routes::metrics::get_metric))
        .route("/metrics/:id", put(routes::metrics::update_metric))
        .route("/metrics/:id", delete(routes::metrics::delete_metric))
        // Export routes
        .route("/export", get(routes::export::export_data))
        // Insight routes (MemMachine integration)
        .route("/insights", post(routes::insights::generate_insight))
        .route("/correlations", get(routes::correlations::get_correlations))
        // Sync routes (MemMachine integration)
        .route("/sync", post(routes::sync::trigger_sync))
        .route("/sync/status", get(routes::sync::get_sync_status))
        // Import routes - with larger body limit for file uploads (50 MB)
        .route("/import/apple-health", post(routes::apple_health::import_apple_health))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        // WebSocket route
        .route("/ws", get(websocket_handler));

    let health_routes = Router::new()
        .route("/live", get(routes::health::liveness))
        .route("/ready", get(routes::health::readiness))
        .route("/", get(routes::health::full_health));

    // Create shared state
    let shared_state = Arc::new(state);

    Router::new()
        .nest("/api/v1", api_routes)
        .nest("/health", health_routes)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive()) // Configure properly in production
        .with_state(shared_state)
}

/// Start the API server
pub async fn serve(state: AppState, config: &ApiConfig) -> Result<(), ApiError> {
    let router = build_router(state);

    let addr = config.addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Chronicle API listening on {}", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| ApiError::Internal(format!("Server error: {}", e)))?;

    tracing::info!("Chronicle API shut down gracefully");
    Ok(())
}

/// Wait for shutdown signal
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::QueryExecutor;
    use crate::storage::{StorageConfig, StorageEngine};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tempfile::tempdir;
    use tower::util::ServiceExt;

    async fn create_test_app() -> (Router, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let config = StorageConfig::new(dir.path());
        let storage = Arc::new(StorageEngine::new(config).await.unwrap());
        let executor = Arc::new(QueryExecutor::new(Arc::clone(&storage)));
        let api_config = ApiConfig::default();

        let state = AppState::new(storage, executor, api_config);
        let router = build_router(state);

        (router, dir)
    }

    #[tokio::test]
    async fn test_health_live() {
        let (app, _dir) = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/live")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_ready() {
        let (app, _dir) = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_full() {
        let (app, _dir) = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_metrics_empty() {
        let (app, _dir) = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ingest_single() {
        let (app, _dir) = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"metric": "mood", "value": 7.5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_ingest_invalid_json() {
        let (app, _dir) = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest")
                    .header("Content-Type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_metric() {
        let (app, _dir) = create_test_app().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/metrics")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"name": "mood", "unit": "1-10", "category": "mood", "aggregation": "average"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
    }
}
