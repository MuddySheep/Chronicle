//! CSV Import
//!
//! Generic CSV file import for Chronicle.
//! Supports flexible column mapping and multiple timestamp formats.

use super::*;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use std::path::Path;

/// CSV file importer with configurable column mapping
pub struct CsvImporter {
    /// Column index for timestamps (0-indexed)
    timestamp_column: usize,
    /// Format string for parsing timestamps
    timestamp_format: String,
    /// Mapping of column indices to metric names
    metric_columns: Vec<(usize, String)>,
    /// Whether the CSV has a header row
    has_header: bool,
}

/// Result of a CSV import operation
#[derive(Debug)]
pub struct CsvImportResult {
    pub points: Vec<(String, DataPoint)>,
    pub rows_processed: usize,
    pub rows_failed: usize,
    pub errors: Vec<String>,
}

impl Default for CsvImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl CsvImporter {
    /// Create a new CSV importer with default settings
    pub fn new() -> Self {
        Self {
            timestamp_column: 0,
            timestamp_format: "%Y-%m-%d".to_string(),
            metric_columns: Vec::new(),
            has_header: true,
        }
    }

    /// Set the timestamp column index
    pub fn with_timestamp_column(mut self, column: usize) -> Self {
        self.timestamp_column = column;
        self
    }

    /// Set the timestamp format string
    pub fn with_timestamp_format(mut self, format: &str) -> Self {
        self.timestamp_format = format.to_string();
        self
    }

    /// Add a metric column mapping
    pub fn with_metric_column(mut self, column: usize, metric_name: &str) -> Self {
        self.metric_columns.push((column, metric_name.to_string()));
        self
    }

    /// Set whether the CSV has a header row
    pub fn with_header(mut self, has_header: bool) -> Self {
        self.has_header = has_header;
        self
    }

    /// Auto-detect column mapping from header row
    pub fn auto_detect_columns(&mut self, headers: &csv::StringRecord) {
        self.metric_columns.clear();

        for (idx, header) in headers.iter().enumerate() {
            let header_lower = header.to_lowercase();

            // Skip timestamp column
            if header_lower.contains("date")
                || header_lower.contains("time")
                || header_lower.contains("timestamp")
            {
                self.timestamp_column = idx;
                continue;
            }

            // Add as metric column
            self.metric_columns.push((idx, header_lower.replace(' ', "_")));
        }
    }

    /// Parse a timestamp string using the configured format
    fn parse_timestamp(&self, ts_str: &str) -> Result<i64, IntegrationError> {
        // Try the configured format first
        if let Ok(dt) = NaiveDateTime::parse_from_str(ts_str, &self.timestamp_format) {
            return Ok(dt.and_utc().timestamp_millis());
        }

        // Try as date only
        if let Ok(date) = NaiveDate::parse_from_str(ts_str, &self.timestamp_format) {
            return Ok(date
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis());
        }

        // Try common formats
        let formats = [
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M:%SZ",
            "%Y-%m-%d",
            "%m/%d/%Y",
            "%d/%m/%Y",
            "%Y/%m/%d",
        ];

        for fmt in formats {
            if let Ok(dt) = NaiveDateTime::parse_from_str(ts_str, fmt) {
                return Ok(dt.and_utc().timestamp_millis());
            }
            if let Ok(date) = NaiveDate::parse_from_str(ts_str, fmt) {
                return Ok(date
                    .and_hms_opt(12, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp_millis());
            }
        }

        // Try RFC 3339
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Ok(dt.with_timezone(&Utc).timestamp_millis());
        }

        Err(IntegrationError::ParseError(format!(
            "Could not parse timestamp: {}",
            ts_str
        )))
    }

    /// Import data from a CSV file
    pub fn import(&self, path: &Path) -> Result<CsvImportResult, IntegrationError> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(self.has_header)
            .flexible(true)
            .from_path(path)?;

        let mut points = Vec::new();
        let mut rows_processed = 0;
        let mut rows_failed = 0;
        let mut errors = Vec::new();

