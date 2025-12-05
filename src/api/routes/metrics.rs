//! Metrics Routes
//!
//! CRUD endpoints for metric definitions.
//!
//! - GET /api/v1/metrics - List all metrics
//! - POST /api/v1/metrics - Create a new metric
//! - GET /api/v1/metrics/:id - Get a specific metric
//! - PUT /api/v1/metrics/:id - Update a metric
//! - DELETE /api/v1/metrics/:id - Delete a metric (soft delete)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;

use crate::api::dto::{
    CreateMetricRequest, MetricListResponse, MetricResponse, UpdateMetricRequest,
};
use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::storage::{AggregationType, Category, Metric};

/// GET /api/v1/metrics
///
/// List all registered metrics.
pub async fn list_metrics(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<MetricListResponse>> {
    let metrics = state.storage.get_metrics().await;

    let responses: Vec<MetricResponse> = metrics.iter().map(metric_to_response).collect();

    Ok(Json(MetricListResponse {
        total: responses.len(),
        metrics: responses,
    }))
}

/// GET /api/v1/metrics/:id
///
/// Get a specific metric by ID.
pub async fn get_metric(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
) -> ApiResult<Json<MetricResponse>> {
    let metrics = state.storage.get_metrics().await;

    let metric = metrics
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("Metric with id {} not found", id)))?;

    Ok(Json(metric_to_response(&metric)))
}

/// POST /api/v1/metrics
///
/// Create a new metric definition.
pub async fn create_metric(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateMetricRequest>,
) -> ApiResult<(StatusCode, Json<MetricResponse>)> {
    // Validate request
    validate_create_request(&req)?;

    // Check if metric already exists
    if state.storage.get_metric(&req.name).await.is_some() {
        return Err(ApiError::Validation(format!(
            "Metric '{}' already exists",
            req.name
        )));
    }

    // Parse category
    let category = parse_category(&req.category)?;

    // Parse aggregation type
    let aggregation = parse_aggregation_type(&req.aggregation)?;

    // Create metric
    let mut metric = Metric::new(&req.name, &req.unit, category, aggregation);
    if let Some(desc) = &req.description {
        metric = metric.description(desc);
    }

    // Register metric
    let id = state.storage.register_metric(metric.clone()).await?;

    // Fetch the registered metric to get complete info
    let registered = state
        .storage
        .get_metric(&req.name)
        .await
        .unwrap_or_else(|| {
            let mut m = metric;
            m.id = id;
            m
        });

    tracing::info!(metric_name = %req.name, metric_id = id, "Created metric");

    Ok((StatusCode::CREATED, Json(metric_to_response(&registered))))
}

/// PUT /api/v1/metrics/:id
///
/// Update a metric (limited fields).
pub async fn update_metric(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
    Json(_req): Json<UpdateMetricRequest>,
) -> ApiResult<Json<MetricResponse>> {
    let metrics = state.storage.get_metrics().await;

    let metric = metrics
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("Metric with id {} not found", id)))?;

    // NOTE: For now, we can't actually update metrics in storage since we only have register_metric
    // This would require adding an update method to StorageEngine
    // For now, we return the metric as-is with a warning

    tracing::warn!(
        metric_id = id,
        "Metric update requested but not implemented - returning existing metric"
    );

    // In a real implementation, we would:
    // 1. Update unit if provided: req.unit
    // 2. Update description if provided: req.description
    // 3. Save the changes

    Ok(Json(metric_to_response(metric)))
}

/// DELETE /api/v1/metrics/:id
///
/// Soft delete a metric (data is retained, metric is hidden).
pub async fn delete_metric(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u32>,
) -> ApiResult<StatusCode> {
    let metrics = state.storage.get_metrics().await;

    let _metric = metrics
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("Metric with id {} not found", id)))?;

    // NOTE: For now, we can't actually delete metrics
    // This would require adding a delete method to StorageEngine
    // For now, we just acknowledge the request

    tracing::warn!(
        metric_id = id,
        "Metric delete requested but not implemented"
    );

    // In a real implementation, we would soft-delete by:
    // 1. Marking the metric as deleted
    // 2. Not returning it in list queries
    // 3. Keeping the data for historical queries

    Ok(StatusCode::NO_CONTENT)
}

/// Validate create metric request
fn validate_create_request(req: &CreateMetricRequest) -> ApiResult<()> {
    if req.name.is_empty() {
        return Err(ApiError::Validation("Metric name cannot be empty".to_string()));
    }

    if req.name.len() > 100 {
        return Err(ApiError::Validation(
            "Metric name exceeds maximum length of 100 characters".to_string(),
        ));
    }

    // Name must be alphanumeric with underscores
    if !req
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_')
    {
        return Err(ApiError::Validation(
            "Metric name must contain only alphanumeric characters and underscores".to_string(),
        ));
    }

    if req.unit.len() > 50 {
        return Err(ApiError::Validation(
            "Unit exceeds maximum length of 50 characters".to_string(),
        ));
    }

    Ok(())
}

/// Parse category string
fn parse_category(s: &str) -> ApiResult<Category> {
    match s.to_lowercase().as_str() {
        "health" => Ok(Category::Health),
        "productivity" => Ok(Category::Productivity),
        "mood" => Ok(Category::Mood),
        "habit" => Ok(Category::Habit),
        "custom" => Ok(Category::Custom),
        _ => Err(ApiError::Validation(format!(
            "Invalid category: {}. Use health, productivity, mood, habit, or custom",
            s
        ))),
    }
}

/// Parse aggregation type string
fn parse_aggregation_type(s: &str) -> ApiResult<AggregationType> {
    match s.to_lowercase().as_str() {
        "sum" => Ok(AggregationType::Sum),
        "average" | "avg" => Ok(AggregationType::Average),
        "last" => Ok(AggregationType::Last),
        "max" => Ok(AggregationType::Max),
        "min" => Ok(AggregationType::Min),
        "count" => Ok(AggregationType::Count),
        _ => Err(ApiError::Validation(format!(
            "Invalid aggregation type: {}. Use sum, average, last, max, min, or count",
            s
        ))),
    }
}

/// Convert Metric to MetricResponse
fn metric_to_response(metric: &Metric) -> MetricResponse {
    MetricResponse {
        id: metric.id,
        name: metric.name.clone(),
        unit: metric.unit.clone(),
        category: format!("{}", metric.category),
        aggregation: format!("{:?}", metric.aggregation).to_lowercase(),
        description: metric.description.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_category() {
        assert!(matches!(parse_category("health"), Ok(Category::Health)));
        assert!(matches!(parse_category("MOOD"), Ok(Category::Mood)));
        assert!(parse_category("invalid").is_err());
    }

    #[test]
    fn test_parse_aggregation_type() {
        assert!(matches!(
            parse_aggregation_type("sum"),
            Ok(AggregationType::Sum)
        ));
        assert!(matches!(
            parse_aggregation_type("AVERAGE"),
            Ok(AggregationType::Average)
        ));
        assert!(parse_aggregation_type("invalid").is_err());
    }

    #[test]
    fn test_validate_create_request() {
        let valid = CreateMetricRequest {
            name: "mood".to_string(),
            unit: "1-10".to_string(),
            category: "mood".to_string(),
            aggregation: "average".to_string(),
            description: None,
        };
        assert!(validate_create_request(&valid).is_ok());

        let empty_name = CreateMetricRequest {
            name: "".to_string(),
            ..valid.clone()
        };
        assert!(validate_create_request(&empty_name).is_err());
    }
}
