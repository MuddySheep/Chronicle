//! Correlation Routes
//!
//! Endpoints for viewing metric correlations.
//!
//! - GET /api/v1/correlations - Get all metric correlations

use axum::{
    extract::{Query, State},
    Json,
};
use std::sync::Arc;

use crate::api::dto::{CorrelationDto, CorrelationParams, CorrelationsResponse};
use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;

/// GET /api/v1/correlations
///
/// Calculate and return correlations between all metric pairs.
/// Correlations are calculated using Pearson coefficient over daily averages.
pub async fn get_correlations(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CorrelationParams>,
) -> ApiResult<Json<CorrelationsResponse>> {
    let days = params.days.unwrap_or(30);
    if days < 7 || days > 365 {
        return Err(ApiError::Validation(
            "days must be between 7 and 365".to_string(),
        ));
    }

    // Check if correlation engine is available
    let correlation_engine = state.correlation_engine.as_ref().ok_or_else(|| {
        ApiError::Validation("MemMachine integration not configured".to_string())
    })?;

    // Calculate correlations
    let correlations = correlation_engine.calculate_all(days).await;

    // Convert to DTOs
    let correlation_dtos: Vec<CorrelationDto> = correlations
        .into_iter()
        .map(|c| CorrelationDto {
            metric_a: c.metric_a,
            metric_b: c.metric_b,
            coefficient: c.coefficient,
            strength: c.strength,
            direction: c.direction,
            sample_size: c.sample_size,
            last_calculated: c.last_calculated,
        })
        .collect();

    Ok(Json(CorrelationsResponse {
        correlations: correlation_dtos,
        window_days: days,
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_days_validation() {
        // Would be tested in integration tests
    }
}
