//! Global Application State
//!
//! Reactive state management using Leptos signals.

use leptos::*;
use std::collections::HashMap;

/// Global application state provided to all components
#[derive(Clone)]
pub struct GlobalState {
    /// Available metrics from the API
    pub metrics: RwSignal<Vec<Metric>>,
    /// Currently selected metrics for display
    pub selected_metrics: RwSignal<Vec<String>>,
    /// Current time range for queries
    pub time_range: RwSignal<TimeRange>,
    /// Chart data keyed by metric name
    pub chart_data: RwSignal<HashMap<String, Vec<DataPoint>>>,
    /// WebSocket connection status
    pub ws_connected: RwSignal<bool>,
    /// Last sync timestamp
    pub last_sync: RwSignal<Option<i64>>,
    /// Global loading state
    pub loading: RwSignal<bool>,
    /// Error message to display
    pub error: RwSignal<Option<String>>,
    /// Success message (for toasts)
    pub success: RwSignal<Option<String>>,
}

/// Metric definition from the API
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct Metric {
    pub id: u32,
    pub name: String,
    pub unit: String,
    pub category: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub min_value: Option<f64>,
    #[serde(default)]
    pub max_value: Option<f64>,
}

/// A single data point for charting
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct DataPoint {
    pub timestamp: i64,
    pub value: f64,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

/// Time range for queries
#[derive(Clone, Debug, PartialEq)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
    pub label: String,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self::all_time()
    }
}

impl TimeRange {
    /// Create a time range that covers all data (from epoch to now)
    pub fn all_time() -> Self {
        let end = chrono::Utc::now().timestamp_millis();
        let start = 0; // From epoch - covers all historical data
        Self {
            start,
            end,
            label: "All Time".to_string(),
        }
    }

    /// Create a time range for the last N days
    pub fn last_days(days: i64) -> Self {
        let end = chrono::Utc::now().timestamp_millis();
        let start = end - (days * 24 * 60 * 60 * 1000);
        Self {
            start,
            end,
            label: if days == 1 {
                "Today".to_string()
            } else {
                format!("Last {} days", days)
            },
        }
    }

    /// Create a time range for a specific month
    pub fn month(year: i32, month: u32) -> Self {
        use chrono::{Datelike, TimeZone, Utc};

        let start_date = Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).unwrap();
        let end_date = if month == 12 {
            Utc.with_ymd_and_hms(year + 1, 1, 1, 0, 0, 0).unwrap()
        } else {
            Utc.with_ymd_and_hms(year, month + 1, 1, 0, 0, 0).unwrap()
        };

        Self {
            start: start_date.timestamp_millis(),
            end: end_date.timestamp_millis(),
            label: format!("{} {}",
                match month {
                    1 => "January", 2 => "February", 3 => "March",
                    4 => "April", 5 => "May", 6 => "June",
                    7 => "July", 8 => "August", 9 => "September",
                    10 => "October", 11 => "November", 12 => "December",
                    _ => "Unknown",
                },
                year
            ),
        }
    }

    /// Duration in milliseconds
    pub fn duration_ms(&self) -> i64 {
        self.end - self.start
    }

    /// Duration in days
    pub fn duration_days(&self) -> i64 {
        self.duration_ms() / (24 * 60 * 60 * 1000)
    }
}

/// Provide global state to the component tree
pub fn provide_global_state() {
    let state = GlobalState {
        metrics: create_rw_signal(Vec::new()),
        selected_metrics: create_rw_signal(vec![
            "heart_rate".to_string(),
            "steps".to_string(),
        ]),
        time_range: create_rw_signal(TimeRange::default()),
        chart_data: create_rw_signal(HashMap::new()),
        ws_connected: create_rw_signal(false),
        last_sync: create_rw_signal(None),
        loading: create_rw_signal(false),
        error: create_rw_signal(None),
        success: create_rw_signal(None),
    };

    provide_context(state);
}

impl GlobalState {
    /// Get the current value for a metric
    pub fn current_value(&self, metric_name: &str) -> Option<f64> {
        self.chart_data.get()
            .get(metric_name)
            .and_then(|points| points.last())
            .map(|p| p.value)
    }

    /// Get the average value for a metric
    pub fn average_value(&self, metric_name: &str) -> Option<f64> {
        let data = self.chart_data.get();
        let points = data.get(metric_name)?;
        if points.is_empty() {
            return None;
        }
        let sum: f64 = points.iter().map(|p| p.value).sum();
        Some(sum / points.len() as f64)
    }

    /// Get value change compared to average
    pub fn value_vs_average(&self, metric_name: &str) -> Option<f64> {
        let current = self.current_value(metric_name)?;
        let avg = self.average_value(metric_name)?;
        Some(current - avg)
    }

    /// Add a new data point to a metric
    pub fn add_data_point(&self, metric_name: &str, point: DataPoint) {
        self.chart_data.update(|data| {
            data.entry(metric_name.to_string())
                .or_insert_with(Vec::new)
                .push(point);
        });
    }

    /// Show a success message (auto-clears after timeout)
    pub fn show_success(&self, message: &str) {
        self.success.set(Some(message.to_string()));

        let success_signal = self.success;
        gloo_timers::callback::Timeout::new(3000, move || {
            success_signal.set(None);
        }).forget();
    }

    /// Show an error message (auto-clears after timeout)
    pub fn show_error(&self, message: &str) {
        self.error.set(Some(message.to_string()));

        let error_signal = self.error;
        gloo_timers::callback::Timeout::new(5000, move || {
            error_signal.set(None);
        }).forget();
    }

    /// Clear error message
    pub fn clear_error(&self) {
        self.error.set(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_range_default() {
        let range = TimeRange::default();
        assert_eq!(range.duration_days(), 7);
    }

    #[test]
    fn test_time_range_last_days() {
        let range = TimeRange::last_days(30);
        assert_eq!(range.duration_days(), 30);
        assert_eq!(range.label, "Last 30 days");
    }
}
