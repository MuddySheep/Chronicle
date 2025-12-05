//! Query Abstract Syntax Tree
//!
//! Defines the AST for Chronicle Query Language (CQL), a SQL-like query language
//! optimized for time-series data queries.
//!
//! # Example Queries
//!
//! ```text
//! SELECT mood WHERE time >= now() - 7d
//! SELECT AVG(mood) GROUP BY day
//! SELECT mood, energy WHERE tags.location = 'office'
//! ```

use crate::storage::TimeRange;
use chrono::{Datelike, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

/// A parsed query ready for execution
#[derive(Debug, Clone)]
pub struct Query {
    /// Metrics/columns to select
    pub select: Vec<SelectItem>,
    /// Time range to query
    pub time_range: TimeRange,
    /// Filters to apply
    pub filters: Vec<Filter>,
    /// Optional grouping clause
    pub group_by: Option<GroupByClause>,
    /// Optional limit on results
    pub limit: Option<usize>,
}

impl Query {
    /// Start building a query with SELECT clause
    pub fn select(metrics: &[&str]) -> QueryBuilder {
        QueryBuilder::new(metrics)
    }

    /// Create a simple query for a single metric
    pub fn metric(name: &str) -> QueryBuilder {
        QueryBuilder::new(&[name])
    }
}

/// An item in the SELECT clause
#[derive(Debug, Clone, PartialEq)]
pub struct SelectItem {
    /// Metric name to select
    pub metric: String,
    /// Optional aggregation function
    pub aggregation: Option<AggregationFunc>,
    /// Optional alias for the result column
    pub alias: Option<String>,
}

impl SelectItem {
    /// Create a new select item for a metric
    pub fn new(metric: impl Into<String>) -> Self {
        Self {
            metric: metric.into(),
            aggregation: None,
            alias: None,
        }
    }

    /// Add an aggregation function
    pub fn with_aggregation(mut self, agg: AggregationFunc) -> Self {
        self.aggregation = Some(agg);
        self
    }

    /// Add an alias
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Get the display name (alias or metric name)
    pub fn display_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.metric)
    }
}

/// Aggregation functions available in queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AggregationFunc {
    /// Average of values
    Avg,
    /// Sum of values
    Sum,
    /// Minimum value
    Min,
    /// Maximum value
    Max,
    /// Count of values
    Count,
    /// Last value in the group
    Last,
    /// First value in the group
    First,
}

impl AggregationFunc {
    /// Apply aggregation to a slice of values
    pub fn apply(&self, values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }

        Some(match self {
            Self::Avg => values.iter().sum::<f64>() / values.len() as f64,
            Self::Sum => values.iter().sum(),
            Self::Min => values.iter().cloned().fold(f64::INFINITY, f64::min),
            Self::Max => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            Self::Count => values.len() as f64,
            Self::Last => *values.last()?,
            Self::First => *values.first()?,
        })
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "avg" | "average" => Some(Self::Avg),
            "sum" => Some(Self::Sum),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "count" => Some(Self::Count),
            "last" => Some(Self::Last),
            "first" => Some(Self::First),
            _ => None,
        }
    }
}

impl std::fmt::Display for AggregationFunc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Avg => write!(f, "AVG"),
            Self::Sum => write!(f, "SUM"),
            Self::Min => write!(f, "MIN"),
            Self::Max => write!(f, "MAX"),
            Self::Count => write!(f, "COUNT"),
            Self::Last => write!(f, "LAST"),
            Self::First => write!(f, "FIRST"),
        }
    }
}

/// A filter condition in the WHERE clause
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    /// Field to filter on
    pub field: FilterField,
    /// Comparison operator
    pub op: Operator,
    /// Value to compare against
    pub value: FilterValue,
}

impl Filter {
    /// Create a new filter
    pub fn new(field: FilterField, op: Operator, value: FilterValue) -> Self {
        Self { field, op, value }
    }

    /// Create a tag filter
    pub fn tag(key: impl Into<String>, op: Operator, value: impl Into<String>) -> Self {
        Self {
            field: FilterField::Tag(key.into()),
            op,
            value: FilterValue::String(value.into()),
        }
    }

