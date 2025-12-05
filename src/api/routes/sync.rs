//! Sync Routes
//!
//! Endpoints for managing MemMachine synchronization.
//!
//! - POST /api/v1/sync - Trigger manual sync
//! - GET /api/v1/sync/status - Get sync status

use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;

use crate::api::dto::{SyncResponse, SyncStatusResponse};
use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;

/// POST /api/v1/sync
///
/// Manually trigger a sync to MemMachine.
/// Syncs all data since the last sync timestamp.
pub async fn trigger_sync(
    State(state): State<Arc<AppState>>,
) -> ApiResult<(StatusCode, Json<SyncResponse>)> {
    // Check if sync manager is available
    let sync_manager = state.sync_manager.as_ref().ok_or_else(|| {
        ApiError::Validation("MemMachine integration not configured".to_string())
    })?;

    // Check if sync is enabled
    if !sync_manager.is_enabled() {
        return Err(ApiError::Validation("MemMachine sync is disabled".to_string()));
    }

    // Trigger sync
    match sync_manager.sync().await {
        Ok(status) => {
            tracing::info!(
                items = status.items_synced,
                duration_ms = status.duration_ms,
                "Manual sync completed"
            );

            Ok((
                StatusCode::OK,
                Json(SyncResponse {
                    status: "success".to_string(),
                    items_synced: status.items_synced,
                    duration_ms: status.duration_ms,
                    error: None,
                }),
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, "Manual sync failed");

            Ok((
                StatusCode::OK,
                Json(SyncResponse {
                    status: "failed".to_string(),
                    items_synced: 0,
                    duration_ms: 0,
                    error: Some(e.to_string()),
                }),
            ))
        }
    }
}

/// GET /api/v1/sync/status
///
/// Get the current sync status including last sync time and pending items.
pub async fn get_sync_status(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<SyncStatusResponse>> {
    // Check if sync manager is available
    let sync_manager = state.sync_manager.as_ref().ok_or_else(|| {
        ApiError::Validation("MemMachine integration not configured".to_string())
    })?;

    let sync_state = sync_manager.get_status().await;

    let last_sync = if sync_state.last_sync_timestamp > 0 {
        Some(
            chrono::Utc
                .timestamp_millis_opt(sync_state.last_sync_timestamp)
                .single()
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default(),
        )
    } else {
        None
    };

    let last_status = sync_state.last_sync_status.as_ref().map(|s| SyncResponse {
        status: if s.success { "success" } else { "failed" }.to_string(),
        items_synced: s.items_synced,
        duration_ms: s.duration_ms,
        error: s.error.clone(),
    });

    Ok(Json(SyncStatusResponse {
        enabled: sync_manager.is_enabled(),
        last_sync,
        pending_sync: sync_state.pending_sync,
        last_status,
    }))
}

use chrono::TimeZone;

#[cfg(test)]
mod tests {
    #[test]
    fn test_sync_status() {
        // Would be tested in integration tests
    }
}
