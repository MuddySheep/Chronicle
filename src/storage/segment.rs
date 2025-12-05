//! Segment file format for Chronicle storage
//!
//! Segment files store compressed blocks of time-series data.
//!
//! Layout:
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ HEADER (64 bytes)                       │
//! │   magic: [u8; 4] = "CHRN"               │
//! │   version: u16                          │
//! │   block_count: u32                      │
//! │   min_timestamp: i64                    │
//! │   max_timestamp: i64                    │
//! │   compression: u8                       │
//! │   checksum: u32                         │
//! │   reserved: [u8; 31]                    │
//! ├─────────────────────────────────────────┤
//! │ BLOCKS (variable)                       │
//! │   For each block:                       │
//! │     block_size: u32                     │
//! │     compressed_data: [u8; block_size]   │
//! │     block_checksum: u32                 │
//! ├─────────────────────────────────────────┤
//! │ FOOTER                                  │
//! │   block_offsets: Vec<u64>               │
//! │   footer_size: u32                      │
//! │   segment_checksum: u32                 │
//! └─────────────────────────────────────────┘
//! ```

use crate::storage::compression::{compress_block, decompress_block};
use crate::storage::error::{StorageError, StorageResult};
use crate::storage::types::{DataPoint, TimeRange};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Magic bytes for segment file identification
const SEGMENT_MAGIC: [u8; 4] = *b"CHRN";

/// Current segment format version
const SEGMENT_VERSION: u16 = 1;

/// Header size in bytes
const HEADER_SIZE: usize = 64;

/// Compression type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionType {
    None = 0,
    Lz4 = 1,
}

impl TryFrom<u8> for CompressionType {
    type Error = StorageError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CompressionType::None),
            1 => Ok(CompressionType::Lz4),
            _ => Err(StorageError::InvalidSegment(format!(
                "Unknown compression type: {}",
                value
            ))),
        }
    }
}

/// Segment file header
#[derive(Debug, Clone)]
pub struct SegmentHeader {
    /// Magic bytes (should be "CHRN")
    pub magic: [u8; 4],
    /// Format version
    pub version: u16,
    /// Number of blocks in segment
    pub block_count: u32,
    /// Minimum timestamp across all points
    pub min_timestamp: i64,
    /// Maximum timestamp across all points
    pub max_timestamp: i64,
    /// Compression type used
    pub compression: CompressionType,
    /// Header checksum
    pub checksum: u32,
}

impl SegmentHeader {
    /// Create a new header for a segment
    pub fn new(compression: CompressionType) -> Self {
        Self {
            magic: SEGMENT_MAGIC,
            version: SEGMENT_VERSION,
            block_count: 0,
            min_timestamp: i64::MAX,
            max_timestamp: i64::MIN,
            compression,
            checksum: 0,
        }
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];

        buf[0..4].copy_from_slice(&self.magic);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..10].copy_from_slice(&self.block_count.to_le_bytes());
        buf[10..18].copy_from_slice(&self.min_timestamp.to_le_bytes());
        buf[18..26].copy_from_slice(&self.max_timestamp.to_le_bytes());
        buf[26] = self.compression as u8;
        // bytes 27-59 reserved

        // Calculate checksum of header (excluding checksum field)
        let checksum = crc32fast::hash(&buf[0..60]);
        buf[60..64].copy_from_slice(&checksum.to_le_bytes());

        buf
    }

    /// Parse header from bytes
    pub fn from_bytes(buf: &[u8; HEADER_SIZE]) -> StorageResult<Self> {
        // Verify checksum first
        let stored_checksum = u32::from_le_bytes([buf[60], buf[61], buf[62], buf[63]]);
        let computed_checksum = crc32fast::hash(&buf[0..60]);

        if stored_checksum != computed_checksum {
            return Err(StorageError::Corruption(format!(
                "Header checksum mismatch: stored={}, computed={}",
                stored_checksum, computed_checksum
            )));
        }

        // Parse magic
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&buf[0..4]);

        if magic != SEGMENT_MAGIC {
            return Err(StorageError::InvalidSegment(format!(
                "Invalid magic: {:?}",
                magic
            )));
        }

        let version = u16::from_le_bytes([buf[4], buf[5]]);
        if version > SEGMENT_VERSION {
            return Err(StorageError::InvalidSegment(format!(
                "Unsupported version: {}",
                version
            )));
        }

        let block_count = u32::from_le_bytes([buf[6], buf[7], buf[8], buf[9]]);
        let min_timestamp = i64::from_le_bytes([
            buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16], buf[17],
        ]);
        let max_timestamp = i64::from_le_bytes([
            buf[18], buf[19], buf[20], buf[21], buf[22], buf[23], buf[24], buf[25],
        ]);
        let compression = CompressionType::try_from(buf[26])?;

        Ok(Self {
            magic,
            version,
            block_count,
            min_timestamp,
            max_timestamp,
            compression,
            checksum: stored_checksum,
        })
    }

    /// Update timestamp bounds
    pub fn update_timestamps(&mut self, points: &[DataPoint]) {
        for point in points {
            self.min_timestamp = self.min_timestamp.min(point.timestamp);
            self.max_timestamp = self.max_timestamp.max(point.timestamp);
        }
    }
}

