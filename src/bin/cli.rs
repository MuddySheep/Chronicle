//! Chronicle CLI
//!
//! Command-line interface for Chronicle operations:
//! - Log data points
//! - Query data
//! - Check status
//! - Import/Export data

use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "chronicle")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Personal time-series database for life tracking")]
#[command(long_about = "Chronicle is a personal time-series intelligence system.\nTrack your metrics, discover patterns, and get personalized insights.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// API server URL
    #[arg(long, default_value = "http://localhost:8082", global = true)]
    pub api_url: String,

    /// Output format (table, json, csv)
    #[arg(short, long, default_value = "table", global = true)]
    pub format: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Log a data point
    Log {
        /// Metric name
        metric: String,
        /// Value
        value: f64,
        /// Timestamp (default: now). Supports: "now", "yesterday", ISO 8601, Unix timestamp
        #[arg(short, long)]
        time: Option<String>,
        /// Tags in key=value format
        #[arg(short = 'T', long)]
        tags: Vec<String>,
    },

    /// Query data
    Query {
        /// Metrics to query (comma-separated or multiple args)
        metrics: Vec<String>,
        /// Time range (e.g., 7d, 30d, 1w, 3m)
        #[arg(short, long, default_value = "7d")]
        last: String,
        /// Group by interval (hour, day, week)
        #[arg(short, long)]
        group_by: Option<String>,
        /// Aggregation function (avg, sum, min, max, last)
        #[arg(short, long, default_value = "avg")]
        aggregation: String,
    },

    /// Show system status
    Status,

    /// List all metrics
    Metrics,

    /// Import data from CSV
    Import {
        /// Path to CSV file
        path: PathBuf,
        /// Timestamp column (0-indexed)
        #[arg(long, default_value = "0")]
        timestamp_col: usize,
        /// Timestamp format (strftime format)
        #[arg(long, default_value = "%Y-%m-%d")]
        timestamp_format: String,
        /// Dry run (don't actually import)
        #[arg(long)]
        dry_run: bool,
    },

    /// Export data
    Export {
        /// Time range (e.g., 7d, 30d, 1y)
        #[arg(short, long, default_value = "30d")]
        last: String,
        /// Metrics to export (empty = all)
        #[arg(short, long)]
        metrics: Vec<String>,
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Sync an integration
    Sync {
        /// Integration name (fitbit, github, etc.)
        integration: String,
    },

    /// Generate default config file
    Config {
        /// Output path (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    match cli.command {
        Commands::Log {
            metric,
            value,
            time,
            tags,
        } => {
            // Parse timestamp
            let timestamp = match time.as_deref() {
                None | Some("now") => Utc::now().timestamp_millis(),
                Some("yesterday") => (Utc::now() - Duration::days(1)).timestamp_millis(),
                Some(s) => {
                    // Try parsing as ISO 8601
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                        dt.timestamp_millis()
                    } else if let Ok(ts) = s.parse::<i64>() {
                        ts
                    } else {
                        eprintln!("Invalid timestamp format: {}", s);
                        std::process::exit(1);
                    }
                }
            };

            // Parse tags
            let mut tag_map = HashMap::new();
            for tag in tags {
                if let Some((k, v)) = tag.split_once('=') {
                    tag_map.insert(k.to_string(), v.to_string());
                }
            }

            let body = serde_json::json!({
                "metric": metric,
                "value": value,
                "timestamp": timestamp,
                "tags": tag_map,
            });

            let response = client
                .post(format!("{}/api/v1/ingest", cli.api_url))
                .json(&body)
                .send()
                .await?;

            if response.status().is_success() {
                let dt = chrono::DateTime::from_timestamp_millis(timestamp)
                    .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                println!("Logged {}: {} at {}", metric, value, dt);
            } else {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                eprintln!("Failed ({}): {}", status, text);
                std::process::exit(1);
            }
        }

        Commands::Query {
            metrics,
            last,
            group_by,
            aggregation,
        } => {
            // Parse time range
            let duration = parse_duration(&last)?;
            let end = Utc::now();
            let start = end - duration;

            // Flatten metrics (support comma-separated)
            let metrics: Vec<String> = metrics
                .iter()
                .flat_map(|m| m.split(',').map(|s| s.trim().to_string()))
                .collect();

            let body = serde_json::json!({
                "select": metrics,
                "time_range": {
                    "start": start.timestamp_millis(),
                    "end": end.timestamp_millis(),
                },
                "group_by": group_by,
                "aggregation": aggregation,
            });

            let response = client
                .post(format!("{}/api/v1/query", cli.api_url))
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                eprintln!("Query failed ({}): {}", status, text);
                std::process::exit(1);
            }

            let data: serde_json::Value = response.json().await?;

            match cli.format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&data)?);
                }
                "csv" => {
                    print_csv(&data, &metrics);
                }
                _ => {
                    print_table(&data, &metrics);
                }
            }
        }

        Commands::Status => {
            let response = client
                .get(format!("{}/health", cli.api_url))
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    let health: serde_json::Value = resp.json().await?;

                    println!("Chronicle v{}", env!("CARGO_PKG_VERSION"));
                    println!();
                    println!(
                        "API Status: {}",
                        health["status"].as_str().unwrap_or("unknown")
                    );

                    if let Some(storage) = health.get("storage_stats") {
                        println!();
                        println!("Storage:");
                        if let Some(points) = storage["total_points"].as_u64() {
                            println!("  Total points: {}", points);
                        }
                        if let Some(metrics) = storage["total_metrics"].as_u64() {
                            println!("  Total metrics: {}", metrics);
                        }
                    }

                    if let Some(uptime) = health["uptime_seconds"].as_u64() {
                        println!();
                        println!("Uptime: {}", format_duration(uptime));
                    }
                }
                Ok(resp) => {
                    eprintln!("API returned error: {}", resp.status());
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Cannot connect to Chronicle API at {}", cli.api_url);
                    eprintln!("Error: {}", e);
                    eprintln!();
                    eprintln!("Make sure the Chronicle API server is running:");
                    eprintln!("  cargo run --bin chronicle-api");
                    std::process::exit(1);
                }
            }
        }

        Commands::Metrics => {
            let response = client
                .get(format!("{}/api/v1/metrics", cli.api_url))
                .send()
                .await?;

            if !response.status().is_success() {
                eprintln!("Failed to fetch metrics: {}", response.status());
                std::process::exit(1);
            }

            let metrics: Vec<serde_json::Value> = response.json().await?;

            if metrics.is_empty() {
                println!("No metrics defined yet.");
                println!();
                println!("Create your first metric with:");
                println!("  chronicle log mood 7.5");
            } else {
                println!("{:<20} {:<10} {:<15} {}", "Name", "Unit", "Category", "ID");
                println!("{}", "-".repeat(60));

                for metric in metrics {
                    println!(
                        "{:<20} {:<10} {:<15} {}",
                        metric["name"].as_str().unwrap_or("-"),
                        metric["unit"].as_str().unwrap_or("-"),
                        metric["category"].as_str().unwrap_or("-"),
                        metric["id"].as_u64().unwrap_or(0)
                    );
                }
            }
        }

        Commands::Import {
            path,
            timestamp_col,
            timestamp_format,
            dry_run,
        } => {
            use chronicle::integrations::CsvImporter;

            if !path.exists() {
                eprintln!("File not found: {:?}", path);
                std::process::exit(1);
            }

            // Read header to auto-detect columns
            let mut reader = csv::Reader::from_path(&path)?;
            let headers = reader.headers()?.clone();

            println!("Detected columns:");
            for (i, h) in headers.iter().enumerate() {
                let marker = if i == timestamp_col { " (timestamp)" } else { "" };
                println!("  {}: {}{}", i, h, marker);
            }
            println!();

            // Build importer
            let mut importer = CsvImporter::new()
                .with_timestamp_column(timestamp_col)
                .with_timestamp_format(&timestamp_format);

            // Add all non-timestamp columns as metrics
            for (i, header) in headers.iter().enumerate() {
                if i != timestamp_col {
                    importer = importer.with_metric_column(i, header);
                }
            }

            let result = importer.import(&path)?;

            println!("Import results:");
            println!("  Rows processed: {}", result.rows_processed);
            println!("  Rows failed: {}", result.rows_failed);
            println!("  Data points: {}", result.points.len());

            if !result.errors.is_empty() {
                println!();
                println!("Errors (first 10):");
                for error in result.errors.iter().take(10) {
                    println!("  {}", error);
                }
            }

            if dry_run {
                println!();
                println!("(Dry run - no data was imported)");
            } else if !result.points.is_empty() {
                println!();
                println!("Importing data...");

                let mut success = 0;
                let mut failed = 0;

                for (metric_name, point) in result.points {
                    let body = serde_json::json!({
                        "metric": metric_name,
                        "value": point.value,
                        "timestamp": point.timestamp,
                    });

                    match client
                        .post(format!("{}/api/v1/ingest", cli.api_url))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => success += 1,
                        _ => failed += 1,
                    }
                }

                println!("  Imported: {}", success);
                if failed > 0 {
                    println!("  Failed: {}", failed);
                }
            }
        }

        Commands::Export {
            last,
            metrics,
            output,
        } => {
            let duration = parse_duration(&last)?;
            let end = Utc::now();
            let start = end - duration;

            let mut url = format!(
                "{}/api/v1/export?format=csv&start={}&end={}",
                cli.api_url,
                start.timestamp_millis(),
                end.timestamp_millis()
            );

            if !metrics.is_empty() {
                url.push_str(&format!("&metrics={}", metrics.join(",")));
            }

            let response = client.get(&url).send().await?;

            if !response.status().is_success() {
                eprintln!("Export failed: {}", response.status());
                std::process::exit(1);
            }

            let data = response.text().await?;

            match output {
                Some(path) => {
                    std::fs::write(&path, &data)?;
                    println!("Exported to {:?}", path);
                }
                None => {
                    print!("{}", data);
                }
            }
        }

        Commands::Sync { integration } => {
            let response = client
                .post(format!(
                    "{}/api/v1/integrations/{}/sync",
                    cli.api_url, integration
                ))
                .send()
                .await?;

            if response.status().is_success() {
                let result: serde_json::Value = response.json().await?;
                println!(
                    "Synced {} points",
                    result["points_synced"].as_u64().unwrap_or(0)
                );
            } else {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                eprintln!("Sync failed ({}): {}", status, text);
                std::process::exit(1);
            }
        }

        Commands::Config { output } => {
            let config = chronicle::config::generate_default_config();

            match output {
                Some(path) => {
                    // Create parent directory if needed
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&path, &config)?;
                    println!("Config written to {:?}", path);
                }
                None => {
                    print!("{}", config);
                }
            }
        }
    }

    Ok(())
}

