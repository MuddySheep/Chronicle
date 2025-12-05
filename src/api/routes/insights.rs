//! Insight Routes
//!
//! Endpoints for generating AI-powered insights using MemMachine.
//!
//! - POST /api/v1/insights - Ask a question about your data

use axum::{extract::State, Json};
use std::sync::Arc;

use crate::api::dto::{InsightRequest, InsightResponseDto};
use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;

/// POST /api/v1/insights
///
/// Generate an insight by asking a question about your data.
/// Uses MemMachine for context and pattern matching.
pub async fn generate_insight(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InsightRequest>,
) -> ApiResult<Json<InsightResponseDto>> {
    // Validate request
    if req.question.trim().is_empty() {
        return Err(ApiError::Validation("question cannot be empty".to_string()));
    }

    let context_days = req.context_days.unwrap_or(14);
    if context_days < 1 || context_days > 365 {
        return Err(ApiError::Validation(
            "context_days must be between 1 and 365".to_string(),
        ));
    }

    // Check if insight engine is available
    let insight_engine = state.insight_engine.as_ref().ok_or_else(|| {
        ApiError::Validation("MemMachine integration not configured".to_string())
    })?;

    // Generate insight
    let response = insight_engine
        .generate_insight(&req.question, context_days)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to generate insight: {}", e)))?;

    Ok(Json(InsightResponseDto {
        insight: response.insight,
        supporting_data: response.supporting_data,
        related_patterns: response.related_patterns,
        recommendations: response.recommendations,
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_empty_question_validation() {
        // This would be tested in integration tests with the actual route
    }
}
