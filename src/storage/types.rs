//! Core data types for the Chronicle time-series storage engine
//!
//! This module defines the fundamental types used throughout the storage layer:
//! - `DataPoint`: A single time-series measurement
//! - `Metric`: Definition of what's being measured
//! - `TimeRange`: A time interval for queries
//! - `Category` and `AggregationType`: Classification enums

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single time-series data point
///
/// Represents one measurement at a specific point in time.
/// Size is typically 40-100 bytes depending on tags.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataPoint {
    /// Unix timestamp in milliseconds
    pub timestamp: i64,
    /// Reference to metric definition
    pub metric_id: u32,
    /// The actual measured value
    pub value: f64,
    /// Optional tags for filtering and grouping
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl DataPoint {
    /// Create a new data point with current timestamp
    pub fn new(metric_id: u32, value: f64) -> Self {
        Self {
            timestamp: Utc::now().timestamp_millis(),
            metric_id,
            value,
            tags: HashMap::new(),
        }
    }

    /// Create a data point with a specific timestamp
    pub fn with_timestamp(metric_id: u32, value: f64, timestamp: i64) -> Self {
        Self {
            timestamp,
            metric_id,
            value,
            tags: HashMap::new(),
        }
    }

    /// Builder method: set timestamp
    pub fn timestamp(mut self, timestamp: i64) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Builder method: add a tag
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Builder method: add multiple tags
    pub fn tags(mut self, tags: HashMap<String, String>) -> Self {
        self.tags.extend(tags);
        self
    }

    /// Check if this point has a specific tag value
    pub fn has_tag(&self, key: &str, value: &str) -> bool {
        self.tags.get(key).map(|v| v == value).unwrap_or(false)
    }

    /// Get estimated size in bytes (for buffer management)
    pub fn estimated_size(&self) -> usize {
        // Base: timestamp(8) + metric_id(4) + value(8) = 20 bytes
        // Tags: variable
        let tag_size: usize = self
            .tags
            .iter()
            .map(|(k, v)| k.len() + v.len() + 16) // 16 bytes overhead per entry
            .sum();
        20 + tag_size + 24 // 24 bytes for HashMap overhead
    }
}

/// Category of metric for organization and display
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Physical health metrics (heart rate, steps, sleep)
    Health,
    /// Work and focus metrics (focus time, tasks completed)
    Productivity,
    /// Emotional state metrics (mood, stress, energy)
    Mood,
    /// Habit tracking (meditation, exercise, reading)
    Habit,
    /// User-defined category
    Custom,
}

impl Category {
    /// Get all categories for iteration
    pub fn all() -> &'static [Category] {
        &[
            Category::Health,
            Category::Productivity,
            Category::Mood,
            Category::Habit,
            Category::Custom,
        ]
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Category::Health => write!(f, "health"),
            Category::Productivity => write!(f, "productivity"),
            Category::Mood => write!(f, "mood"),
            Category::Habit => write!(f, "habit"),
            Category::Custom => write!(f, "custom"),
        }
    }
}

/// How to aggregate metric values over time periods
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AggregationType {
    /// Sum values (steps, calories burned)
    Sum,
    /// Average values (mood, energy level)
    Average,
    /// Use last value in period (current weight)
    Last,
    /// Maximum value (peak heart rate)
    Max,
    /// Minimum value (resting heart rate)
    Min,
    /// Count of occurrences
    Count,
}

impl AggregationType {
    /// Aggregate a slice of values according to this type
    pub fn aggregate(&self, values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }

        Some(match self {
            AggregationType::Sum => values.iter().sum(),
            AggregationType::Average => values.iter().sum::<f64>() / values.len() as f64,
            AggregationType::Last => *values.last().unwrap(),
            AggregationType::Max => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            AggregationType::Min => values.iter().cloned().fold(f64::INFINITY, f64::min),
            AggregationType::Count => values.len() as f64,
        })
    }
}

