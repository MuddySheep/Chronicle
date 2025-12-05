# Chronicle Project Status

## What We Built (8 Layers Complete)

### Layer 1-3: Core Storage Engine
- Append-only log with LZ4 compression
- B-tree indexes for fast time-range queries
- Write-ahead log for durability
- SQLite-backed index storage

### Layer 4: REST API (Axum)
- `POST /api/v1/ingest` - Write data points
- `POST /api/v1/query` - Query time-series data
- `GET /api/v1/metrics` - List all metrics
- `GET /api/v1/export` - Export data
- `GET /health` - Health check
- `GET /api/v1/ws` - WebSocket endpoint

### Layer 5: MemMachine Integration
- Sync data to MemMachine for AI insights
- Correlation analysis engine
- Pattern detection via MemMachine's memory system

### Layer 6: WebSocket Real-Time
- Live data streaming
- Pub/sub for metric updates
- Connection hub for multiple clients

### Layer 7: Dashboard UI (Leptos/WASM)
- Dashboard with charts and metrics
- Settings page
- Metrics management
- Data entry forms
- Real-time updates via WebSocket

### Layer 8: Integrations & Polish
- Fitbit integration (OAuth)
- GitHub integration
- CSV import
- CLI tool (`chronicle-cli`)
- Configuration system (TOML + env vars)

---

## Current Issues

### Issue 1: MemMachine Not Connected

**Problem**: Chronicle API starts without MemMachine integration because the `MEMMACHINE_URL` environment variable isn't being passed correctly through the bash/PowerShell layer.

**Log Evidence**:
```
INFO chronicle_api: MemMachine integration disabled (set MEMMACHINE_URL to enable)
```

**Root Cause**: Claude Code's bash tool runs through WSL/bash which doesn't properly pass Windows environment variables to PowerShell subprocesses.

**Solution**: Run Chronicle API directly from a Windows terminal:

```cmd
:: In Windows Command Prompt
cd C:\Users\Brandon\Documents\cc\MermaidStress\rust\Chronicle
set MEMMACHINE_URL=http://localhost:8080
cargo run --bin chronicle-api
```

Or PowerShell:
```powershell
$env:MEMMACHINE_URL = "http://localhost:8080"
cd C:\Users\Brandon\Documents\cc\MermaidStress\rust\Chronicle
cargo run --bin chronicle-api
```

**When connected, you'll see**:
```
INFO chronicle_api: MemMachine integration enabled: http://localhost:8080
INFO chronicle_api: MemMachine connection verified
INFO chronicle_api: Starting background sync to MemMachine
```

---

### Issue 2: WebSocket Connection Failing

**Problem**: Dashboard shows "Disconnected" and console shows WebSocket errors.

**Console Error**:
```
WebSocket connection to 'ws://localhost:8082/api/v1/ws' failed
```

**Possible Causes**:

1. **Browser Local Storage**: May have cached an old/wrong API URL
   - Fix: Clear local storage for `http://127.0.0.1:8084`
   - Delete key: `chronicle_api_url`

2. **API Not Running**: The Chronicle API server might not be running
   - Check: `curl http://localhost:8082/health`
   - Should return: `{"status":"healthy",...}`

3. **WebSocket Route**: The server's WebSocket handler at `/api/v1/ws`
   - The UI now correctly connects to `ws://localhost:8082/api/v1/ws`

4. **CORS Issues**: WebSocket connections from different origins
   - API allows CORS from `localhost:8084` and `127.0.0.1:8084`

**Fixes Applied**:
- `chronicle-ui/src/state/websocket.rs`: Fixed URL construction
- `chronicle-ui/src/api/client.rs`: Added URL normalization (removes trailing slashes)
- Used `get_untracked()` to avoid reactive context warnings

---

### Issue 3: API URL Mismatch

**Problem**: UI was making requests to `/api//ws` (double slash) or `/api/ws/ws`.

**Root Cause**:
- Local storage had a cached API URL with trailing slash
- URL construction logic was appending `/ws` incorrectly

**Fix Applied**:
- `get_api_base()` now trims trailing slashes
- WebSocket URL construction simplified to `{api_base}/ws`

---

## Service Status

| Service | Port | Status | Notes |
|---------|------|--------|-------|
| MemMachine | 8080 | Running | AI memory/insights backend |
| Chronicle API | 8082 | Running | Without MemMachine (env var issue) |
| Dashboard UI | 8084 | Running | WebSocket disconnected |

---

## To Get Everything Working

### Step 1: Stop Current API
Kill any running Chronicle API processes.

### Step 2: Start API with MemMachine
Open a **Windows Command Prompt** (not through Claude Code):

```cmd
cd C:\Users\Brandon\Documents\cc\MermaidStress\rust\Chronicle
set MEMMACHINE_URL=http://localhost:8080
cargo run --bin chronicle-api
```

### Step 3: Clear Browser Cache
1. Open http://127.0.0.1:8084
2. Open DevTools (F12)
3. Application tab → Local Storage → Clear `chronicle_api_url`
4. Hard refresh (Ctrl+Shift+R)

### Step 4: Verify Connections
- API Health: http://localhost:8082/health
- WebSocket: Should show "Connected" in dashboard footer
- MemMachine: API logs should show "MemMachine connection verified"

---

## File Locations

```
Chronicle/
├── src/
│   ├── bin/
│   │   ├── api.rs          # API server binary
│   │   └── cli.rs          # CLI tool binary
│   ├── storage/            # Layer 1-3: Storage engine
│   ├── api/                # Layer 4: REST API
│   ├── memmachine/         # Layer 5: MemMachine integration
│   ├── websocket/          # Layer 6: WebSocket
│   ├── integrations/       # Layer 8: External integrations
│   └── config.rs           # Configuration system
├── chronicle-ui/           # Layer 7: Dashboard UI
│   ├── src/
│   │   ├── api/client.rs   # HTTP client
│   │   ├── state/
│   │   │   ├── global.rs   # Global state
│   │   │   └── websocket.rs # WebSocket client (FIXED)
│   │   ├── pages/          # Dashboard pages
│   │   └── components/     # UI components
│   └── Trunk.toml          # Build config (port 8084)
└── chronicle_data/         # Data storage directory
```

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MEMMACHINE_URL` | (none) | MemMachine server URL (required for insights) |
| `MEMMACHINE_USER_ID` | `default-user` | User ID for MemMachine |
| `CHRONICLE_HOST` | `0.0.0.0` | API bind address |
| `CHRONICLE_PORT` | `8082` | API port |
| `CHRONICLE_DATA_DIR` | `chronicle_data` | Data storage directory |
| `CHRONICLE_LOG_LEVEL` | `info` | Log verbosity |

---

## Quick Test Commands

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

# List metrics
curl http://localhost:8082/api/v1/metrics
```

---

## Next Steps

1. **Immediate**: Get API running with MemMachine connected (manual terminal)
2. **Immediate**: Verify WebSocket connects after browser cache clear
3. **Future**: Add startup script/batch file for easier launching
4. **Future**: Consider Docker Compose for all services
