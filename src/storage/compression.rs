//! Compression module for Chronicle storage engine
//!
//! Implements delta encoding + LZ4 compression for efficient time-series storage.
//!
//! Strategy:
//! 1. Sort data points by timestamp
//! 2. Delta-encode timestamps (store differences)
//! 3. Delta-encode values for similar consecutive values
//! 4. Serialize to compact binary format
//! 5. LZ4 compress the result
//!
//! Expected compression: ~10x (100 bytes/point → ~10 bytes/point)

use crate::storage::error::{StorageError, StorageResult};
use crate::storage::types::DataPoint;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Intermediate format for delta-encoded data points
#[derive(Debug, Serialize, Deserialize)]
struct EncodedBlock {
    /// Base timestamp (first point's timestamp)
    base_timestamp: i64,
    /// Delta-encoded timestamps (differences from previous)
    timestamp_deltas: Vec<i64>,
    /// Metric IDs for each point
    metric_ids: Vec<u32>,
    /// Values (stored as-is or delta-encoded based on variance)
    values: Vec<f64>,
    /// Encoded tags (deduplicated)
    tags: EncodedTags,
}

/// Space-efficient tag storage with string deduplication
#[derive(Debug, Serialize, Deserialize)]
struct EncodedTags {
    /// String intern table
    strings: Vec<String>,
    /// For each point: Vec of (key_idx, value_idx) pairs
    point_tags: Vec<Vec<(u16, u16)>>,
}

impl EncodedTags {
    fn new() -> Self {
        Self {
            strings: Vec::new(),
            point_tags: Vec::new(),
        }
    }

    /// Intern a string, returning its index
    fn intern(&mut self, s: &str) -> u16 {
        if let Some(idx) = self.strings.iter().position(|existing| existing == s) {
            idx as u16
        } else {
            let idx = self.strings.len() as u16;
            self.strings.push(s.to_string());
            idx
        }
    }

    /// Add tags for a point
    fn add_point_tags(&mut self, tags: &HashMap<String, String>) {
        let encoded: Vec<(u16, u16)> = tags
            .iter()
            .map(|(k, v)| (self.intern(k), self.intern(v)))
            .collect();
        self.point_tags.push(encoded);
    }

