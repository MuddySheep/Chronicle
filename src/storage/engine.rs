//! Chronicle Storage Engine
//!
//! The main storage engine orchestrates all components:
//! - Write path: DataPoint → WAL → Buffer → Segment
//! - Read path: Query → Index → Segment → Decompress → Filter
//!
//! Thread-safe via Tokio's async RwLock for concurrent access.

use crate::index::{IndexConfig, IndexManager};
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::segment::{CompressionType, Segment};
use crate::storage::types::{Category, DataPoint, Metric, QueryFilter, TimeRange};
use crate::storage::wal::{WalSyncMode, WriteAheadLog};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

/// Configuration for the storage engine
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Root directory for all data
    pub data_dir: PathBuf,
    /// Target size per block in bytes (default: 64KB)
    pub block_size: usize,
    /// Maximum time before flush in milliseconds (default: 5000)
    pub flush_interval_ms: u64,
    /// Compression type to use
    pub compression: CompressionType,
    /// WAL sync strategy
    pub wal_sync: WalSyncMode,
    /// Maximum segment size in bytes before rotation (default: 64MB)
    pub max_segment_size: u64,
    /// Enable tag indexing
    pub enable_tag_index: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("chronicle_data"),
            block_size: 64 * 1024,        // 64KB
            flush_interval_ms: 5000,       // 5 seconds
            compression: CompressionType::Lz4,
            wal_sync: WalSyncMode::Batched,
            max_segment_size: 64 * 1024 * 1024, // 64MB
            enable_tag_index: true,
        }
    }
}

impl StorageConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            ..Default::default()
        }
    }

    /// Get path to segments directory
    pub fn segments_dir(&self) -> PathBuf {
        self.data_dir.join("segments")
    }

    /// Get path to WAL file
    pub fn wal_path(&self) -> PathBuf {
        self.data_dir.join("wal").join("current.wal")
    }

    /// Get path to metrics registry file
    pub fn metrics_path(&self) -> PathBuf {
        self.data_dir.join("meta").join("metrics.json")
    }

    /// Get path to config file
    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("meta").join("config.json")
    }
}

/// Registry for metric definitions
#[derive(Debug, Default)]
pub struct MetricRegistry {
    /// Metrics indexed by ID
    metrics: Vec<Metric>,
    /// Name to ID lookup
    name_to_id: HashMap<String, u32>,
}

impl MetricRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from JSON file
    pub fn load(path: &Path) -> StorageResult<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let content = std::fs::read_to_string(path)?;
        let metrics: Vec<Metric> = serde_json::from_str(&content)?;

        let mut registry = Self::new();
        for metric in metrics {
            registry.metrics.push(metric.clone());
            registry.name_to_id.insert(metric.name.clone(), metric.id);
        }

        Ok(registry)
    }

    /// Save to JSON file
    pub fn save(&self, path: &Path) -> StorageResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(&self.metrics)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Register a new metric
    pub fn register(&mut self, mut metric: Metric) -> u32 {
        // Check if already exists
        if let Some(&id) = self.name_to_id.get(&metric.name) {
            return id;
        }

        let id = self.metrics.len() as u32;
        metric.id = id;
        self.name_to_id.insert(metric.name.clone(), id);
        self.metrics.push(metric);
        id
    }

    /// Get metric by name
    pub fn get_by_name(&self, name: &str) -> Option<&Metric> {
        self.name_to_id
            .get(name)
            .and_then(|&id| self.metrics.get(id as usize))
    }

    /// Get metric by ID
    pub fn get_by_id(&self, id: u32) -> Option<&Metric> {
        self.metrics.get(id as usize)
    }

    /// Get all metrics
    pub fn all(&self) -> &[Metric] {
        &self.metrics
    }

    /// Get metrics by category
    pub fn by_category(&self, category: Category) -> Vec<&Metric> {
        self.metrics
            .iter()
            .filter(|m| m.category == category)
            .collect()
    }
}

/// Internal state for the storage engine
struct EngineState {
    /// Loaded segments (sorted by min_timestamp)
    segments: Vec<Segment>,
    /// Current segment being written to
    current_segment_id: u32,
}

/// The main Chronicle storage engine
pub struct StorageEngine {
    /// Configuration
    config: StorageConfig,
    /// Write-ahead log
    wal: Arc<RwLock<WriteAheadLog>>,
    /// Write buffer (accumulates points before flush)
    write_buffer: Arc<RwLock<Vec<DataPoint>>>,
    /// Metric registry
    metrics: Arc<RwLock<MetricRegistry>>,
    /// Engine state (segments)
    state: Arc<RwLock<EngineState>>,
    /// Index manager for efficient queries (std::sync::Mutex because SQLite is !Send)
    index: Arc<Mutex<IndexManager>>,
    /// Shutdown signal
    shutdown: Arc<RwLock<bool>>,
}