        for (line_num, result) in reader.records().enumerate() {
            let actual_line = if self.has_header {
                line_num + 2
            } else {
                line_num + 1
            };

            let record = match result {
                Ok(r) => r,
                Err(e) => {
                    errors.push(format!("Line {}: {}", actual_line, e));
                    rows_failed += 1;
                    continue;
                }
            };

            // Parse timestamp
            let ts_str = match record.get(self.timestamp_column) {
                Some(s) => s.trim(),
                None => {
                    errors.push(format!("Line {}: missing timestamp column", actual_line));
                    rows_failed += 1;
                    continue;
                }
            };

            let timestamp = match self.parse_timestamp(ts_str) {
                Ok(ts) => ts,
                Err(e) => {
                    errors.push(format!("Line {}: {}", actual_line, e));
                    rows_failed += 1;
                    continue;
                }
            };

            // Parse each metric column
            let mut row_has_data = false;
            for (col_idx, metric_name) in &self.metric_columns {
                if let Some(value_str) = record.get(*col_idx) {
                    let value_str = value_str.trim();
                    if value_str.is_empty() {
                        continue;
                    }

                    match value_str.parse::<f64>() {
                        Ok(value) => {
                            let point = DataPoint::new(0, value).timestamp(timestamp);
                            points.push((metric_name.clone(), point));
                            row_has_data = true;
                        }
                        Err(_) => {
                            // Not a valid number, skip silently
                        }
                    }
                }
            }

            if row_has_data {
                rows_processed += 1;
            }
        }

        // Truncate errors if too many
        if errors.len() > 100 {
            let total = errors.len();
            errors.truncate(100);
            errors.push(format!("... and {} more errors", total - 100));
        }

        Ok(CsvImportResult {
            points,
            rows_processed,
            rows_failed,
            errors,
        })
    }

    /// Import from a CSV string (useful for testing)
    pub fn import_str(&self, csv_data: &str) -> Result<CsvImportResult, IntegrationError> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(self.has_header)
            .flexible(true)
            .from_reader(csv_data.as_bytes());

        let mut points = Vec::new();
        let mut rows_processed = 0;
        let mut rows_failed = 0;
        let mut errors = Vec::new();

        for (line_num, result) in reader.records().enumerate() {
            let actual_line = if self.has_header {
                line_num + 2
            } else {
                line_num + 1
            };

            let record = match result {
                Ok(r) => r,
                Err(e) => {
                    errors.push(format!("Line {}: {}", actual_line, e));
                    rows_failed += 1;
                    continue;
                }
            };

            // Parse timestamp
            let ts_str = match record.get(self.timestamp_column) {
                Some(s) => s.trim(),
                None => {
                    errors.push(format!("Line {}: missing timestamp column", actual_line));
                    rows_failed += 1;
                    continue;
                }
            };

            let timestamp = match self.parse_timestamp(ts_str) {
                Ok(ts) => ts,
                Err(e) => {
                    errors.push(format!("Line {}: {}", actual_line, e));
                    rows_failed += 1;
                    continue;
                }
            };

            // Parse each metric column
            let mut row_has_data = false;
            for (col_idx, metric_name) in &self.metric_columns {
                if let Some(value_str) = record.get(*col_idx) {
                    let value_str = value_str.trim();
                    if value_str.is_empty() {
                        continue;
                    }

                    match value_str.parse::<f64>() {
                        Ok(value) => {
                            let point = DataPoint::new(0, value).timestamp(timestamp);
                            points.push((metric_name.clone(), point));
                            row_has_data = true;
                        }
                        Err(_) => {
                            // Not a valid number, skip silently
                        }
                    }
                }
            }

            if row_has_data {
                rows_processed += 1;
            }
        }

        Ok(CsvImportResult {
            points,
            rows_processed,
            rows_failed,
            errors,
        })
    }
}

/// Parse a simple format CSV with auto-detection
pub fn import_simple_csv(path: &Path) -> Result<CsvImportResult, IntegrationError> {
    // First, read the header to detect columns
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader
        .headers()
        .map_err(|e| IntegrationError::ParseError(e.to_string()))?
        .clone();

    let mut importer = CsvImporter::new();
    importer.auto_detect_columns(&headers);

    importer.import(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_csv_import() {
        let csv_data = "date,mood,energy
2024-01-15,7.5,6.0
2024-01-16,8.0,7.0
2024-01-17,6.5,5.5";

        let importer = CsvImporter::new()
            .with_timestamp_column(0)
            .with_timestamp_format("%Y-%m-%d")
            .with_metric_column(1, "mood")
            .with_metric_column(2, "energy");

        let result = importer.import_str(csv_data).unwrap();

        assert_eq!(result.rows_processed, 3);
        assert_eq!(result.rows_failed, 0);
        assert_eq!(result.points.len(), 6); // 2 metrics * 3 rows
    }

    #[test]
    fn test_csv_with_missing_values() {
        let csv_data = "date,mood,energy
2024-01-15,7.5,
2024-01-16,,7.0
2024-01-17,6.5,5.5";

        let importer = CsvImporter::new()
            .with_timestamp_column(0)
            .with_timestamp_format("%Y-%m-%d")
            .with_metric_column(1, "mood")
            .with_metric_column(2, "energy");

        let result = importer.import_str(csv_data).unwrap();

        assert_eq!(result.rows_processed, 3);
        assert_eq!(result.points.len(), 4); // Some values missing
    }
}
