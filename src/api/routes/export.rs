//! Export Routes
//!
//! Data export endpoint for backup and analysis.
//!
//! - GET /api/v1/export - Export data as streaming response

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use std::sync::Arc;

use crate::api::dto::ExportParams;
use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::storage::TimeRange;

/// GET /api/v1/export
///
/// Export data in the specified format.
/// Supports streaming for large datasets.
pub async fn export_data(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportParams>,
) -> ApiResult<Response> {
    // Check if export is enabled
    if !state.config.enable_export {
        return Err(ApiError::Validation(
            "Export feature is disabled".to_string(),
        ));
    }

    // Parse time range
    let start = parse_export_timestamp(&params.start)?;
    let end = parse_export_timestamp(&params.end)?;

    if start >= end {
        return Err(ApiError::Validation(
            "start must be before end".to_string(),
        ));
    }

    let time_range = TimeRange::new(start, end);

    // Parse metrics filter
    let metric_names: Option<Vec<&str>> = params
        .metrics
        .as_ref()
        .map(|m| m.split(',').map(|s| s.trim()).collect());

    // Fetch data
    let points = if let Some(names) = metric_names {
        let mut all_points = Vec::new();
        for name in names {
            match state.storage.query_metric(name, time_range).await {
                Ok(pts) => all_points.extend(pts),
                Err(e) => {
                    tracing::warn!(metric = %name, error = %e, "Failed to query metric for export");
                }
            }
        }
        all_points.sort_by_key(|p| p.timestamp);
        all_points
    } else {
        state.storage.query(time_range, None).await?
    };

    // Get metric registry for name lookup
    let metrics = state.storage.get_metrics().await;

    // Format response
    let content_type = match params.format.to_lowercase().as_str() {
        "csv" => "text/csv",
        "json" => "application/json",
        "ndjson" | _ => "application/x-ndjson",
    };

    let body = match params.format.to_lowercase().as_str() {
        "csv" => format_csv(&points, &metrics),
        "json" => format_json(&points, &metrics),
        "ndjson" | _ => format_ndjson(&points, &metrics),
    };

    let filename = format!(
        "chronicle_export_{}.{}",
        Utc::now().format("%Y%m%d_%H%M%S"),
        match params.format.to_lowercase().as_str() {
            "csv" => "csv",
            "json" => "json",
            _ => "ndjson",
        }
    );

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        Body::from(body),
    )
        .into_response())
}

/// Parse timestamp for export
fn parse_export_timestamp(s: &str) -> ApiResult<i64> {
    // Handle relative times
    if s.starts_with("now") {
        let now = Utc::now().timestamp_millis();
        if s == "now" {
            return Ok(now);
        }

        let re = regex::Regex::new(r"^now-(\d+)([hdwm])$")
            .map_err(|_| ApiError::Internal("Regex error".to_string()))?;

        if let Some(caps) = re.captures(s) {
            let amount: i64 = caps[1]
                .parse()
                .map_err(|_| ApiError::Validation("Invalid number".to_string()))?;
            let unit = &caps[2];

            let ms = match unit {
                "h" => amount * 3600 * 1000,
                "d" => amount * 24 * 3600 * 1000,
                "w" => amount * 7 * 24 * 3600 * 1000,
                "m" => amount * 30 * 24 * 3600 * 1000,
                _ => return Err(ApiError::Validation("Invalid time unit".to_string())),
            };

            return Ok(now - ms);
        }

        return Err(ApiError::Validation(format!(
            "Cannot parse time: {}",
            s
        )));
    }

    // Try ISO 8601
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp_millis());
    }

    // Try date only
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(date
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis());
    }

    Err(ApiError::Validation(format!(
        "Cannot parse timestamp: {}",
        s
    )))
}

/// Format as CSV
fn format_csv(
    points: &[crate::storage::DataPoint],
    metrics: &[crate::storage::Metric],
) -> String {
    let mut csv = String::new();

    // Header
    csv.push_str("timestamp,metric,value,tags\n");

    // Rows
    for point in points {
        let metric_name = metrics
            .iter()
            .find(|m| m.id == point.metric_id)
            .map(|m| m.name.as_str())
            .unwrap_or("unknown");

        let tags_json = serde_json::to_string(&point.tags).unwrap_or_default();

        csv.push_str(&format!(
            "{},{},{},\"{}\"\n",
            point.timestamp,
            metric_name,
            point.value,
            tags_json.replace('"', "\"\"")
        ));
    }

    csv
}

/// Format as JSON array
fn format_json(
    points: &[crate::storage::DataPoint],
    metrics: &[crate::storage::Metric],
) -> String {
    let records: Vec<serde_json::Value> = points
        .iter()
        .map(|point| {
            let metric_name = metrics
                .iter()
                .find(|m| m.id == point.metric_id)
                .map(|m| m.name.as_str())
                .unwrap_or("unknown");

            serde_json::json!({
                "timestamp": point.timestamp,
                "metric": metric_name,
                "value": point.value,
                "tags": point.tags
            })
        })
        .collect();

    serde_json::to_string_pretty(&records).unwrap_or_default()
}

/// Format as newline-delimited JSON
fn format_ndjson(
    points: &[crate::storage::DataPoint],
    metrics: &[crate::storage::Metric],
) -> String {
    let mut ndjson = String::new();

    for point in points {
        let metric_name = metrics
            .iter()
            .find(|m| m.id == point.metric_id)
            .map(|m| m.name.as_str())
            .unwrap_or("unknown");

        let record = serde_json::json!({
            "timestamp": point.timestamp,
            "metric": metric_name,
            "value": point.value,
            "tags": point.tags
        });

        if let Ok(json) = serde_json::to_string(&record) {
            ndjson.push_str(&json);
            ndjson.push('\n');
        }
    }

    ndjson
}