/// Metadata for a single block within a segment
#[derive(Debug, Clone)]
pub struct BlockMeta {
    /// Offset from start of file
    pub offset: u64,
    /// Size of compressed data
    pub size: u32,
    /// Number of points in this block
    pub point_count: u32,
    /// Minimum timestamp in block
    pub min_timestamp: i64,
    /// Maximum timestamp in block
    pub max_timestamp: i64,
}

impl BlockMeta {
    /// Check if block overlaps with time range
    pub fn overlaps(&self, range: &TimeRange) -> bool {
        self.min_timestamp < range.end && self.max_timestamp >= range.start
    }
}

/// A segment file containing compressed data blocks
pub struct Segment {
    /// File path
    pub path: PathBuf,
    /// Segment header
    pub header: SegmentHeader,
    /// Block metadata (for seeking)
    pub blocks: Vec<BlockMeta>,
    /// File handle for reading
    reader: Option<BufReader<File>>,
}

impl Segment {
    /// Create a new segment file
    pub fn create(path: impl AsRef<Path>, compression: CompressionType) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create file and write initial header
        let mut file = BufWriter::new(File::create(&path)?);
        let header = SegmentHeader::new(compression);
        file.write_all(&header.to_bytes())?;
        file.flush()?;

        Ok(Self {
            path,
            header,
            blocks: Vec::new(),
            reader: None,
        })
    }

    /// Open an existing segment file
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = BufReader::new(File::open(&path)?);

        // Read header
        let mut header_buf = [0u8; HEADER_SIZE];
        file.read_exact(&mut header_buf)?;
        let header = SegmentHeader::from_bytes(&header_buf)?;

        // Read footer to get block offsets
        let blocks = Self::read_footer(&mut file, &header)?;

        Ok(Self {
            path,
            header,
            blocks,
            reader: Some(file),
        })
    }

    /// Read footer from segment file
    fn read_footer(file: &mut BufReader<File>, header: &SegmentHeader) -> StorageResult<Vec<BlockMeta>> {
        if header.block_count == 0 {
            return Ok(Vec::new());
        }

        // Seek to end minus footer size indicator (last 4 bytes)
        file.seek(SeekFrom::End(-8))?;

        let mut footer_size_buf = [0u8; 4];
        file.read_exact(&mut footer_size_buf)?;
        let footer_size = u32::from_le_bytes(footer_size_buf);

        let mut checksum_buf = [0u8; 4];
        file.read_exact(&mut checksum_buf)?;
        let stored_checksum = u32::from_le_bytes(checksum_buf);

        // Seek to footer start
        file.seek(SeekFrom::End(-(footer_size as i64) - 8))?;

        // Read footer data
        let mut footer_data = vec![0u8; footer_size as usize];
        file.read_exact(&mut footer_data)?;

        // Verify checksum
        let computed_checksum = crc32fast::hash(&footer_data);
        if stored_checksum != computed_checksum {
            return Err(StorageError::Corruption("Footer checksum mismatch".into()));
        }

        // Parse block metadata
        // Format: for each block: offset(8) + size(4) + point_count(4) + min_ts(8) + max_ts(8) = 32 bytes
        let blocks_per_entry = 32;
        let mut blocks = Vec::with_capacity(header.block_count as usize);

        for i in 0..header.block_count as usize {
            let base = i * blocks_per_entry;
            if base + blocks_per_entry > footer_data.len() {
                break;
            }

            let offset = u64::from_le_bytes([
                footer_data[base],
                footer_data[base + 1],
                footer_data[base + 2],
                footer_data[base + 3],
                footer_data[base + 4],
                footer_data[base + 5],
                footer_data[base + 6],
                footer_data[base + 7],
            ]);
            let size = u32::from_le_bytes([
                footer_data[base + 8],
                footer_data[base + 9],
                footer_data[base + 10],
                footer_data[base + 11],
            ]);
            let point_count = u32::from_le_bytes([
                footer_data[base + 12],
                footer_data[base + 13],
                footer_data[base + 14],
                footer_data[base + 15],
            ]);
            let min_timestamp = i64::from_le_bytes([
                footer_data[base + 16],
                footer_data[base + 17],
                footer_data[base + 18],
                footer_data[base + 19],
                footer_data[base + 20],
                footer_data[base + 21],
                footer_data[base + 22],
                footer_data[base + 23],
            ]);
            let max_timestamp = i64::from_le_bytes([
                footer_data[base + 24],
                footer_data[base + 25],
                footer_data[base + 26],
                footer_data[base + 27],
                footer_data[base + 28],
                footer_data[base + 29],
                footer_data[base + 30],
                footer_data[base + 31],
            ]);

            blocks.push(BlockMeta {
                offset,
                size,
                point_count,
                min_timestamp,
                max_timestamp,
            });
        }

        // Reset to after header for reading
        file.seek(SeekFrom::Start(HEADER_SIZE as u64))?;

        Ok(blocks)
    }

    /// Append a block of data points to the segment
    pub fn append_block(&mut self, points: &[DataPoint]) -> StorageResult<()> {
        if points.is_empty() {
            return Ok(());
        }

        // Compress the block
        let compressed = compress_block(points)?;

        // Calculate timestamps
        let (min_ts, max_ts) = points.iter().fold((i64::MAX, i64::MIN), |(min, max), p| {
            (min.min(p.timestamp), max.max(p.timestamp))
        });

        // Open file for appending
        let file = OpenOptions::new().read(true).write(true).open(&self.path)?;

        let mut writer = BufWriter::new(file);

        // Calculate block offset (after header + existing blocks)
        let block_offset = if self.blocks.is_empty() {
            HEADER_SIZE as u64
        } else {
            let last = self.blocks.last().unwrap();
            last.offset + last.size as u64 + 8 // +8 for block header (size + checksum)
        };

        // Seek to write position
        writer.seek(SeekFrom::Start(block_offset))?;

        // Write block: size (4) + data (N) + checksum (4)
        let checksum = crc32fast::hash(&compressed);
        writer.write_all(&(compressed.len() as u32).to_le_bytes())?;
        writer.write_all(&compressed)?;
        writer.write_all(&checksum.to_le_bytes())?;

        // Add block metadata
        self.blocks.push(BlockMeta {
            offset: block_offset,
            size: compressed.len() as u32,
            point_count: points.len() as u32,
            min_timestamp: min_ts,
            max_timestamp: max_ts,
        });

        // Update header
        self.header.block_count = self.blocks.len() as u32;
        self.header.min_timestamp = self.header.min_timestamp.min(min_ts);
        self.header.max_timestamp = self.header.max_timestamp.max(max_ts);

        // Write footer
        self.write_footer(&mut writer)?;

        // Rewrite header with updated counts
        writer.seek(SeekFrom::Start(0))?;
        writer.write_all(&self.header.to_bytes())?;

        writer.flush()?;

        Ok(())
    }

    /// Write footer with block offsets
    fn write_footer<W: Write + Seek>(&self, writer: &mut W) -> StorageResult<()> {
        // Build footer data
        let mut footer_data = Vec::with_capacity(self.blocks.len() * 32);

        for block in &self.blocks {
            footer_data.extend_from_slice(&block.offset.to_le_bytes());
            footer_data.extend_from_slice(&block.size.to_le_bytes());
            footer_data.extend_from_slice(&block.point_count.to_le_bytes());
            footer_data.extend_from_slice(&block.min_timestamp.to_le_bytes());
            footer_data.extend_from_slice(&block.max_timestamp.to_le_bytes());
        }

        let checksum = crc32fast::hash(&footer_data);

        // Write footer
        writer.write_all(&footer_data)?;
        writer.write_all(&(footer_data.len() as u32).to_le_bytes())?;
        writer.write_all(&checksum.to_le_bytes())?;

        Ok(())
    }

    /// Read and decompress a specific block
    pub fn read_block(&mut self, block_idx: usize) -> StorageResult<Vec<DataPoint>> {
        let block_meta = self.blocks.get(block_idx).ok_or_else(|| {
            StorageError::InvalidSegment(format!("Block index out of range: {}", block_idx))
        })?;

        // Ensure reader is available
        if self.reader.is_none() {
            self.reader = Some(BufReader::new(File::open(&self.path)?));
        }

        let reader = self.reader.as_mut().unwrap();

        // Seek to block
        reader.seek(SeekFrom::Start(block_meta.offset))?;

        // Read block size
        let mut size_buf = [0u8; 4];
        reader.read_exact(&mut size_buf)?;
        let size = u32::from_le_bytes(size_buf);

        // Read compressed data
        let mut data = vec![0u8; size as usize];
        reader.read_exact(&mut data)?;

        // Read and verify checksum
        let mut checksum_buf = [0u8; 4];
        reader.read_exact(&mut checksum_buf)?;
        let stored_checksum = u32::from_le_bytes(checksum_buf);
        let computed_checksum = crc32fast::hash(&data);

        if stored_checksum != computed_checksum {
            return Err(StorageError::Corruption(format!(
                "Block {} checksum mismatch",
                block_idx
            )));
        }

        // Decompress
        decompress_block(&data)
    }

    /// Read all blocks that overlap with a time range
    pub fn read_range(&mut self, range: &TimeRange) -> StorageResult<Vec<DataPoint>> {
        let mut results = Vec::new();

        for idx in 0..self.blocks.len() {
            if self.blocks[idx].overlaps(range) {
                let points = self.read_block(idx)?;
                // Filter to exact range
                results.extend(points.into_iter().filter(|p| range.contains(p.timestamp)));
            }
        }

        Ok(results)
    }

    /// Check if this segment overlaps with a time range
    pub fn overlaps(&self, range: &TimeRange) -> bool {
        self.header.min_timestamp < range.end && self.header.max_timestamp >= range.start
    }

    /// Get total point count across all blocks
    pub fn point_count(&self) -> u64 {
        self.blocks.iter().map(|b| b.point_count as u64).sum()
    }

    /// Get segment ID from filename
    pub fn id(&self) -> Option<u32> {
        self.path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("segment_"))
            .and_then(|s| s.parse().ok())
    }
}