impl StorageEngine {
    /// Create a new storage engine
    pub async fn new(config: StorageConfig) -> StorageResult<Self> {
        // Create directory structure
        std::fs::create_dir_all(&config.data_dir)?;
        std::fs::create_dir_all(config.segments_dir())?;
        std::fs::create_dir_all(config.data_dir.join("wal"))?;
        std::fs::create_dir_all(config.data_dir.join("meta"))?;
        std::fs::create_dir_all(config.data_dir.join("index"))?;

        // Load metric registry
        let metrics = MetricRegistry::load(&config.metrics_path())?;

        // Open WAL
        let wal = WriteAheadLog::open(config.wal_path(), config.wal_sync)?;

        // Recover from WAL if needed
        let recovered_points = wal.recover()?;
        let has_recovered = !recovered_points.is_empty();
        if has_recovered {
            tracing::info!("Recovered {} points from WAL", recovered_points.len());
        }

        // Load existing segments
        let (segments, max_segment_id) = Self::load_segments(&config.segments_dir())?;

        // Initialize index manager
        let index_config = IndexConfig {
            enable_tags: config.enable_tag_index,
        };
        let index = IndexManager::with_config(&config.data_dir, index_config)?;

        let engine = Self {
            config: config.clone(),
            wal: Arc::new(RwLock::new(wal)),
            write_buffer: Arc::new(RwLock::new(recovered_points)),
            metrics: Arc::new(RwLock::new(metrics)),
            state: Arc::new(RwLock::new(EngineState {
                segments,
                current_segment_id: max_segment_id + 1,
            })),
            index: Arc::new(Mutex::new(index)),
            shutdown: Arc::new(RwLock::new(false)),
        };

        // Flush recovered points
        if has_recovered {
            engine.flush().await?;
        }

        Ok(engine)
    }

    /// Load all segments from directory
    fn load_segments(dir: &Path) -> StorageResult<(Vec<Segment>, u32)> {
        let mut segments = Vec::new();
        let mut max_id = 0u32;

        if !dir.exists() {
            return Ok((segments, max_id));
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "dat").unwrap_or(false) {
                match Segment::open(&path) {
                    Ok(segment) => {
                        if let Some(id) = segment.id() {
                            max_id = max_id.max(id);
                        }
                        segments.push(segment);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to open segment {:?}: {}", path, e);
                    }
                }
            }
        }

        // Sort by min_timestamp
        segments.sort_by_key(|s| s.header.min_timestamp);

