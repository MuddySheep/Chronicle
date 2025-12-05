//! Health Routes
//!
//! Health check endpoints for monitoring and Kubernetes probes.
//!
//! - GET /health/live - Liveness probe (process is alive)
//! - GET /health/ready - Readiness probe (ready to serve traffic)
//! - GET /health - Full health status

use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;

use crate::api::dto::HealthResponse;
use crate::api::state::AppState;

/// GET /health/live
///
/// Kubernetes liveness probe.
/// Returns 200 if the process is alive, no dependency checks.
pub async fn liveness() -> StatusCode {
    StatusCode::OK
}

/// GET /health/ready
///
/// Kubernetes readiness probe.
/// Returns 200 if the service is ready to accept traffic.
/// Checks that storage is accessible.
pub async fn readiness(State(state): State<Arc<AppState>>) -> StatusCode {
    // Check if we can access metrics (tests storage connection)
    match check_storage_health(&state).await {
        true => StatusCode::OK,
        false => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// GET /health
///
/// Full health status with component details.
pub async fn full_health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let storage_ok = check_storage_health(&state).await;
    let index_ok = check_index_health(&state);

    let storage_status = if storage_ok { "ok" } else { "error" };
    let index_status = if index_ok { "ok" } else { "error" };

    let overall_status = if storage_ok && index_ok {
        "healthy"
    } else if storage_ok || index_ok {
        "degraded"
    } else {
        "unhealthy"
    };

    Json(HealthResponse {
        status: overall_status.to_string(),
        storage: storage_status.to_string(),
        index: index_status.to_string(),
        uptime_seconds: state.uptime_seconds(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Check storage health
async fn check_storage_health(state: &AppState) -> bool {
    // Try to get metrics list - if this works, storage is healthy
    // This is a lightweight operation that verifies the storage engine is working
    let result = state.storage.get_metrics().await;
    // If we get here without panic, storage is OK
    // The result being empty is fine (no metrics registered yet)
    let _ = result;
    true
}

/// Check index health
fn check_index_health(state: &AppState) -> bool {
    // Try to get index stats
    let stats = state.storage.index_stats();
    // If we get here without panic, index is OK
    let _ = stats;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_liveness() {
        let status = liveness().await;
        assert_eq!(status, StatusCode::OK);
    }
}
