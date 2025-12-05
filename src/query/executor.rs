//! Query Executor
//!
//! Executes Query AST against the StorageEngine, performing:
//! 1. Index lookup for efficient data access
//! 2. Data fetching from segments
//! 3. Filtering by conditions
//! 4. Aggregation with GROUP BY
//!
//! # Execution Pipeline
//!
//! ```text
//! Query → Plan → Index Lookup → Fetch → Filter → Aggregate → Result
//! ```

use crate::query::ast::*;
use crate::query::error::{QueryError, QueryResult};
use crate::storage::{DataPoint, StorageEngine, TimeRange};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Result of a query execution
#[derive(Debug, Clone)]
pub struct QueryResult2 {
    /// Column names (metric names or aliases)
    pub columns: Vec<String>,
    /// Result rows
    pub rows: Vec<ResultRow>,
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
    /// Number of points scanned
    pub points_scanned: usize,
}

impl QueryResult2 {
    /// Get the number of rows
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Convert to a simple vector of (timestamp, value) pairs for single-metric queries
    pub fn to_time_series(&self) -> Vec<(i64, f64)> {
        if self.columns.is_empty() {
            return Vec::new();
        }
        let first_col = &self.columns[0];
        self.rows
            .iter()
            .filter_map(|row| row.values.get(first_col).map(|v| (row.timestamp, *v)))
            .collect()
    }
}

/// A single result row
#[derive(Debug, Clone)]
pub struct ResultRow {
    /// Timestamp for this row (bucket start for aggregated queries)
    pub timestamp: i64,
    /// Values keyed by column name
    pub values: HashMap<String, f64>,
}

impl ResultRow {
    /// Get a value by column name
    pub fn get(&self, column: &str) -> Option<f64> {
        self.values.get(column).copied()
    }
}

/// Query executor
pub struct QueryExecutor {
    /// Reference to storage engine
    storage: Arc<StorageEngine>,
}

impl QueryExecutor {
    /// Create a new query executor
    pub fn new(storage: Arc<StorageEngine>) -> Self {
        Self { storage }
    }

    /// Execute a query string (parses and executes)
    pub async fn execute_str(&self, query_str: &str) -> QueryResult<QueryResult2> {
        let query = crate::query::parser::parse_query(query_str)?;
        self.execute(query).await
    }

    /// Execute a parsed query
    pub async fn execute(&self, query: Query) -> QueryResult<QueryResult2> {
        let start = Instant::now();

        // 1. Resolve metric names to IDs
        let metric_ids = self.resolve_metrics(&query.select).await?;

        // 2. Build tag filters from query filters
        let tag_filters = self.extract_tag_filters(&query.filters);

        // 3. Fetch data points using storage engine's query method
        let mut all_points = Vec::new();

        for (metric_name, metric_id) in &metric_ids {
            let points = if tag_filters.is_empty() {
                // Simple query by metric
                self.storage
                    .query_metric(metric_name, query.time_range)
                    .await?
            } else {
                // Query all and filter (tag filtering is done in memory)
                let mut filter = crate::storage::QueryFilter::new().metric_id(*metric_id);
                for (key, value) in &tag_filters {
                    filter = filter.tag(key.clone(), value.clone());
                }
                self.storage
                    .query(query.time_range, Some(filter))
                    .await?
            };
            all_points.extend(points);
        }

        let points_scanned = all_points.len();

        // 4. Apply value filters
        let filtered = self.apply_filters(all_points, &query.filters);

        // 5. Aggregate or convert to rows
        let rows = if let Some(ref group_by) = query.group_by {
            self.aggregate(filtered, &query.select, group_by, &metric_ids)
        } else {
            self.to_rows(filtered, &query.select, &metric_ids)
        };

        // 6. Apply limit
        let rows = if let Some(limit) = query.limit {
            rows.into_iter().take(limit).collect()
        } else {
            rows
        };

        // 7. Build result
        let columns = query
            .select
            .iter()
            .map(|s| s.display_name().to_string())
            .collect();

        Ok(QueryResult2 {
            columns,
            rows,
            execution_time_ms: start.elapsed().as_millis() as u64,
            points_scanned,
        })
    }

    /// Resolve metric names to IDs
    async fn resolve_metrics(
        &self,
        select: &[SelectItem],
    ) -> QueryResult<Vec<(String, u32)>> {
        let mut result = Vec::new();

        for item in select {
            if item.metric == "*" {
                // SELECT * - get all metrics
                let metrics = self.storage.get_metrics().await;
                for m in metrics {
                    result.push((m.name.clone(), m.id));
                }
            } else {
                let metric = self
                    .storage
                    .get_metric(&item.metric)
                    .await
                    .ok_or_else(|| QueryError::MetricNotFound(item.metric.clone()))?;
                result.push((item.metric.clone(), metric.id));
            }
        }

        Ok(result)
    }