    /// Create a value filter
    pub fn value(op: Operator, value: f64) -> Self {
        Self {
            field: FilterField::Value,
            op,
            value: FilterValue::Number(value),
        }
    }
}

/// Fields that can be filtered
#[derive(Debug, Clone, PartialEq)]
pub enum FilterField {
    /// Filter by metric name
    Metric,
    /// Filter by a tag value
    Tag(String),
    /// Filter by the data point value
    Value,
}

/// Comparison operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// Equal to
    Eq,
    /// Not equal to
    Ne,
    /// Greater than
    Gt,
    /// Greater than or equal to
    Gte,
    /// Less than
    Lt,
    /// Less than or equal to
    Lte,
}

impl Operator {
    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "=" | "==" => Some(Self::Eq),
            "!=" | "<>" => Some(Self::Ne),
            ">" => Some(Self::Gt),
            ">=" => Some(Self::Gte),
            "<" => Some(Self::Lt),
            "<=" => Some(Self::Lte),
            _ => None,
        }
    }

    /// Compare two f64 values
    pub fn compare_f64(&self, a: f64, b: f64) -> bool {
        match self {
            Self::Eq => (a - b).abs() < f64::EPSILON,
            Self::Ne => (a - b).abs() >= f64::EPSILON,
            Self::Gt => a > b,
            Self::Gte => a >= b,
            Self::Lt => a < b,
            Self::Lte => a <= b,
        }
    }

    /// Compare two strings
    pub fn compare_str(&self, a: &str, b: &str) -> bool {
        match self {
            Self::Eq => a == b,
            Self::Ne => a != b,
            // String comparisons for ordering (lexicographic)
            Self::Gt => a > b,
            Self::Gte => a >= b,
            Self::Lt => a < b,
            Self::Lte => a <= b,
        }
    }
}

impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eq => write!(f, "="),
            Self::Ne => write!(f, "!="),
            Self::Gt => write!(f, ">"),
            Self::Gte => write!(f, ">="),
            Self::Lt => write!(f, "<"),
            Self::Lte => write!(f, "<="),
        }
    }
}

/// Values used in filter comparisons
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    /// String value
    String(String),
    /// Numeric value
    Number(f64),
}

/// GROUP BY clause specification
#[derive(Debug, Clone, PartialEq)]
pub struct GroupByClause {
    /// Time interval to group by
    pub interval: GroupByInterval,
}

impl GroupByClause {
    /// Create a new GROUP BY clause
    pub fn new(interval: GroupByInterval) -> Self {
        Self { interval }
    }
}

/// Time intervals for grouping
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupByInterval {
    /// Group by hour
    Hour,
    /// Group by day
    Day,
    /// Group by week (starts on Monday)
    Week,
    /// Group by calendar month
    Month,
}

impl GroupByInterval {
    /// Truncate a timestamp to the start of this interval
    ///
    /// # Arguments
    /// * `timestamp` - Unix timestamp in milliseconds
    ///
    /// # Returns
    /// The timestamp truncated to the start of the interval
    pub fn truncate(&self, timestamp: i64) -> i64 {
        let dt = match Utc.timestamp_millis_opt(timestamp) {
            chrono::LocalResult::Single(dt) => dt,
            _ => return timestamp,
        };

        let truncated = match self {
            Self::Hour => dt
                .with_minute(0)
                .and_then(|d| d.with_second(0))
                .and_then(|d| d.with_nanosecond(0))
                .unwrap_or(dt),
            Self::Day => dt
                .with_hour(0)
                .and_then(|d| d.with_minute(0))
                .and_then(|d| d.with_second(0))
                .and_then(|d| d.with_nanosecond(0))
                .unwrap_or(dt),
            Self::Week => {
                let days_since_monday = dt.weekday().num_days_from_monday() as i64;
                let monday = dt - chrono::Duration::days(days_since_monday);
                monday
                    .with_hour(0)
                    .and_then(|d| d.with_minute(0))
                    .and_then(|d| d.with_second(0))
                    .and_then(|d| d.with_nanosecond(0))
                    .unwrap_or(monday)
            }
            Self::Month => dt
                .with_day(1)
                .and_then(|d| d.with_hour(0))
                .and_then(|d| d.with_minute(0))
                .and_then(|d| d.with_second(0))
                .and_then(|d| d.with_nanosecond(0))
                .unwrap_or(dt),
        };

        truncated.timestamp_millis()
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "hour" | "h" => Some(Self::Hour),
            "day" | "d" => Some(Self::Day),
            "week" | "w" => Some(Self::Week),
            "month" | "m" => Some(Self::Month),
            _ => None,
        }
    }

    /// Get the duration in milliseconds (approximate for variable intervals)
    pub fn approx_duration_ms(&self) -> i64 {
        match self {
            Self::Hour => 3600 * 1000,
            Self::Day => 24 * 3600 * 1000,
            Self::Week => 7 * 24 * 3600 * 1000,
            Self::Month => 30 * 24 * 3600 * 1000,
        }
    }
}

