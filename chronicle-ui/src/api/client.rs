//! HTTP API Client
//!
//! Functions for communicating with the Chronicle REST API.

use gloo_net::http::Request;
use std::collections::HashMap;

use crate::state::global::{DataPoint, Metric};

/// Default API base URL
pub const DEFAULT_API_BASE: &str = "http://localhost:8082/api/v1";

/// Get the API base URL from local storage or use default
pub fn get_api_base() -> String {
    let url = if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            if let Ok(Some(url)) = storage.get_item("chronicle_api_url") {
                url
            } else {
                DEFAULT_API_BASE.to_string()
            }
        } else {
            DEFAULT_API_BASE.to_string()
        }
    } else {
        DEFAULT_API_BASE.to_string()
    };
    // Normalize: remove trailing slash
    url.trim_end_matches('/').to_string()
}

/// Set the API base URL in local storage
pub fn set_api_base(url: &str) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item("chronicle_api_url", url);
        }
    }
}

// ============ Response Types ============

#[derive(Debug, serde::Deserialize)]
pub struct MetricListResponse {
    pub metrics: Vec<Metric>,
}

#[derive(Debug, serde::Deserialize)]
pub struct IngestResponse {
    pub status: String,
    pub timestamp: i64,
    pub metric_id: u32,
}

#[derive(Debug, serde::Deserialize)]
pub struct QueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<QueryRow>,
    #[serde(default)]
    pub execution_time_ms: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
pub struct QueryRow {
    pub timestamp: i64,
    #[serde(flatten)]
    pub values: HashMap<String, serde_json::Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct InsightResponse {
    pub insight: String,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub supporting_data: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CorrelationResponse {
    pub correlations: Vec<Correlation>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Correlation {
    pub metric_a: String,
    pub metric_b: String,
    pub correlation: f64,
    pub sample_count: usize,
    #[serde(default)]
    pub lag_days: Option<i32>,
}

#[derive(Debug, serde::Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_seconds: u64,
    pub storage_stats: Option<StorageStats>,
}

#[derive(Debug, serde::Deserialize)]
pub struct StorageStats {
    pub total_points: u64,
    pub total_metrics: u32,
    pub total_segments: usize,
}

#[derive(Debug, serde::Deserialize)]
pub struct ApiError {
    pub error: String,
    #[serde(default)]
    pub code: Option<String>,
}

// ============ API Functions ============

/// Fetch all metrics
pub async fn fetch_metrics() -> Result<Vec<Metric>, String> {
    let api_base = get_api_base();

    let response = Request::get(&format!("{}/metrics", api_base))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Unknown error".to_string(), code: None });
        return Err(error.error);
    }

    let result: MetricListResponse = response.json().await
        .map_err(|e| format!("Parse error: {}", e))?;

    Ok(result.metrics)
}

/// Create a new metric
pub async fn create_metric(
    name: &str,
    unit: &str,
    category: &str,
    aggregation: &str,
) -> Result<Metric, String> {
    #[derive(serde::Serialize)]
    struct CreateMetricRequest {
        name: String,
        unit: String,
        category: String,
        aggregation: String,
    }

    let api_base = get_api_base();

    let response = Request::post(&format!("{}/metrics", api_base))
        .json(&CreateMetricRequest {
            name: name.to_string(),
            unit: unit.to_string(),
            category: category.to_string(),
            aggregation: aggregation.to_string(),
        })
        .map_err(|e| format!("Request build error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Unknown error".to_string(), code: None });
        return Err(error.error);
    }

    response.json().await
        .map_err(|e| format!("Parse error: {}", e))
}

/// Fetch chart data for selected metrics
pub async fn fetch_chart_data(
    metrics: &[String],
    start: i64,
    end: i64,
) -> Result<HashMap<String, Vec<DataPoint>>, String> {
    #[derive(serde::Serialize)]
    struct QueryRequest {
        select: Vec<String>,
        time_range: TimeRangeDto,
        #[serde(skip_serializing_if = "Option::is_none")]
        group_by: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        aggregation: Option<String>,
    }

    #[derive(serde::Serialize)]
    struct TimeRangeDto {
        start: String,
        end: String,
    }

    let api_base = get_api_base();

    // Determine appropriate grouping based on time range
    let duration_days = (end - start) / (24 * 60 * 60 * 1000);
    let group_by = if duration_days > 90 {
        Some("week".to_string())
    } else if duration_days > 30 {
        Some("day".to_string())
    } else if duration_days > 7 {
        Some("day".to_string())
    } else {
        None // Raw data for short ranges
    };

    let request = QueryRequest {
        select: metrics.to_vec(),
        time_range: TimeRangeDto {
            start: start.to_string(),
            end: end.to_string(),
        },
        group_by,
        aggregation: Some("avg".to_string()),
    };

    let response = Request::post(&format!("{}/query", api_base))
        .json(&request)
        .map_err(|e| format!("Request build error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Unknown error".to_string(), code: None });
        return Err(error.error);
    }

    let query_response: QueryResponse = response.json().await
        .map_err(|e| format!("Parse error: {}", e))?;

    // Transform response to per-metric series
    let mut result: HashMap<String, Vec<DataPoint>> = HashMap::new();

    for metric in metrics {
        let points: Vec<DataPoint> = query_response.rows.iter()
            .filter_map(|row| {
                row.values.get(metric).and_then(|v| {
                    v.as_f64().map(|value| DataPoint {
                        timestamp: row.timestamp,
                        value,
                        tags: HashMap::new(),
                    })
                })
            })
            .collect();

        result.insert(metric.clone(), points);
    }

    Ok(result)
}

