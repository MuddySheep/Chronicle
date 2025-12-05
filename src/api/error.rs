//! API Error Types
//!
//! Defines error types for the API layer and implements conversion
//! to HTTP responses with appropriate status codes.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

/// API error types
#[derive(Error, Debug)]
pub enum ApiError {
    /// Request validation failed
    #[error("Validation error: {0}")]
    Validation(String),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Query parsing or execution error
    #[error("Query error: {0}")]
    Query(#[from] crate::query::QueryError),

    /// Storage layer error
    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),

    /// Internal server error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Service unavailable (dependency down)
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Error response body
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
    pub request_id: String,
}

/// Error details
#[derive(Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            ApiError::Query(e) => {
                // Check if it's a metric not found error
                if e.to_string().contains("not found") {
                    (StatusCode::NOT_FOUND, "METRIC_NOT_FOUND")
                } else {
                    (StatusCode::BAD_REQUEST, "QUERY_ERROR")
                }
            }
            ApiError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            ApiError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE")
            }
            ApiError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "IO_ERROR"),
        };

        let request_id = uuid::Uuid::new_v4().to_string();

        // Log the error
        tracing::error!(
            request_id = %request_id,
            error_code = %code,
            error_message = %self,
            "API error occurred"
        );

        let body = ErrorResponse {
            error: ErrorBody {
                code: code.to_string(),
                message: self.to_string(),
            },
            request_id,
        };

        (status, Json(body)).into_response()
    }
}

/// Result type for API operations
pub type ApiResult<T> = Result<T, ApiError>;
