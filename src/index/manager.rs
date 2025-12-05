//! Index Manager - Coordinates all Chronicle indexes
//!
//! Provides a unified interface to:
//! - TimeIndex (primary, for time range queries)
//! - MetricIndex (for metric → segment mapping)
//! - TagIndex (optional, for tag-based filtering)
//!
//! # Query Optimization
//!
//! The IndexManager combines indexes to minimize data scanned:
//!
//! ```text
//! Query: "mood where location=home, last 7 days"
//!
//! 1. TimeIndex: Find block locations in [now-7d, now]
//!    → [(seg1, blk0), (seg1, blk1), (seg2, blk0), (seg3, blk0)]
//!
//! 2. MetricIndex: Filter to segments with "mood" metric
//!    → [seg1, seg3] (seg2 doesn't have mood)
//!
//! 3. TagIndex: Filter to locations with "location=home"
//!    → [(seg1, blk1), (seg3, blk0)]
//!
//! 4. Read only these 2 blocks instead of scanning everything!
//! ```

use crate::index::{DataLocation, IndexStats, MetricIndex, TagIndex, TimeIndex};
use crate::storage::{StorageError, TimeRange};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Configuration for the index manager
#[derive(Debug, Clone)]
pub struct IndexConfig {
    /// Enable tag indexing (uses more memory/disk)
    pub enable_tags: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self { enable_tags: true }
    }
}

/// Coordinates all index types for efficient queries
pub struct IndexManager {
    /// Time-based index (SQLite B-tree)
    time_index: TimeIndex,
    /// Metric → segments mapping
    metric_index: MetricIndex,
    /// Tag-based inverted index (optional)
    tag_index: TagIndex,
    /// Configuration
    config: IndexConfig,
}

