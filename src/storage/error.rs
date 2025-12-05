//! Storage engine error types
//!
//! Defines all errors that can occur in the storage layer.

use thiserror::Error;

/// Errors that can occur in the storage engine
#[derive(Error, Debug)]
pub enum StorageError {
    /// I/O operation failed
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization/deserialization failed
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Compression or decompression failed
    #[error("Compression error: {0}")]
    Compression(String),

    /// Data corruption detected (checksum mismatch, invalid magic, etc.)
    #[error("Corrupt data: {0}")]
    Corruption(String),

    /// Requested metric does not exist
    #[error("Metric not found: {0}")]
    MetricNotFound(String),

    /// Invalid time range (start >= end)
    #[error("Invalid time range: start must be less than end")]
    InvalidTimeRange,

    /// Segment file format error
    #[error("Invalid segment format: {0}")]
    InvalidSegment(String),

    /// WAL format or recovery error
    #[error("WAL error: {0}")]
    WalError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Lock acquisition failed
    #[error("Lock error: {0}")]
    Lock(String),
}

impl From<bincode::Error> for StorageError {
    fn from(err: bincode::Error) -> Self {
        StorageError::Serialization(err.to_string())
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(err: serde_json::Error) -> Self {
        StorageError::Serialization(err.to_string())
    }
}

/// Result type alias for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = StorageError::MetricNotFound("mood".to_string());
        assert_eq!(err.to_string(), "Metric not found: mood");

        let err = StorageError::InvalidTimeRange;
        assert_eq!(
            err.to_string(),
            "Invalid time range: start must be less than end"
        );
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let storage_err: StorageError = io_err.into();
        assert!(matches!(storage_err, StorageError::Io(_)));
    }
}
