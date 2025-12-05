//! Benchmarks for Chronicle storage engine
//!
//! Run with: cargo bench

use chronicle::storage::*;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use tempfile::tempdir;

fn create_test_points(count: usize) -> Vec<DataPoint> {
    (0..count)
        .map(|i| {
            DataPoint::with_timestamp(0, i as f64, i as i64 * 1000)
                .tag("source", "bench")
        })
        .collect()
}

fn bench_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression");

    for size in [100, 1000, 10000] {
        let points = create_test_points(size);

        group.throughput(Throughput::Elements(size as u64));

        group.bench_function(format!("compress_{}", size), |b| {
            b.iter(|| {
                compress_block(black_box(&points)).unwrap()
            })
        });

        let compressed = compress_block(&points).unwrap();

        group.bench_function(format!("decompress_{}", size), |b| {
            b.iter(|| {
                decompress_block(black_box(&compressed)).unwrap()
            })
        });
    }

    group.finish();
}

fn bench_wal(c: &mut Criterion) {
    let mut group = c.benchmark_group("wal");

    group.bench_function("append_single", |b| {
        let dir = tempdir().unwrap();
        let mut wal = WriteAheadLog::open(
            dir.path().join("bench.wal"),
            WalSyncMode::None, // No fsync for benchmarking raw performance
        ).unwrap();

        let point = DataPoint::with_timestamp(0, 7.5, 1000);

        b.iter(|| {
            wal.append(black_box(&point)).unwrap()
        });
    });

    group.bench_function("append_batch_100", |b| {
        let dir = tempdir().unwrap();
        let mut wal = WriteAheadLog::open(
            dir.path().join("bench.wal"),
            WalSyncMode::None,
        ).unwrap();

        let points = create_test_points(100);

        b.iter(|| {
            wal.append_batch(black_box(&points)).unwrap()
        });
    });

    group.finish();
}

fn bench_segment(c: &mut Criterion) {
    let mut group = c.benchmark_group("segment");

    group.bench_function("append_block_1000", |b| {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bench.dat");
        let mut segment = Segment::create(&path, CompressionType::Lz4).unwrap();

        let points = create_test_points(1000);

        b.iter(|| {
            segment.append_block(black_box(&points)).unwrap()
        });
    });

    group.bench_function("read_block", |b| {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bench.dat");
        let mut segment = Segment::create(&path, CompressionType::Lz4).unwrap();

        let points = create_test_points(1000);
        segment.append_block(&points).unwrap();

        b.iter(|| {
            segment.read_block(black_box(0)).unwrap()
        });
    });

    group.finish();
}

fn bench_engine(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("engine");

    group.bench_function("write_single", |b| {
        b.iter_custom(|iters| {
            rt.block_on(async {
                let dir = tempdir().unwrap();
                let config = StorageConfig::new(dir.path());
                let engine = StorageEngine::new(config).await.unwrap();

                let metric_id = engine.register_metric(
                    Metric::new("bench", "units", Category::Custom, AggregationType::Average)
                ).await.unwrap();

                let start = std::time::Instant::now();

                for _ in 0..iters {
                    let point = DataPoint::new(metric_id, 7.5);
                    engine.write(point).await.unwrap();
                }

                start.elapsed()
            })
        });
    });

    group.bench_function("write_batch_1000", |b| {
        b.iter_custom(|iters| {
            rt.block_on(async {
                let dir = tempdir().unwrap();
                let config = StorageConfig::new(dir.path());
                let engine = StorageEngine::new(config).await.unwrap();

                let metric_id = engine.register_metric(
                    Metric::new("bench", "units", Category::Custom, AggregationType::Average)
                ).await.unwrap();

                let points: Vec<DataPoint> = (0..1000)
                    .map(|i| DataPoint::new(metric_id, i as f64))
                    .collect();

                let start = std::time::Instant::now();

                for _ in 0..iters {
                    engine.write_batch(points.clone()).await.unwrap();
                }

                start.elapsed()
            })
        });
    });

    group.bench_function("query_week", |b| {
        b.iter_custom(|iters| {
            rt.block_on(async {
                let dir = tempdir().unwrap();
                let config = StorageConfig::new(dir.path());
                let engine = StorageEngine::new(config).await.unwrap();

                let metric_id = engine.register_metric(
                    Metric::new("bench", "units", Category::Custom, AggregationType::Average)
                ).await.unwrap();

                // Setup: write a week of hourly data
                let now = chrono::Utc::now().timestamp_millis();
                let points: Vec<DataPoint> = (0..168)
                    .map(|i| DataPoint::with_timestamp(metric_id, i as f64, now - i * 3600000))
                    .collect();

                engine.write_batch(points).await.unwrap();
                engine.flush().await.unwrap();

                let range = TimeRange::last_days(7);

                let start = std::time::Instant::now();

                for _ in 0..iters {
                    let _ = engine.query(black_box(range), None).await.unwrap();
                }

                start.elapsed()
            })
        });
    });

    group.finish();
}

criterion_group!(benches, bench_compression, bench_wal, bench_segment, bench_engine);
criterion_main!(benches);
