//! Chronicle Index Structures
//!
//! Provides efficient indexing for time-series queries:
//!
//! - **TimeIndex**: SQLite-backed B-tree for O(log n) time range queries
//! - **MetricIndex**: In-memory HashMap for O(1) metric→segment lookup
//! - **TagIndex**: Inverted index for tag-based filtering
//!
//! # Architecture
//!
//! ```text
//! Query: "mood data from last 7 days"
//!        ↓
//! TimeIndex: Find blocks in range → [(seg1, blk0), (seg1, blk1), (seg2, blk0)]
//!        ↓
//! MetricIndex: Filter to segments with "mood" → [seg1, seg2]
//!        ↓
//! Read only relevant blocks → Fast!
//! ```

mod manager;
mod metric_index;
mod tag_index;
mod time_index;

pub use manager::{IndexConfig, IndexManager};
pub use metric_index::MetricIndex;
pub use tag_index::TagIndex;
pub use time_index::TimeIndex;

use serde::{Deserialize, Serialize};

/// Location of data within storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataLocation {
    /// Segment file ID
    pub segment_id: u32,
    /// Block index within segment
    pub block_idx: u32,
}

impl DataLocation {
    pub fn new(segment_id: u32, block_idx: u32) -> Self {
        Self {
            segment_id,
            block_idx,
        }
    }
}

/// Index entry with timestamp and location
#[derive(Debug, Clone, Copy)]
pub struct TimeEntry {
    pub timestamp: i64,
    pub location: DataLocation,
}

/// Statistics about index usage
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Number of entries in time index
    pub time_entries: u64,
    /// Number of metrics indexed
    pub metrics_indexed: usize,
    /// Number of segments in metric index
    pub segments_indexed: usize,
    /// Number of tag keys indexed
    pub tag_keys: usize,
}
