//! Apple Health Import Routes
//!
//! Endpoints for importing Apple Health export data.
//!
//! - POST /api/v1/import/apple-health - Import Apple Health ZIP export

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use crate::api::error::{ApiError, ApiResult};
use crate::api::state::AppState;
use crate::storage::{AggregationType, Category, DataPoint, Metric};
use crate::websocket::WsEvent;

/// Request body for Apple Health import
#[derive(Debug, serde::Deserialize)]
pub struct AppleHealthImportRequest {
    /// Base64-encoded ZIP file data
    pub data: String,
    /// Format hint (should be "zip")
    pub format: String,
}

/// Response from Apple Health import
#[derive(Debug, serde::Serialize)]
pub struct AppleHealthImportResponse {
    pub imported_count: usize,
    pub metrics_created: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// POST /api/v1/import/apple-health
///
/// Import Apple Health export ZIP file.
pub async fn import_apple_health(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AppleHealthImportRequest>,
) -> ApiResult<(StatusCode, Json<AppleHealthImportResponse>)> {
    if req.format != "zip" {
        return Err(ApiError::Validation("Format must be 'zip'".to_string()));
    }

    // Decode base64
    let zip_data = base64_decode(&req.data)
        .map_err(|e| ApiError::Validation(format!("Invalid base64 data: {}", e)))?;

    // Parse ZIP and extract health data
    let health_records = parse_apple_health_zip(&zip_data)
        .map_err(|e| ApiError::Internal(format!("Failed to parse Apple Health export: {}", e)))?;

    let mut imported_count = 0;
    let mut metrics_created = Vec::new();
    let mut errors = Vec::new();

    // Group records by metric type
    let mut metric_ids: HashMap<String, u32> = HashMap::new();

    for record in health_records {
        // Resolve or create metric
        let metric_id = match metric_ids.get(&record.metric_name) {
            Some(id) => *id,
            None => {
                let id = resolve_or_create_metric(&state, &record.metric_name, &record.unit).await?;
                if !metric_ids.contains_key(&record.metric_name) {
                    metrics_created.push(record.metric_name.clone());
                }
                metric_ids.insert(record.metric_name.clone(), id);
                id
            }
        };

        // Create DataPoint
        let mut point = DataPoint::with_timestamp(metric_id, record.value, record.timestamp);
        for (k, v) in &record.tags {
            point = point.tag(k, v);
        }

        // Write to storage
        match state.storage.write(point).await {
            Ok(_) => {
                imported_count += 1;

                // Publish to WebSocket
                let event = WsEvent::data_point(
                    &record.metric_name,
                    record.value,
                    record.timestamp,
                    record.tags.clone(),
                );
                state.ws_hub.publish(event);
            }
            Err(e) => {
                errors.push(format!("Failed to write record: {}", e));
            }
        }
    }

    tracing::info!(
        imported = imported_count,
        metrics = metrics_created.len(),
        errors = errors.len(),
        "Apple Health import completed"
    );

    Ok((
        StatusCode::OK,
        Json(AppleHealthImportResponse {
            imported_count,
            metrics_created,
            errors,
        }),
    ))
}

/// A parsed health record from Apple Health export
#[derive(Debug)]
struct HealthRecord {
    metric_name: String,
    value: f64,
    timestamp: i64,
    unit: String,
    tags: HashMap<String, String>,
}

/// Parse Apple Health export ZIP file and extract health records
fn parse_apple_health_zip(data: &[u8]) -> Result<Vec<HealthRecord>, String> {
    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| format!("Invalid ZIP file: {}", e))?;

    // Look for export.xml in the archive
    let mut xml_content = String::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

        let name = file.name().to_string();
        if name.ends_with("export.xml") || name.ends_with("Export.xml") {
            file.read_to_string(&mut xml_content)
                .map_err(|e| format!("Failed to read export.xml: {}", e))?;
            break;
        }
    }

    if xml_content.is_empty() {
        return Err("No export.xml found in ZIP file".to_string());
    }

    // Parse the XML and extract health records
    parse_health_xml(&xml_content)
}

/// Parse Apple Health export XML content
fn parse_health_xml(xml: &str) -> Result<Vec<HealthRecord>, String> {
    let mut records = Vec::new();

    // Simple XML parsing - look for Record elements
    // Apple Health XML format:
    // <Record type="HKQuantityTypeIdentifierHeartRate" sourceName="Apple Watch"
    //         unit="count/min" creationDate="2024-01-15" startDate="2024-01-15"
    //         endDate="2024-01-15" value="72"/>

    for line in xml.lines() {
        let line = line.trim();
        if !line.starts_with("<Record ") {
            continue;
        }

        // Extract attributes
        let record_type = extract_attr(line, "type").unwrap_or_default();
        let value_str = extract_attr(line, "value").unwrap_or_default();
        let unit = extract_attr(line, "unit").unwrap_or_default();
        let start_date = extract_attr(line, "startDate").unwrap_or_default();
        let source = extract_attr(line, "sourceName").unwrap_or_default();

        // Parse value
        let value: f64 = match value_str.parse() {
            Ok(v) => v,
            Err(_) => continue, // Skip non-numeric values
        };

        // Parse timestamp
        let timestamp = parse_apple_date(&start_date).unwrap_or_else(|| Utc::now().timestamp_millis());

        // Convert Apple Health type to metric name
        let metric_name = convert_health_type(&record_type);

        // Build tags
        let mut tags = HashMap::new();
        if !source.is_empty() {
            tags.insert("source".to_string(), source);
        }
        tags.insert("apple_type".to_string(), record_type);

        records.push(HealthRecord {
            metric_name,
            value,
            timestamp,
            unit,
            tags,
        });
    }

    Ok(records)
}

