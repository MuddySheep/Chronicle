//! WebSocket Message Types
//!
//! Defines all message types for WebSocket communication between
//! clients (dashboards) and the Chronicle server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Messages sent from client to server
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Subscribe to topics for real-time updates
    Subscribe {
        /// List of topics to subscribe to (e.g., "metrics.mood", "metrics.*")
        topics: Vec<String>,
    },
    /// Unsubscribe from topics
    Unsubscribe {
        /// List of topics to unsubscribe from
        topics: Vec<String>,
    },
    /// Ping for keepalive
    Ping,
}

/// Messages sent from server to client
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// A new data point was ingested
    DataPoint {
        /// Metric name
        metric: String,
        /// Metric value
        value: f64,
        /// Timestamp in milliseconds
        timestamp: i64,
        /// Optional tags
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        tags: HashMap<String, String>,
    },
    /// Subscription confirmed
    Subscribed {
        /// Topics successfully subscribed to
        topics: Vec<String>,
    },
    /// Unsubscription confirmed
    Unsubscribed {
        /// Topics successfully unsubscribed from
        topics: Vec<String>,
    },
    /// Pong response to ping
    Pong,
    /// Error message
    Error {
        /// Error description
        message: String,
    },
    /// Connection established
    Connected {
        /// Unique connection identifier
        connection_id: String,
    },
}

/// Internal event for broadcasting through the hub
#[derive(Debug, Clone)]
pub struct WsEvent {
    /// Topic this event belongs to (e.g., "metrics.mood")
    pub topic: String,
    /// The message to send to subscribers
    pub message: ServerMessage,
}

impl WsEvent {
    /// Create a data point event from ingested data
    pub fn data_point(
        metric: &str,
        value: f64,
        timestamp: i64,
        tags: HashMap<String, String>,
    ) -> Self {
        Self {
            topic: format!("metrics.{}", metric),
            message: ServerMessage::DataPoint {
                metric: metric.to_string(),
                value,
                timestamp,
                tags,
            },
        }
    }

    /// Create an insight event
    pub fn insight(content: &str) -> Self {
        Self {
            topic: "insights".to_string(),
            message: ServerMessage::Error {
                message: content.to_string(), // Placeholder, would use proper insight message
            },
        }
    }

    /// Create a system event
    pub fn system(message: &str) -> Self {
        Self {
            topic: "system".to_string(),
            message: ServerMessage::Error {
                message: message.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_deserialize_subscribe() {
        let json = r#"{"type": "subscribe", "topics": ["metrics.mood", "metrics.energy"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Subscribe { topics } => {
                assert_eq!(topics.len(), 2);
                assert_eq!(topics[0], "metrics.mood");
            }
            _ => panic!("Expected Subscribe"),
        }
    }

    #[test]
    fn test_client_message_deserialize_ping() {
        let json = r#"{"type": "ping"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientMessage::Ping));
    }

    #[test]
    fn test_server_message_serialize_data_point() {
        let msg = ServerMessage::DataPoint {
            metric: "mood".to_string(),
            value: 7.5,
            timestamp: 1699000000000,
            tags: HashMap::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"data_point\""));
        assert!(json.contains("\"metric\":\"mood\""));
        assert!(json.contains("\"value\":7.5"));
    }

    #[test]
    fn test_server_message_serialize_connected() {
        let msg = ServerMessage::Connected {
            connection_id: "abc-123".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"connected\""));
        assert!(json.contains("\"connection_id\":\"abc-123\""));
    }

    #[test]
    fn test_ws_event_data_point() {
        let event = WsEvent::data_point("mood", 8.0, 1699000000000, HashMap::new());
        assert_eq!(event.topic, "metrics.mood");
        match event.message {
            ServerMessage::DataPoint { metric, value, .. } => {
                assert_eq!(metric, "mood");
                assert_eq!(value, 8.0);
            }
            _ => panic!("Expected DataPoint"),
        }
    }
}
