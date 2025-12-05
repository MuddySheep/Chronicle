//! Tag Index - Inverted index for tag-based queries
//!
//! Maps "key:value" → Vec<DataLocation> for efficient tag filtering.
//!
//! # Example
//! ```ignore
//! // Query: mood data where location=home
//! let locations = tag_index.find("location", "home");
//! // locations = [(seg1, blk0), (seg1, blk2), (seg3, blk1)]
//! ```
//!
//! # Design Notes
//! - Optional index (not all deployments need tag queries)
//! - In-memory with JSON persistence
//! - Deduplicates locations automatically

use crate::index::DataLocation;
use crate::storage::StorageError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

/// Inverted index for tag-based queries
///
/// Provides O(1) lookup for tag → locations mapping
#[derive(Debug)]
pub struct TagIndex {
    /// "key:value" → set of locations
    index: HashMap<String, HashSet<DataLocation>>,
    /// All known tag keys (for enumeration)
    keys: HashSet<String>,
    /// Path to persistence file
    path: PathBuf,
    /// Track if modified since last save
    dirty: bool,
    /// Whether this index is enabled
    enabled: bool,
}

/// Serialization format for JSON persistence
#[derive(Serialize, Deserialize)]
struct TagIndexData {
    version: u32,
    keys: Vec<String>,
    index: HashMap<String, Vec<DataLocation>>,
}

impl TagIndex {
    /// Create a new tag index
    pub fn new(data_dir: &Path) -> Result<Self, StorageError> {
        let path = data_dir.join("tag_index.json");

        let (index, keys) = if path.exists() {
            Self::load_from_file(&path)?
        } else {
            (HashMap::new(), HashSet::new())
        };

        Ok(Self {
            index,
            keys,
            path,
            dirty: false,
            enabled: true,
        })
    }

    /// Create a disabled (no-op) tag index
    pub fn disabled() -> Self {
        Self {
            index: HashMap::new(),
            keys: HashSet::new(),
            path: PathBuf::new(),
            dirty: false,
            enabled: false,
        }
    }

    /// Load index from JSON file
    fn load_from_file(
        path: &Path,
    ) -> Result<(HashMap<String, HashSet<DataLocation>>, HashSet<String>), StorageError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let data: TagIndexData = serde_json::from_reader(reader)
            .map_err(|e| StorageError::Serialization(format!("Failed to load tag index: {}", e)))?;

        let keys: HashSet<String> = data.keys.into_iter().collect();
        let index = data
            .index
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();

