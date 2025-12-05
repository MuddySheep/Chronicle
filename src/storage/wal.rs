//! Write-Ahead Log (WAL) for durability guarantees
//!
//! The WAL ensures no data is lost in case of crashes by persisting
//! every write before acknowledging it. On recovery, unflushed entries
//! are replayed to the write buffer.
//!
//! Format per entry:
//! - length: u32 (4 bytes)
//! - data: [u8; length] (serialized DataPoint)
//! - crc: u32 (4 bytes, CRC32 of length + data)

use crate::storage::error::{StorageError, StorageResult};
use crate::storage::types::DataPoint;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Sync strategy for WAL writes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalSyncMode {
    /// Fsync after every write (safest, slowest)
    EveryWrite,
    /// Fsync in batches (balanced)
    Batched,
    /// No fsync, rely on OS (fastest, risk of loss)
    None,
}

impl Default for WalSyncMode {
    fn default() -> Self {
        WalSyncMode::Batched
    }
}

/// Write-Ahead Log for durability
pub struct WriteAheadLog {
    /// File handle for writing
    writer: BufWriter<File>,
    /// Path to WAL file
    path: PathBuf,
    /// Number of entries written
    entry_count: u64,
    /// Bytes written since last sync
    bytes_since_sync: usize,
    /// Sync mode
    sync_mode: WalSyncMode,
    /// Batch sync threshold (bytes)
    sync_threshold: usize,
}

impl WriteAheadLog {
    /// Open or create a WAL file
    pub fn open(path: impl AsRef<Path>, sync_mode: WalSyncMode) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;

        // Count existing entries
        let entry_count = Self::count_entries(&path)?;

