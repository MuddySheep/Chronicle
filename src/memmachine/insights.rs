//! Insight Engine
//!
//! Generates insights by combining Chronicle data with MemMachine context.
//! Uses rule-based analysis (with potential for LLM integration).

use crate::memmachine::client::{MemMachineClient, MemMachineError, MemoryResult};
use crate::query::{AggregationFunc, GroupByInterval, Query, QueryError, QueryExecutor};
use crate::storage::{StorageEngine, TimeRange};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// Generate insights using MemMachine context
pub struct InsightEngine {
    client: Arc<MemMachineClient>,
    storage: Arc<StorageEngine>,
    executor: Arc<QueryExecutor>,
}

impl InsightEngine {
    /// Create a new insight engine
    pub fn new(
        client: Arc<MemMachineClient>,
        storage: Arc<StorageEngine>,
        executor: Arc<QueryExecutor>,
    ) -> Self {
        Self {
            client,
            storage,
            executor,
        }
    }

    /// Generate insight for a user question
    ///
    /// This method:
    /// 1. Searches MemMachine for relevant context
    /// 2. Queries Chronicle for recent data
    /// 3. Generates a rule-based insight
    /// 4. Stores the interaction in MemMachine
    pub async fn generate_insight(
        &self,
        question: &str,
        context_days: i64,
    ) -> Result<InsightResponse, InsightError> {
        let session_id = format!("insight-{}", uuid::Uuid::new_v4());

        // 1. Search MemMachine for relevant context
        let memories = self
            .client
            .search_memories(&session_id, question, 10)
            .await
            .unwrap_or_default();

        tracing::debug!(
            question = %question,
            memories_found = memories.len(),
            "Searched MemMachine for context"
        );

        // 2. Query recent data from Chronicle
        let range = TimeRange::last_days(context_days);
        let metrics = self.storage.get_metrics().await;

        let mut metric_data = HashMap::new();
        let mut metric_trends = HashMap::new();

        for metric in &metrics {
            // Get daily averages
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

                if !values.is_empty() {
                    let avg = values.iter().sum::<f64>() / values.len() as f64;
                    metric_data.insert(metric.name.clone(), avg);

                    // Calculate trend (comparing first half to second half)
                    if values.len() >= 4 {
                        let mid = values.len() / 2;
                        let first_half_avg =
                            values[..mid].iter().sum::<f64>() / mid as f64;
                        let second_half_avg =
                            values[mid..].iter().sum::<f64>() / (values.len() - mid) as f64;
                        let trend = second_half_avg - first_half_avg;
                        metric_trends.insert(metric.name.clone(), trend);
                    }
                }
            }
        }

        // 3. Generate insight (rule-based)
        let insight = self.generate_rule_based_insight(question, &memories, &metric_data, &metric_trends);

        // 4. Store this interaction in MemMachine
        let interaction_content = format!(
            "User asked: {}\nInsight provided: {}",
            question, insight.insight
        );
        let _ = self
            .client
            .add_episodic_memory(&session_id, &interaction_content, "insight_query", HashMap::new())
            .await;

