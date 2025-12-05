# Chronicle - Personal Time-Series Intelligence System

## Project Overview

Chronicle is a **full-stack Rust application** for storing, querying, and analyzing personal time-series data. It integrates with **MemMachine** for AI-powered insights and pattern detection.

**Tech Stack:**
- Backend: Rust, Axum, Tokio
- Frontend: Leptos (WASM), Tailwind CSS
- Storage: Custom append-only log with LZ4 compression, SQLite indexes
- Real-time: WebSocket pub/sub
- AI Integration: MemMachine (separate service on port 8080)

---

## Architecture (8 Layers)

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 8: Integrations & Polish                             │
│  - Fitbit, GitHub, CSV import                               │
│  - CLI tool, Configuration system                           │
├─────────────────────────────────────────────────────────────┤
│  Layer 7: Dashboard UI (Leptos/WASM) - Port 8084            │
│  - Reactive components, Charts, Data entry                  │
├─────────────────────────────────────────────────────────────┤
│  Layer 6: WebSocket Real-Time                               │
│  - Live updates, Pub/sub, Connection hub                    │
├─────────────────────────────────────────────────────────────┤
│  Layer 5: MemMachine Integration                            │
│  - AI insights, Correlation analysis, Pattern detection     │
├─────────────────────────────────────────────────────────────┤
│  Layer 4: REST API (Axum) - Port 8082                       │
│  - CRUD endpoints, Query engine, Export                     │
├─────────────────────────────────────────────────────────────┤
│  Layer 3: Query Engine                                      │
│  - Time-range queries, Aggregations, Filtering              │
├─────────────────────────────────────────────────────────────┤
│  Layer 2: Index Layer                                       │
│  - B-tree indexes, SQLite-backed, Metric registry           │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: Storage Engine                                    │
│  - Append-only log, LZ4 compression, WAL durability         │
└─────────────────────────────────────────────────────────────┘
```

---

## Directory Structure

```
Chronicle/
├── Cargo.toml                 # Main crate dependencies
├── CLAUDE.md                  # This file
├── CHRONICLE_STATUS.md        # Current issues and status
│
├── src/
│   ├── lib.rs                 # Library exports
│   ├── main.rs                # Default binary
│   │
│   ├── bin/
│   │   ├── api.rs             # API server binary (chronicle-api)
│   │   └── cli.rs             # CLI tool binary (chronicle-cli)
│   │
│   ├── storage/               # Layer 1: Storage Engine
│   │   ├── mod.rs
│   │   ├── engine.rs          # StorageEngine - main interface
│   │   ├── types.rs           # DataPoint, Metric, TimeRange
│   │   ├── segment.rs         # Segment file management
│   │   ├── wal.rs             # Write-ahead log
│   │   └── compression.rs     # LZ4 compression
│   │
│   ├── index/                 # Layer 2: Index Layer
│   │   ├── mod.rs
│   │   ├── manager.rs         # IndexManager
│   │   ├── btree.rs           # B-tree implementation
│   │   └── metric_registry.rs # Metric definitions
│   │
│   ├── query/                 # Layer 3: Query Engine
│   │   ├── mod.rs
│   │   ├── parser.rs          # Query parsing
│   │   ├── executor.rs        # QueryExecutor
│   │   └── aggregation.rs     # Aggregation functions
│   │
│   ├── api/                   # Layer 4: REST API
│   │   ├── mod.rs             # Router setup, serve()
│   │   ├── handlers.rs        # Request handlers
│   │   ├── error.rs           # ApiError types
│   │   └── state.rs           # AppState
│   │
│   ├── memmachine/            # Layer 5: MemMachine Integration
│   │   ├── mod.rs
│   │   ├── client.rs          # MemMachineClient
│   │   ├── sync.rs            # SyncManager
│   │   ├── insights.rs        # InsightEngine
│   │   └── correlation.rs     # CorrelationEngine
│   │
│   ├── websocket/             # Layer 6: WebSocket
│   │   ├── mod.rs
│   │   ├── hub.rs             # ConnectionHub
│   │   ├── handler.rs         # websocket_handler
│   │   └── messages.rs        # Message types
│   │
│   ├── integrations/          # Layer 8: External Integrations
│   │   ├── mod.rs             # Integration trait
│   │   ├── fitbit.rs          # Fitbit OAuth + API
│   │   ├── github.rs          # GitHub API
│   │   ├── csv_import.rs      # CSV file import
│   │   └── scheduler.rs       # Periodic sync scheduler
│   │
│   └── config.rs              # Configuration system
│
├── chronicle-ui/              # Layer 7: Dashboard UI
│   ├── Cargo.toml             # UI crate dependencies
│   ├── Trunk.toml             # Trunk build config (port 8084)
│   ├── index.html             # HTML entry point
│   │
│   └── src/
│       ├── main.rs            # WASM entry point
│       ├── app.rs             # Root App component
│       │
│       ├── api/
│       │   └── client.rs      # HTTP client functions
│       │
│       ├── state/
│       │   ├── mod.rs
│       │   ├── global.rs      # GlobalState (signals)
│       │   └── websocket.rs   # WebSocket client
│       │
│       ├── components/
│       │   ├── mod.rs
│       │   ├── nav.rs         # Navigation bar
│       │   ├── chart.rs       # Canvas chart component
│       │   ├── metric_card.rs # Metric display card
│       │   ├── data_entry.rs  # Data input form
│       │   ├── insight_card.rs# AI insight display
│       │   ├── loading.rs     # Loading spinner
│       │   └── toast.rs       # Toast notifications
│       │
│       └── pages/
│           ├── mod.rs
│           ├── dashboard.rs   # Main dashboard
│           ├── metrics.rs     # Metrics management
│           ├── insights.rs    # AI insights page
│           └── settings.rs    # Settings page
│
└── chronicle_data/            # Runtime data directory
    ├── segments/              # Data segment files
    ├── wal/                   # Write-ahead log
    └── index.db               # SQLite index database
