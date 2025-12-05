//! Query error types
//!
//! Defines all error conditions that can occur during query parsing and execution.

use thiserror::Error;

/// Errors that can occur during query operations
#[derive(Error, Debug)]
pub enum QueryError {
    /// Query parsing failed
    #[error("Parse error: {0}")]
    Parse(String),

    /// Invalid time range specified
    #[error("Invalid time range: {0}")]
    InvalidTimeRange(String),

    /// Referenced metric does not exist
    #[error("Metric not found: {0}")]
    MetricNotFound(String),

    /// Storage layer error
    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),

    /// Index error during query planning
    #[error("Index error: {0}")]
    Index(String),

    /// Query execution failed
    #[error("Execution error: {0}")]
    Execution(String),

    /// Invalid aggregation operation
    #[error("Invalid aggregation: {0}")]
    InvalidAggregation(String),

    /// Invalid filter operation
    #[error("Invalid filter: {0}")]
    InvalidFilter(String),
}

/// Result type for query operations
pub type QueryResult<T> = Result<T, QueryError>;
