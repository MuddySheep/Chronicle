//! Data Transfer Objects
//!
//! Request and response types for the API endpoints.
//! These types are serialized/deserialized to/from JSON.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================
// INGEST DTOs
// ============================================

/// Single data point ingest request
#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    /// Metric name
    pub metric: String,
    /// Value to record
    pub value: f64,
    /// Optional timestamp (ms since epoch), defaults to now
    #[serde(default)]
    pub timestamp: Option<i64>,
    /// Optional tags for filtering
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

/// Single data point ingest response
#[derive(Debug, Serialize)]
pub struct IngestResponse {
    /// Status: "ok"
    pub status: String,
    /// Timestamp of the ingested point
    pub timestamp: i64,
    /// ID of the metric
    pub metric_id: u32,
}

/// Batch ingest request
#[derive(Debug, Deserialize)]
pub struct BatchIngestRequest {
    /// Array of data points to ingest
    pub points: Vec<IngestRequest>,
}

/// Batch ingest response
#[derive(Debug, Serialize)]
pub struct BatchIngestResponse {
    /// Status: "ok" or "partial"
    pub status: String,
    /// Number of points accepted
    pub accepted: usize,
    /// Number of points rejected
    pub rejected: usize,
    /// Errors for rejected points
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<BatchError>,
}

/// Error for a single point in batch ingest
#[derive(Debug, Serialize)]
pub struct BatchError {
    /// Index of the failed point
    pub index: usize,
    /// Error message
    pub error: String,
}

// ============================================
// QUERY DTOs
// ============================================

/// Query request
#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    /// Metrics to select
    pub select: Vec<String>,
    /// Time range to query
    pub time_range: TimeRangeDto,
    /// Optional GROUP BY interval
    #[serde(default)]
    pub group_by: Option<String>,
    /// Optional aggregation function
    #[serde(default)]
    pub aggregation: Option<String>,
    /// Optional filters
    #[serde(default)]
    pub filters: Vec<FilterDto>,
    /// Optional limit on results
    #[serde(default)]
    pub limit: Option<usize>,
    /// Output format: json, csv, chart
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "json".to_string()
}

/// Time range specification
#[derive(Debug, Deserialize)]
pub struct TimeRangeDto {
    /// Start time (ISO 8601 or relative like "now-7d")
    pub start: String,
    /// End time (ISO 8601 or relative like "now")
    pub end: String,
}

/// Filter specification
#[derive(Debug, Deserialize)]
pub struct FilterDto {
    /// Tag key to filter on
    pub tag: String,
    /// Operator: eq, ne, gt, gte, lt, lte
    pub op: String,
    /// Value to compare
    pub value: String,
}

/// Query response (JSON format)
#[derive(Debug, Serialize)]
pub struct QueryResponse {
    /// Column names
    pub columns: Vec<String>,
    /// Data rows
    pub rows: Vec<QueryRow>,
    /// Query metadata
    pub meta: QueryMeta,
}

/// Single row in query response
#[derive(Debug, Serialize, Clone)]
pub struct QueryRow {
    /// Timestamp for this row
    pub timestamp: i64,
    /// Values keyed by column name
    #[serde(flatten)]
    pub values: HashMap<String, f64>,
}

/// Query metadata
#[derive(Debug, Serialize)]
pub struct QueryMeta {
    /// Query execution time in milliseconds
    pub execution_time_ms: u64,
    /// Number of rows returned
    pub row_count: usize,
}

/// Chart-formatted query response
#[derive(Debug, Serialize)]
pub struct ChartResponse {
    /// Labels for x-axis
    pub labels: Vec<String>,
    /// Data series
    pub datasets: Vec<ChartDataset>,
}

/// Single dataset for chart
#[derive(Debug, Serialize)]
pub struct ChartDataset {
    /// Dataset label
    pub label: String,
    /// Data values
    pub data: Vec<f64>,
    /// Suggested color
    pub color: String,
}

// ============================================
// METRIC DTOs
// ============================================

/// Create metric request
#[derive(Debug, Clone, Deserialize)]
pub struct CreateMetricRequest {
    /// Metric name (unique)
    pub name: String,
    /// Unit of measurement
    pub unit: String,
    /// Category: health, productivity, mood, habit, custom
    pub category: String,
    /// Aggregation type: sum, average, last, max, min
    pub aggregation: String,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
}