/// Definition of a metric (what's being measured)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Metric {
    /// Unique identifier
    pub id: u32,
    /// Human-readable name (e.g., "mood", "steps", "focus_minutes")
    pub name: String,
    /// Unit of measurement (e.g., "1-10", "steps", "minutes")
    pub unit: String,
    /// Category for organization
    pub category: Category,
    /// How to aggregate over time
    pub aggregation: AggregationType,
    /// Optional description
    #[serde(default)]
    pub description: Option<String>,
    /// Optional min value for validation
    #[serde(default)]
    pub min_value: Option<f64>,
    /// Optional max value for validation
    #[serde(default)]
    pub max_value: Option<f64>,
}

impl Metric {
    /// Create a new metric with required fields
    pub fn new(
        name: impl Into<String>,
        unit: impl Into<String>,
        category: Category,
        aggregation: AggregationType,
    ) -> Self {
        Self {
            id: 0, // Will be assigned by registry
            name: name.into(),
            unit: unit.into(),
            category,
            aggregation,
            description: None,
            min_value: None,
            max_value: None,
        }
    }

    /// Builder: set description
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Builder: set valid range
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min_value = Some(min);
        self.max_value = Some(max);
        self
    }

    /// Validate a value against this metric's constraints
    pub fn validate_value(&self, value: f64) -> bool {
        if let Some(min) = self.min_value {
            if value < min {
                return false;
            }
        }
        if let Some(max) = self.max_value {
            if value > max {
                return false;
            }
        }
        true
    }
}

/// Time range for queries (half-open interval: [start, end))
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    /// Start timestamp (inclusive), in milliseconds
    pub start: i64,
    /// End timestamp (exclusive), in milliseconds
    pub end: i64,
}

impl TimeRange {
    /// Create a new time range
    ///
    /// # Panics
    /// Panics if start >= end
    pub fn new(start: i64, end: i64) -> Self {
        assert!(start < end, "TimeRange: start must be less than end");
        Self { start, end }
    }

    /// Create a time range, returning None if invalid
    pub fn try_new(start: i64, end: i64) -> Option<Self> {
        if start < end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    /// Create a range for the last N hours from now
    pub fn last_hours(hours: i64) -> Self {
        let end = Utc::now().timestamp_millis();
        let start = end - (hours * 3600 * 1000);
        Self { start, end }
    }

    /// Create a range for the last N days from now
    pub fn last_days(days: i64) -> Self {
        Self::last_hours(days * 24)
    }

    /// Create a range for the last N minutes from now
    pub fn last_minutes(minutes: i64) -> Self {
        let end = Utc::now().timestamp_millis();
        let start = end - (minutes * 60 * 1000);
        Self { start, end }
    }

    /// Create a range for a specific day (UTC)
    pub fn day(year: i32, month: u32, day: u32) -> Option<Self> {
        use chrono::{NaiveDate, TimeZone};
        let date = NaiveDate::from_ymd_opt(year, month, day)?;
        let start = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?);
        let end = start + chrono::Duration::days(1);
        Some(Self {
            start: start.timestamp_millis(),
            end: end.timestamp_millis(),
        })
    }

    /// Check if a timestamp falls within this range
    pub fn contains(&self, timestamp: i64) -> bool {
        timestamp >= self.start && timestamp < self.end
    }

    /// Check if this range overlaps with another
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start < other.end && self.end > other.start
    }

    /// Get the duration in milliseconds
    pub fn duration_millis(&self) -> i64 {
        self.end - self.start
    }

    /// Get the duration in seconds
    pub fn duration_secs(&self) -> i64 {
        self.duration_millis() / 1000
    }

    /// Expand this range by a duration on both sides
    pub fn expand(&self, millis: i64) -> Self {
        Self {
            start: self.start - millis,
            end: self.end + millis,
        }
    }

    /// Get intersection with another range, if any
    pub fn intersection(&self, other: &TimeRange) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        Self::try_new(start, end)
    }
}

/// Query filter for data points
#[derive(Debug, Clone, Default)]
pub struct QueryFilter {
    /// Filter by metric ID
    pub metric_id: Option<u32>,
    /// Filter by metric name (alternative to ID)
    pub metric_name: Option<String>,
    /// Filter by tag values
    pub tags: HashMap<String, String>,
    /// Filter by category
    pub category: Option<Category>,
}