impl std::fmt::Display for GroupByInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hour => write!(f, "hour"),
            Self::Day => write!(f, "day"),
            Self::Week => write!(f, "week"),
            Self::Month => write!(f, "month"),
        }
    }
}

/// Builder for constructing queries programmatically
#[derive(Debug, Clone)]
pub struct QueryBuilder {
    select: Vec<SelectItem>,
    time_range: Option<TimeRange>,
    filters: Vec<Filter>,
    group_by: Option<GroupByClause>,
    limit: Option<usize>,
}

impl QueryBuilder {
    /// Create a new query builder with the given metrics
    pub fn new(metrics: &[&str]) -> Self {
        Self {
            select: metrics
                .iter()
                .map(|m| SelectItem::new(*m))
                .collect(),
            time_range: None,
            filters: Vec::new(),
            group_by: None,
            limit: None,
        }
    }

    /// Set an explicit time range
    pub fn time_range(mut self, range: TimeRange) -> Self {
        self.time_range = Some(range);
        self
    }

    /// Query the last N days
    pub fn last_days(self, days: i64) -> Self {
        self.time_range(TimeRange::last_days(days))
    }

    /// Query the last N hours
    pub fn last_hours(self, hours: i64) -> Self {
        self.time_range(TimeRange::last_hours(hours))
    }

    /// Query the last N minutes
    pub fn last_minutes(self, minutes: i64) -> Self {
        self.time_range(TimeRange::last_minutes(minutes))
    }

    /// Add a GROUP BY clause
    pub fn group_by(mut self, interval: GroupByInterval) -> Self {
        self.group_by = Some(GroupByClause::new(interval));
        self
    }

    /// Apply an aggregation to all select items
    pub fn with_aggregation(mut self, agg: AggregationFunc) -> Self {
        for item in &mut self.select {
            item.aggregation = Some(agg);
        }
        self
    }

    /// Add a filter
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Add a tag filter
    pub fn filter_tag(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.filter(Filter::tag(key, Operator::Eq, value))
    }

    /// Add a value filter
    pub fn filter_value(self, op: Operator, value: f64) -> Self {
        self.filter(Filter::value(op, value))
    }