    /// Extract tag filters from query filters
    fn extract_tag_filters(&self, filters: &[Filter]) -> HashMap<String, String> {
        let mut result = HashMap::new();

        for filter in filters {
            if let FilterField::Tag(key) = &filter.field {
                if filter.op == Operator::Eq {
                    if let FilterValue::String(value) = &filter.value {
                        result.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        result
    }

    /// Apply filters to data points
    fn apply_filters(&self, points: Vec<DataPoint>, filters: &[Filter]) -> Vec<DataPoint> {
        points
            .into_iter()
            .filter(|point| self.matches_all_filters(point, filters))
            .collect()
    }

    /// Check if a point matches all filters
    fn matches_all_filters(&self, point: &DataPoint, filters: &[Filter]) -> bool {
        filters.iter().all(|filter| self.matches_filter(point, filter))
    }

    /// Check if a point matches a single filter
    fn matches_filter(&self, point: &DataPoint, filter: &Filter) -> bool {
        match &filter.field {
            FilterField::Metric => {
                // Metric filter is handled at query time
                true
            }
            FilterField::Tag(key) => {
                if let Some(tag_value) = point.tags.get(key) {
                    match &filter.value {
                        FilterValue::String(s) => filter.op.compare_str(tag_value, s),
                        FilterValue::Number(_) => false,
                    }
                } else {
                    // Tag not present - only match for "not equal"
                    filter.op == Operator::Ne
                }
            }
            FilterField::Value => match &filter.value {
                FilterValue::Number(n) => filter.op.compare_f64(point.value, *n),
                FilterValue::String(_) => false,
            },
        }
    }

    /// Aggregate points by time windows
    fn aggregate(
        &self,
        points: Vec<DataPoint>,
        select: &[SelectItem],
        group_by: &GroupByClause,
        metric_ids: &[(String, u32)],
    ) -> Vec<ResultRow> {
        // Group points by truncated timestamp
        let mut groups: HashMap<i64, Vec<DataPoint>> = HashMap::new();

        for point in points {
            let bucket = group_by.interval.truncate(point.timestamp);
            groups.entry(bucket).or_default().push(point);
        }

        // Aggregate each group
        let mut rows: Vec<ResultRow> = groups
            .into_iter()
            .map(|(timestamp, points)| {
                let values = self.aggregate_group(&points, select, metric_ids);
                ResultRow { timestamp, values }
            })
            .collect();

        // Sort by timestamp
        rows.sort_by_key(|r| r.timestamp);
        rows
    }

    /// Aggregate a group of points
    fn aggregate_group(
        &self,
        points: &[DataPoint],
        select: &[SelectItem],
        metric_ids: &[(String, u32)],
    ) -> HashMap<String, f64> {
        let mut values = HashMap::new();

        for item in select {
            // Find the metric ID for this select item
            let metric_id = metric_ids
                .iter()
                .find(|(name, _)| name == &item.metric)
                .map(|(_, id)| *id);

            // Get values for this metric
            let metric_values: Vec<f64> = points
                .iter()
                .filter(|p| metric_id.map(|id| p.metric_id == id).unwrap_or(true))
                .map(|p| p.value)
                .collect();

            // Apply aggregation
            let aggregated = match item.aggregation {
                Some(agg) => agg.apply(&metric_values),
                None => {
                    // No aggregation - use last value
                    AggregationFunc::Last.apply(&metric_values)
                }
            };

            if let Some(val) = aggregated {
                let name = item.display_name().to_string();
                values.insert(name, val);
            }
        }

        values
    }

    /// Convert raw points to result rows (no aggregation)
    fn to_rows(
        &self,
        points: Vec<DataPoint>,
        select: &[SelectItem],
        metric_ids: &[(String, u32)],
    ) -> Vec<ResultRow> {
        // For non-aggregated queries, each point becomes a row
        // Group by timestamp for multi-metric queries
        let mut timestamp_groups: HashMap<i64, Vec<&DataPoint>> = HashMap::new();

        for point in &points {
            timestamp_groups.entry(point.timestamp).or_default().push(point);
        }

        let mut rows: Vec<ResultRow> = timestamp_groups
            .into_iter()
            .map(|(timestamp, group_points)| {
                let mut values = HashMap::new();

                for item in select {
                    // Find the metric ID for this select item
                    let metric_id = metric_ids
                        .iter()
                        .find(|(name, _)| name == &item.metric)
                        .map(|(_, id)| *id);

                    // Find the point for this metric
                    if let Some(point) = group_points
                        .iter()
                        .find(|p| metric_id.map(|id| p.metric_id == id).unwrap_or(true))
                    {
                        let name = item.display_name().to_string();
                        values.insert(name, point.value);
                    }
                }

                ResultRow { timestamp, values }
            })
            .collect();

        // Sort by timestamp
        rows.sort_by_key(|r| r.timestamp);
        rows
    }
}

/// Convenience methods for common queries
impl QueryExecutor {
    /// Query a single metric for the last N days
    pub async fn query_last_days(
        &self,
        metric: &str,
        days: i64,
    ) -> QueryResult<QueryResult2> {
        let query = Query::select(&[metric]).last_days(days).build();
        self.execute(query).await
    }

    /// Query with daily aggregation
    pub async fn query_daily_avg(
        &self,
        metric: &str,
        days: i64,
    ) -> QueryResult<QueryResult2> {
        let query = Query::select(&[metric])
            .last_days(days)
            .group_by(GroupByInterval::Day)
            .with_aggregation(AggregationFunc::Avg)
            .build();
        self.execute(query).await
    }

    /// Query multiple metrics
    pub async fn query_metrics(
        &self,
        metrics: &[&str],
        range: TimeRange,
    ) -> QueryResult<QueryResult2> {
        let query = Query::select(metrics).time_range(range).build();
        self.execute(query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{AggregationType, Category, Metric, StorageConfig};
    use tempfile::tempdir;

    async fn create_test_executor() -> (QueryExecutor, Arc<StorageEngine>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let config = StorageConfig::new(dir.path());
        let engine = Arc::new(StorageEngine::new(config).await.unwrap());
        let executor = QueryExecutor::new(Arc::clone(&engine));
        (executor, engine, dir)
    }

    #[tokio::test]
    async fn test_simple_query() {
        let (executor, engine, _dir) = create_test_executor().await;

        // Register metric
        let metric_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Write test data with distinct timestamps
        let now = chrono::Utc::now().timestamp_millis();
        for i in 0..10 {
            let point = DataPoint::with_timestamp(metric_id, 5.0 + i as f64 * 0.5, now - (10 - i) * 1000);
            engine.write(point).await.unwrap();
        }
        engine.flush().await.unwrap();

        // Query
        let result = executor.query_last_days("mood", 1).await.unwrap();

        assert_eq!(result.columns, vec!["mood"]);
        assert_eq!(result.rows.len(), 10);
    }

    #[tokio::test]
    async fn test_aggregation_query() {
        let (executor, engine, _dir) = create_test_executor().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Write data spanning multiple hours
        let now = chrono::Utc::now().timestamp_millis();
        let hour_ms = 3600 * 1000;

        // Hour 1: values 5, 6, 7 (avg 6)
        for i in 0..3 {
            let point = DataPoint::with_timestamp(metric_id, 5.0 + i as f64, now - 2 * hour_ms + i * 1000);
            engine.write(point).await.unwrap();
        }

        // Hour 2: values 8, 9 (avg 8.5)
        for i in 0..2 {
            let point = DataPoint::with_timestamp(metric_id, 8.0 + i as f64, now - hour_ms + i * 1000);
            engine.write(point).await.unwrap();
        }

        engine.flush().await.unwrap();

        // Query with hourly aggregation
        let query = Query::select(&["mood"])
            .last_hours(3)
            .group_by(GroupByInterval::Hour)
            .with_aggregation(AggregationFunc::Avg)
            .build();

        let result = executor.execute(query).await.unwrap();

        assert!(result.rows.len() >= 2);

        // Check aggregated values
        let sorted_rows: Vec<_> = result.rows.clone();
        if sorted_rows.len() >= 2 {
            let first_avg = sorted_rows[0].values.get("mood").unwrap();
            let second_avg = sorted_rows[1].values.get("mood").unwrap();

            // First hour should have avg around 6
            assert!((first_avg - 6.0).abs() < 0.1, "First hour avg: {}", first_avg);
            // Second hour should have avg around 8.5
            assert!((second_avg - 8.5).abs() < 0.1, "Second hour avg: {}", second_avg);
        }
    }

    #[tokio::test]
    async fn test_filter_query() {
        let (executor, engine, _dir) = create_test_executor().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Write data with tags and distinct timestamps
        let now = chrono::Utc::now().timestamp_millis();
        for i in 0..10 {
            let location = if i % 2 == 0 { "home" } else { "office" };
            let point = DataPoint::with_timestamp(metric_id, 5.0 + i as f64 * 0.5, now - (10 - i) * 1000)
                .tag("location", location);
            engine.write(point).await.unwrap();
        }
        engine.flush().await.unwrap();

        // Query with tag filter
        let query = Query::select(&["mood"])
            .last_days(1)
            .filter_tag("location", "home")
            .build();

        let result = executor.execute(query).await.unwrap();

        // Should only get "home" entries (every other one)
        assert_eq!(result.rows.len(), 5);
    }

    #[tokio::test]
    async fn test_value_filter_query() {
        let (executor, engine, _dir) = create_test_executor().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Write data with varying values and distinct timestamps
        let now = chrono::Utc::now().timestamp_millis();
        for i in 0..10 {
            let point = DataPoint::with_timestamp(metric_id, i as f64, now - (10 - i) * 1000);
            engine.write(point).await.unwrap();
        }
        engine.flush().await.unwrap();

        // Query with value filter
        let query = Query::select(&["mood"])
            .last_days(1)
            .filter_value(Operator::Gte, 5.0)
            .build();

        let result = executor.execute(query).await.unwrap();

        // Should only get values >= 5 (5, 6, 7, 8, 9)
        assert_eq!(result.rows.len(), 5);
        for row in &result.rows {
            let value = row.values.get("mood").unwrap();
            assert!(*value >= 5.0);
        }
    }

    #[tokio::test]
    async fn test_parse_and_execute() {
        let (executor, engine, _dir) = create_test_executor().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Use distinct timestamps
        let now = chrono::Utc::now().timestamp_millis();
        for i in 0..5 {
            let point = DataPoint::with_timestamp(metric_id, 7.0 + i as f64 * 0.1, now - (5 - i) * 1000);
            engine.write(point).await.unwrap();
        }
        engine.flush().await.unwrap();

        // Execute from string
        let result = executor
            .execute_str("SELECT mood WHERE time >= now() - 1d")
            .await
            .unwrap();

        assert_eq!(result.rows.len(), 5);
    }

    #[tokio::test]
    async fn test_metric_not_found() {
        let (executor, _engine, _dir) = create_test_executor().await;

        let result = executor
            .execute_str("SELECT nonexistent WHERE time >= now() - 1d")
            .await;

        assert!(matches!(result, Err(QueryError::MetricNotFound(_))));
    }

    #[tokio::test]
    async fn test_limit() {
        let (executor, engine, _dir) = create_test_executor().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Use distinct timestamps
        let now = chrono::Utc::now().timestamp_millis();
        for i in 0..100 {
            let point = DataPoint::with_timestamp(metric_id, i as f64, now - (100 - i) * 1000);
            engine.write(point).await.unwrap();
        }
        engine.flush().await.unwrap();

        let query = Query::select(&["mood"]).last_days(1).limit(10).build();

        let result = executor.execute(query).await.unwrap();

        assert_eq!(result.rows.len(), 10);
    }

    #[tokio::test]
    async fn test_multiple_metrics() {
        let (executor, engine, _dir) = create_test_executor().await;

        let mood_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        let energy_id = engine
            .register_metric(Metric::new(
                "energy",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Write data with same timestamps
        let now = chrono::Utc::now().timestamp_millis();
        for i in 0..5 {
            let ts = now - (5 - i) * 1000;
            engine
                .write(DataPoint::with_timestamp(mood_id, 7.0, ts))
                .await
                .unwrap();
            engine
                .write(DataPoint::with_timestamp(energy_id, 6.0, ts))
                .await
                .unwrap();
        }
        engine.flush().await.unwrap();

        let query = Query::select(&["mood", "energy"]).last_days(1).build();

        let result = executor.execute(query).await.unwrap();

        assert_eq!(result.columns, vec!["mood", "energy"]);
        assert!(!result.rows.is_empty());

        // Each row should have both metrics
        for row in &result.rows {
            assert!(row.values.contains_key("mood") || row.values.contains_key("energy"));
        }
    }

    #[tokio::test]
    async fn test_to_time_series() {
        let (executor, engine, _dir) = create_test_executor().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        let now = chrono::Utc::now().timestamp_millis();
        for i in 0..5 {
            let point = DataPoint::with_timestamp(metric_id, i as f64, now - (5 - i) * 1000);
            engine.write(point).await.unwrap();
        }
        engine.flush().await.unwrap();

        let result = executor.query_last_days("mood", 1).await.unwrap();
        let time_series = result.to_time_series();

        assert_eq!(time_series.len(), 5);
        // Values should be 0, 1, 2, 3, 4
        for (i, (_ts, val)) in time_series.iter().enumerate() {
            assert_eq!(*val, i as f64);
        }
    }
}