        Ok((index, keys))
    }

    /// Check if the index is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Add a tag entry
    pub fn add(&mut self, key: &str, value: &str, location: DataLocation) {
        if !self.enabled {
            return;
        }

        let tag_key = format!("{}:{}", key, value);

        self.keys.insert(key.to_string());

        let inserted = self
            .index
            .entry(tag_key)
            .or_insert_with(HashSet::new)
            .insert(location);

        if inserted {
            self.dirty = true;
        }
    }

    /// Add multiple tag entries for a location
    pub fn add_tags(&mut self, tags: &HashMap<String, String>, location: DataLocation) {
        if !self.enabled {
            return;
        }

        for (key, value) in tags {
            self.add(key, value, location);
        }
    }

    /// Find all locations with a specific tag value
    pub fn find(&self, key: &str, value: &str) -> Vec<DataLocation> {
        if !self.enabled {
            return Vec::new();
        }

        let tag_key = format!("{}:{}", key, value);
        self.index
            .get(&tag_key)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Find all locations matching any of the given tag values for a key
    pub fn find_any(&self, key: &str, values: &[&str]) -> Vec<DataLocation> {
        if !self.enabled {
            return Vec::new();
        }

        let mut result = HashSet::new();

        for value in values {
            let tag_key = format!("{}:{}", key, value);
            if let Some(locations) = self.index.get(&tag_key) {
                result.extend(locations);
            }
        }

        result.into_iter().collect()
    }

    /// Find all locations matching all given tags (intersection)
    pub fn find_all(&self, tags: &[(&str, &str)]) -> Vec<DataLocation> {
        if !self.enabled || tags.is_empty() {
            return Vec::new();
        }

        let mut result: Option<HashSet<DataLocation>> = None;

        for (key, value) in tags {
            let tag_key = format!("{}:{}", key, value);
            let locations: HashSet<DataLocation> = self
                .index
                .get(&tag_key)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default();

            result = Some(match result {
                Some(existing) => existing.intersection(&locations).copied().collect(),
                None => locations,
            });
        }

        result.map(|s| s.into_iter().collect()).unwrap_or_default()
    }

    /// Get all unique values for a given tag key
    pub fn get_values(&self, key: &str) -> Vec<String> {
        if !self.enabled {
            return Vec::new();
        }

        let prefix = format!("{}:", key);
        self.index
            .keys()
            .filter_map(|tag_key| {
                if tag_key.starts_with(&prefix) {
                    Some(tag_key[prefix.len()..].to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all known tag keys
    pub fn get_keys(&self) -> Vec<String> {
        self.keys.iter().cloned().collect()
    }

    /// Check if a tag key exists
    pub fn has_key(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    /// Remove all entries for a segment
    pub fn remove_segment(&mut self, segment_id: u32) {
        if !self.enabled {
            return;
        }

        for locations in self.index.values_mut() {
            let before = locations.len();
            locations.retain(|loc| loc.segment_id != segment_id);
            if locations.len() != before {
                self.dirty = true;
            }
        }

        // Clean up empty entries
        self.index.retain(|_, locations| !locations.is_empty());
    }

    /// Remove all entries for a specific location
    pub fn remove_location(&mut self, location: DataLocation) {
        if !self.enabled {
            return;
        }

        for locations in self.index.values_mut() {
            if locations.remove(&location) {
                self.dirty = true;
            }
        }

        // Clean up empty entries
        self.index.retain(|_, locations| !locations.is_empty());
    }

    /// Get count of unique tag:value combinations
    pub fn tag_count(&self) -> usize {
        self.index.len()
    }

    /// Get count of tag keys
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Get total number of location entries (with duplicates across tags)
    pub fn location_count(&self) -> usize {
        self.index.values().map(|s| s.len()).sum()
    }

    /// Persist index to JSON file
    pub fn persist(&mut self) -> Result<(), StorageError> {
        if !self.enabled || !self.dirty {
            return Ok(());
        }

        // Create parent directory if needed
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let data = TagIndexData {
            version: 1,
            keys: self.keys.iter().cloned().collect(),
            index: self
                .index
                .iter()
                .map(|(k, v)| (k.clone(), v.iter().copied().collect()))
                .collect(),
        };

        let file = File::create(&self.path)?;
        let writer = BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &data).map_err(|e| {
            StorageError::Serialization(format!("Failed to persist tag index: {}", e))
        })?;

        self.dirty = false;
        Ok(())
    }

    /// Check if there are unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear the entire index
    pub fn clear(&mut self) {
        if !self.enabled {
            return;
        }

        self.index.clear();
        self.keys.clear();
        self.dirty = true;
    }
}

impl Drop for TagIndex {
    fn drop(&mut self) {
        // Auto-persist on drop (best effort)
        if self.enabled && self.dirty {
            let _ = self.persist();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_tag_index_creation() {
        let dir = tempdir().unwrap();
        let index = TagIndex::new(dir.path()).unwrap();

        assert!(index.is_enabled());
        assert_eq!(index.tag_count(), 0);
        assert_eq!(index.key_count(), 0);
    }

    #[test]
    fn test_add_and_find() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc1 = DataLocation::new(1, 0);
        let loc2 = DataLocation::new(1, 1);
        let loc3 = DataLocation::new(2, 0);

        index.add("location", "home", loc1);
        index.add("location", "home", loc2);
        index.add("location", "work", loc3);

        let home_locations = index.find("location", "home");
        assert_eq!(home_locations.len(), 2);
        assert!(home_locations.contains(&loc1));
        assert!(home_locations.contains(&loc2));

        let work_locations = index.find("location", "work");
        assert_eq!(work_locations.len(), 1);
        assert!(work_locations.contains(&loc3));
    }

    #[test]
    fn test_add_tags() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc = DataLocation::new(1, 0);
        let mut tags = HashMap::new();
        tags.insert("source".to_string(), "api".to_string());
        tags.insert("device".to_string(), "phone".to_string());

        index.add_tags(&tags, loc);

        assert!(index.find("source", "api").contains(&loc));
        assert!(index.find("device", "phone").contains(&loc));
    }

    #[test]
    fn test_find_any() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc1 = DataLocation::new(1, 0);
        let loc2 = DataLocation::new(2, 0);
        let loc3 = DataLocation::new(3, 0);

        index.add("status", "good", loc1);
        index.add("status", "ok", loc2);
        index.add("status", "bad", loc3);

        let good_or_ok = index.find_any("status", &["good", "ok"]);
        assert_eq!(good_or_ok.len(), 2);
        assert!(good_or_ok.contains(&loc1));
        assert!(good_or_ok.contains(&loc2));
    }

    #[test]
    fn test_find_all() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc1 = DataLocation::new(1, 0);
        let loc2 = DataLocation::new(2, 0);

        // loc1 has both tags
        index.add("source", "api", loc1);
        index.add("device", "phone", loc1);

        // loc2 has only one tag
        index.add("source", "api", loc2);

        // Find entries with both tags
        let both_tags = index.find_all(&[("source", "api"), ("device", "phone")]);
        assert_eq!(both_tags.len(), 1);
        assert!(both_tags.contains(&loc1));
    }

    #[test]
    fn test_get_values() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc = DataLocation::new(1, 0);
        index.add("location", "home", loc);
        index.add("location", "work", loc);
        index.add("location", "gym", loc);

        let values = index.get_values("location");
        assert_eq!(values.len(), 3);
        assert!(values.contains(&"home".to_string()));
        assert!(values.contains(&"work".to_string()));
        assert!(values.contains(&"gym".to_string()));
    }

    #[test]
    fn test_get_keys() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc = DataLocation::new(1, 0);
        index.add("source", "api", loc);
        index.add("device", "phone", loc);
        index.add("location", "home", loc);

        let keys = index.get_keys();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"source".to_string()));
        assert!(keys.contains(&"device".to_string()));
        assert!(keys.contains(&"location".to_string()));
    }

    #[test]
    fn test_remove_segment() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc1 = DataLocation::new(1, 0);
        let loc2 = DataLocation::new(2, 0);

        index.add("tag", "value", loc1);
        index.add("tag", "value", loc2);

        assert_eq!(index.find("tag", "value").len(), 2);

        index.remove_segment(1);

        let remaining = index.find("tag", "value");
        assert_eq!(remaining.len(), 1);
        assert!(remaining.contains(&loc2));
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();

        // Create and populate
        {
            let mut index = TagIndex::new(dir.path()).unwrap();
            index.add("key1", "value1", DataLocation::new(1, 0));
            index.add("key2", "value2", DataLocation::new(2, 0));
            index.persist().unwrap();
        }

        // Reopen and verify
        {
            let index = TagIndex::new(dir.path()).unwrap();
            assert_eq!(index.key_count(), 2);
            assert_eq!(index.find("key1", "value1").len(), 1);
            assert_eq!(index.find("key2", "value2").len(), 1);
        }
    }

    #[test]
    fn test_disabled_index() {
        let index = TagIndex::disabled();

        assert!(!index.is_enabled());
        assert!(index.find("any", "thing").is_empty());
        assert!(index.get_keys().is_empty());
    }

    #[test]
    fn test_deduplication() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        let loc = DataLocation::new(1, 0);

        // Add same entry multiple times
        index.add("tag", "value", loc);
        index.add("tag", "value", loc);
        index.add("tag", "value", loc);

        // Should only have one entry
        assert_eq!(index.find("tag", "value").len(), 1);
    }

    #[test]
    fn test_clear() {
        let dir = tempdir().unwrap();
        let mut index = TagIndex::new(dir.path()).unwrap();

        index.add("key", "value", DataLocation::new(1, 0));
        assert_eq!(index.tag_count(), 1);

        index.clear();
        assert_eq!(index.tag_count(), 0);
        assert_eq!(index.key_count(), 0);
    }
}