    /// Set a limit on results
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Build the query
    pub fn build(self) -> Query {
        Query {
            select: self.select,
            time_range: self.time_range.unwrap_or_else(|| TimeRange::last_days(7)),
            filters: self.filters,
            group_by: self.group_by,
            limit: self.limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_builder_basic() {
        let query = Query::select(&["mood"]).last_days(7).build();

        assert_eq!(query.select.len(), 1);
        assert_eq!(query.select[0].metric, "mood");
        assert!(query.select[0].aggregation.is_none());
        assert!(query.group_by.is_none());
    }

    #[test]
    fn test_query_builder_with_aggregation() {
        let query = Query::select(&["mood"])
            .last_days(30)
            .group_by(GroupByInterval::Day)
            .with_aggregation(AggregationFunc::Avg)
            .build();

        assert_eq!(query.select[0].aggregation, Some(AggregationFunc::Avg));
        assert_eq!(
            query.group_by.as_ref().map(|g| g.interval),
            Some(GroupByInterval::Day)
        );
    }

    #[test]
    fn test_query_builder_multiple_metrics() {
        let query = Query::select(&["mood", "energy", "focus"])
            .last_hours(24)
            .build();

        assert_eq!(query.select.len(), 3);
        assert_eq!(query.select[0].metric, "mood");
        assert_eq!(query.select[1].metric, "energy");
        assert_eq!(query.select[2].metric, "focus");
    }

    #[test]
    fn test_query_builder_with_filters() {
        let query = Query::select(&["mood"])
            .last_days(7)
            .filter_tag("location", "office")
            .filter_value(Operator::Gte, 5.0)
            .build();

        assert_eq!(query.filters.len(), 2);
    }

    #[test]
    fn test_group_by_truncate_hour() {
        // 2024-01-15 14:35:42.123 UTC
        let timestamp = 1705329342123_i64;
        let truncated = GroupByInterval::Hour.truncate(timestamp);

        // Should be 2024-01-15 14:00:00.000 UTC
        let expected = 1705327200000_i64;
        assert_eq!(truncated, expected);
    }

    #[test]
    fn test_group_by_truncate_day() {
        // 2024-01-15 14:35:42.123 UTC
        let timestamp = 1705329342123_i64;
        let truncated = GroupByInterval::Day.truncate(timestamp);

        // Should be 2024-01-15 00:00:00.000 UTC
        let expected = 1705276800000_i64;
        assert_eq!(truncated, expected);
    }

    #[test]
    fn test_group_by_truncate_week() {
        // 2024-01-15 (Monday) 14:35:42.123 UTC
        let timestamp = 1705329342123_i64;
        let truncated = GroupByInterval::Week.truncate(timestamp);

        // Should be 2024-01-15 00:00:00.000 UTC (Monday)
        let expected = 1705276800000_i64;
        assert_eq!(truncated, expected);

        // 2024-01-17 (Wednesday) 14:35:42.123 UTC
        let timestamp2 = 1705502142123_i64;
        let truncated2 = GroupByInterval::Week.truncate(timestamp2);

        // Should still be 2024-01-15 00:00:00.000 UTC (Monday)
        assert_eq!(truncated2, expected);
    }

    #[test]
    fn test_group_by_truncate_month() {
        // 2024-01-15 14:35:42.123 UTC
        let timestamp = 1705329342123_i64;
        let truncated = GroupByInterval::Month.truncate(timestamp);

        // Should be 2024-01-01 00:00:00.000 UTC
        let expected = 1704067200000_i64;
        assert_eq!(truncated, expected);
    }

    #[test]
    fn test_aggregation_functions() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

        assert_eq!(AggregationFunc::Avg.apply(&values), Some(3.0));
        assert_eq!(AggregationFunc::Sum.apply(&values), Some(15.0));
        assert_eq!(AggregationFunc::Min.apply(&values), Some(1.0));
        assert_eq!(AggregationFunc::Max.apply(&values), Some(5.0));
        assert_eq!(AggregationFunc::Count.apply(&values), Some(5.0));
        assert_eq!(AggregationFunc::First.apply(&values), Some(1.0));
        assert_eq!(AggregationFunc::Last.apply(&values), Some(5.0));

        // Empty slice
        let empty: Vec<f64> = vec![];
        assert_eq!(AggregationFunc::Avg.apply(&empty), None);
    }

    #[test]
    fn test_operator_compare() {
        assert!(Operator::Eq.compare_f64(5.0, 5.0));
        assert!(!Operator::Eq.compare_f64(5.0, 6.0));

        assert!(Operator::Gt.compare_f64(6.0, 5.0));
        assert!(!Operator::Gt.compare_f64(5.0, 5.0));

        assert!(Operator::Gte.compare_f64(5.0, 5.0));
        assert!(Operator::Gte.compare_f64(6.0, 5.0));

        assert!(Operator::Lt.compare_f64(4.0, 5.0));
        assert!(Operator::Lte.compare_f64(5.0, 5.0));

        assert!(Operator::Ne.compare_f64(4.0, 5.0));
    }

    #[test]
    fn test_operator_compare_str() {
        assert!(Operator::Eq.compare_str("hello", "hello"));
        assert!(!Operator::Eq.compare_str("hello", "world"));

        assert!(Operator::Ne.compare_str("hello", "world"));
    }
}
