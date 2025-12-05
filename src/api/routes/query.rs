//! Query Routes
//!
//! Endpoint for executing Chronicle queries.
//!
//! - POST /api/v1/query - Execute a query

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{TimeZone, Utc};
use std::sync::Arc;

use crate::api::dto::{
    ChartDataset, ChartResponse, FilterDto, QueryMeta, QueryRequest, QueryResponse, QueryRow,
    TimeRangeDto,
};
use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::query::{AggregationFunc, Filter, FilterField, FilterValue, GroupByInterval, Operator, Query};
use crate::storage::TimeRange;

/// POST /api/v1/query
///
/// Execute a query and return results.
pub async fn execute_query(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> ApiResult<Response> {
    // Validate request
    if req.select.is_empty() {
        return Err(ApiError::Validation("select cannot be empty".to_string()));
    }

    // Parse time range
    let time_range = parse_time_range(&req.time_range)?;

    // Build query
    let metrics: Vec<&str> = req.select.iter().map(|s| s.as_str()).collect();
    let mut builder = Query::select(&metrics).time_range(time_range);

    // Add GROUP BY
    if let Some(ref group_by) = req.group_by {
        let interval = parse_group_by(group_by)?;
        builder = builder.group_by(interval);
    }

    // Add aggregation
    if let Some(ref agg) = req.aggregation {
        let agg_func = parse_aggregation(agg)?;
        builder = builder.with_aggregation(agg_func);
    }

    // Add filters
    for filter_dto in &req.filters {
        let filter = parse_filter(filter_dto)?;
        builder = builder.filter(filter);
    }

    // Add limit
    if let Some(limit) = req.limit {
        builder = builder.limit(limit);
    }

    let query = builder.build();

    // Execute query
    let result = state.executor.execute(query).await?;

    // Format response based on requested format
    match req.format.to_lowercase().as_str() {
        "csv" => Ok(format_csv_response(&result)),
        "chart" => Ok(format_chart_response(&result)),
        _ => Ok(format_json_response(&result)),
    }
}

/// Format response as JSON
fn format_json_response(result: &crate::query::QueryResultData) -> Response {
    let rows: Vec<QueryRow> = result
        .rows
        .iter()
        .map(|r| QueryRow {
            timestamp: r.timestamp,
            values: r.values.clone(),
        })
        .collect();

    let response = QueryResponse {
        columns: result.columns.clone(),
        rows: rows.clone(),
        meta: QueryMeta {
            execution_time_ms: result.execution_time_ms,
            row_count: rows.len(),
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Format response as CSV
fn format_csv_response(result: &crate::query::QueryResultData) -> Response {
    let mut csv = String::new();

    // Header
    csv.push_str("timestamp");
    for col in &result.columns {
        csv.push(',');
        csv.push_str(col);
    }
    csv.push('\n');

    // Rows
    for row in &result.rows {
        csv.push_str(&row.timestamp.to_string());
        for col in &result.columns {
            csv.push(',');
            if let Some(val) = row.values.get(col) {
                csv.push_str(&val.to_string());
            }
        }
        csv.push('\n');
    }

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/csv")],
        csv,
    )
        .into_response()
}

/// Format response for charts
fn format_chart_response(result: &crate::query::QueryResultData) -> Response {
    // Generate labels from timestamps
    let labels: Vec<String> = result
        .rows
        .iter()
        .map(|r| {
            Utc.timestamp_millis_opt(r.timestamp)
                .single()
                .map(|dt| dt.format("%b %d").to_string())
                .unwrap_or_default()
        })
        .collect();

    // Color palette
    let colors = ["#4CAF50", "#2196F3", "#FF9800", "#9C27B0", "#F44336"];

    // Build datasets
    let datasets: Vec<ChartDataset> = result
        .columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let data: Vec<f64> = result
                .rows
                .iter()
                .map(|r| r.values.get(col).copied().unwrap_or(0.0))
                .collect();

            ChartDataset {
                label: col.clone(),
                data,
                color: colors[i % colors.len()].to_string(),
            }
        })
        .collect();

    let response = ChartResponse { labels, datasets };

    (StatusCode::OK, Json(response)).into_response()
}

/// Parse time range from DTO
fn parse_time_range(dto: &TimeRangeDto) -> ApiResult<TimeRange> {
    let start = parse_timestamp(&dto.start)?;
    let end = parse_timestamp(&dto.end)?;

    if start >= end {
        return Err(ApiError::Validation(
            "start must be before end".to_string(),
        ));
    }

    Ok(TimeRange::new(start, end))
}

/// Parse a timestamp string
fn parse_timestamp(s: &str) -> ApiResult<i64> {
    // Try parsing as raw milliseconds timestamp first (most common from frontend)
    if let Ok(ts) = s.parse::<i64>() {
        return Ok(ts);
    }

    // Handle relative times like "now", "now-7d"
    if s.starts_with("now") {
        return parse_relative_time(s);
    }

    // Try parsing as ISO 8601
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp_millis());
    }

    // Try parsing as ISO 8601 without timezone (assume UTC)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc().timestamp_millis());
    }

    // Try parsing as date only
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