/// Builder for creating segments with multiple blocks
pub struct SegmentBuilder {
    path: PathBuf,
    compression: CompressionType,
    target_block_size: usize,
    buffer: Vec<DataPoint>,
    segment: Option<Segment>,
}

impl SegmentBuilder {
    pub fn new(path: impl AsRef<Path>, compression: CompressionType) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            compression,
            target_block_size: 64 * 1024, // 64KB target
            buffer: Vec::new(),
            segment: None,
        }
    }

    pub fn target_block_size(mut self, size: usize) -> Self {
        self.target_block_size = size;
        self
    }

    /// Add points, flushing to blocks as needed
    pub fn add_points(&mut self, points: Vec<DataPoint>) -> StorageResult<()> {
        self.buffer.extend(points);

        // Estimate buffer size
        let estimated_size: usize = self.buffer.iter().map(|p| p.estimated_size()).sum();

        if estimated_size >= self.target_block_size {
            self.flush_buffer()?;
        }

        Ok(())
    }

    /// Flush buffer to a block
    fn flush_buffer(&mut self) -> StorageResult<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        // Create segment if needed
        if self.segment.is_none() {
            self.segment = Some(Segment::create(&self.path, self.compression)?);
        }

        let segment = self.segment.as_mut().unwrap();
        let points = std::mem::take(&mut self.buffer);
        segment.append_block(&points)?;

        Ok(())
    }

    /// Finish building and return the segment
    pub fn finish(mut self) -> StorageResult<Option<Segment>> {
        self.flush_buffer()?;
        Ok(self.segment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_segment_header_roundtrip() {
        let mut header = SegmentHeader::new(CompressionType::Lz4);
        header.block_count = 5;
        header.min_timestamp = 1000;
        header.max_timestamp = 5000;

        let bytes = header.to_bytes();
        let restored = SegmentHeader::from_bytes(&bytes).unwrap();

        assert_eq!(restored.magic, SEGMENT_MAGIC);
        assert_eq!(restored.version, SEGMENT_VERSION);
        assert_eq!(restored.block_count, 5);
        assert_eq!(restored.min_timestamp, 1000);
        assert_eq!(restored.max_timestamp, 5000);
        assert_eq!(restored.compression, CompressionType::Lz4);
    }

    #[test]
    fn test_segment_create_and_append() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("segment_001.dat");

        // Create segment and append blocks
        {
            let mut segment = Segment::create(&path, CompressionType::Lz4).unwrap();

            let points: Vec<DataPoint> = (0..100)
                .map(|i| DataPoint::with_timestamp(1, i as f64, 1000 + i * 100))
                .collect();

            segment.append_block(&points).unwrap();
            assert_eq!(segment.header.block_count, 1);
        }

        // Reopen and verify
        {
            let mut segment = Segment::open(&path).unwrap();
            assert_eq!(segment.header.block_count, 1);
            assert_eq!(segment.blocks.len(), 1);

            let points = segment.read_block(0).unwrap();
            assert_eq!(points.len(), 100);
            assert_eq!(points[0].timestamp, 1000);
            assert_eq!(points[99].timestamp, 10900);
        }
    }

    #[test]
    fn test_segment_multiple_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("segment_001.dat");

        {
            let mut segment = Segment::create(&path, CompressionType::Lz4).unwrap();

            // Add multiple blocks
            for block_num in 0..5 {
                let points: Vec<DataPoint> = (0..50)
                    .map(|i| {
                        DataPoint::with_timestamp(
                            1,
                            (block_num * 50 + i) as f64,
                            (block_num * 50 + i) * 1000,
                        )
                    })
                    .collect();

                segment.append_block(&points).unwrap();
            }

            assert_eq!(segment.header.block_count, 5);
        }

        // Reopen and read all
        {
            let mut segment = Segment::open(&path).unwrap();
            assert_eq!(segment.blocks.len(), 5);

            let mut all_points = Vec::new();
            for idx in 0..5 {
                all_points.extend(segment.read_block(idx).unwrap());
            }

            assert_eq!(all_points.len(), 250);
        }
    }

    #[test]
    fn test_segment_range_query() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("segment_001.dat");

        {
            let mut segment = Segment::create(&path, CompressionType::Lz4).unwrap();

            // Data spanning 0-10000ms
            let points: Vec<DataPoint> = (0..100)
                .map(|i| DataPoint::with_timestamp(1, i as f64, i * 100))
                .collect();

            segment.append_block(&points).unwrap();
        }

        {
            let mut segment = Segment::open(&path).unwrap();

            // Query middle range
            let range = TimeRange::new(2000, 5000);
            let points = segment.read_range(&range).unwrap();

            assert!(!points.is_empty());
            for point in &points {
                assert!(point.timestamp >= 2000);
                assert!(point.timestamp < 5000);
            }
        }
    }

    #[test]
    fn test_segment_builder() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("segment_001.dat");

        // Use builder to create segment
        let mut builder = SegmentBuilder::new(&path, CompressionType::Lz4).target_block_size(1024);

        // Add many points
        for batch in 0..10 {
            let points: Vec<DataPoint> = (0..100)
                .map(|i| DataPoint::with_timestamp(1, (batch * 100 + i) as f64, (batch * 100 + i) * 1000))
                .collect();

            builder.add_points(points).unwrap();
        }

        let segment = builder.finish().unwrap().unwrap();
        assert!(segment.header.block_count > 0);
        assert_eq!(segment.point_count(), 1000);
    }

    #[test]
    fn test_segment_overlaps() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("segment_001.dat");

        let mut segment = Segment::create(&path, CompressionType::Lz4).unwrap();

        let points: Vec<DataPoint> = (0..100)
            .map(|i| DataPoint::with_timestamp(1, i as f64, 1000 + i * 100))
            .collect();

        segment.append_block(&points).unwrap();

        // Segment spans 1000-10900
        assert!(segment.overlaps(&TimeRange::new(500, 1500)));   // Overlaps start
        assert!(segment.overlaps(&TimeRange::new(5000, 6000)));  // Overlaps middle
        assert!(segment.overlaps(&TimeRange::new(10000, 12000))); // Overlaps end
        assert!(!segment.overlaps(&TimeRange::new(0, 1000)));    // Before (exclusive end)
        assert!(!segment.overlaps(&TimeRange::new(11000, 12000))); // After
    }
}