impl QueryFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn metric_id(mut self, id: u32) -> Self {
        self.metric_id = Some(id);
        self
    }

    pub fn metric_name(mut self, name: impl Into<String>) -> Self {
        self.metric_name = Some(name.into());
        self
    }

    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    pub fn category(mut self, cat: Category) -> Self {
        self.category = Some(cat);
        self
    }

    /// Check if a data point matches this filter
    pub fn matches(&self, point: &DataPoint, metric: Option<&Metric>) -> bool {
        // Check metric_id
        if let Some(id) = self.metric_id {
            if point.metric_id != id {
                return false;
            }
        }

        // Check tags
        for (key, value) in &self.tags {
            if !point.has_tag(key, value) {
                return false;
            }
        }

        // Check category (requires metric info)
        if let Some(cat) = self.category {
            if let Some(m) = metric {
                if m.category != cat {
                    return false;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_point_creation() {
        let point = DataPoint::new(1, 7.5).tag("location", "home");

        assert_eq!(point.metric_id, 1);
        assert_eq!(point.value, 7.5);
        assert!(point.has_tag("location", "home"));
        assert!(!point.has_tag("location", "work"));
    }

    #[test]
    fn test_data_point_serialization() {
        let point = DataPoint::new(1, 7.5).tag("source", "manual");
        let json = serde_json::to_string(&point).unwrap();
        let restored: DataPoint = serde_json::from_str(&json).unwrap();

        assert_eq!(point.metric_id, restored.metric_id);
        assert_eq!(point.value, restored.value);
        assert_eq!(point.tags, restored.tags);
    }

    #[test]
    fn test_time_range_contains() {
        let range = TimeRange::new(1000, 2000);

        assert!(!range.contains(999));
        assert!(range.contains(1000));
        assert!(range.contains(1500));
        assert!(range.contains(1999));
        assert!(!range.contains(2000));
    }

    #[test]
    fn test_time_range_overlaps() {
        let range1 = TimeRange::new(1000, 2000);
        let range2 = TimeRange::new(1500, 2500);
        let range3 = TimeRange::new(2000, 3000);
        let range4 = TimeRange::new(500, 1500);

        assert!(range1.overlaps(&range2));
        assert!(!range1.overlaps(&range3)); // Adjacent, not overlapping
        assert!(range1.overlaps(&range4));
    }

    #[test]
    fn test_aggregation_types() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

        assert_eq!(AggregationType::Sum.aggregate(&values), Some(15.0));
        assert_eq!(AggregationType::Average.aggregate(&values), Some(3.0));
        assert_eq!(AggregationType::Last.aggregate(&values), Some(5.0));
        assert_eq!(AggregationType::Max.aggregate(&values), Some(5.0));
        assert_eq!(AggregationType::Min.aggregate(&values), Some(1.0));
        assert_eq!(AggregationType::Count.aggregate(&values), Some(5.0));

        // Empty slice
        let empty: Vec<f64> = vec![];
        assert_eq!(AggregationType::Sum.aggregate(&empty), None);
    }

    #[test]
    fn test_metric_validation() {
        let metric = Metric::new("mood", "1-10", Category::Mood, AggregationType::Average)
            .range(1.0, 10.0);

        assert!(metric.validate_value(5.0));
        assert!(metric.validate_value(1.0));
        assert!(metric.validate_value(10.0));
        assert!(!metric.validate_value(0.0));
        assert!(!metric.validate_value(11.0));
    }

    #[test]
    fn test_query_filter() {
        let point = DataPoint::new(1, 7.5).tag("source", "manual");
        let metric = Metric::new("mood", "1-10", Category::Mood, AggregationType::Average);

        // Match by metric_id
        let filter = QueryFilter::new().metric_id(1);
        assert!(filter.matches(&point, Some(&metric)));

        let filter = QueryFilter::new().metric_id(2);
        assert!(!filter.matches(&point, Some(&metric)));

        // Match by tag
        let filter = QueryFilter::new().tag("source", "manual");
        assert!(filter.matches(&point, Some(&metric)));

        let filter = QueryFilter::new().tag("source", "api");
        assert!(!filter.matches(&point, Some(&metric)));
    }
}