/// Parse relative time like "now-7d"
fn parse_relative_time(s: &str) -> ApiResult<i64> {
    let now = Utc::now().timestamp_millis();

    if s == "now" {
        return Ok(now);
    }

    // Parse "now-Nd", "now-Nh", "now-Nw", "now-Nm"
    let re = regex::Regex::new(r"^now-(\d+)([hdwm])$")
        .map_err(|_| ApiError::Internal("Regex error".to_string()))?;

    if let Some(caps) = re.captures(s) {
        let amount: i64 = caps[1]
            .parse()
            .map_err(|_| ApiError::Validation("Invalid number in time expression".to_string()))?;
        let unit = &caps[2];

        let ms = match unit {
            "h" => amount * 3600 * 1000,
            "d" => amount * 24 * 3600 * 1000,
            "w" => amount * 7 * 24 * 3600 * 1000,
            "m" => amount * 30 * 24 * 3600 * 1000,
            _ => {
                return Err(ApiError::Validation(format!(
                    "Invalid time unit: {}",
                    unit
                )))
            }
        };

        return Ok(now - ms);
    }

    Err(ApiError::Validation(format!(
        "Cannot parse relative time: {}",
        s
    )))
}

/// Parse GROUP BY interval
fn parse_group_by(s: &str) -> ApiResult<GroupByInterval> {
    match s.to_lowercase().as_str() {
        "hour" | "h" => Ok(GroupByInterval::Hour),
        "day" | "d" => Ok(GroupByInterval::Day),
        "week" | "w" => Ok(GroupByInterval::Week),
        "month" | "m" => Ok(GroupByInterval::Month),
        _ => Err(ApiError::Validation(format!(
            "Invalid group_by interval: {}. Use hour, day, week, or month",
            s
        ))),
    }
}

/// Parse aggregation function
fn parse_aggregation(s: &str) -> ApiResult<AggregationFunc> {
    match s.to_lowercase().as_str() {
        "avg" | "average" => Ok(AggregationFunc::Avg),
        "sum" => Ok(AggregationFunc::Sum),
        "min" => Ok(AggregationFunc::Min),
        "max" => Ok(AggregationFunc::Max),
        "count" => Ok(AggregationFunc::Count),
        "last" => Ok(AggregationFunc::Last),
        "first" => Ok(AggregationFunc::First),
        _ => Err(ApiError::Validation(format!(
            "Invalid aggregation: {}. Use avg, sum, min, max, count, last, or first",
            s
        ))),
    }
}

/// Parse operator string
fn parse_operator(s: &str) -> ApiResult<Operator> {
    match s.to_lowercase().as_str() {
        "eq" | "=" | "==" => Ok(Operator::Eq),
        "ne" | "!=" | "<>" => Ok(Operator::Ne),
        "gt" | ">" => Ok(Operator::Gt),
        "gte" | ">=" => Ok(Operator::Gte),
        "lt" | "<" => Ok(Operator::Lt),
        "lte" | "<=" => Ok(Operator::Lte),
        _ => Err(ApiError::Validation(format!(
            "Invalid operator: {}. Use eq, ne, gt, gte, lt, or lte",
            s
        ))),
    }
}

/// Parse filter from DTO
fn parse_filter(dto: &FilterDto) -> ApiResult<Filter> {
    let op = parse_operator(&dto.op)?;

    // Try to parse value as number, otherwise use as string
    let value = if let Ok(num) = dto.value.parse::<f64>() {
        FilterValue::Number(num)
    } else {
        FilterValue::String(dto.value.clone())
    };

    Ok(Filter::new(FilterField::Tag(dto.tag.clone()), op, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_time() {
        let now = Utc::now().timestamp_millis();

        let result = parse_relative_time("now").unwrap();
        assert!((result - now).abs() < 1000);

        let result = parse_relative_time("now-7d").unwrap();
        let expected = now - 7 * 24 * 3600 * 1000;
        assert!((result - expected).abs() < 1000);

        let result = parse_relative_time("now-24h").unwrap();
        let expected = now - 24 * 3600 * 1000;
        assert!((result - expected).abs() < 1000);
    }

    #[test]
    fn test_parse_timestamp_iso() {
        let result = parse_timestamp("2024-01-15T10:30:00Z").unwrap();
        assert!(result > 0);
    }

    #[test]
    fn test_parse_group_by() {
        assert!(matches!(parse_group_by("day"), Ok(GroupByInterval::Day)));
        assert!(matches!(parse_group_by("HOUR"), Ok(GroupByInterval::Hour)));
        assert!(parse_group_by("invalid").is_err());
    }

    #[test]
    fn test_parse_aggregation() {
        assert!(matches!(parse_aggregation("avg"), Ok(AggregationFunc::Avg)));
        assert!(matches!(parse_aggregation("SUM"), Ok(AggregationFunc::Sum)));
        assert!(parse_aggregation("invalid").is_err());
    }
}