impl IndexManager {
    /// Create a new index manager
    pub fn new(data_dir: &Path) -> Result<Self, StorageError> {
        Self::with_config(data_dir, IndexConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(data_dir: &Path, config: IndexConfig) -> Result<Self, StorageError> {
        let index_dir = data_dir.join("index");
        std::fs::create_dir_all(&index_dir)?;

        let time_index = TimeIndex::new(&index_dir)?;
        let metric_index = MetricIndex::new(&index_dir)?;

        let tag_index = if config.enable_tags {
            TagIndex::new(&index_dir)?
        } else {
            TagIndex::disabled()
        };

        Ok(Self {
            time_index,
            metric_index,
            tag_index,
            config,
        })
    }

    // ==================== Query Methods ====================

    /// Find all locations for a time range
    ///
    /// This is the primary query path - uses the TimeIndex B-tree
    /// for O(log n) lookup.
    pub fn find_by_time_range(&self, range: &TimeRange) -> Vec<DataLocation> {
        self.time_index.find_range(range.start, range.end)
    }

    /// Find segments containing a specific metric
    pub fn find_segments_by_metric(&self, metric_id: u32) -> Vec<u32> {
        self.metric_index.get_segments(metric_id)
    }

    /// Find locations by tag value
    pub fn find_by_tag(&self, key: &str, value: &str) -> Vec<DataLocation> {
        self.tag_index.find(key, value)
    }

    /// Optimized query combining time range and metric filter
    ///
    /// Returns only locations in the time range AND in segments
    /// containing the specified metric.
    pub fn find_by_time_and_metric(
        &self,
        range: &TimeRange,
        metric_id: u32,
    ) -> Vec<DataLocation> {
        // Get segments with this metric
        let metric_segments: HashSet<u32> = self
            .metric_index
            .get_segments(metric_id)
            .into_iter()
            .collect();

        if metric_segments.is_empty() {
            return Vec::new();
        }

        // Get time range locations, filtered by metric segments
        self.time_index
            .find_range(range.start, range.end)
            .into_iter()
            .filter(|loc| metric_segments.contains(&loc.segment_id))
            .collect()
    }

    /// Optimized query combining time range, metric, and tags
    pub fn find_by_time_metric_and_tags(
        &self,
        range: &TimeRange,
        metric_id: Option<u32>,
        tags: &HashMap<String, String>,
    ) -> Vec<DataLocation> {
        // Start with time range
        let mut locations: HashSet<DataLocation> = self
            .time_index
            .find_range(range.start, range.end)
            .into_iter()
            .collect();

        // Filter by metric if specified
        if let Some(mid) = metric_id {
            let metric_segments: HashSet<u32> = self
                .metric_index
                .get_segments(mid)
                .into_iter()
                .collect();

            locations.retain(|loc| metric_segments.contains(&loc.segment_id));
        }

        // Filter by tags if any
        if !tags.is_empty() && self.tag_index.is_enabled() {
            let tag_pairs: Vec<(&str, &str)> = tags
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();

            let tag_locations: HashSet<DataLocation> =
                self.tag_index.find_all(&tag_pairs).into_iter().collect();

            locations.retain(|loc| tag_locations.contains(loc));
        }

        locations.into_iter().collect()
    }

    /// Find the location containing or preceding a timestamp
    pub fn find_floor(&self, timestamp: i64) -> Option<DataLocation> {
        self.time_index.find_floor(timestamp)
    }

    // ==================== Index Update Methods ====================

    /// Index a new segment
    ///
    /// Call this when a new segment is created or a block is appended.
    ///
    /// # Arguments
    /// * `segment_id` - The segment being indexed
    /// * `block_boundaries` - Vec of (block_idx, min_timestamp) pairs
    /// * `metrics` - Metric IDs present in this segment
    /// * `tags` - Optional: tags and their locations in this segment
    pub fn index_segment(
        &mut self,
        segment_id: u32,
        block_boundaries: &[(u32, i64)],
        metrics: &[u32],
        tags: Option<&HashMap<String, Vec<(u32, String)>>>, // key → [(block_idx, value)]
    ) -> Result<(), StorageError> {
        // 1. Update time index with block boundaries
        self.time_index.insert_range(segment_id, block_boundaries)?;

        // 2. Update metric index
        for &metric_id in metrics {
            self.metric_index.add_segment(metric_id, segment_id);
        }

        // 3. Update tag index if enabled and tags provided
        if let Some(tag_map) = tags {
            if self.tag_index.is_enabled() {
                for (key, entries) in tag_map {
                    for (block_idx, value) in entries {
                        let location = DataLocation::new(segment_id, *block_idx);
                        self.tag_index.add(key, value, location);
                    }
                }
            }
        }

        Ok(())
    }

    /// Index a single block (simpler API for incremental indexing)
    pub fn index_block(
        &mut self,
        segment_id: u32,
        block_idx: u32,
        min_timestamp: i64,
        metrics: &[u32],
        tags: &HashMap<String, String>,
    ) -> Result<(), StorageError> {
        // Time index
        self.time_index
            .insert(min_timestamp, segment_id, block_idx)?;

        // Metric index
        for &metric_id in metrics {
            self.metric_index.add_segment(metric_id, segment_id);
        }

        // Tag index
        if self.tag_index.is_enabled() && !tags.is_empty() {
            let location = DataLocation::new(segment_id, block_idx);
            self.tag_index.add_tags(tags, location);
        }

        Ok(())
    }

    /// Remove a segment from all indexes (used during compaction)
    pub fn remove_segment(&mut self, segment_id: u32) -> Result<(), StorageError> {
        self.time_index.remove_segment(segment_id)?;
        self.metric_index.remove_segment(segment_id);
        self.tag_index.remove_segment(segment_id);
        Ok(())
    }

    // ==================== Persistence Methods ====================

    /// Persist all indexes to disk
    pub fn persist(&mut self) -> Result<(), StorageError> {
        // Time index uses SQLite, auto-persisted
        self.time_index.checkpoint()?;
        self.metric_index.persist()?;
        self.tag_index.persist()?;
        Ok(())
    }

    /// Optimize indexes (compact, vacuum, etc.)
    pub fn optimize(&mut self) -> Result<(), StorageError> {
        self.time_index.optimize()?;
        // Metric and tag indexes are HashMap-based, no optimization needed
        Ok(())
    }

    // ==================== Stats Methods ====================

    /// Get statistics about all indexes
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            time_entries: self.time_index.count(),
            metrics_indexed: self.metric_index.metric_count(),
            segments_indexed: self.metric_index.total_segments(),
            tag_keys: self.tag_index.key_count(),
        }
    }