        Ok(Self {
            writer: BufWriter::new(file),
            path,
            entry_count,
            bytes_since_sync: 0,
            sync_mode,
            sync_threshold: 64 * 1024, // 64KB default batch
        })
    }

    /// Count entries in existing WAL (for recovery)
    fn count_entries(path: &Path) -> StorageResult<u64> {
        if !path.exists() {
            return Ok(0);
        }

        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut count = 0u64;

        loop {
            match Self::read_entry_from(&mut reader) {
                Ok(Some(_)) => count += 1,
                Ok(None) => break, // EOF
                Err(e) => {
                    // Log corruption but continue counting valid entries
                    tracing::warn!("WAL corruption at entry {}: {}", count, e);
                    break;
                }
            }
        }

        Ok(count)
    }

    /// Append a data point to the WAL
    pub fn append(&mut self, point: &DataPoint) -> StorageResult<()> {
        // Serialize the data point
        let data = bincode::serialize(point)?;

        // Calculate CRC
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&(data.len() as u32).to_le_bytes());
        hasher.update(&data);
        let crc = hasher.finalize();

        // Write: length (4) + data (N) + crc (4)
        self.writer.write_all(&(data.len() as u32).to_le_bytes())?;
        self.writer.write_all(&data)?;
        self.writer.write_all(&crc.to_le_bytes())?;

        self.entry_count += 1;
        self.bytes_since_sync += 8 + data.len();

        // Sync based on mode
        self.maybe_sync()?;

        Ok(())
    }

    /// Append multiple data points efficiently
    pub fn append_batch(&mut self, points: &[DataPoint]) -> StorageResult<()> {
        for point in points {
            // Serialize the data point
            let data = bincode::serialize(point)?;

            // Calculate CRC
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&(data.len() as u32).to_le_bytes());
            hasher.update(&data);
            let crc = hasher.finalize();

            // Write: length (4) + data (N) + crc (4)
            self.writer.write_all(&(data.len() as u32).to_le_bytes())?;
            self.writer.write_all(&data)?;
            self.writer.write_all(&crc.to_le_bytes())?;

            self.entry_count += 1;
            self.bytes_since_sync += 8 + data.len();
        }

        // Sync based on mode
        self.maybe_sync()?;

        Ok(())
    }

    /// Conditionally sync based on mode and threshold
    fn maybe_sync(&mut self) -> StorageResult<()> {
        match self.sync_mode {
            WalSyncMode::EveryWrite => {
                self.sync()?;
            }
            WalSyncMode::Batched => {
                if self.bytes_since_sync >= self.sync_threshold {
                    self.sync()?;
                }
            }
            WalSyncMode::None => {
                // Just flush the buffer, no fsync
                self.writer.flush()?;
            }
        }
        Ok(())
    }

    /// Force sync to disk
    pub fn sync(&mut self) -> StorageResult<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        self.bytes_since_sync = 0;
        Ok(())
    }

    /// Read all entries for recovery
    pub fn recover(&self) -> StorageResult<Vec<DataPoint>> {
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut points = Vec::new();

        loop {
            match Self::read_entry_from(&mut reader) {
                Ok(Some(point)) => points.push(point),
                Ok(None) => break, // EOF
                Err(e) => {
                    tracing::warn!("WAL recovery stopped at entry {}: {}", points.len(), e);
                    break;
                }
            }
        }

        Ok(points)
    }

    /// Read a single entry from a reader
    fn read_entry_from<R: Read>(reader: &mut R) -> StorageResult<Option<DataPoint>> {
        // Read length
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let len = u32::from_le_bytes(len_buf) as usize;

        // Sanity check on length (max 1MB per entry)
        if len > 1_000_000 {
            return Err(StorageError::WalError(format!(
                "Entry length too large: {}",
                len
            )));
        }

        // Read data
        let mut data = vec![0u8; len];
        reader.read_exact(&mut data)?;

        // Read CRC
        let mut crc_buf = [0u8; 4];
        reader.read_exact(&mut crc_buf)?;
        let stored_crc = u32::from_le_bytes(crc_buf);

        // Verify CRC
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&len_buf);
        hasher.update(&data);
        let computed_crc = hasher.finalize();

        if stored_crc != computed_crc {
            return Err(StorageError::Corruption(format!(
                "CRC mismatch: stored={}, computed={}",
                stored_crc, computed_crc
            )));
        }

        // Deserialize
        let point: DataPoint = bincode::deserialize(&data)?;
        Ok(Some(point))
    }

    /// Truncate the WAL (after successful flush to segment)
    pub fn truncate(&mut self) -> StorageResult<()> {
        // Sync first to ensure all data is written
        self.sync()?;

        // Close current writer
        drop(std::mem::replace(
            &mut self.writer,
            BufWriter::new(File::open(&self.path)?),
        ));

        // Truncate the file
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)?;

        self.writer = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );

        drop(file); // Close truncate handle

        self.entry_count = 0;
        self.bytes_since_sync = 0;

        Ok(())
    }

    /// Get the number of entries in the WAL
    pub fn entry_count(&self) -> u64 {
        self.entry_count
    }

    /// Check if WAL has pending entries
    pub fn has_pending(&self) -> bool {
        self.entry_count > 0
    }

    /// Get the file size
    pub fn file_size(&self) -> StorageResult<u64> {
        Ok(std::fs::metadata(&self.path)?.len())
    }
}

/// WAL entry iterator for streaming recovery
pub struct WalIterator {
    reader: BufReader<File>,
    entries_read: u64,
}

impl WalIterator {
    pub fn new(path: impl AsRef<Path>) -> StorageResult<Self> {
        let file = File::open(path.as_ref())?;
        Ok(Self {
            reader: BufReader::new(file),
            entries_read: 0,
        })
    }
}

impl Iterator for WalIterator {
    type Item = StorageResult<DataPoint>;

    fn next(&mut self) -> Option<Self::Item> {
        match WriteAheadLog::read_entry_from(&mut self.reader) {
            Ok(Some(point)) => {
                self.entries_read += 1;
                Some(Ok(point))
            }
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom};
    use tempfile::tempdir;

    #[test]
    fn test_wal_basic_operations() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        // Create and write
        {
            let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();

            let point1 = DataPoint::with_timestamp(1, 7.5, 1000);
            let point2 = DataPoint::with_timestamp(2, 8.0, 2000);

            wal.append(&point1).unwrap();
            wal.append(&point2).unwrap();

            assert_eq!(wal.entry_count(), 2);
        }

        // Recover
        {
            let wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            let recovered = wal.recover().unwrap();

            assert_eq!(recovered.len(), 2);
            assert_eq!(recovered[0].timestamp, 1000);
            assert_eq!(recovered[0].value, 7.5);
            assert_eq!(recovered[1].timestamp, 2000);
            assert_eq!(recovered[1].value, 8.0);
        }
    }

