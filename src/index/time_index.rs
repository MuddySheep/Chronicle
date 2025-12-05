//! Time Index - SQLite-backed B-tree for time range queries
//!
//! Uses SQLite's built-in B-tree for efficient time range lookups.
//! Indexes block boundaries rather than individual points for efficiency.
//!
//! # Performance
//! - Insert: O(log n) per entry
//! - Range query: O(log n + k) where k = results
//! - Expected: 1M entries in < 10 seconds, range query < 10ms

use crate::index::DataLocation;
use crate::storage::StorageError;
use rusqlite::{params, Connection, OpenFlags};
use std::path::{Path, PathBuf};

/// SQLite-backed time index for O(log n) time range queries
pub struct TimeIndex {
    conn: Connection,
    path: PathBuf,
}

impl TimeIndex {
    /// Create or open a time index
    pub fn new(data_dir: &Path) -> Result<Self, StorageError> {
        let path = data_dir.join("time_index.db");

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Configure for performance
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = 10000;
            PRAGMA temp_store = MEMORY;
            ",
        )
        .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Create table and index
        conn.execute(
            "CREATE TABLE IF NOT EXISTS time_index (
                timestamp INTEGER NOT NULL,
                segment_id INTEGER NOT NULL,
                block_idx INTEGER NOT NULL,
                PRIMARY KEY (timestamp, segment_id, block_idx)
            )",
            [],
        )
        .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Create index on timestamp for fast range queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON time_index(timestamp)",
            [],
        )
        .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Create index for segment lookups
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_segment ON time_index(segment_id)",
            [],
        )
        .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(Self { conn, path })
    }

    /// Insert block boundaries for a segment
    ///
    /// # Arguments
    /// * `segment_id` - The segment being indexed
    /// * `block_boundaries` - Vec of (block_idx, min_timestamp) pairs
    pub fn insert_range(
        &mut self,
        segment_id: u32,
        block_boundaries: &[(u32, i64)],
    ) -> Result<(), StorageError> {
        if block_boundaries.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction()
            .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO time_index (timestamp, segment_id, block_idx)
                     VALUES (?, ?, ?)",
                )
                .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

            for (block_idx, timestamp) in block_boundaries {
                stmt.execute(params![timestamp, segment_id, block_idx])
                    .map_err(|e| {
                        StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
                    })?;
            }
        }

        tx.commit()
            .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    /// Insert a single entry
    pub fn insert(
        &mut self,
        timestamp: i64,
        segment_id: u32,
        block_idx: u32,
    ) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO time_index (timestamp, segment_id, block_idx)
                 VALUES (?, ?, ?)",
                params![timestamp, segment_id, block_idx],
            )
            .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    /// Find all locations in a time range [start, end)
    ///
    /// Returns locations sorted by timestamp
    pub fn find_range(&self, start: i64, end: i64) -> Vec<DataLocation> {
        let mut stmt = match self.conn.prepare_cached(
            "SELECT DISTINCT segment_id, block_idx FROM time_index
             WHERE timestamp >= ? AND timestamp < ?
             ORDER BY timestamp, segment_id, block_idx",
        ) {
            Ok(stmt) => stmt,
            Err(e) => {
                tracing::error!("Failed to prepare time index query: {}", e);
                return Vec::new();
            }
        };

        let rows = match stmt.query_map(params![start, end], |row| {
            Ok(DataLocation {
                segment_id: row.get(0)?,
                block_idx: row.get(1)?,
            })
        }) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!("Failed to execute time index query: {}", e);
                return Vec::new();
            }
        };

        rows.filter_map(Result::ok).collect()
    }

    /// Find locations that might contain data before a timestamp
    /// (for finding the block that contains the start of a range)
    pub fn find_floor(&self, timestamp: i64) -> Option<DataLocation> {
        let mut stmt = match self.conn.prepare_cached(
            "SELECT segment_id, block_idx FROM time_index
             WHERE timestamp <= ?
             ORDER BY timestamp DESC
             LIMIT 1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return None,
        };

        stmt.query_row(params![timestamp], |row| {
            Ok(DataLocation {
                segment_id: row.get(0)?,
                block_idx: row.get(1)?,
            })
        })
        .ok()
    }

    /// Find locations that might contain data after a timestamp
    pub fn find_ceiling(&self, timestamp: i64) -> Option<DataLocation> {
        let mut stmt = match self.conn.prepare_cached(
            "SELECT segment_id, block_idx FROM time_index
             WHERE timestamp >= ?
             ORDER BY timestamp ASC
             LIMIT 1",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return None,
        };

        stmt.query_row(params![timestamp], |row| {
            Ok(DataLocation {
                segment_id: row.get(0)?,
                block_idx: row.get(1)?,
            })
        })
        .ok()
    }

    /// Find all locations for a specific segment
    pub fn find_by_segment(&self, segment_id: u32) -> Vec<DataLocation> {
        let mut stmt = match self.conn.prepare_cached(
            "SELECT segment_id, block_idx FROM time_index
             WHERE segment_id = ?
             ORDER BY block_idx",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };

        let rows = match stmt.query_map(params![segment_id], |row| {
            Ok(DataLocation {
                segment_id: row.get(0)?,
                block_idx: row.get(1)?,
            })
        }) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };

        rows.filter_map(Result::ok).collect()
    }

    /// Remove all entries for a segment (used during compaction)
    pub fn remove_segment(&mut self, segment_id: u32) -> Result<(), StorageError> {
        self.conn
            .execute(
                "DELETE FROM time_index WHERE segment_id = ?",
                params![segment_id],
            )
            .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    /// Get count of entries in the index
    pub fn count(&self) -> u64 {
        let result: Result<i64, _> =
            self.conn
                .query_row("SELECT COUNT(*) FROM time_index", [], |row| row.get(0));

        result.unwrap_or(0) as u64
    }

    /// Get timestamp range covered by index
    pub fn time_bounds(&self) -> Option<(i64, i64)> {
        let result: Result<(i64, i64), _> = self.conn.query_row(
            "SELECT MIN(timestamp), MAX(timestamp) FROM time_index",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        result.ok()
    }

    /// Optimize the index (VACUUM)
    pub fn optimize(&mut self) -> Result<(), StorageError> {
        self.conn
            .execute("VACUUM", [])
            .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    /// Force checkpoint for WAL mode
    pub fn checkpoint(&mut self) -> Result<(), StorageError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| StorageError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(())
    }

    /// Get the database file path
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_time_index_creation() {
        let dir = tempdir().unwrap();
        let index = TimeIndex::new(dir.path()).unwrap();
        assert_eq!(index.count(), 0);
    }

    #[test]
    fn test_insert_and_query() {
        let dir = tempdir().unwrap();
        let mut index = TimeIndex::new(dir.path()).unwrap();

        // Insert some entries
        index.insert(1000, 1, 0).unwrap();
        index.insert(2000, 1, 1).unwrap();
        index.insert(3000, 2, 0).unwrap();

        assert_eq!(index.count(), 3);

        // Query range
        let results = index.find_range(1500, 2500);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].segment_id, 1);
        assert_eq!(results[0].block_idx, 1);
    }

    #[test]
    fn test_insert_range() {
        let dir = tempdir().unwrap();
        let mut index = TimeIndex::new(dir.path()).unwrap();

        // Insert block boundaries for segment 1
        let boundaries: Vec<(u32, i64)> = (0..100).map(|i| (i as u32, i as i64 * 1000)).collect();

        index.insert_range(1, &boundaries).unwrap();

        assert_eq!(index.count(), 100);

        // Query middle range
        let results = index.find_range(25000, 75000);
        assert!(results.len() >= 50);
    }

    #[test]
    fn test_find_floor() {
        let dir = tempdir().unwrap();
        let mut index = TimeIndex::new(dir.path()).unwrap();

        index.insert(1000, 1, 0).unwrap();
        index.insert(2000, 1, 1).unwrap();
        index.insert(3000, 2, 0).unwrap();

        // Find floor for timestamp 2500
        let floor = index.find_floor(2500).unwrap();
        assert_eq!(floor.segment_id, 1);
        assert_eq!(floor.block_idx, 1);

        // Find floor for timestamp before all data
        let floor = index.find_floor(500);
        assert!(floor.is_none());
    }

    #[test]
    fn test_find_ceiling() {
        let dir = tempdir().unwrap();
        let mut index = TimeIndex::new(dir.path()).unwrap();

        index.insert(1000, 1, 0).unwrap();
        index.insert(2000, 1, 1).unwrap();
        index.insert(3000, 2, 0).unwrap();

        // Find ceiling for timestamp 1500
        let ceiling = index.find_ceiling(1500).unwrap();
        assert_eq!(ceiling.segment_id, 1);
        assert_eq!(ceiling.block_idx, 1);
    }

    #[test]
    fn test_remove_segment() {
        let dir = tempdir().unwrap();
        let mut index = TimeIndex::new(dir.path()).unwrap();

        // Insert entries for two segments
        index.insert_range(1, &[(0, 1000), (1, 2000)]).unwrap();
        index.insert_range(2, &[(0, 3000), (1, 4000)]).unwrap();

        assert_eq!(index.count(), 4);

        // Remove segment 1
        index.remove_segment(1).unwrap();

        assert_eq!(index.count(), 2);

        // Only segment 2 entries remain
        let results = index.find_range(0, 5000);
        assert!(results.iter().all(|loc| loc.segment_id == 2));
    }

    #[test]
    fn test_time_bounds() {
        let dir = tempdir().unwrap();
        let mut index = TimeIndex::new(dir.path()).unwrap();

        index.insert(1000, 1, 0).unwrap();
        index.insert(5000, 2, 0).unwrap();
        index.insert(3000, 1, 1).unwrap();

        let (min, max) = index.time_bounds().unwrap();
        assert_eq!(min, 1000);
        assert_eq!(max, 5000);
    }

    #[test]
    fn test_large_insert_performance() {
        let dir = tempdir().unwrap();
        let mut index = TimeIndex::new(dir.path()).unwrap();

        // Insert 100K entries (should complete quickly)
        let start = std::time::Instant::now();

        let boundaries: Vec<(u32, i64)> =
            (0..100_000).map(|i| (i as u32 % 1000, i as i64)).collect();

        for segment_id in 0..100 {
            let segment_boundaries: Vec<(u32, i64)> = boundaries
                .iter()
                .skip(segment_id * 1000)
                .take(1000)
                .copied()
                .collect();
            index.insert_range(segment_id as u32, &segment_boundaries).unwrap();
        }

        let elapsed = start.elapsed();
        println!("Inserted 100K entries in {:?}", elapsed);

        // Should complete in reasonable time
        assert!(elapsed.as_secs() < 10, "Insert took too long: {:?}", elapsed);

        // Query should be fast
        let query_start = std::time::Instant::now();
        let results = index.find_range(25000, 75000);
        let query_elapsed = query_start.elapsed();

        println!(
            "Range query returned {} results in {:?}",
            results.len(),
            query_elapsed
        );
        assert!(
            query_elapsed.as_millis() < 100,
            "Query took too long: {:?}",
            query_elapsed
        );
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();

        // Create and populate
        {
            let mut index = TimeIndex::new(dir.path()).unwrap();
            index.insert(1000, 1, 0).unwrap();
            index.insert(2000, 1, 1).unwrap();
        }

        // Reopen and verify
        {
            let index = TimeIndex::new(dir.path()).unwrap();
            assert_eq!(index.count(), 2);

            let results = index.find_range(0, 3000);
            assert_eq!(results.len(), 2);
        }
    }
}
