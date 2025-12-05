//! Metric Index - In-memory HashMap with JSON persistence
//!
//! Maps metric_id → Set<segment_id> for O(1) lookup of which
//! segments contain data for a given metric.
//!
//! # Usage
//! ```ignore
//! // When querying "mood" metric:
//! let segments = metric_index.get_segments(mood_id);
//! // segments = [1, 3, 5] - only scan these segments
//! ```

use crate::storage::StorageError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

/// In-memory metric index with JSON persistence
///
/// Provides O(1) lookup for metric → segments mapping
#[derive(Debug)]
pub struct MetricIndex {
    /// metric_id → set of segment_ids containing that metric
    index: HashMap<u32, HashSet<u32>>,
    /// Path to persistence file
    path: PathBuf,
    /// Track if modified since last save
    dirty: bool,
}

/// Serialization format for JSON persistence
#[derive(Serialize, Deserialize)]
struct MetricIndexData {
    /// Version for future compatibility
    version: u32,
    /// The actual index data: metric_id → [segment_ids]
    index: HashMap<u32, Vec<u32>>,
}

impl MetricIndex {
    /// Create or load a metric index
    pub fn new(data_dir: &Path) -> Result<Self, StorageError> {
        let path = data_dir.join("metric_index.json");

        let index = if path.exists() {
            Self::load_from_file(&path)?
        } else {
            HashMap::new()
        };

        Ok(Self {
            index,
            path,
            dirty: false,
        })
    }

    /// Load index from JSON file
    fn load_from_file(path: &Path) -> Result<HashMap<u32, HashSet<u32>>, StorageError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let data: MetricIndexData = serde_json::from_reader(reader).map_err(|e| {
            StorageError::Serialization(format!("Failed to load metric index: {}", e))
        })?;

        // Convert Vec to HashSet
        let index = data
            .index
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();

