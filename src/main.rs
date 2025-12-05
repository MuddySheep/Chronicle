//! Chronicle CLI
//!
//! Command-line interface for the Chronicle time-series database.

use chronicle::storage::*;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "chronicle=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Chronicle Time-Series Engine v{}", env!("CARGO_PKG_VERSION"));

    // Create storage engine
    let config = StorageConfig::default();
    tracing::info!("Data directory: {:?}", config.data_dir);

    let engine = Arc::new(StorageEngine::new(config).await?);

    // Start background flush task
    let flush_handle = engine.start_background_flush();

    // Register some default metrics
    register_default_metrics(&engine).await?;

    // Print stats
    let stats = engine.stats().await;
    tracing::info!("Storage stats: {}", stats);

    // Demo: write some test data
    demo_write(&engine).await?;

    // Demo: query data
    demo_query(&engine).await?;

    // Shutdown
    tracing::info!("Shutting down...");
    engine.shutdown().await?;
    flush_handle.abort();

    tracing::info!("Chronicle shutdown complete");
    Ok(())
}

async fn register_default_metrics(engine: &StorageEngine) -> StorageResult<()> {
    // Health metrics
    engine
        .register_metric(
            Metric::new("heart_rate", "bpm", Category::Health, AggregationType::Average)
                .description("Heart rate in beats per minute")
                .range(30.0, 220.0),
        )
        .await?;

    engine
        .register_metric(
            Metric::new("steps", "count", Category::Health, AggregationType::Sum)
                .description("Steps walked"),
        )
        .await?;

    engine
        .register_metric(
            Metric::new("sleep_hours", "hours", Category::Health, AggregationType::Sum)
                .description("Hours of sleep")
                .range(0.0, 24.0),
        )
        .await?;

    // Mood metrics
    engine
        .register_metric(
            Metric::new("mood", "1-10", Category::Mood, AggregationType::Average)
                .description("Overall mood rating")
                .range(1.0, 10.0),
        )
        .await?;

    engine
        .register_metric(
            Metric::new("energy", "1-10", Category::Mood, AggregationType::Average)
                .description("Energy level")
                .range(1.0, 10.0),
        )
        .await?;

    engine
        .register_metric(
            Metric::new("stress", "1-10", Category::Mood, AggregationType::Average)
                .description("Stress level")
                .range(1.0, 10.0),
        )
        .await?;

    // Productivity metrics
    engine
        .register_metric(
            Metric::new(
                "focus_minutes",
                "minutes",
                Category::Productivity,
                AggregationType::Sum,
            )
            .description("Minutes of focused work"),
        )
        .await?;

    engine
        .register_metric(
            Metric::new(
                "tasks_completed",
                "count",
                Category::Productivity,
                AggregationType::Sum,
            )
            .description("Number of tasks completed"),
        )
        .await?;

    // Habit metrics
    engine
        .register_metric(
            Metric::new(
                "meditation",
                "minutes",
                Category::Habit,
                AggregationType::Sum,
            )
            .description("Minutes of meditation"),
        )
        .await?;

    engine
        .register_metric(
            Metric::new("water", "glasses", Category::Habit, AggregationType::Sum)
                .description("Glasses of water consumed"),
        )
        .await?;

    let metrics = engine.get_metrics().await;
    tracing::info!("Registered {} metrics", metrics.len());

    Ok(())
}

async fn demo_write(engine: &StorageEngine) -> StorageResult<()> {
    tracing::info!("Writing demo data...");

    let mood_metric = engine.get_metric("mood").await.unwrap();
    let energy_metric = engine.get_metric("energy").await.unwrap();
    let steps_metric = engine.get_metric("steps").await.unwrap();

    // Write some sample data
    let now = chrono::Utc::now().timestamp_millis();

    // Hourly mood readings for the past day
    for i in 0..24 {
        let timestamp = now - (i * 3600 * 1000); // i hours ago

        // Mood varies throughout the day
        let hour_of_day = 24 - i;
        let mood_value = 5.0 + 2.0 * ((hour_of_day as f64 * std::f64::consts::PI / 12.0).sin());

        engine
            .write(
                DataPoint::with_timestamp(mood_metric.id, mood_value, timestamp)
                    .tag("source", "demo"),
            )
            .await?;

        // Energy correlates with mood
        let energy_value = mood_value - 0.5 + rand_simple();
        engine
            .write(
                DataPoint::with_timestamp(energy_metric.id, energy_value.clamp(1.0, 10.0), timestamp)
                    .tag("source", "demo"),
            )
            .await?;
    }

    // Steps data
    for i in 0..24 {
        let timestamp = now - (i * 3600 * 1000);
        let steps = (500.0 + 300.0 * rand_simple()) as f64;

        engine
            .write(
                DataPoint::with_timestamp(steps_metric.id, steps, timestamp).tag("source", "demo"),
            )
            .await?;
    }

    engine.flush().await?;

    tracing::info!("Demo data written successfully");
    Ok(())
}

async fn demo_query(engine: &StorageEngine) -> StorageResult<()> {
    tracing::info!("Querying demo data...");

    // Query last 24 hours of mood data
    let range = TimeRange::last_hours(24);
    let mood_points = engine.query_metric("mood", range).await?;

    if !mood_points.is_empty() {
        let avg_mood: f64 = mood_points.iter().map(|p| p.value).sum::<f64>() / mood_points.len() as f64;
        let min_mood = mood_points.iter().map(|p| p.value).fold(f64::INFINITY, f64::min);
        let max_mood = mood_points.iter().map(|p| p.value).fold(f64::NEG_INFINITY, f64::max);

        tracing::info!(
            "Mood (24h): {} readings, avg={:.1}, min={:.1}, max={:.1}",
            mood_points.len(),
            avg_mood,
            min_mood,
            max_mood
        );
    }

    // Query steps
    let steps_points = engine.query_metric("steps", range).await?;
    if !steps_points.is_empty() {
        let total_steps: f64 = steps_points.iter().map(|p| p.value).sum();
        tracing::info!("Steps (24h): {:.0} total", total_steps);
    }

    Ok(())
}

/// Simple random number generator (0.0 to 1.0)
fn rand_simple() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    (nanos % 1000) as f64 / 1000.0
}