        Ok(insight)
    }

    /// Generate rule-based insight from data
    fn generate_rule_based_insight(
        &self,
        question: &str,
        memories: &[MemoryResult],
        data: &HashMap<String, f64>,
        trends: &HashMap<String, f64>,
    ) -> InsightResponse {
        let question_lower = question.to_lowercase();
        let mut insight_parts = Vec::new();
        let mut supporting_data = HashMap::new();
        let mut recommendations = Vec::new();

        // Detect question type and generate appropriate insight
        if question_lower.contains("mood") {
            self.analyze_mood(&mut insight_parts, &mut supporting_data, &mut recommendations, data, trends);
        }

        if question_lower.contains("sleep") {
            self.analyze_sleep(&mut insight_parts, &mut supporting_data, &mut recommendations, data, trends);
        }

        if question_lower.contains("energy") {
            self.analyze_energy(&mut insight_parts, &mut supporting_data, &mut recommendations, data, trends);
        }

        if question_lower.contains("productivity") || question_lower.contains("focus") {
            self.analyze_productivity(&mut insight_parts, &mut supporting_data, &mut recommendations, data, trends);
        }

        if question_lower.contains("why") || question_lower.contains("low") || question_lower.contains("drop") {
            // User is asking about a decline - look for correlations
            self.analyze_decline(&mut insight_parts, &mut supporting_data, &mut recommendations, data, trends);
        }

        if question_lower.contains("pattern") || question_lower.contains("trend") {
            self.analyze_patterns(&mut insight_parts, &mut supporting_data, &mut recommendations, data, trends);
        }

        // Extract related patterns from memories
        let related_patterns: Vec<String> = memories
            .iter()
            .filter_map(|m| {
                if m.episode_type.as_deref() == Some("pattern")
                    || m.episode_type.as_deref() == Some("correlation")
                {
                    Some(m.content.clone())
                } else {
                    None
                }
            })
            .take(3)
            .collect();

        InsightResponse {
            insight: if insight_parts.is_empty() {
                format!(
                    "Based on your data over the past period, I found {} metrics being tracked. \
                     Try asking about specific metrics like mood, sleep, energy, or productivity \
                     for more detailed insights.",
                    data.len()
                )
            } else {
                insight_parts.join(" ")
            },
            supporting_data,
            related_patterns,
            recommendations,
        }
    }

    fn analyze_mood(
        &self,
        insight_parts: &mut Vec<String>,
        supporting_data: &mut HashMap<String, f64>,
        recommendations: &mut Vec<String>,
        data: &HashMap<String, f64>,
        trends: &HashMap<String, f64>,
    ) {
        if let Some(&mood_avg) = data.get("mood") {
            supporting_data.insert("mood_avg".to_string(), mood_avg);

            if mood_avg < 5.0 {
                insight_parts.push("Your mood has been lower than usual recently.".to_string());
                recommendations.push("Consider tracking what activities improve your mood.".to_string());
            } else if mood_avg > 7.0 {
                insight_parts.push("Your mood has been quite positive recently!".to_string());
            } else {
                insight_parts.push(format!(
                    "Your mood has been in a moderate range (averaging {:.1}).",
                    mood_avg
                ));
            }

            // Check trend
            if let Some(&trend) = trends.get("mood") {
                supporting_data.insert("mood_trend".to_string(), trend);
                if trend < -0.5 {
                    insight_parts.push("Your mood appears to be trending downward.".to_string());
                } else if trend > 0.5 {
                    insight_parts.push("Your mood appears to be improving!".to_string());
                }
            }

            // Check sleep correlation
            if let Some(&sleep) = data.get("sleep_hours").or(data.get("sleep")) {
                supporting_data.insert("sleep_avg".to_string(), sleep);
                if sleep < 6.5 && mood_avg < 6.0 {
                    insight_parts.push(format!(
                        "Your sleep has been below optimal (averaging {:.1} hours), which may be affecting your mood.",
                        sleep
                    ));
                    recommendations.push("Try to get at least 7 hours of sleep.".to_string());
                }
            }
        }
    }

    fn analyze_sleep(
        &self,
        insight_parts: &mut Vec<String>,
        supporting_data: &mut HashMap<String, f64>,
        recommendations: &mut Vec<String>,
        data: &HashMap<String, f64>,
        trends: &HashMap<String, f64>,
    ) {
        let sleep_key = if data.contains_key("sleep_hours") {
            "sleep_hours"
        } else {
            "sleep"
        };

        if let Some(&sleep_avg) = data.get(sleep_key) {
            supporting_data.insert("sleep_avg".to_string(), sleep_avg);

            if sleep_avg < 6.0 {
                insight_parts.push(format!(
                    "Your sleep has been quite low, averaging only {:.1} hours.",
                    sleep_avg
                ));
                recommendations.push("Aim for 7-8 hours of sleep for optimal recovery.".to_string());
            } else if sleep_avg < 7.0 {
                insight_parts.push(format!(
                    "Your sleep has been slightly below optimal at {:.1} hours average.",
                    sleep_avg
                ));
            } else if sleep_avg > 8.5 {
                insight_parts.push(format!(
                    "You've been getting plenty of sleep ({:.1} hours average).",
                    sleep_avg
                ));
            } else {
                insight_parts.push(format!(
                    "Your sleep duration looks healthy at {:.1} hours average.",
                    sleep_avg
                ));
            }

            if let Some(&trend) = trends.get(sleep_key) {
                supporting_data.insert("sleep_trend".to_string(), trend);
                if trend < -0.5 {
                    insight_parts.push("Your sleep duration has been decreasing.".to_string());
                }
            }
        }
    }

    fn analyze_energy(
        &self,
        insight_parts: &mut Vec<String>,
        supporting_data: &mut HashMap<String, f64>,
        recommendations: &mut Vec<String>,
        data: &HashMap<String, f64>,
        trends: &HashMap<String, f64>,
    ) {
        if let Some(&energy_avg) = data.get("energy") {
            supporting_data.insert("energy_avg".to_string(), energy_avg);

            if energy_avg < 5.0 {
                insight_parts.push("Your energy levels have been on the lower side.".to_string());

                // Check possible causes
                if let Some(&sleep) = data.get("sleep_hours").or(data.get("sleep")) {
                    if sleep < 7.0 {
                        insight_parts.push("This might be related to your sleep patterns.".to_string());
                        recommendations.push("Improving sleep quality often boosts energy.".to_string());
                    }
                }
            } else if energy_avg > 7.0 {
                insight_parts.push("Your energy levels have been good!".to_string());
            }

            if let Some(&trend) = trends.get("energy") {
                supporting_data.insert("energy_trend".to_string(), trend);
                if trend > 0.5 {
                    insight_parts.push("Your energy appears to be improving.".to_string());
                }
            }
        }
    }

    fn analyze_productivity(
        &self,
        insight_parts: &mut Vec<String>,
        supporting_data: &mut HashMap<String, f64>,
        recommendations: &mut Vec<String>,
        data: &HashMap<String, f64>,
        _trends: &HashMap<String, f64>,
    ) {
        let productivity_keys = ["productivity", "focus", "focus_hours", "deep_work"];

        for key in productivity_keys {
            if let Some(&value) = data.get(key) {
                supporting_data.insert(format!("{}_avg", key), value);
                insight_parts.push(format!(
                    "Your {} has been averaging {:.1}.",
                    key.replace('_', " "),
                    value
                ));

                // Check correlation with sleep
                if let Some(&sleep) = data.get("sleep_hours").or(data.get("sleep")) {
                    if sleep < 6.5 && value < 5.0 {
                        recommendations.push(
                            "Low sleep may be affecting your focus. Try prioritizing rest.".to_string()
                        );
                    }
                }
                break; // Only report on first found productivity metric
            }
        }
    }

    fn analyze_decline(
        &self,
        insight_parts: &mut Vec<String>,
        supporting_data: &mut HashMap<String, f64>,
        recommendations: &mut Vec<String>,
        data: &HashMap<String, f64>,
        trends: &HashMap<String, f64>,
    ) {
        // Find metrics that are declining
        let declining: Vec<(&String, &f64)> = trends
            .iter()
            .filter(|(_, &trend)| trend < -0.3)
            .collect();

        if !declining.is_empty() {
            for (metric, trend) in &declining {
                supporting_data.insert(format!("{}_trend", metric), **trend);
            }

            let metric_names: Vec<&str> = declining.iter().map(|(n, _)| n.as_str()).collect();
            insight_parts.push(format!(
                "I noticed a decline in: {}.",
                metric_names.join(", ")
            ));

            // Look for potential causes
            if let Some(&sleep) = data.get("sleep_hours").or(data.get("sleep")) {
                if sleep < 6.5 {
                    insight_parts.push(format!(
                        "Your sleep has been low ({:.1}h average), which often affects other metrics.",
                        sleep
                    ));
                    recommendations.push("Try improving your sleep to see if other metrics follow.".to_string());
                }
            }
        }
    }

    fn analyze_patterns(
        &self,
        insight_parts: &mut Vec<String>,
        supporting_data: &mut HashMap<String, f64>,
        _recommendations: &mut Vec<String>,
        data: &HashMap<String, f64>,
        trends: &HashMap<String, f64>,
    ) {
        // Report on all tracked metrics
        insight_parts.push(format!(
            "You're currently tracking {} metrics.",
            data.len()
        ));

        // Find improving metrics
        let improving: Vec<&String> = trends
            .iter()
            .filter(|(_, &t)| t > 0.3)
            .map(|(n, _)| n)
            .collect();

        if !improving.is_empty() {
            insight_parts.push(format!(
                "Improving: {}.",
                improving.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }

        // Find declining metrics
        let declining: Vec<&String> = trends
            .iter()
            .filter(|(_, &t)| t < -0.3)
            .map(|(n, _)| n)
            .collect();

        if !declining.is_empty() {
            insight_parts.push(format!(
                "Declining: {}.",
                declining.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }

        // Add all data to supporting_data
        for (key, value) in data {
            supporting_data.insert(format!("{}_avg", key), *value);
        }
    }
}

/// Response from insight generation
#[derive(Debug, Clone, serde::Serialize)]
pub struct InsightResponse {
    /// The main insight text
    pub insight: String,
    /// Supporting data points used in the insight
    pub supporting_data: HashMap<String, f64>,
    /// Related patterns from MemMachine
    pub related_patterns: Vec<String>,
    /// Actionable recommendations
    pub recommendations: Vec<String>,
}

/// Errors that can occur during insight generation
#[derive(Debug, Error)]
pub enum InsightError {
    #[error("MemMachine error: {0}")]
    MemMachine(#[from] MemMachineError),

    #[error("Query error: {0}")]
    Query(#[from] QueryError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insight_response_serializes() {
        let response = InsightResponse {
            insight: "Test insight".to_string(),
            supporting_data: HashMap::from([("mood_avg".to_string(), 7.5)]),
            related_patterns: vec!["Pattern 1".to_string()],
            recommendations: vec!["Recommendation 1".to_string()],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Test insight"));
        assert!(json.contains("7.5"));
    }
}