        tracing::info!("Loaded {} segments", segments.len());
        Ok((segments, max_id))
    }

    /// Write a single data point
    pub async fn write(&self, point: DataPoint) -> StorageResult<()> {
        // Validate metric exists
        {
            let registry = self.metrics.read().await;
            if registry.get_by_id(point.metric_id).is_none() {
                return Err(StorageError::MetricNotFound(format!(
                    "metric_id {}",
                    point.metric_id
                )));
            }
        }

        // Append to WAL first (durability)
        {
            let mut wal = self.wal.write().await;
            wal.append(&point)?;
        }

        // Add to write buffer
        let should_flush = {
            let mut buffer = self.write_buffer.write().await;
            buffer.push(point);

            // Check if buffer should be flushed
            let buffer_size: usize = buffer.iter().map(|p| p.estimated_size()).sum();
            buffer_size >= self.config.block_size
        };

        if should_flush {
            self.flush().await?;
        }

        Ok(())
    }

    /// Write multiple data points (batch)
    pub async fn write_batch(&self, points: Vec<DataPoint>) -> StorageResult<()> {
        if points.is_empty() {
            return Ok(());
        }

        // Validate all metrics exist
        {
            let registry = self.metrics.read().await;
            for point in &points {
                if registry.get_by_id(point.metric_id).is_none() {
                    return Err(StorageError::MetricNotFound(format!(
                        "metric_id {}",
                        point.metric_id
                    )));
                }
            }
        }

        // Append to WAL
        {
            let mut wal = self.wal.write().await;
            wal.append_batch(&points)?;
        }

        // Add to write buffer
        let should_flush = {
            let mut buffer = self.write_buffer.write().await;
            buffer.extend(points);

            let buffer_size: usize = buffer.iter().map(|p| p.estimated_size()).sum();
            buffer_size >= self.config.block_size
        };

        if should_flush {
            self.flush().await?;
        }

        Ok(())
    }

    /// Force flush write buffer to segment
    pub async fn flush(&self) -> StorageResult<()> {
        // Take buffer contents
        let points = {
            let mut buffer = self.write_buffer.write().await;
            std::mem::take(&mut *buffer)
        };

        if points.is_empty() {
            return Ok(());
        }

        tracing::debug!("Flushing {} points to segment", points.len());

        // Collect info for indexing before moving points
        let min_timestamp = points.iter().map(|p| p.timestamp).min().unwrap_or(0);
        let metrics: Vec<u32> = points.iter().map(|p| p.metric_id).collect::<HashSet<_>>().into_iter().collect();

        // Collect all unique tags from the points
        let mut all_tags: HashMap<String, String> = HashMap::new();
        for point in &points {
            for (k, v) in &point.tags {
                all_tags.insert(k.clone(), v.clone());
            }
        }

        // Write to segment
        let (segment_id, block_idx) = {
            let mut state = self.state.write().await;

            // Get or create current segment
            let segment_path = self
                .config
                .segments_dir()
                .join(format!("segment_{:06}.dat", state.current_segment_id));

            let mut segment = if segment_path.exists() {
                Segment::open(&segment_path)?
            } else {
                Segment::create(&segment_path, self.config.compression)?
            };

            let block_idx = segment.header.block_count;
            let segment_id = state.current_segment_id;

            // Append block
            segment.append_block(&points)?;

            // Check if segment needs rotation
            let segment_size = std::fs::metadata(&segment_path)?.len();
            if segment_size >= self.config.max_segment_size {
                tracing::info!(
                    "Rotating segment {} (size: {} bytes)",
                    state.current_segment_id,
                    segment_size
                );
                state.segments.push(segment);
                state.current_segment_id += 1;
            } else {
                // Update or add to segments list
                let existing_idx = state.segments.iter().position(|s| s.path == segment.path);
                if let Some(idx) = existing_idx {
                    state.segments[idx] = segment;
                } else {
                    state.segments.push(segment);
                }
            }

            // Re-sort segments
            state.segments.sort_by_key(|s| s.header.min_timestamp);

            (segment_id, block_idx)
        };

        // Update indexes
        {
            let mut index = self.index.lock().map_err(|e| {
                StorageError::Lock(format!("Failed to acquire index lock: {}", e))
            })?;
            index.index_block(segment_id, block_idx, min_timestamp, &metrics, &all_tags)?;
        }

        // Truncate WAL after successful flush
        {
            let mut wal = self.wal.write().await;
            wal.truncate()?;
        }

        Ok(())
    }

    /// Query data points in a time range
    pub async fn query(
        &self,
        range: TimeRange,
        filter: Option<QueryFilter>,
    ) -> StorageResult<Vec<DataPoint>> {
        let mut results = Vec::new();

        // First check write buffer for unflushed points
        {
            let buffer = self.write_buffer.read().await;
            for point in buffer.iter() {
                if range.contains(point.timestamp) {
                    if let Some(ref f) = filter {
                        let registry = self.metrics.read().await;
                        let metric = registry.get_by_id(point.metric_id);
                        if f.matches(point, metric) {
                            results.push(point.clone());
                        }
                    } else {
                        results.push(point.clone());
                    }
                }
            }
        }

        // Query segments
        {
            let mut state = self.state.write().await;

            for segment in &mut state.segments {
                if !segment.overlaps(&range) {
                    continue;
                }

                let points = segment.read_range(&range)?;

                if let Some(ref f) = filter {
                    let registry = self.metrics.read().await;
                    for point in points {
                        let metric = registry.get_by_id(point.metric_id);
                        if f.matches(&point, metric) {
                            results.push(point);
                        }
                    }
                } else {
                    results.extend(points);
                }
            }
        }

        // Sort by timestamp
        results.sort_by_key(|p| p.timestamp);

        Ok(results)
    }

    /// Query with metric name filter (convenience method)
    pub async fn query_metric(
        &self,
        metric_name: &str,
        range: TimeRange,
    ) -> StorageResult<Vec<DataPoint>> {
        let metric_id = {
            let registry = self.metrics.read().await;
            registry
                .get_by_name(metric_name)
                .map(|m| m.id)
                .ok_or_else(|| StorageError::MetricNotFound(metric_name.to_string()))?
        };

        self.query(range, Some(QueryFilter::new().metric_id(metric_id)))
            .await
    }

    /// Register a new metric
    pub async fn register_metric(&self, metric: Metric) -> StorageResult<u32> {
        let id = {
            let mut registry = self.metrics.write().await;
            let id = registry.register(metric);
            registry.save(&self.config.metrics_path())?;
            id
        };

        Ok(id)
    }

    /// Get all registered metrics
    pub async fn get_metrics(&self) -> Vec<Metric> {
        let registry = self.metrics.read().await;
        registry.all().to_vec()
    }

    /// Get a specific metric by name
    pub async fn get_metric(&self, name: &str) -> Option<Metric> {
        let registry = self.metrics.read().await;
        registry.get_by_name(name).cloned()
    }

    /// Get storage statistics
    pub async fn stats(&self) -> StorageStats {
        let state = self.state.read().await;
        let buffer = self.write_buffer.read().await;
        let wal = self.wal.read().await;

        let segment_count = state.segments.len();
        let total_points: u64 = state.segments.iter().map(|s| s.point_count()).sum();
        let buffer_points = buffer.len();
        let wal_entries = wal.entry_count();

        // Calculate total storage size
        let storage_size: u64 = state
            .segments
            .iter()
            .filter_map(|s| std::fs::metadata(&s.path).ok())
            .map(|m| m.len())
            .sum();

        StorageStats {
            segment_count,
            total_points,
            buffer_points,
            wal_entries,
            storage_size_bytes: storage_size,
        }
    }

    /// Start background flush task
    pub fn start_background_flush(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let engine = Arc::clone(self);
        let flush_interval = Duration::from_millis(engine.config.flush_interval_ms);

        tokio::spawn(async move {
            let mut ticker = interval(flush_interval);

            loop {
                ticker.tick().await;

                // Check shutdown
                if *engine.shutdown.read().await {
                    break;
                }

                // Flush if buffer has data
                let has_data = !engine.write_buffer.read().await.is_empty();
                if has_data {
                    if let Err(e) = engine.flush().await {
                        tracing::error!("Background flush failed: {}", e);
                    }
                }
            }

            // Final flush on shutdown
            if let Err(e) = engine.flush().await {
                tracing::error!("Final flush failed: {}", e);
            }
        })
    }

    /// Shutdown the engine gracefully
    pub async fn shutdown(&self) -> StorageResult<()> {
        *self.shutdown.write().await = true;

        // Final flush
        self.flush().await?;

        // Sync WAL
        {
            let mut wal = self.wal.write().await;
            wal.sync()?;
        }

        // Persist indexes
        {
            let mut index = self.index.lock().map_err(|e| {
                StorageError::Lock(format!("Failed to acquire index lock: {}", e))
            })?;
            index.persist()?;
        }

        Ok(())
    }

    /// Get the data directory path
    pub fn data_dir(&self) -> &Path {
        &self.config.data_dir
    }

    /// Get index statistics
    pub fn index_stats(&self) -> crate::index::IndexStats {
        let index = self.index.lock().unwrap();
        index.stats()
    }

    /// Get time bounds of all indexed data
    pub fn time_bounds(&self) -> Option<(i64, i64)> {
        let index = self.index.lock().unwrap();
        index.time_bounds()
    }
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub segment_count: usize,
    pub total_points: u64,
    pub buffer_points: usize,
    pub wal_entries: u64,
    pub storage_size_bytes: u64,
}