/// Submit a single data point
pub async fn submit_data_point(
    metric: &str,
    value: f64,
    tags: Option<HashMap<String, String>>,
) -> Result<IngestResponse, String> {
    #[derive(serde::Serialize)]
    struct IngestRequest {
        metric: String,
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        tags: Option<HashMap<String, String>>,
    }

    let api_base = get_api_base();

    let response = Request::post(&format!("{}/ingest", api_base))
        .json(&IngestRequest {
            metric: metric.to_string(),
            value,
            tags,
        })
        .map_err(|e| format!("Request build error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Unknown error".to_string(), code: None });
        return Err(error.error);
    }

    response.json().await
        .map_err(|e| format!("Parse error: {}", e))
}

/// Fetch an insight from MemMachine
pub async fn fetch_insight(question: &str, context_days: i64) -> Result<String, String> {
    #[derive(serde::Serialize)]
    struct InsightRequest {
        question: String,
        context_days: i64,
    }

    let api_base = get_api_base();

    let response = Request::post(&format!("{}/insights", api_base))
        .json(&InsightRequest {
            question: question.to_string(),
            context_days,
        })
        .map_err(|e| format!("Request build error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Unable to generate insight".to_string(), code: None });
        return Err(error.error);
    }

    let insight_response: InsightResponse = response.json().await
        .map_err(|e| format!("Parse error: {}", e))?;

    Ok(insight_response.insight)
}

/// Fetch correlations
pub async fn fetch_correlations(days: i64) -> Result<Vec<Correlation>, String> {
    let api_base = get_api_base();

    let response = Request::get(&format!("{}/correlations?days={}", api_base, days))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Unknown error".to_string(), code: None });
        return Err(error.error);
    }

    let result: CorrelationResponse = response.json().await
        .map_err(|e| format!("Parse error: {}", e))?;

    Ok(result.correlations)
}

/// Check API health
pub async fn check_health() -> Result<HealthResponse, String> {
    let api_base = get_api_base();
    let health_url = api_base.replace("/api/v1", "/health");

    let response = Request::get(&health_url)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        return Err("API is not healthy".to_string());
    }

    response.json().await
        .map_err(|e| format!("Parse error: {}", e))
}

/// Trigger sync with MemMachine
pub async fn trigger_sync() -> Result<(), String> {
    let api_base = get_api_base();

    let response = Request::post(&format!("{}/sync", api_base))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Sync failed".to_string(), code: None });
        return Err(error.error);
    }

    Ok(())
}

/// Export data
pub async fn export_data(
    format: &str,
    metrics: Option<Vec<String>>,
    start: Option<i64>,
    end: Option<i64>,
) -> Result<String, String> {
    let api_base = get_api_base();

    let mut url = format!("{}/export?format={}", api_base, format);

    if let Some(m) = metrics {
        url.push_str(&format!("&metrics={}", m.join(",")));
    }
    if let Some(s) = start {
        url.push_str(&format!("&start={}", s));
    }
    if let Some(e) = end {
        url.push_str(&format!("&end={}", e));
    }

    let response = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Export failed".to_string(), code: None });
        return Err(error.error);
    }

    response.text().await
        .map_err(|e| format!("Parse error: {}", e))
}

/// Response from Apple Health import
#[derive(Debug, serde::Deserialize)]
pub struct ImportResult {
    pub imported_count: usize,
    pub metrics_created: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Import Apple Health data from ZIP file bytes
pub async fn import_apple_health(data: &[u8]) -> Result<ImportResult, String> {
    let api_base = get_api_base();

    // Use base64 encoding to send binary data as JSON
    let base64_data = base64_encode(data);

    #[derive(serde::Serialize)]
    struct ImportRequest {
        data: String,
        format: String,
    }

    let response = Request::post(&format!("{}/import/apple-health", api_base))
        .json(&ImportRequest {
            data: base64_data,
            format: "zip".to_string(),
        })
        .map_err(|e| format!("Request build error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok() {
        let error: ApiError = response.json().await
            .unwrap_or(ApiError { error: "Import failed".to_string(), code: None });
        return Err(error.error);
    }

    response.json().await
        .map_err(|e| format!("Parse error: {}", e))
}

/// Simple base64 encoding for binary data
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let b0 = data[i] as usize;
        let b1 = if i + 1 < data.len() { data[i + 1] as usize } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as usize } else { 0 };

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if i + 1 < data.len() {
            result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('=');
        }

        if i + 2 < data.len() {
            result.push(ALPHABET[b2 & 0x3f] as char);
        } else {
            result.push('=');
        }

        i += 3;
    }

    result
}