```

---

## Key Types

### DataPoint (src/storage/types.rs)
```rust
pub struct DataPoint {
    pub timestamp: i64,      // Unix millis
    pub metric_id: u32,      // Reference to Metric
    pub value: f64,          // The measurement
    pub tags: HashMap<String, String>,
}
```

### Metric (src/storage/types.rs)
```rust
pub struct Metric {
    pub id: u32,
    pub name: String,
    pub unit: String,
    pub category: Category,
    pub aggregation: AggregationType,
    pub description: Option<String>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
}
```

### TimeRange (src/storage/types.rs)
```rust
pub struct TimeRange {
    pub start: i64,  // Unix millis
    pub end: i64,    // Unix millis
}
```

### GlobalState (chronicle-ui/src/state/global.rs)
```rust
pub struct GlobalState {
    pub metrics: RwSignal<Vec<Metric>>,
    pub selected_metrics: RwSignal<Vec<String>>,
    pub time_range: RwSignal<TimeRange>,
    pub chart_data: RwSignal<HashMap<String, Vec<DataPoint>>>,
    pub ws_connected: RwSignal<bool>,
    pub loading: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    pub success: RwSignal<Option<String>>,
}
```

---

## API Endpoints

### Data Ingestion
```
POST /api/v1/ingest
Body: { "metric": "mood", "value": 7.5, "timestamp"?: i64, "tags"?: {} }
Response: { "status": "ok", "timestamp": i64, "metric_id": u32 }
```

### Query
```
POST /api/v1/query
Body: {
  "select": ["mood", "energy"],
  "time_range": { "start": i64, "end": i64 },
  "group_by"?: "hour" | "day" | "week",
  "aggregation"?: "avg" | "sum" | "min" | "max"
}
Response: { "columns": [...], "rows": [...] }
```

### Metrics
```
GET /api/v1/metrics
Response: [{ "id": 0, "name": "mood", "unit": "1-10", ... }]

POST /api/v1/metrics
Body: { "name": "mood", "unit": "1-10", "category": "mood", "aggregation": "average" }
```

### Insights (requires MemMachine)
```
POST /api/v1/insights
Body: { "metrics": ["mood", "sleep"], "days": 30 }
Response: { "insight": "Your mood correlates with sleep quality...", ... }

GET /api/v1/correlations?days=30
Response: [{ "metric_a": "mood", "metric_b": "sleep", "correlation": 0.72, ... }]
```

### Export
```
GET /api/v1/export?format=csv&start=...&end=...&metrics=mood,energy
```

### WebSocket
```
GET /api/v1/ws
Messages:
  → { "type": "subscribe", "topics": ["metrics.mood"] }
  ← { "type": "data_point", "metric": "mood", "value": 7.5, "timestamp": ... }
