//! External Integrations
//!
//! This module provides integrations with external data sources:
//! - Fitbit (activity, sleep, heart rate)
//! - GitHub (commits, PRs)
//! - Apple Health (via export file)
//! - CSV import (generic)

mod fitbit;
mod github;
mod csv_import;
mod scheduler;

pub use fitbit::FitbitIntegration;
pub use github::GitHubIntegration;
pub use csv_import::CsvImporter;
pub use scheduler::{IntegrationScheduler, IntegrationStatus, ScheduleConfig};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use crate::storage::DataPoint;

/// Common trait for all integrations
#[async_trait]
pub trait Integration: Send + Sync {
    /// Unique name for this integration
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// Metrics this integration provides
    fn metrics_provided(&self) -> Vec<MetricDefinition>;

    /// Check if authenticated
    fn is_authenticated(&self) -> bool;

    /// Perform authentication (OAuth, token, etc.)
    async fn authenticate(&mut self, credentials: AuthCredentials) -> Result<(), IntegrationError>;

    /// Sync data since given timestamp
    async fn sync(&self, since: Option<DateTime<Utc>>) -> Result<SyncResult, IntegrationError>;
}

/// Definition of a metric provided by an integration
#[derive(Debug, Clone)]
pub struct MetricDefinition {
    pub name: String,
    pub unit: String,
    pub category: String,
    pub aggregation: String,
}

/// Authentication credentials for integrations
#[derive(Debug)]
pub enum AuthCredentials {
    /// OAuth 2.0 authorization code flow
    OAuth { code: String, redirect_uri: String },
    /// Simple token-based auth
    Token { token: String },
    /// File-based (e.g., Apple Health export)
    File { path: std::path::PathBuf },
}

/// Result of a sync operation
#[derive(Debug)]
pub struct SyncResult {
    pub points: Vec<DataPoint>,
    pub metrics_synced: Vec<String>,
    pub earliest: Option<DateTime<Utc>>,
    pub latest: Option<DateTime<Utc>>,
}

/// Errors that can occur during integration operations
#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u64),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("Not authenticated")]
    NotAuthenticated,
}
