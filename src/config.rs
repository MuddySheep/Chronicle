//! Configuration System
//!
//! Handles loading configuration from files and environment variables.
//! Supports TOML config files and environment variable overrides.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub storage: StorageConfig,

    #[serde(default)]
    pub api: ApiConfig,

    #[serde(default)]
    pub memmachine: MemMachineConfig,

    #[serde(default)]
    pub integrations: IntegrationsConfig,

    #[serde(default)]
    pub logging: LoggingConfig,
}

/// Storage engine configuration
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: String,

    #[serde(default = "default_block_size")]
    pub block_size: usize,

    #[serde(default = "default_flush_interval")]
    pub flush_interval_ms: u64,

    #[serde(default = "default_wal_enabled")]
    pub wal_enabled: bool,
}

fn default_data_dir() -> String {
    dirs::data_local_dir()
        .map(|p| p.join("chronicle").to_string_lossy().to_string())
        .unwrap_or_else(|| "./chronicle_data".to_string())
}

fn default_block_size() -> usize {
    64 * 1024 // 64 KB
}

fn default_flush_interval() -> u64 {
    5000 // 5 seconds
}

fn default_wal_enabled() -> bool {
    true
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            block_size: default_block_size(),
            flush_interval_ms: default_flush_interval(),
            wal_enabled: default_wal_enabled(),
        }
    }
}

/// API server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default)]
    pub cors_origins: Vec<String>,

    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8082
}

fn default_request_timeout() -> u64 {
    30
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            cors_origins: vec![
                "http://localhost:8084".to_string(),
                "http://127.0.0.1:8084".to_string(),
            ],
            request_timeout_secs: default_request_timeout(),
        }
    }
}

/// MemMachine integration configuration
#[derive(Debug, Clone, Deserialize)]
pub struct MemMachineConfig {
    #[serde(default = "default_memmachine_url")]
    pub url: String,

    #[serde(default = "default_group_id")]
    pub group_id: String,

    #[serde(default = "default_sync_interval")]
    pub sync_interval_hours: u64,

    #[serde(default = "default_memmachine_enabled")]
    pub enabled: bool,
}

fn default_memmachine_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_group_id() -> String {
    "chronicle".to_string()
}

fn default_sync_interval() -> u64 {
    1
}

fn default_memmachine_enabled() -> bool {
    true
}

impl Default for MemMachineConfig {
    fn default() -> Self {
        Self {
            url: default_memmachine_url(),
            group_id: default_group_id(),
            sync_interval_hours: default_sync_interval(),
            enabled: default_memmachine_enabled(),
        }
    }
}

/// External integrations configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct IntegrationsConfig {
    pub fitbit: Option<FitbitIntegrationConfig>,
    pub github: Option<GitHubIntegrationConfig>,
}

/// Fitbit integration configuration
#[derive(Debug, Clone, Deserialize)]
pub struct FitbitIntegrationConfig {
    #[serde(default)]
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_fitbit_redirect")]
    pub redirect_uri: String,
    #[serde(default = "default_fitbit_interval")]
    pub sync_interval_hours: u64,
}

fn default_fitbit_redirect() -> String {
    "http://localhost:8082/api/v1/integrations/fitbit/callback".to_string()
}

fn default_fitbit_interval() -> u64 {
    6
}

/// GitHub integration configuration
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubIntegrationConfig {
    #[serde(default)]
    pub enabled: bool,
    pub token: String,
    pub username: String,
    #[serde(default = "default_github_interval")]
    pub sync_interval_hours: u64,
}

fn default_github_interval() -> u64 {
    1
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,

    #[serde(default = "default_log_format")]
    pub format: String,

    pub file: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            file: None,
        }
    }
}