```

### Health
```
GET /health
Response: { "status": "healthy", "storage": "ok", "uptime_seconds": 123 }
```

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MEMMACHINE_URL` | *(none)* | **Required for AI features**. MemMachine server URL |
| `MEMMACHINE_USER_ID` | `default-user` | User ID for MemMachine |
| `CHRONICLE_HOST` | `0.0.0.0` | API bind address |
| `CHRONICLE_PORT` | `8082` | API port |
| `CHRONICLE_DATA_DIR` | `chronicle_data` | Data storage directory |
| `CHRONICLE_AUTO_CREATE_METRICS` | `true` | Auto-create metrics on ingest |
| `RUST_LOG` | `info` | Log level |

---

## Running the Project

### Prerequisites
- Rust toolchain with `wasm32-unknown-unknown` target
- Trunk (`cargo install trunk`)
- MemMachine running on port 8080 (optional, for AI features)

### Start API Server
```bash
cd Chronicle
set MEMMACHINE_URL=http://localhost:8080  # Windows
cargo run --bin chronicle-api
# Listens on http://localhost:8082
```

### Start Dashboard UI
```bash
cd Chronicle/chronicle-ui
trunk serve
# Serves on http://localhost:8084
```

### CLI Tool
```bash
cargo run --bin chronicle-cli -- status
cargo run --bin chronicle-cli -- log mood 7.5
cargo run --bin chronicle-cli -- query mood energy --last 7d
```

---

## Dependencies

### Backend (Cargo.toml)
- `tokio` - Async runtime
- `axum` - Web framework
- `serde` / `serde_json` - Serialization
- `rusqlite` - SQLite for indexes
- `lz4_flex` - Compression
- `reqwest` - HTTP client (MemMachine)
- `chrono` - Date/time handling
- `tracing` - Logging
- `clap` - CLI parsing
- `toml` - Config files
- `csv` - CSV import

### Frontend (chronicle-ui/Cargo.toml)
- `leptos` - Reactive UI framework (CSR mode)
- `leptos_router` - SPA routing
- `gloo-net` - HTTP requests
- `gloo-timers` - Timers
- `web-sys` - DOM/WebSocket APIs
- `chrono` - Date formatting

---

## Code Conventions

### Rust Style
- Use `thiserror` for error types
- Async functions return `Result<T, ErrorType>`
- Clone state before moving into closures (Leptos pattern)
- Use `tracing` macros for logging: `tracing::info!()`, `tracing::error!()`

### Leptos Patterns
```rust
// Signals for reactive state
let (value, set_value) = create_signal(initial);

// Read in reactive context (auto-tracks)
let current = value.get();

// Read outside reactive context (no tracking)
let current = value.get_untracked();

// Update signal
set_value.set(new_value);
value.update(|v| *v += 1);
```

### API Handler Pattern
```rust
pub async fn handler(
    State(state): State<AppState>,
    Json(request): Json<RequestType>,
) -> Result<Json<ResponseType>, ApiError> {
    // ... implementation
    Ok(Json(response))
}
```

---

## Known Issues (see CHRONICLE_STATUS.md)

1. **MemMachine Connection**: Environment variable `MEMMACHINE_URL` must be set before starting API. Claude Code's bash layer has issues passing env vars - run from native Windows terminal.

2. **WebSocket "Disconnected"**: Clear browser local storage and hard refresh after code changes.

3. **Multiple API Processes**: Kill existing `chronicle-api.exe` before starting new instance.

---

## Critical Constraint

**NEVER delete or erase anything in MemMachine.** Chronicle syncs data TO MemMachine for AI insights. MemMachine is the source of truth for the user's memory/knowledge system.

---

## Testing

```bash
# Test API health
curl http://localhost:8082/health

# Log a data point
curl -X POST http://localhost:8082/api/v1/ingest \
  -H "Content-Type: application/json" \
  -d '{"metric":"mood","value":7.5}'

# Query data
curl -X POST http://localhost:8082/api/v1/query \
  -H "Content-Type: application/json" \
  -d '{"select":["mood"],"time_range":{"start":0,"end":9999999999999}}'
```

---

## Future Enhancements

- [ ] Apple Health integration (via export XML)
- [ ] Docker Compose for all services
- [ ] API authentication (JWT)
- [ ] Data encryption at rest
- [ ] Mobile-responsive dashboard improvements
- [ ] Prometheus metrics endpoint