/// Extract an attribute value from an XML element string
fn extract_attr(element: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = element.find(&pattern)? + pattern.len();
    let rest = &element[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Parse Apple Health date format to Unix timestamp (milliseconds)
fn parse_apple_date(date_str: &str) -> Option<i64> {
    // Apple Health uses format like: 2024-01-15 10:30:00 -0500
    // or ISO8601 format

    // Try parsing with chrono
    if let Ok(dt) = chrono::DateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S %z") {
        return Some(dt.timestamp_millis());
    }

    // Try ISO8601
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date_str) {
        return Some(dt.timestamp_millis());
    }

    // Try date only
    if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let dt = date.and_hms_opt(0, 0, 0)?;
        return Some(dt.and_utc().timestamp_millis());
    }

    None
}

/// Convert Apple Health type identifier to a user-friendly metric name
fn convert_health_type(health_type: &str) -> String {
    // Remove "HKQuantityTypeIdentifier" or "HKCategoryTypeIdentifier" prefix
    let clean = health_type
        .strip_prefix("HKQuantityTypeIdentifier")
        .or_else(|| health_type.strip_prefix("HKCategoryTypeIdentifier"))
        .unwrap_or(health_type);

    // Convert CamelCase to snake_case and lowercase
    let mut result = String::new();
    for (i, c) in clean.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }

    // Map common types to Chronicle metric names
    match result.as_str() {
        "heart_rate" => "heart_rate".to_string(),
        "step_count" => "steps".to_string(),
        "distance_walking_running" => "distance".to_string(),
        "active_energy_burned" => "calories_active".to_string(),
        "basal_energy_burned" => "calories_basal".to_string(),
        "sleep_analysis" => "sleep".to_string(),
        "body_mass" => "weight".to_string(),
        "body_mass_index" => "bmi".to_string(),
        "height" => "height".to_string(),
        "blood_pressure_systolic" => "blood_pressure_systolic".to_string(),
        "blood_pressure_diastolic" => "blood_pressure_diastolic".to_string(),
        "oxygen_saturation" => "spo2".to_string(),
        "respiratory_rate" => "respiratory_rate".to_string(),
        "vo2_max" => "vo2_max".to_string(),
        "resting_heart_rate" => "resting_heart_rate".to_string(),
        "walking_heart_rate_average" => "walking_heart_rate".to_string(),
        "heart_rate_variability_sdnn" => "hrv".to_string(),
        "flights_climbed" => "flights_climbed".to_string(),
        "apple_stand_hour" => "stand_hours".to_string(),
        "apple_exercise_time" => "exercise_minutes".to_string(),
        "mindful_session" => "mindfulness_minutes".to_string(),
        "dietary_water" => "water".to_string(),
        "dietary_caffeine" => "caffeine".to_string(),
        _ => result,
    }
}

/// Resolve or create a metric
async fn resolve_or_create_metric(state: &AppState, name: &str, unit: &str) -> ApiResult<u32> {
    if let Some(metric) = state.storage.get_metric(name).await {
        return Ok(metric.id);
    }

    // Determine category based on metric name
    // Available categories: Health, Productivity, Mood, Habit, Custom
    let category = if name.contains("heart") || name.contains("blood") || name.contains("spo2")
        || name.contains("step") || name.contains("distance") || name.contains("calories")
        || name.contains("weight") || name.contains("bmi") || name.contains("height")
        || name.contains("sleep") || name.contains("vo2") || name.contains("respiratory")
    {
        Category::Health
    } else if name.contains("exercise") || name.contains("workout") {
        Category::Habit
    } else if name.contains("energy") || name.contains("mood") {
        Category::Mood
    } else {
        Category::Custom
    };

    let metric = Metric::new(name, unit, category, AggregationType::Average);
    let id = state.storage.register_metric(metric).await?;
    tracing::info!(metric_name = %name, metric_id = id, "Created metric from Apple Health import");
    Ok(id)
}

/// Simple base64 decoding
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn decode_char(c: u8) -> Option<u8> {
        ALPHABET.iter().position(|&x| x == c).map(|p| p as u8)
    }

    let input = input.trim();
    let input: String = input.chars().filter(|c| !c.is_whitespace()).collect();

    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut result = Vec::new();
    let bytes: Vec<u8> = input.bytes().collect();

    let mut i = 0;
    while i < bytes.len() {
        let b0 = if bytes[i] == b'=' { 0 } else { decode_char(bytes[i]).ok_or("Invalid base64")? };
        let b1 = if i + 1 >= bytes.len() || bytes[i + 1] == b'=' { 0 } else { decode_char(bytes[i + 1]).ok_or("Invalid base64")? };
        let b2 = if i + 2 >= bytes.len() || bytes[i + 2] == b'=' { 0 } else { decode_char(bytes[i + 2]).ok_or("Invalid base64")? };
        let b3 = if i + 3 >= bytes.len() || bytes[i + 3] == b'=' { 0 } else { decode_char(bytes[i + 3]).ok_or("Invalid base64")? };

        result.push((b0 << 2) | (b1 >> 4));

        if i + 2 < bytes.len() && bytes[i + 2] != b'=' {
            result.push((b1 << 4) | (b2 >> 2));
        }

        if i + 3 < bytes.len() && bytes[i + 3] != b'=' {
            result.push((b2 << 6) | b3);
        }

        i += 4;
    }

    Ok(result)
}
