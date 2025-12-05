//! Chronicle Storage Engine
//!
//! This module provides the core time-series storage functionality:
//!
//! - **types**: Core data structures (DataPoint, Metric, TimeRange)
//! - **compression**: Delta encoding + LZ4 compression
//! - **wal**: Write-ahead log for durability
//! - **segment**: Segment file format
//! - **engine**: Main storage engine orchestrating all components
//! - **error**: Error types
//!
//! # Architecture
//!
//! ```text
//! Write Path:
//!   DataPoint → WAL (fsync) → Buffer → Compress → Segment
//!
//! Read Path:
//!   Query → Find Segments → Decompress → Filter → Results
//! ```
//!
//! # Example
//!
//! ```rust,no_run
//! use chronicle::storage::{StorageEngine, StorageConfig, DataPoint, Metric, Category, AggregationType, TimeRange};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create engine
//!     let config = StorageConfig::new("./data");
//!     let engine = StorageEngine::new(config).await?;
//!
//!     // Register a metric
//!     let mood_id = engine.register_metric(
//!         Metric::new("mood", "1-10", Category::Mood, AggregationType::Average)
//!     ).await?;
//!
//!     // Write data
//!     engine.write(DataPoint::new(mood_id, 7.5)).await?;
//!
//!     // Query data
//!     let range = TimeRange::last_hours(24);
//!     let points = engine.query(range, None).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod compression;
pub mod engine;
pub mod error;
pub mod segment;
pub mod types;
pub mod wal;

// Re-export commonly used types
pub use compression::{compress_block, compression_stats, decompress_block, CompressionStats};
pub use engine::{MetricRegistry, StorageConfig, StorageEngine, StorageStats};
pub use error::{StorageError, StorageResult};
pub use segment::{BlockMeta, CompressionType, Segment, SegmentBuilder, SegmentHeader};
pub use types::{AggregationType, Category, DataPoint, Metric, QueryFilter, TimeRange};
pub use wal::{WalSyncMode, WriteAheadLog};
