//! Correlation Engine
//!
//! Calculates Pearson correlation coefficients between all metric pairs.
//! Strong correlations are synced to MemMachine as learned patterns.

use crate::memmachine::client::{MemMachineClient, MemMachineError};
use crate::query::{AggregationFunc, GroupByInterval, Query, QueryExecutor};
use crate::storage::{StorageEngine, TimeRange};
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Calculate correlations between metrics
pub struct CorrelationEngine {
    storage: Arc<StorageEngine>,
    executor: Arc<QueryExecutor>,
    client: Arc<MemMachineClient>,
}

/// A correlation between two metrics
#[derive(Debug, Clone, Serialize)]
pub struct Correlation {
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

impl CorrelationEngine {
    /// Create a new correlation engine
    pub fn new(
        storage: Arc<StorageEngine>,
        executor: Arc<QueryExecutor>,
        client: Arc<MemMachineClient>,
    ) -> Self {
        Self {
            storage,
            executor,
            client,
        }
    }

    /// Calculate correlations for all metric pairs over last N days
    ///
    /// Returns correlations sorted by absolute strength (strongest first).
    /// Only includes correlations with |r| > 0.3 (weak or stronger).
    pub async fn calculate_all(&self, days: i64) -> Vec<Correlation> {
        let metrics = self.storage.get_metrics().await;
        let range = TimeRange::last_days(days);

        // Fetch daily averages for all metrics
        let mut metric_values: HashMap<String, Vec<f64>> = HashMap::new();
        let mut metric_timestamps: HashMap<String, Vec<i64>> = HashMap::new();

        for metric in &metrics {
            let query = Query::select(&[metric.name.as_str()])
                .time_range(range)
                .group_by(GroupByInterval::Day)
                .with_aggregation(AggregationFunc::Avg)
                .build();

            if let Ok(result) = self.executor.execute(query).await {
                let values: Vec<f64> = result
                    .rows
                    .iter()
                    .filter_map(|r| r.values.get(&metric.name).copied())
                    .collect();

                let timestamps: Vec<i64> = result.rows.iter().map(|r| r.timestamp).collect();

                // Need at least 7 days of data for meaningful correlation
                if values.len() >= 7 {
                    metric_values.insert(metric.name.clone(), values);
                    metric_timestamps.insert(metric.name.clone(), timestamps);
                }
            }
        }

        // Calculate pairwise correlations
        let mut correlations = Vec::new();
        let metric_names: Vec<&String> = metric_values.keys().collect();
        let now = Utc::now().to_rfc3339();

        for i in 0..metric_names.len() {
            for j in (i + 1)..metric_names.len() {
                let a_name = metric_names[i];
                let b_name = metric_names[j];

                let a_values = &metric_values[a_name];
                let b_values = &metric_values[b_name];
                let a_timestamps = &metric_timestamps[a_name];
                let b_timestamps = &metric_timestamps[b_name];

                // Align values by timestamp
                let (aligned_a, aligned_b) = align_by_timestamp(
                    a_values,
                    a_timestamps,
                    b_values,
                    b_timestamps,
                );

                if aligned_a.len() < 7 {
                    continue;
                }

                let r = pearson_correlation(&aligned_a, &aligned_b);

                // Only include meaningful correlations
                if r.abs() > 0.3 && !r.is_nan() {
                    correlations.push(Correlation {
                        metric_a: a_name.clone(),
                        metric_b: b_name.clone(),
                        coefficient: (r * 100.0).round() / 100.0, // Round to 2 decimals
                        strength: correlation_strength(r),
                        direction: if r > 0.0 {
                            "positive".to_string()
                        } else {
                            "negative".to_string()
                        },
                        sample_size: aligned_a.len(),
                        last_calculated: now.clone(),
                    });
                }
            }
        }

        // Sort by absolute correlation strength (strongest first)
        correlations.sort_by(|a, b| {
            b.coefficient
                .abs()
                .partial_cmp(&a.coefficient.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        correlations
    }

    /// Store strong correlations in MemMachine profile
    ///
    /// Only syncs correlations with |r| > 0.5 (moderate or stronger).
    pub async fn sync_to_memmachine(
        &self,
        correlations: &[Correlation],
    ) -> Result<u32, MemMachineError> {
        let session_id = format!("correlation-sync-{}", Utc::now().format("%Y%m%d"));
        let mut synced = 0;

        for corr in correlations.iter().filter(|c| c.coefficient.abs() > 0.5) {
            let content = format!(
                "{} {} correlates with {} (r={:.2}, {} correlation)",
                corr.metric_a,
                corr.direction,
                corr.metric_b,
                corr.coefficient,
                corr.strength
            );

            match self
                .client
                .add_unified_memory(&session_id, &content, "correlation")
                .await
            {
                Ok(_) => {
                    synced += 1;
                    tracing::debug!(
                        metric_a = %corr.metric_a,
                        metric_b = %corr.metric_b,
                        r = corr.coefficient,
                        "Synced correlation to MemMachine"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to sync correlation");
                }
            }
        }

        Ok(synced)
    }

    /// Get a summary of correlations for a specific metric
    pub async fn correlations_for_metric(&self, metric_name: &str, days: i64) -> Vec<Correlation> {
        let all = self.calculate_all(days).await;
        all.into_iter()
            .filter(|c| c.metric_a == metric_name || c.metric_b == metric_name)
            .collect()
    }
}

/// Align two value arrays by their timestamps
fn align_by_timestamp(
    a_values: &[f64],
    a_timestamps: &[i64],
    b_values: &[f64],
    b_timestamps: &[i64],
) -> (Vec<f64>, Vec<f64>) {
    let mut aligned_a = Vec::new();
    let mut aligned_b = Vec::new();

    // Create a map of timestamp to value for B
    let b_map: HashMap<i64, f64> = b_timestamps
        .iter()
        .zip(b_values.iter())
        .map(|(&t, &v)| (normalize_day(t), v))
        .collect();

    // Find matching timestamps
    for (i, &ts) in a_timestamps.iter().enumerate() {
        let day = normalize_day(ts);
        if let Some(&b_val) = b_map.get(&day) {
            aligned_a.push(a_values[i]);
            aligned_b.push(b_val);
        }
    }

    (aligned_a, aligned_b)
}

/// Normalize timestamp to start of day (for alignment)
fn normalize_day(ts: i64) -> i64 {
    // Round down to start of day (UTC)
    (ts / (24 * 3600 * 1000)) * (24 * 3600 * 1000)
}

/// Calculate Pearson correlation coefficient
///
/// Returns a value between -1 and 1:
/// - 1: perfect positive correlation
/// - 0: no correlation
/// - -1: perfect negative correlation
pub fn pearson_correlation(x: &[f64], y: &[f64]) -> f64 {
    if x.len() != y.len() || x.is_empty() {
        return 0.0;
    }

    let n = x.len() as f64;

    let sum_x: f64 = x.iter().sum();
    let sum_y: f64 = y.iter().sum();
    let sum_xy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
    let sum_x2: f64 = x.iter().map(|a| a * a).sum();
    let sum_y2: f64 = y.iter().map(|b| b * b).sum();

    let numerator = n * sum_xy - sum_x * sum_y;
    let denominator = ((n * sum_x2 - sum_x.powi(2)) * (n * sum_y2 - sum_y.powi(2))).sqrt();

    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

/// Convert correlation coefficient to human-readable strength
fn correlation_strength(r: f64) -> String {
    let abs_r = r.abs();
    if abs_r > 0.7 {
        "strong".to_string()
    } else if abs_r > 0.5 {
        "moderate".to_string()
    } else if abs_r > 0.3 {
        "weak".to_string()
    } else {
        "negligible".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pearson_correlation_perfect_positive() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![2.0, 4.0, 6.0, 8.0, 10.0];
        let r = pearson_correlation(&x, &y);
        assert!((r - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_pearson_correlation_perfect_negative() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![10.0, 8.0, 6.0, 4.0, 2.0];
        let r = pearson_correlation(&x, &y);
        assert!((r + 1.0).abs() < 0.001);
    }

    #[test]
    fn test_pearson_correlation_no_correlation() {
        // Alternating pattern has near-zero correlation with linear
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let y = vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let r = pearson_correlation(&x, &y);
        // Should be close to 0
        assert!(r.abs() < 0.5, "Expected low correlation, got {}", r);
    }

    #[test]
    fn test_pearson_correlation_empty() {
        let x: Vec<f64> = vec![];
        let y: Vec<f64> = vec![];
        let r = pearson_correlation(&x, &y);
        assert_eq!(r, 0.0);
    }

    #[test]
    fn test_correlation_strength() {
        assert_eq!(correlation_strength(0.8), "strong");
        assert_eq!(correlation_strength(-0.75), "strong");
        assert_eq!(correlation_strength(0.6), "moderate");
        assert_eq!(correlation_strength(-0.55), "moderate");
        assert_eq!(correlation_strength(0.4), "weak");
        assert_eq!(correlation_strength(-0.35), "weak");
        assert_eq!(correlation_strength(0.2), "negligible");
    }

    #[test]
    fn test_normalize_day() {
        // Jan 15, 2024 at 10:30:00 UTC
        let ts = 1705315800000_i64;
        let normalized = normalize_day(ts);
        // Should be Jan 15, 2024 at 00:00:00 UTC
        assert_eq!(normalized, 1705276800000);
    }

    #[test]
    fn test_align_by_timestamp() {
        let a_vals = vec![1.0, 2.0, 3.0];
        let a_ts = vec![1000, 2000, 3000].iter().map(|&x| x * 24 * 3600 * 1000).collect::<Vec<_>>();

        let b_vals = vec![10.0, 30.0]; // Missing value at day 2000
        let b_ts = vec![1000, 3000].iter().map(|&x| x * 24 * 3600 * 1000).collect::<Vec<_>>();

        let (aligned_a, aligned_b) = align_by_timestamp(&a_vals, &a_ts, &b_vals, &b_ts);

        assert_eq!(aligned_a.len(), 2);
        assert_eq!(aligned_b.len(), 2);
        assert_eq!(aligned_a, vec![1.0, 3.0]);
        assert_eq!(aligned_b, vec![10.0, 30.0]);
    }

    #[test]
    fn test_correlation_serializes() {
        let corr = Correlation {
            metric_a: "mood".to_string(),
            metric_b: "sleep".to_string(),
            coefficient: 0.72,
            strength: "strong".to_string(),
            direction: "positive".to_string(),
            sample_size: 30,
            last_calculated: "2024-01-15T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&corr).unwrap();
        assert!(json.contains("\"coefficient\":0.72"));
        assert!(json.contains("\"strength\":\"strong\""));
    }
}