        Ok(index)
    }

    /// Add a segment to a metric's segment set
    pub fn add_segment(&mut self, metric_id: u32, segment_id: u32) {
        let inserted = self
            .index
            .entry(metric_id)
            .or_insert_with(HashSet::new)
            .insert(segment_id);

        if inserted {
            self.dirty = true;
        }
    }

    /// Add multiple segments for a metric
    pub fn add_segments(&mut self, metric_id: u32, segment_ids: &[u32]) {
        let entry = self.index.entry(metric_id).or_insert_with(HashSet::new);

        for &segment_id in segment_ids {
            if entry.insert(segment_id) {
                self.dirty = true;
            }
        }
    }

    /// Remove a segment from all metrics (used during compaction)
    pub fn remove_segment(&mut self, segment_id: u32) {
        for segments in self.index.values_mut() {
            if segments.remove(&segment_id) {
                self.dirty = true;
            }
        }

        // Clean up empty entries
        self.index.retain(|_, segments| !segments.is_empty());
    }

    /// Get all segment IDs containing a metric
    pub fn get_segments(&self, metric_id: u32) -> Vec<u32> {
        self.index
            .get(&metric_id)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Check if a metric exists in any segment
    pub fn has_metric(&self, metric_id: u32) -> bool {
        self.index
            .get(&metric_id)
            .map(|set| !set.is_empty())
            .unwrap_or(false)
    }

    /// Get all indexed metrics
    pub fn get_all_metrics(&self) -> Vec<u32> {
        self.index.keys().copied().collect()
    }

    /// Get count of segments for a metric
    pub fn segment_count(&self, metric_id: u32) -> usize {
        self.index.get(&metric_id).map(|s| s.len()).unwrap_or(0)
    }

    /// Get total number of metrics indexed
    pub fn metric_count(&self) -> usize {
        self.index.len()
    }

    /// Get total number of unique segments across all metrics
    pub fn total_segments(&self) -> usize {
        let all_segments: HashSet<u32> = self.index.values().flatten().copied().collect();
        all_segments.len()
    }

    /// Persist index to JSON file
    pub fn persist(&mut self) -> Result<(), StorageError> {
        if !self.dirty {
            return Ok(());
        }

        // Create parent directory if needed
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Convert HashSet to Vec for JSON serialization
        let data = MetricIndexData {
            version: 1,
            index: self
                .index
                .iter()
                .map(|(&k, v)| (k, v.iter().copied().collect()))
                .collect(),
        };

        let file = File::create(&self.path)?;
        let writer = BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &data).map_err(|e| {
            StorageError::Serialization(format!("Failed to persist metric index: {}", e))
        })?;

        self.dirty = false;
        Ok(())
    }

    /// Force persist regardless of dirty flag
    pub fn force_persist(&mut self) -> Result<(), StorageError> {
        self.dirty = true;
        self.persist()
    }

    /// Check if there are unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Get the persistence file path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Intersect with a set of segment IDs (filter operation)
    pub fn intersect_segments(&self, metric_id: u32, segments: &[u32]) -> Vec<u32> {
        let segment_set: HashSet<u32> = segments.iter().copied().collect();

        self.index
            .get(&metric_id)
            .map(|metric_segments| {
                metric_segments
                    .intersection(&segment_set)
                    .copied()
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for MetricIndex {
    fn drop(&mut self) {
        // Auto-persist on drop (best effort)
        if self.dirty {
            let _ = self.persist();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_metric_index_creation() {
        let dir = tempdir().unwrap();
        let index = MetricIndex::new(dir.path()).unwrap();

        assert_eq!(index.metric_count(), 0);
        assert_eq!(index.total_segments(), 0);
    }

    #[test]
    fn test_add_and_get_segments() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        // Add segments for metric 1
        index.add_segment(1, 100);
        index.add_segment(1, 200);
        index.add_segment(1, 300);

        // Add segments for metric 2
        index.add_segment(2, 100); // Shared with metric 1
        index.add_segment(2, 400);

        let segments_1 = index.get_segments(1);
        assert_eq!(segments_1.len(), 3);
        assert!(segments_1.contains(&100));
        assert!(segments_1.contains(&200));
        assert!(segments_1.contains(&300));

        let segments_2 = index.get_segments(2);
        assert_eq!(segments_2.len(), 2);
        assert!(segments_2.contains(&100));
        assert!(segments_2.contains(&400));

        // Non-existent metric
        let segments_3 = index.get_segments(3);
        assert!(segments_3.is_empty());
    }

    #[test]
    fn test_add_segments_batch() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        index.add_segments(1, &[100, 200, 300]);

        let segments = index.get_segments(1);
        assert_eq!(segments.len(), 3);
    }

    #[test]
    fn test_remove_segment() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        index.add_segment(1, 100);
        index.add_segment(1, 200);
        index.add_segment(2, 100);
        index.add_segment(2, 300);

        // Remove segment 100 from all metrics
        index.remove_segment(100);

        let segments_1 = index.get_segments(1);
        assert_eq!(segments_1.len(), 1);
        assert!(!segments_1.contains(&100));

        let segments_2 = index.get_segments(2);
        assert_eq!(segments_2.len(), 1);
        assert!(!segments_2.contains(&100));
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();

        // Create and populate
        {
            let mut index = MetricIndex::new(dir.path()).unwrap();
            index.add_segment(1, 100);
            index.add_segment(1, 200);
            index.add_segment(2, 300);
            index.persist().unwrap();
        }

        // Reopen and verify
        {
            let index = MetricIndex::new(dir.path()).unwrap();
            assert_eq!(index.metric_count(), 2);

            let segments_1 = index.get_segments(1);
            assert_eq!(segments_1.len(), 2);

            let segments_2 = index.get_segments(2);
            assert_eq!(segments_2.len(), 1);
        }
    }

    #[test]
    fn test_has_metric() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        index.add_segment(1, 100);

        assert!(index.has_metric(1));
        assert!(!index.has_metric(2));
    }

    #[test]
    fn test_get_all_metrics() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        index.add_segment(1, 100);
        index.add_segment(5, 200);
        index.add_segment(10, 300);

        let metrics = index.get_all_metrics();
        assert_eq!(metrics.len(), 3);
        assert!(metrics.contains(&1));
        assert!(metrics.contains(&5));
        assert!(metrics.contains(&10));
    }

    #[test]
    fn test_intersect_segments() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        index.add_segments(1, &[100, 200, 300, 400]);

        // Intersect with subset
        let result = index.intersect_segments(1, &[100, 300, 500]);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&100));
        assert!(result.contains(&300));
    }

    #[test]
    fn test_dirty_flag() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        assert!(!index.is_dirty());

        index.add_segment(1, 100);
        assert!(index.is_dirty());

        index.persist().unwrap();
        assert!(!index.is_dirty());

        // Duplicate add shouldn't set dirty
        index.add_segment(1, 100);
        assert!(!index.is_dirty());

        // New segment should set dirty
        index.add_segment(1, 200);
        assert!(index.is_dirty());
    }

    #[test]
    fn test_total_segments() {
        let dir = tempdir().unwrap();
        let mut index = MetricIndex::new(dir.path()).unwrap();

        // Same segment shared across metrics
        index.add_segment(1, 100);
        index.add_segment(2, 100);
        index.add_segment(1, 200);
        index.add_segment(3, 300);

        // Should count unique segments only
        assert_eq!(index.total_segments(), 3);
    }
}