impl Config {
    /// Load configuration from a file
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.to_path_buf(),
            error: e.to_string(),
        })?;

        let config: Config = toml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            error: e.to_string(),
        })?;

        Ok(config)
    }

    /// Load configuration from environment variables only
    pub fn from_env() -> Self {
        let mut config = Config::default();

        // Storage overrides
        if let Ok(data_dir) = std::env::var("CHRONICLE_DATA_DIR") {
            config.storage.data_dir = data_dir;
        }

        // API overrides
        if let Ok(host) = std::env::var("CHRONICLE_API_HOST") {
            config.api.host = host;
        }
        if let Ok(port) = std::env::var("CHRONICLE_API_PORT") {
            if let Ok(p) = port.parse() {
                config.api.port = p;
            }
        }

        // MemMachine overrides
        if let Ok(url) = std::env::var("CHRONICLE_MEMMACHINE_URL") {
            config.memmachine.url = url;
        }
        if let Ok(group_id) = std::env::var("CHRONICLE_MEMMACHINE_GROUP") {
            config.memmachine.group_id = group_id;
        }

        // Logging overrides
        if let Ok(level) = std::env::var("CHRONICLE_LOG_LEVEL") {
            config.logging.level = level;
        }
        if let Ok(format) = std::env::var("CHRONICLE_LOG_FORMAT") {
            config.logging.format = format;
        }

        config
    }

    /// Load configuration with environment variable overrides
    pub fn load_with_env(path: &Path) -> Result<Self, ConfigError> {
        let mut config = Self::load(path)?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Load from default locations or environment
    pub fn load_default() -> Self {
        // Try default config locations
        let config_paths = [
            dirs::config_dir().map(|p| p.join("chronicle").join("config.toml")),
            Some(PathBuf::from("/etc/chronicle/config.toml")),
            Some(PathBuf::from("./config.toml")),
        ];

        for path_opt in config_paths.iter().flatten() {
            if path_opt.exists() {
                match Self::load_with_env(path_opt) {
                    Ok(config) => {
                        tracing::info!("Loaded config from {:?}", path_opt);
                        return config;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load config from {:?}: {}", path_opt, e);
                    }
                }
            }
        }

        // Fall back to environment-only config
        tracing::info!("Using default config with environment overrides");
        Self::from_env()
    }

    /// Apply environment variable overrides to an existing config
    fn apply_env_overrides(&mut self) {
        // Storage overrides
        if let Ok(data_dir) = std::env::var("CHRONICLE_DATA_DIR") {
            self.storage.data_dir = data_dir;
        }

        // API overrides
        if let Ok(host) = std::env::var("CHRONICLE_API_HOST") {
            self.api.host = host;
        }
        if let Ok(port) = std::env::var("CHRONICLE_API_PORT") {
            if let Ok(p) = port.parse() {
                self.api.port = p;
            }
        }

        // MemMachine overrides
        if let Ok(url) = std::env::var("CHRONICLE_MEMMACHINE_URL") {
            self.memmachine.url = url;
        }
        if let Ok(group_id) = std::env::var("CHRONICLE_MEMMACHINE_GROUP") {
            self.memmachine.group_id = group_id;
        }

        // Logging overrides
        if let Ok(level) = std::env::var("CHRONICLE_LOG_LEVEL") {
            self.logging.level = level;
        }
        if let Ok(format) = std::env::var("CHRONICLE_LOG_FORMAT") {
            self.logging.format = format;
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            api: ApiConfig::default(),
            memmachine: MemMachineConfig::default(),
            integrations: IntegrationsConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file {path:?}: {error}")]
    Io { path: PathBuf, error: String },

    #[error("Failed to parse config file {path:?}: {error}")]
    Parse { path: PathBuf, error: String },
}

/// Generate a default config file content
pub fn generate_default_config() -> String {
    r#"# Chronicle Configuration
#
# Environment variables override these settings:
# - CHRONICLE_DATA_DIR
# - CHRONICLE_API_HOST
# - CHRONICLE_API_PORT
# - CHRONICLE_MEMMACHINE_URL
# - CHRONICLE_LOG_LEVEL
# - CHRONICLE_LOG_FORMAT

[storage]
# Directory for storing data files
data_dir = "~/.local/share/chronicle"

# Block size for storage (bytes)
block_size = 65536

# How often to flush data to disk (ms)
flush_interval_ms = 5000

# Enable write-ahead log for durability
wal_enabled = true

[api]
# API server host
host = "0.0.0.0"

# API server port
port = 8082

# Allowed CORS origins
cors_origins = ["http://localhost:8084", "http://127.0.0.1:8084"]

# Request timeout in seconds
request_timeout_secs = 30

[memmachine]
# MemMachine server URL
url = "http://localhost:8080"

# Group ID for storing memories
group_id = "chronicle"

# How often to sync data to MemMachine (hours)
sync_interval_hours = 1

# Enable MemMachine integration
enabled = true

[integrations.fitbit]
# Enable Fitbit integration
enabled = false

# Fitbit OAuth credentials (get from dev.fitbit.com)
client_id = ""
client_secret = ""

# OAuth callback URL
redirect_uri = "http://localhost:8082/api/v1/integrations/fitbit/callback"

# Sync interval (hours)
sync_interval_hours = 6

[integrations.github]
# Enable GitHub integration
enabled = false

# GitHub Personal Access Token
token = ""

# GitHub username
username = ""

# Sync interval (hours)
sync_interval_hours = 1

[logging]
# Log level: trace, debug, info, warn, error
level = "info"

# Log format: pretty (for development) or json (for production)
format = "pretty"

# Optional log file path
# file = "/var/log/chronicle/chronicle.log"
"#
    .to_string()
}
