//! Ingest Routes
//!
//! Endpoints for ingesting time-series data points.
//!
//! - POST /api/v1/ingest - Single point
//! - POST /api/v1/ingest/batch - Batch of points

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use std::sync::Arc;

use crate::api::dto::{
    BatchError, BatchIngestRequest, BatchIngestResponse, IngestRequest, IngestResponse,
};
use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::storage::{AggregationType, Category, DataPoint, Metric};
use crate::websocket::WsEvent;

/// POST /api/v1/ingest
///
/// Ingest a single data point.
pub async fn ingest_single(
    State(state): State<Arc<AppState>>,
    Json(req): Json<IngestRequest>,
) -> ApiResult<(StatusCode, Json<IngestResponse>)> {
    // Validate request
    validate_ingest_request(&req)?;

    // Resolve metric name to ID
    let metric_id = resolve_or_create_metric(&state, &req.metric).await?;

    // Create DataPoint
    let timestamp = req.timestamp.unwrap_or_else(|| Utc::now().timestamp_millis());
    let mut point = DataPoint::with_timestamp(metric_id, req.value, timestamp);

    for (k, v) in &req.tags {
        point = point.tag(k, v);
    }

    // Write to storage
    state.storage.write(point).await?;

    // Publish to WebSocket subscribers
    let event = WsEvent::data_point(&req.metric, req.value, timestamp, req.tags.clone());
    state.ws_hub.publish(event);

    Ok((
        StatusCode::CREATED,
        Json(IngestResponse {
            status: "ok".to_string(),
            timestamp,
            metric_id,
        }),
    ))
}

/// POST /api/v1/ingest/batch
///
/// Ingest multiple data points in a single request.
pub async fn ingest_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchIngestRequest>,
) -> ApiResult<(StatusCode, Json<BatchIngestResponse>)> {
    // Validate batch size
    if req.points.is_empty() {
        return Err(ApiError::Validation("Empty batch".to_string()));
    }

    if req.points.len() > 10_000 {
        return Err(ApiError::Validation(
            "Batch size exceeds maximum of 10,000 points".to_string(),
        ));
    }

    let mut accepted = 0;
    let mut errors = Vec::new();

    for (index, point_req) in req.points.into_iter().enumerate() {
        match process_single_point(&state, point_req).await {
            Ok(_) => accepted += 1,
            Err(e) => {
                errors.push(BatchError {
                    index,
                    error: e.to_string(),
                });
            }
        }
    }

    let status = if errors.is_empty() {
        StatusCode::CREATED
    } else if accepted > 0 {
        StatusCode::MULTI_STATUS
    } else {
        StatusCode::BAD_REQUEST
    };

    let status_str = if errors.is_empty() { "ok" } else { "partial" };

    Ok((
        status,
        Json(BatchIngestResponse {
            status: status_str.to_string(),
            accepted,
            rejected: errors.len(),
            errors,
        }),
    ))
}

/// Validate an ingest request
fn validate_ingest_request(req: &IngestRequest) -> ApiResult<()> {
    if req.metric.is_empty() {
        return Err(ApiError::Validation("Metric name cannot be empty".to_string()));
    }

    if req.metric.len() > 100 {
        return Err(ApiError::Validation(
            "Metric name exceeds maximum length of 100 characters".to_string(),
        ));
    }

    if !req.value.is_finite() {
        return Err(ApiError::Validation("Value must be a finite number".to_string()));
    }

    // Validate timestamp if provided (not too far in the past or future)
    if let Some(ts) = req.timestamp {
        let now = Utc::now().timestamp_millis();
        let one_year_ms = 365 * 24 * 60 * 60 * 1000_i64;

        if ts < now - one_year_ms * 10 {
            return Err(ApiError::Validation(
                "Timestamp is more than 10 years in the past".to_string(),
            ));
        }

        if ts > now + one_year_ms {
            return Err(ApiError::Validation(
                "Timestamp is more than 1 year in the future".to_string(),
            ));
        }
    }

    // Validate tags
    for (key, value) in &req.tags {
        if key.is_empty() {
            return Err(ApiError::Validation("Tag key cannot be empty".to_string()));
        }
        if key.len() > 50 {
            return Err(ApiError::Validation(
                "Tag key exceeds maximum length of 50 characters".to_string(),
            ));
        }
        if value.len() > 200 {
            return Err(ApiError::Validation(
                "Tag value exceeds maximum length of 200 characters".to_string(),
            ));
        }
    }

    Ok(())
}

/// Resolve a metric name to ID, optionally creating it
async fn resolve_or_create_metric(state: &AppState, name: &str) -> ApiResult<u32> {
    // Try to find existing metric
    if let Some(metric) = state.storage.get_metric(name).await {
        return Ok(metric.id);
    }

    // Auto-create if enabled
    if state.config.auto_create_metrics {
        let metric = Metric::new(name, "", Category::Custom, AggregationType::Average);
        let id = state.storage.register_metric(metric).await?;
        tracing::info!(metric_name = %name, metric_id = id, "Auto-created metric");
        Ok(id)
    } else {
        Err(ApiError::NotFound(format!("Metric '{}' not found", name)))
    }
}

/// Process a single point from batch
async fn process_single_point(state: &AppState, req: IngestRequest) -> ApiResult<()> {
    validate_ingest_request(&req)?;

    let metric_id = resolve_or_create_metric(state, &req.metric).await?;
    let timestamp = req.timestamp.unwrap_or_else(|| Utc::now().timestamp_millis());

    let mut point = DataPoint::with_timestamp(metric_id, req.value, timestamp);

    // Clone tags for WebSocket event before moving into point
    let tags_for_ws = req.tags.clone();
    let metric_name = req.metric.clone();
    let value = req.value;

    for (k, v) in req.tags {
        point = point.tag(k, v);
    }

    state.storage.write(point).await?;

    // Publish to WebSocket subscribers
    let event = WsEvent::data_point(&metric_name, value, timestamp, tags_for_ws);
    state.ws_hub.publish(event);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_validate_ingest_request_valid() {
        let req = IngestRequest {
            metric: "mood".to_string(),
            value: 7.5,
            timestamp: None,
            tags: HashMap::new(),
        };
        assert!(validate_ingest_request(&req).is_ok());
    }

    #[test]
    fn test_validate_ingest_request_empty_metric() {
        let req = IngestRequest {
            metric: "".to_string(),
            value: 7.5,
            timestamp: None,
            tags: HashMap::new(),
        };
        assert!(validate_ingest_request(&req).is_err());
    }

    #[test]
    fn test_validate_ingest_request_invalid_value() {
        let req = IngestRequest {
            metric: "mood".to_string(),
            value: f64::INFINITY,
            timestamp: None,
            tags: HashMap::new(),
        };
        assert!(validate_ingest_request(&req).is_err());
    }
}