/// Update metric request
#[derive(Debug, Deserialize)]
pub struct UpdateMetricRequest {
    /// New unit (optional)
    #[serde(default)]
    pub unit: Option<String>,
    /// New description (optional)
    #[serde(default)]
    pub description: Option<String>,
}

/// Metric response
#[derive(Debug, Serialize)]
pub struct MetricResponse {
    /// Metric ID
    pub id: u32,
    /// Metric name
    pub name: String,
    /// Unit of measurement
    pub unit: String,
    /// Category
    pub category: String,
    /// Aggregation type
    pub aggregation: String,
    /// Description
    pub description: Option<String>,
}

/// List metrics response
#[derive(Debug, Serialize)]
pub struct MetricListResponse {
    /// List of metrics
    pub metrics: Vec<MetricResponse>,
    /// Total count
    pub total: usize,
}

// ============================================
// HEALTH DTOs
// ============================================

/// Full health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Overall status: healthy, degraded, unhealthy
    pub status: String,
    /// Storage status
    pub storage: String,
    /// Index status
    pub index: String,
    /// Server uptime in seconds
    pub uptime_seconds: u64,
    /// Application version
    pub version: String,
}

// ============================================
// EXPORT DTOs
// ============================================

/// Export query parameters
#[derive(Debug, Deserialize)]
pub struct ExportParams {
    /// Start time (ISO 8601)
    pub start: String,
    /// End time (ISO 8601)
    pub end: String,
    /// Comma-separated metric names
    #[serde(default)]
    pub metrics: Option<String>,
    /// Format: json, csv, ndjson
    #[serde(default = "default_export_format")]
    pub format: String,
}

fn default_export_format() -> String {
    "ndjson".to_string()
}

// ============================================
// INSIGHT DTOs (MemMachine Integration)
// ============================================

/// Insight request
#[derive(Debug, Deserialize)]
pub struct InsightRequest {
    /// The question to ask about your data
    pub question: String,
    /// Number of days of context to consider (default: 14)
    #[serde(default)]
    pub context_days: Option<i64>,
    /// Whether to include raw data in response
    #[serde(default)]
    pub include_data: Option<bool>,
}

/// Insight response
#[derive(Debug, Serialize)]
pub struct InsightResponseDto {
    /// The generated insight text
    pub insight: String,
    /// Supporting data points used in the insight
    pub supporting_data: HashMap<String, f64>,
    /// Related patterns from MemMachine
    pub related_patterns: Vec<String>,
    /// Actionable recommendations
    pub recommendations: Vec<String>,
}

// ============================================
// CORRELATION DTOs
// ============================================

/// Correlation query parameters
#[derive(Debug, Deserialize)]
pub struct CorrelationParams {
    /// Number of days to analyze (default: 30, min: 7, max: 365)
    #[serde(default)]
    pub days: Option<i64>,
}

/// Single correlation
#[derive(Debug, Serialize)]
pub struct CorrelationDto {
    /// First metric name
    pub metric_a: String,
    /// Second metric name
    pub metric_b: String,
    /// Pearson correlation coefficient (-1 to 1)
    pub coefficient: f64,
    /// Human-readable strength: "strong", "moderate", "weak"
    pub strength: String,
    /// Direction: "positive" or "negative"
    pub direction: String,
    /// Number of data points used
    pub sample_size: usize,
    /// When this correlation was calculated
    pub last_calculated: String,
}

/// Correlations response
#[derive(Debug, Serialize)]
pub struct CorrelationsResponse {
    /// List of correlations (sorted by strength)
    pub correlations: Vec<CorrelationDto>,
    /// Number of days analyzed
    pub window_days: i64,
}

// ============================================
// SYNC DTOs (MemMachine Integration)
// ============================================

/// Sync response
#[derive(Debug, Serialize)]
pub struct SyncResponse {
    /// Status: "success" or "failed"
    pub status: String,
    /// Number of items synced
    pub items_synced: u32,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Sync status response
#[derive(Debug, Serialize)]
pub struct SyncStatusResponse {
    /// Whether sync is enabled
    pub enabled: bool,
    /// Last successful sync timestamp (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<String>,
    /// Whether there's pending data to sync
    pub pending_sync: bool,
    /// Status of the last sync attempt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<SyncResponse>,
}
