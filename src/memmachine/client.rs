//! MemMachine REST API Client
//!
//! HTTP client for communicating with MemMachine's REST API.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// MemMachine REST API client
pub struct MemMachineClient {
    client: Client,
    config: MemMachineConfig,
}

/// Configuration for MemMachine client
#[derive(Debug, Clone)]
pub struct MemMachineConfig {
    /// Base URL for MemMachine API (e.g., "http://localhost:8080")
    pub base_url: String,
    /// Group ID for data isolation ("chronicle")
    pub group_id: String,
    /// Agent ID for the Chronicle engine
    pub agent_id: String,
    /// User ID for the human tracking data
    pub user_id: String,
    /// Request timeout in milliseconds
    pub request_timeout_ms: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
}

impl Default for MemMachineConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            group_id: "chronicle".to_string(),
            agent_id: "chronicle-engine".to_string(),
            user_id: "default-user".to_string(),
            request_timeout_ms: 5000,
            max_retries: 3,
        }
    }
}

/// Session context for MemMachine API calls
#[derive(Debug, Clone, Serialize)]
pub struct SessionContext {
    pub group_id: String,
    pub agent_id: Vec<String>,
    pub user_id: Vec<String>,
    pub session_id: String,
}

impl MemMachineClient {
    /// Create a new MemMachine client with the given configuration
    pub fn new(config: MemMachineConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_millis(config.request_timeout_ms))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &MemMachineConfig {
        &self.config
    }

    /// Create session context for API calls
    fn session_context(&self, session_id: &str) -> SessionContext {
        SessionContext {
            group_id: self.config.group_id.clone(),
            agent_id: vec![self.config.agent_id.clone()],
            user_id: vec![self.config.user_id.clone()],
            session_id: session_id.to_string(),
        }
    }

    /// Check if MemMachine is available
    pub async fn health_check(&self) -> Result<(), MemMachineError> {
        let url = format!("{}/health", self.config.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    MemMachineError::Timeout
                } else if e.is_connect() {
                    MemMachineError::Unavailable
                } else {
                    MemMachineError::Request(e)
                }
            })?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(MemMachineError::Unavailable)
        }
    }

    /// Add an episodic memory entry
    ///
    /// Used for daily summaries, insight queries, and other time-bound events.
    pub async fn add_episodic_memory(
        &self,
        session_id: &str,
        content: &str,
        episode_type: &str,
        metadata: HashMap<String, String>,
    ) -> Result<(), MemMachineError> {
        let url = format!("{}/v1/memories/episodic", self.config.base_url);

        let body = AddEpisodicRequest {
            session: self.session_context(session_id),
            producer: self.config.user_id.clone(),
            produced_for: self.config.agent_id.clone(),
            episode_content: content.to_string(),
            episode_type: episode_type.to_string(),
            content_type: "string".to_string(),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
        };

        self.send_post(&url, &body).await
    }

    /// Search memories for relevant context
    ///
    /// Uses semantic similarity to find related memories.
    pub async fn search_memories(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryResult>, MemMachineError> {
        let url = format!("{}/v1/memories/search", self.config.base_url);

        let body = SearchRequest {
            session: self.session_context(session_id),
            query: query.to_string(),
            limit: limit as u32,
        };

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    MemMachineError::Timeout
                } else if e.is_connect() {
                    MemMachineError::Unavailable
                } else {
                    MemMachineError::Request(e)
                }
            })?;

        if response.status().is_success() {
            let result: SearchResponse = response.json().await.map_err(MemMachineError::Request)?;
            Ok(result.results)
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(MemMachineError::ApiError {
                status: status.as_u16(),
                message: text,
            })
        }
    }

    /// Add unified memory (episodic + profile)
    ///
    /// Used for learned patterns and correlations that should be
    /// stored in both episodic and profile memory.
    pub async fn add_unified_memory(
        &self,
        session_id: &str,
        content: &str,
        episode_type: &str,
    ) -> Result<(), MemMachineError> {
        let url = format!("{}/v1/memories", self.config.base_url);

        let body = AddEpisodicRequest {
            session: self.session_context(session_id),
            producer: self.config.agent_id.clone(),
            produced_for: self.config.user_id.clone(),
            episode_content: content.to_string(),
            episode_type: episode_type.to_string(),
            content_type: "string".to_string(),
            metadata: None,
        };

        self.send_post(&url, &body).await
    }

    /// Send a POST request with retry logic
    async fn send_post<T: Serialize>(&self, url: &str, body: &T) -> Result<(), MemMachineError> {
        let mut last_error = MemMachineError::Unavailable;

        for attempt in 0..self.config.max_retries {
            if attempt > 0 {
                // Exponential backoff: 1s, 4s, 9s...
                let delay = std::time::Duration::from_secs((attempt as u64).pow(2));
                tokio::time::sleep(delay).await;
            }

            match self.client.post(url).json(body).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        return Ok(());
                    } else if response.status().as_u16() == 429 {
                        // Rate limited - check Retry-After header
                        if let Some(retry_after) = response.headers().get("Retry-After") {
                            if let Ok(secs) = retry_after.to_str().unwrap_or("5").parse::<u64>() {
                                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                            }
                        }
                        last_error = MemMachineError::RateLimited;
                        continue;
                    } else {
                        let status = response.status();
                        let text = response.text().await.unwrap_or_default();
                        return Err(MemMachineError::ApiError {
                            status: status.as_u16(),
                            message: text,
                        });
                    }
                }
                Err(e) => {
                    last_error = if e.is_timeout() {
                        MemMachineError::Timeout
                    } else if e.is_connect() {
                        MemMachineError::Unavailable
                    } else {
                        MemMachineError::Request(e)
                    };
                    continue;
                }
            }
        }

        Err(last_error)
    }
}

// ============================================
// Request/Response DTOs
// ============================================

#[derive(Debug, Serialize)]
struct AddEpisodicRequest {
    session: SessionContext,
    producer: String,
    produced_for: String,
    episode_content: String,
    episode_type: String,
    content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct SearchRequest {
    session: SessionContext,
    query: String,
    limit: u32,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<MemoryResult>,
}

/// A memory result from MemMachine search
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryResult {
    /// The content of the memory
    pub content: String,
    /// Type of episode (e.g., "daily_summary", "pattern")
    pub episode_type: Option<String>,
    /// When the memory was created
    pub timestamp: Option<String>,
    /// Relevance score (higher is more relevant)
    pub score: Option<f64>,
}

// ============================================
// Errors
// ============================================

/// Errors that can occur when communicating with MemMachine
#[derive(Error, Debug)]
pub enum MemMachineError {
    #[error("MemMachine unavailable")]
    Unavailable,

    #[error("Request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error {status}: {message}")]
    ApiError { status: u16, message: String },

    #[error("Request timeout")]
    Timeout,

    #[error("Rate limited")]
    RateLimited,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MemMachineConfig::default();
        assert_eq!(config.base_url, "http://localhost:8080");
        assert_eq!(config.group_id, "chronicle");
        assert_eq!(config.agent_id, "chronicle-engine");
    }

    #[test]
    fn test_session_context() {
        let config = MemMachineConfig::default();
        let client = MemMachineClient::new(config);

        let ctx = client.session_context("test-session");
        assert_eq!(ctx.group_id, "chronicle");
        assert_eq!(ctx.agent_id, vec!["chronicle-engine"]);
        assert_eq!(ctx.session_id, "test-session");
    }
}