impl std::fmt::Display for StorageStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Segments: {}, Points: {}, Buffer: {}, WAL: {}, Size: {:.2} MB",
            self.segment_count,
            self.total_points,
            self.buffer_points,
            self.wal_entries,
            self.storage_size_bytes as f64 / (1024.0 * 1024.0)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::AggregationType;
    use tempfile::tempdir;

    async fn create_test_engine() -> (StorageEngine, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let config = StorageConfig::new(dir.path());
        let engine = StorageEngine::new(config).await.unwrap();
        (engine, dir)
    }

    #[tokio::test]
    async fn test_engine_creation() {
        let (engine, _dir) = create_test_engine().await;
        let stats = engine.stats().await;
        assert_eq!(stats.segment_count, 0);
        assert_eq!(stats.total_points, 0);
    }

    #[tokio::test]
    async fn test_metric_registration() {
        let (engine, _dir) = create_test_engine().await;

        let id = engine
            .register_metric(Metric::new(
                "mood",
                "1-10",
                Category::Mood,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        assert_eq!(id, 0);

        let metric = engine.get_metric("mood").await.unwrap();
        assert_eq!(metric.name, "mood");
        assert_eq!(metric.category, Category::Mood);
    }

    #[tokio::test]
    async fn test_write_and_query() {
        let (engine, _dir) = create_test_engine().await;

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

        // Write points
        for i in 0..100 {
            let point = DataPoint::new(metric_id, 7.0 + (i as f64 * 0.01));
            engine.write(point).await.unwrap();
        }

        // Flush
        engine.flush().await.unwrap();

        // Query
        let range = TimeRange::last_hours(1);
        let points = engine.query(range, None).await.unwrap();

        assert_eq!(points.len(), 100);
    }

    #[tokio::test]
    async fn test_batch_write() {
        let (engine, _dir) = create_test_engine().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "steps",
                "count",
                Category::Health,
                AggregationType::Sum,
            ))
            .await
            .unwrap();

        let points: Vec<DataPoint> = (0..1000)
            .map(|i| DataPoint::new(metric_id, i as f64 * 100.0))
            .collect();

        engine.write_batch(points).await.unwrap();
        engine.flush().await.unwrap();

        let range = TimeRange::last_hours(1);
        let results = engine.query(range, None).await.unwrap();

        assert_eq!(results.len(), 1000);
    }

    #[tokio::test]
    async fn test_query_with_filter() {
        let (engine, _dir) = create_test_engine().await;

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

        // Write mixed points
        for i in 0..50 {
            engine
                .write(DataPoint::new(mood_id, 7.0 + (i as f64 * 0.01)))
                .await
                .unwrap();
            engine
                .write(DataPoint::new(energy_id, 6.0 + (i as f64 * 0.01)))
                .await
                .unwrap();
        }

        engine.flush().await.unwrap();

        // Query only mood
        let range = TimeRange::last_hours(1);
        let mood_points = engine.query_metric("mood", range).await.unwrap();

        assert_eq!(mood_points.len(), 50);
        for point in mood_points {
            assert_eq!(point.metric_id, mood_id);
        }
    }

    #[tokio::test]
    async fn test_persistence() {
        let dir = tempdir().unwrap();
        let config = StorageConfig::new(dir.path());

        let metric_id;

        // First session: write data
        {
            let engine = StorageEngine::new(config.clone()).await.unwrap();

            metric_id = engine
                .register_metric(Metric::new(
                    "test",
                    "units",
                    Category::Custom,
                    AggregationType::Average,
                ))
                .await
                .unwrap();

            for i in 0..100 {
                engine
                    .write(DataPoint::new(metric_id, i as f64))
                    .await
                    .unwrap();
            }

            engine.flush().await.unwrap();
        }

        // Second session: verify data persisted
        {
            let engine = StorageEngine::new(config).await.unwrap();

            // Metric should exist
            let metric = engine.get_metric("test").await.unwrap();
            assert_eq!(metric.id, metric_id);

            // Data should be queryable
            let range = TimeRange::last_hours(1);
            let points = engine.query(range, None).await.unwrap();

            assert_eq!(points.len(), 100);
        }
    }

    #[tokio::test]
    async fn test_wal_recovery() {
        let dir = tempdir().unwrap();
        let config = StorageConfig::new(dir.path());

        let metric_id;

        // First session: write data WITHOUT flush (simulating crash)
        {
            let engine = StorageEngine::new(config.clone()).await.unwrap();

            metric_id = engine
                .register_metric(Metric::new(
                    "test",
                    "units",
                    Category::Custom,
                    AggregationType::Average,
                ))
                .await
                .unwrap();

            for i in 0..50 {
                engine
                    .write(DataPoint::new(metric_id, i as f64))
                    .await
                    .unwrap();
            }

            // Sync WAL but don't flush to segments
            engine.wal.write().await.sync().unwrap();
            // Drop engine without flush
        }

        // Second session: should recover from WAL
        {
            let engine = StorageEngine::new(config).await.unwrap();

            let range = TimeRange::last_hours(1);
            let points = engine.query(range, None).await.unwrap();

            // Should have recovered all 50 points
            assert_eq!(points.len(), 50);
        }
    }

    #[tokio::test]
    async fn test_empty_query() {
        let (engine, _dir) = create_test_engine().await;

        // Query with no data
        let range = TimeRange::last_hours(1);
        let points = engine.query(range, None).await.unwrap();

        assert!(points.is_empty());
    }

    #[tokio::test]
    async fn test_stats() {
        let (engine, _dir) = create_test_engine().await;

        let metric_id = engine
            .register_metric(Metric::new(
                "test",
                "units",
                Category::Custom,
                AggregationType::Average,
            ))
            .await
            .unwrap();

        // Write and flush
        for i in 0..100 {
            engine
                .write(DataPoint::new(metric_id, i as f64))
                .await
                .unwrap();
        }
        engine.flush().await.unwrap();

        let stats = engine.stats().await;
        assert!(stats.segment_count > 0);
        assert_eq!(stats.total_points, 100);
        assert_eq!(stats.buffer_points, 0);
        assert!(stats.storage_size_bytes > 0);
    }
}