    /// Get time bounds of indexed data
    pub fn time_bounds(&self) -> Option<(i64, i64)> {
        self.time_index.time_bounds()
    }

    /// Check if a metric exists in any segment
    pub fn has_metric(&self, metric_id: u32) -> bool {
        self.metric_index.has_metric(metric_id)
    }

    /// Get all tag keys
    pub fn get_tag_keys(&self) -> Vec<String> {
        self.tag_index.get_keys()
    }

    /// Get all values for a tag key
    pub fn get_tag_values(&self, key: &str) -> Vec<String> {
        self.tag_index.get_values(key)
    }

    /// Check if tag indexing is enabled
    pub fn tags_enabled(&self) -> bool {
        self.config.enable_tags && self.tag_index.is_enabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_manager() -> (IndexManager, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let manager = IndexManager::new(dir.path()).unwrap();
        (manager, dir)
    }

    #[test]
    fn test_manager_creation() {
        let (manager, _dir) = create_test_manager();
        let stats = manager.stats();

        assert_eq!(stats.time_entries, 0);
        assert_eq!(stats.metrics_indexed, 0);
        assert_eq!(stats.segments_indexed, 0);
    }

    #[test]
    fn test_index_segment() {
        let (mut manager, _dir) = create_test_manager();

        // Index a segment with 5 blocks
        let block_boundaries: Vec<(u32, i64)> = (0..5).map(|i| (i, i as i64 * 1000)).collect();

        manager
            .index_segment(1, &block_boundaries, &[10, 20], None)
            .unwrap();

        let stats = manager.stats();
        assert_eq!(stats.time_entries, 5);
        assert_eq!(stats.metrics_indexed, 2);
        assert_eq!(stats.segments_indexed, 1);
    }

    #[test]
    fn test_find_by_time_range() {
        let (mut manager, _dir) = create_test_manager();

        // Index two segments
        manager
            .index_segment(1, &[(0, 1000), (1, 2000), (2, 3000)], &[10], None)
            .unwrap();

        manager
            .index_segment(2, &[(0, 4000), (1, 5000)], &[10], None)
            .unwrap();

        // Query range that spans both segments
        let range = TimeRange::new(1500, 4500);
        let locations = manager.find_by_time_range(&range);

        // Should find blocks: seg1/blk1, seg1/blk2, seg2/blk0
        assert_eq!(locations.len(), 3);
    }

    #[test]
    fn test_find_by_time_and_metric() {
        let (mut manager, _dir) = create_test_manager();

        // Segment 1 has metric 10
        manager
            .index_segment(1, &[(0, 1000), (1, 2000)], &[10], None)
            .unwrap();

        // Segment 2 has metric 20
        manager
            .index_segment(2, &[(0, 1500), (1, 2500)], &[20], None)
            .unwrap();

        // Query for metric 10 in time range
        let range = TimeRange::new(0, 3000);
        let locations = manager.find_by_time_and_metric(&range, 10);

        // Should only find segment 1 blocks
        assert_eq!(locations.len(), 2);
        assert!(locations.iter().all(|loc| loc.segment_id == 1));
    }

    #[test]
    fn test_find_by_tag() {
        let (mut manager, _dir) = create_test_manager();

        // Index with tags
        let mut tags = HashMap::new();
        tags.insert("location".to_string(), "home".to_string());

        manager
            .index_block(1, 0, 1000, &[10], &tags)
            .unwrap();

        tags.insert("location".to_string(), "work".to_string());
        manager
            .index_block(2, 0, 2000, &[10], &tags)
            .unwrap();

        // Find by tag
        let home_locations = manager.find_by_tag("location", "home");
        assert_eq!(home_locations.len(), 1);
        assert_eq!(home_locations[0].segment_id, 1);

        let work_locations = manager.find_by_tag("location", "work");
        assert_eq!(work_locations.len(), 1);
        assert_eq!(work_locations[0].segment_id, 2);
    }

    #[test]
    fn test_combined_query() {
        let (mut manager, _dir) = create_test_manager();

        // Index multiple blocks with different characteristics
        let mut home_tags = HashMap::new();
        home_tags.insert("location".to_string(), "home".to_string());

        let mut work_tags = HashMap::new();
        work_tags.insert("location".to_string(), "work".to_string());

        // Segment 1: metric 10, location=home
        manager.index_block(1, 0, 1000, &[10], &home_tags).unwrap();
        manager.index_block(1, 1, 2000, &[10], &home_tags).unwrap();

        // Segment 2: metric 10, location=work
        manager.index_block(2, 0, 1500, &[10], &work_tags).unwrap();

        // Segment 3: metric 20, location=home
        manager.index_block(3, 0, 1800, &[20], &home_tags).unwrap();

        // Query: metric 10, location=home, time 0-3000
        let range = TimeRange::new(0, 3000);
        let mut query_tags = HashMap::new();
        query_tags.insert("location".to_string(), "home".to_string());

        let locations = manager.find_by_time_metric_and_tags(&range, Some(10), &query_tags);

        // Should only find segment 1 blocks
        assert_eq!(locations.len(), 2);
        assert!(locations.iter().all(|loc| loc.segment_id == 1));
    }

    #[test]
    fn test_remove_segment() {
        let (mut manager, _dir) = create_test_manager();

        manager
            .index_segment(1, &[(0, 1000)], &[10], None)
            .unwrap();
        manager
            .index_segment(2, &[(0, 2000)], &[10], None)
            .unwrap();

        assert_eq!(manager.stats().time_entries, 2);
        assert_eq!(manager.stats().segments_indexed, 2);

        manager.remove_segment(1).unwrap();

        assert_eq!(manager.stats().time_entries, 1);
        assert_eq!(manager.stats().segments_indexed, 1);
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();

        // Create and populate
        {
            let mut manager = IndexManager::new(dir.path()).unwrap();
            manager
                .index_segment(1, &[(0, 1000), (1, 2000)], &[10, 20], None)
                .unwrap();
            manager.persist().unwrap();
        }

        // Reopen and verify
        {
            let manager = IndexManager::new(dir.path()).unwrap();
            let stats = manager.stats();

            assert_eq!(stats.time_entries, 2);
            assert_eq!(stats.metrics_indexed, 2);

            // Query should work
            let range = TimeRange::new(0, 3000);
            let locations = manager.find_by_time_range(&range);
            assert_eq!(locations.len(), 2);
        }
    }

    #[test]
    fn test_disabled_tags() {
        let dir = tempdir().unwrap();
        let config = IndexConfig { enable_tags: false };
        let mut manager = IndexManager::with_config(dir.path(), config).unwrap();

        let mut tags = HashMap::new();
        tags.insert("key".to_string(), "value".to_string());

        manager.index_block(1, 0, 1000, &[10], &tags).unwrap();

        // Tag query should return empty
        assert!(manager.find_by_tag("key", "value").is_empty());
        assert!(!manager.tags_enabled());
    }

    #[test]
    fn test_time_bounds() {
        let (mut manager, _dir) = create_test_manager();

        assert!(manager.time_bounds().is_none());

        manager
            .index_segment(1, &[(0, 1000), (1, 5000)], &[10], None)
            .unwrap();

        let (min, max) = manager.time_bounds().unwrap();
        assert_eq!(min, 1000);
        assert_eq!(max, 5000);
    }

    #[test]
    fn test_find_floor() {
        let (mut manager, _dir) = create_test_manager();

        manager
            .index_segment(1, &[(0, 1000), (1, 2000), (2, 3000)], &[10], None)
            .unwrap();

        // Find floor for timestamp 2500
        let floor = manager.find_floor(2500).unwrap();
        assert_eq!(floor.block_idx, 1); // Block at 2000

        // Before all data
        assert!(manager.find_floor(500).is_none());
    }
}