    #[test]
    fn test_wal_truncate() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();

        // Write some entries
        for i in 0..10 {
            let point = DataPoint::with_timestamp(1, i as f64, i * 1000);
            wal.append(&point).unwrap();
        }
        assert_eq!(wal.entry_count(), 10);

        // Truncate
        wal.truncate().unwrap();
        assert_eq!(wal.entry_count(), 0);

        // Verify file is empty
        let recovered = wal.recover().unwrap();
        assert!(recovered.is_empty());
    }

    #[test]
    fn test_wal_batch_append() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::Batched).unwrap();

        let points: Vec<DataPoint> = (0..100)
            .map(|i| DataPoint::with_timestamp(1, i as f64, i * 1000))
            .collect();

        wal.append_batch(&points).unwrap();
        wal.sync().unwrap();

        assert_eq!(wal.entry_count(), 100);

        let recovered = wal.recover().unwrap();
        assert_eq!(recovered.len(), 100);
    }

    #[test]
    fn test_wal_crc_corruption_detection() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        // Write valid entry
        {
            let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            let point = DataPoint::with_timestamp(1, 7.5, 1000);
            wal.append(&point).unwrap();
        }

        // Corrupt the file
        {
            use std::io::Write;
            let mut file = OpenOptions::new().write(true).open(&wal_path).unwrap();
            file.seek(SeekFrom::Start(10)).unwrap();
            file.write_all(&[0xFF, 0xFF]).unwrap(); // Corrupt data
        }

        // Recovery should detect corruption
        {
            let wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            let recovered = wal.recover().unwrap();
            // Should stop at corrupted entry
            assert!(recovered.is_empty());
        }
    }

    #[test]
    fn test_wal_iterator() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        // Write entries
        {
            let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            for i in 0..5 {
                let point = DataPoint::with_timestamp(1, i as f64, i * 1000);
                wal.append(&point).unwrap();
            }
        }

        // Iterate
        let iter = WalIterator::new(&wal_path).unwrap();
        let points: Vec<DataPoint> = iter.filter_map(|r| r.ok()).collect();

        assert_eq!(points.len(), 5);
        for (i, point) in points.iter().enumerate() {
            assert_eq!(point.value, i as f64);
        }
    }

    #[test]
    fn test_wal_with_tags() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();

        let point = DataPoint::with_timestamp(1, 7.5, 1000)
            .tag("source", "manual")
            .tag("location", "home");

        wal.append(&point).unwrap();

        let recovered = wal.recover().unwrap();
        assert_eq!(recovered.len(), 1);
        assert!(recovered[0].has_tag("source", "manual"));
        assert!(recovered[0].has_tag("location", "home"));
    }

    #[test]
    fn test_wal_persistence_across_opens() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        // First session: write entries
        {
            let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            for i in 0..5 {
                let point = DataPoint::with_timestamp(1, i as f64, i * 1000);
                wal.append(&point).unwrap();
            }
        }

        // Second session: open and verify count
        {
            let wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            assert_eq!(wal.entry_count(), 5);
        }

        // Third session: add more entries
        {
            let mut wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            for i in 5..10 {
                let point = DataPoint::with_timestamp(1, i as f64, i * 1000);
                wal.append(&point).unwrap();
            }
        }

        // Fourth session: verify all entries
        {
            let wal = WriteAheadLog::open(&wal_path, WalSyncMode::EveryWrite).unwrap();
            assert_eq!(wal.entry_count(), 10);
            let recovered = wal.recover().unwrap();
            assert_eq!(recovered.len(), 10);
        }
    }
}
