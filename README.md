# Chronicle

**A Personal Time-Series Intelligence System with AI-Powered Insights**

Chronicle is a full-stack Rust application for storing, querying, visualizing, and analyzing personal time-series data. Whether you're tracking health metrics from Apple Health, mood patterns, productivity scores, or any other personal data, Chronicle gives you complete control over your data with powerful AI-driven insights.

---

## Why Chronicle?

- **Privacy First**: Your data stays on your machine. No cloud services, no data harvesting.
- **Apple Health Integration**: Import your entire health history with one ZIP upload.
- **AI-Powered Insights**: Connect to [MemMachine](https://github.com/MuddySheep/MemMachine) for intelligent pattern detection and natural language queries.
- **Real-Time Updates**: WebSocket-powered live dashboard updates.
- **Full Stack Rust**: Backend API + WASM frontend for blazing performance.

---

## Architecture

```
                          +------------------+
                          |   MemMachine     |
                          |  (AI Insights)   |
                          |   Port 8080      |
                          +--------+---------+
                                   |
                          Natural Language Queries
                          Correlation Analysis
                          Pattern Detection
                                   |
+------------------+      +--------+---------+      +------------------+
|   Chronicle UI   | <--> |   Chronicle API  | <--> |  Storage Engine  |
|   (Leptos/WASM)  |      |     (Axum)       |      | (Append-Only Log)|
|   Port 8084      |      |   Port 8082      |      |  LZ4 Compressed  |
+------------------+      +------------------+      +------------------+
        |                         |
   Reactive Charts           REST + WebSocket
   Data Entry Forms          Real-time Events
   Health Snapshots
```

### The 8-Layer Stack

| Layer | Component | Description |
|-------|-----------|-------------|
| **8** | Integrations | Apple Health, Fitbit, GitHub, CSV Import |
| **7** | Dashboard UI | Leptos/WASM reactive frontend |
| **6** | WebSocket | Real-time pub/sub for live updates |
| **5** | MemMachine | AI insights, correlations, pattern detection |
| **4** | REST API | Axum-powered HTTP endpoints |
| **3** | Query Engine | Time-range queries, aggregations, filtering |
| **2** | Index Layer | B-tree indexes, SQLite-backed metric registry |
| **1** | Storage Engine | Append-only log with LZ4 compression |

---

## MemMachine Integration

**MemMachine** is Chronicle's AI brain. It's a separate service that provides:

- **Natural Language Insights**: Ask questions like "How does my sleep affect my mood?"
- **Correlation Analysis**: Automatically discover relationships between metrics
- **Pattern Detection**: Find trends and anomalies in your data
- **Memory Persistence**: Your insights and patterns are stored for future reference

### How It Works

1. Chronicle syncs your time-series data to MemMachine
2. MemMachine analyzes patterns and stores insights in its memory system
3. You can query MemMachine using natural language through the Chronicle dashboard
4. Insights appear in real-time on your dashboard

### Setting Up MemMachine

```bash
# Clone and run MemMachine (separate repository)
git clone https://github.com/MuddySheep/MemMachine
cd MemMachine
cargo run  # Runs on port 8080
```

Then set the environment variable before starting Chronicle:

```bash
# Windows
set MEMMACHINE_URL=http://localhost:8080

# Linux/macOS
export MEMMACHINE_URL=http://localhost:8080
```

---

## Quick Start

### Prerequisites

- Rust toolchain (1.75+)
- `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
- Trunk: `cargo install trunk`
- (Optional) MemMachine for AI features

### 1. Start the API Server

```bash
cd Chronicle

# With MemMachine (recommended)
set MEMMACHINE_URL=http://localhost:8080  # Windows
cargo run --bin chronicle-api

# API runs on http://localhost:8082
```

### 2. Start the Dashboard

```bash
cd Chronicle/chronicle-ui
trunk serve

# Dashboard runs on http://localhost:8084
```

### 3. Import Your Data

**Apple Health Export:**
1. On your iPhone: Health app → Profile → Export All Health Data
2. Transfer the ZIP file to your computer
3. In Chronicle dashboard, go to Settings → Import
4. Upload the ZIP file (supports exports with 500,000+ data points)

**Manual Entry:**
- Use the "Log Entry" section on the dashboard
- Or use the CLI: `cargo run --bin chronicle-cli -- log mood 7.5`

---

## API Reference

### Ingest Data

```bash
# Single data point
curl -X POST http://localhost:8082/api/v1/ingest \
  -H "Content-Type: application/json" \
  -d '{"metric": "mood", "value": 7.5}'

# Batch ingest
curl -X POST http://localhost:8082/api/v1/ingest/batch \
  -H "Content-Type: application/json" \
  -d '{"points": [{"metric": "mood", "value": 7.5}, {"metric": "energy", "value": 8.0}]}'
```

### Query Data

```bash
curl -X POST http://localhost:8082/api/v1/query \
  -H "Content-Type: application/json" \
  -d '{
    "select": ["heart_rate", "steps"],
    "time_range": {"start": "2024-01-01", "end": "now"},
    "group_by": "day",
    "aggregation": "avg"
  }'