fn parse_duration(s: &str) -> Result<Duration, Box<dyn std::error::Error>> {
    let s = s.trim().to_lowercase();

    if let Some(days) = s.strip_suffix('d') {
        Ok(Duration::days(days.parse()?))
    } else if let Some(weeks) = s.strip_suffix('w') {
        Ok(Duration::weeks(weeks.parse()?))
    } else if let Some(months) = s.strip_suffix('m') {
        Ok(Duration::days(months.parse::<i64>()? * 30))
    } else if let Some(years) = s.strip_suffix('y') {
        Ok(Duration::days(years.parse::<i64>()? * 365))
    } else {
        Err(format!("Invalid duration format: {}. Use: 7d, 4w, 3m, 1y", s).into())
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds < 86400 {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}d {}h", seconds / 86400, (seconds % 86400) / 3600)
    }
}

fn print_table(data: &serde_json::Value, metrics: &[String]) {
    let rows = match data["rows"].as_array() {
        Some(r) => r,
        None => {
            println!("No data");
            return;
        }
    };

    if rows.is_empty() {
        println!("No data for the selected time range");
        return;
    }

    // Header
    print!("{:<12}", "Date");
    for metric in metrics {
        print!(" | {:<10}", metric);
    }
    println!();

    // Separator
    println!("{}", "-".repeat(14 + metrics.len() * 13));

    // Data rows
    for row in rows {
        let ts = row["timestamp"].as_i64().unwrap_or(0);
        let date = chrono::DateTime::from_timestamp_millis(ts)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".to_string());

        print!("{:<12}", date);
        for metric in metrics {
            let val = row[metric]
                .as_f64()
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "-".to_string());
            print!(" | {:<10}", val);
        }
        println!();
    }
}

fn print_csv(data: &serde_json::Value, metrics: &[String]) {
    let rows = match data["rows"].as_array() {
        Some(r) => r,
        None => return,
    };

    // Header
    print!("timestamp");
    for metric in metrics {
        print!(",{}", metric);
    }
    println!();

    // Data
    for row in rows {
        let ts = row["timestamp"].as_i64().unwrap_or(0);
        print!("{}", ts);
        for metric in metrics {
            let val = row[metric]
                .as_f64()
                .map(|v| format!("{:.2}", v))
                .unwrap_or_default();
            print!(",{}", val);
        }
        println!();
    }
}