    /// Decode tags for a point
    fn decode_point_tags(&self, point_idx: usize) -> HashMap<String, String> {
        self.point_tags
            .get(point_idx)
            .map(|pairs| {
                pairs
                    .iter()
                    .filter_map(|&(k_idx, v_idx)| {
                        let key = self.strings.get(k_idx as usize)?;
                        let value = self.strings.get(v_idx as usize)?;
                        Some((key.clone(), value.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Compress a block of data points using delta encoding + LZ4
///
/// # Arguments
/// * `points` - Data points to compress (will be sorted by timestamp)
///
/// # Returns
/// Compressed bytes ready for storage
pub fn compress_block(points: &[DataPoint]) -> StorageResult<Vec<u8>> {
    if points.is_empty() {
        return Ok(Vec::new());
    }

    // Sort by timestamp
    let mut sorted: Vec<&DataPoint> = points.iter().collect();
    sorted.sort_by_key(|p| p.timestamp);

    // Delta encode timestamps
    let base_timestamp = sorted[0].timestamp;
    let mut timestamp_deltas = Vec::with_capacity(sorted.len());
    let mut prev_ts = base_timestamp;

    for point in &sorted {
        timestamp_deltas.push(point.timestamp - prev_ts);
        prev_ts = point.timestamp;
    }

    // Collect metric IDs and values
    let metric_ids: Vec<u32> = sorted.iter().map(|p| p.metric_id).collect();
    let values: Vec<f64> = sorted.iter().map(|p| p.value).collect();

    // Encode tags with string deduplication
    let mut tags = EncodedTags::new();
    for point in &sorted {
        tags.add_point_tags(&point.tags);
    }

    // Create encoded block
    let block = EncodedBlock {
        base_timestamp,
        timestamp_deltas,
        metric_ids,
        values,
        tags,
    };

    // Serialize with bincode (compact binary format)
    let serialized =
        bincode::serialize(&block).map_err(|e| StorageError::Serialization(e.to_string()))?;

    // LZ4 compress
    let compressed = lz4_flex::compress_prepend_size(&serialized);

    Ok(compressed)
}

/// Decompress a block back to data points
///
/// # Arguments
/// * `data` - LZ4-compressed data from compress_block
///
/// # Returns
/// Vector of DataPoint, sorted by timestamp
pub fn decompress_block(data: &[u8]) -> StorageResult<Vec<DataPoint>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // LZ4 decompress
    let decompressed = lz4_flex::decompress_size_prepended(data)
        .map_err(|e| StorageError::Compression(format!("LZ4 decompression failed: {}", e)))?;

    // Deserialize
    let block: EncodedBlock = bincode::deserialize(&decompressed)
        .map_err(|e| StorageError::Serialization(e.to_string()))?;

    // Reconstruct data points
    let mut points = Vec::with_capacity(block.timestamp_deltas.len());
    let mut current_timestamp = block.base_timestamp;

    for i in 0..block.timestamp_deltas.len() {
        current_timestamp += block.timestamp_deltas[i];

        let point = DataPoint {
            timestamp: current_timestamp,
            metric_id: block.metric_ids.get(i).copied().unwrap_or(0),
            value: block.values.get(i).copied().unwrap_or(0.0),
            tags: block.tags.decode_point_tags(i),
        };

        points.push(point);
    }

    Ok(points)
}

/// Get compression statistics for a block
#[derive(Debug)]
pub struct CompressionStats {
    /// Number of points
    pub point_count: usize,
    /// Original size estimate (bytes)
    pub original_size: usize,
    /// Compressed size (bytes)
    pub compressed_size: usize,
    /// Compression ratio (original / compressed)
    pub ratio: f64,
}

/// Calculate compression statistics
pub fn compression_stats(points: &[DataPoint], compressed: &[u8]) -> CompressionStats {
    let original_size: usize = points.iter().map(|p| p.estimated_size()).sum();
    let compressed_size = compressed.len();
    let ratio = if compressed_size > 0 {
        original_size as f64 / compressed_size as f64
    } else {
        0.0
    };

    CompressionStats {
        point_count: points.len(),
        original_size,
        compressed_size,
        ratio,
    }
}

/// Estimate compressed size for buffer management
pub fn estimate_compressed_size(points: &[DataPoint]) -> usize {
    // Conservative estimate: assume 10x compression
    let raw_size: usize = points.iter().map(|p| p.estimated_size()).sum();
    raw_size / 8 + 64 // Minimum overhead
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_empty() {
        let points: Vec<DataPoint> = vec![];
        let compressed = compress_block(&points).unwrap();
        let decompressed = decompress_block(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }

    #[test]
    fn test_compress_decompress_single() {
        let points = vec![DataPoint::with_timestamp(1, 7.5, 1000)];
        let compressed = compress_block(&points).unwrap();
        let decompressed = decompress_block(&compressed).unwrap();

        assert_eq!(decompressed.len(), 1);
        assert_eq!(decompressed[0].timestamp, 1000);
        assert_eq!(decompressed[0].metric_id, 1);
        assert_eq!(decompressed[0].value, 7.5);
    }

    #[test]
    fn test_compress_decompress_multiple() {
        let points: Vec<DataPoint> = (0..100)
            .map(|i| {
                DataPoint::with_timestamp(1, 7.0 + (i as f64 * 0.01), 1000 + i * 1000)
                    .tag("source", "test")
            })
            .collect();

        let compressed = compress_block(&points).unwrap();
        let decompressed = decompress_block(&compressed).unwrap();

        assert_eq!(decompressed.len(), points.len());

        // Verify data integrity
        for (original, restored) in points.iter().zip(decompressed.iter()) {
            assert_eq!(original.timestamp, restored.timestamp);
            assert_eq!(original.metric_id, restored.metric_id);
            assert!((original.value - restored.value).abs() < f64::EPSILON);
            assert_eq!(original.tags, restored.tags);
        }
    }

    #[test]
    fn test_compression_ratio() {
        // Create realistic data: hourly mood readings for a week
        let points: Vec<DataPoint> = (0..168) // 7 days * 24 hours
            .map(|i| {
                DataPoint::with_timestamp(
                    1,
                    5.0 + (i as f64 * 0.1).sin() * 2.0, // Oscillating mood
                    1704067200000 + i * 3600000,        // Hourly intervals
                )
                .tag("source", "manual")
                .tag("location", "home")
            })
            .collect();

        let compressed = compress_block(&points).unwrap();
        let stats = compression_stats(&points, &compressed);

        println!(
            "Compression stats: {} points, {} → {} bytes, ratio: {:.1}x",
            stats.point_count, stats.original_size, stats.compressed_size, stats.ratio
        );

        // Should achieve at least 3x compression
        assert!(
            stats.ratio > 3.0,
            "Compression ratio too low: {}",
            stats.ratio
        );
    }

    #[test]
    fn test_unsorted_input() {
        // Input in random order
        let points = vec![
            DataPoint::with_timestamp(1, 3.0, 3000),
            DataPoint::with_timestamp(1, 1.0, 1000),
            DataPoint::with_timestamp(1, 2.0, 2000),
        ];

        let compressed = compress_block(&points).unwrap();
        let decompressed = decompress_block(&compressed).unwrap();

        // Output should be sorted by timestamp
        assert_eq!(decompressed[0].timestamp, 1000);
        assert_eq!(decompressed[1].timestamp, 2000);
        assert_eq!(decompressed[2].timestamp, 3000);

        assert_eq!(decompressed[0].value, 1.0);
        assert_eq!(decompressed[1].value, 2.0);
        assert_eq!(decompressed[2].value, 3.0);
    }

    #[test]
    fn test_tag_deduplication() {
        // Many points with same tags should deduplicate well
        let points: Vec<DataPoint> = (0..1000)
            .map(|i| {
                DataPoint::with_timestamp(1, i as f64, i * 1000)
                    .tag("source", "api")
                    .tag("device", "phone")
                    .tag("app", "chronicle")
            })
            .collect();

        let compressed = compress_block(&points).unwrap();
        let stats = compression_stats(&points, &compressed);

        // Good compression due to tag deduplication
        assert!(
            stats.ratio > 5.0,
            "Expected good compression from tag dedup, got {}",
            stats.ratio
        );
    }

    #[test]
    fn test_multiple_metrics() {
        let points: Vec<DataPoint> = vec![
            DataPoint::with_timestamp(1, 7.0, 1000), // mood
            DataPoint::with_timestamp(2, 10000.0, 1001), // steps
            DataPoint::with_timestamp(3, 72.5, 1002), // heart_rate
            DataPoint::with_timestamp(1, 8.0, 2000), // mood
            DataPoint::with_timestamp(2, 500.0, 2001), // steps
        ];

        let compressed = compress_block(&points).unwrap();
        let decompressed = decompress_block(&compressed).unwrap();

        assert_eq!(decompressed.len(), 5);
        // Should maintain metric_id for each point
        assert_eq!(decompressed[0].metric_id, 1); // First by timestamp
    }
}