```

### Get AI Insights (requires MemMachine)

```bash
curl -X POST http://localhost:8082/api/v1/insights \
  -H "Content-Type: application/json" \
  -d '{"metrics": ["mood", "sleep_hours"], "days": 30}'
```

### List Metrics

```bash
curl http://localhost:8082/api/v1/metrics
```

### Health Check

```bash
curl http://localhost:8082/health
```

---

## Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `MEMMACHINE_URL` | *(none)* | MemMachine server URL (required for AI) |
| `MEMMACHINE_USER_ID` | `default-user` | User ID for MemMachine |
| `CHRONICLE_HOST` | `0.0.0.0` | API bind address |
| `CHRONICLE_PORT` | `8082` | API port |
| `CHRONICLE_DATA_DIR` | `chronicle_data` | Data storage directory |
| `CHRONICLE_AUTO_CREATE_METRICS` | `true` | Auto-create metrics on ingest |
| `RUST_LOG` | `info` | Log level |

---

## Project Structure

```
Chronicle/
├── src/
│   ├── bin/
│   │   ├── api.rs          # API server binary
│   │   └── cli.rs          # CLI tool
│   ├── storage/            # Layer 1: Append-only log
│   ├── index/              # Layer 2: B-tree indexes
│   ├── query/              # Layer 3: Query engine
│   ├── api/                # Layer 4: REST API
│   ├── memmachine/         # Layer 5: AI integration
│   ├── websocket/          # Layer 6: Real-time
│   └── integrations/       # Layer 8: External data
│
├── chronicle-ui/           # Layer 7: Dashboard
│   ├── src/
│   │   ├── components/     # UI components
│   │   ├── pages/          # Page views
│   │   ├── state/          # Reactive state
│   │   └── api/            # HTTP client
│   └── index.html
│
└── chronicle_data/         # Runtime data (gitignored)
```

---

## Contributing

Contributions are welcome! Here's how to get started:

### Setting Up Development

```bash
# Clone the repo
git clone https://github.com/MuddySheep/Chronicle
cd Chronicle

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run --bin chronicle-api
```

### Contribution Guidelines

1. **Fork** the repository
2. **Create a branch** for your feature: `git checkout -b feature/amazing-feature`
3. **Write tests** for new functionality
4. **Follow Rust conventions**: `cargo fmt` and `cargo clippy`
5. **Submit a PR** with a clear description

### Areas for Contribution

- **New Integrations**: Fitbit, Garmin, Oura Ring, Whoop
- **Visualization**: New chart types, export options
- **Mobile**: React Native or Flutter companion app
- **Documentation**: Tutorials, examples, translations
- **Performance**: Optimization, benchmarking

---

## License

This project is licensed under the **MIT License with Attribution Requirement**.

**You are free to use, modify, and distribute this software, but you must include attribution to the original author:**

```
This project uses Chronicle by MuddySheep (https://github.com/MuddySheep/Chronicle)
```

See [LICENSE](LICENSE) for full details.

---

## Acknowledgments

- Built with [Rust](https://www.rust-lang.org/)
- Frontend powered by [Leptos](https://leptos.dev/)
- Styled with [Tailwind CSS](https://tailwindcss.com/)
- AI insights by [MemMachine](https://github.com/MemMachine/MemMachine)
---

## Support

- **Issues**: [GitHub Issues](https://github.com/MuddySheep/Chronicle/issues)
- **Discussions**: [GitHub Discussions](https://github.com/MuddySheep/Chronicle/discussions)

---

**Chronicle** - *Your data. Your insights. Your control.*
