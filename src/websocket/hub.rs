//! WebSocket Connection Hub
//!
//! Manages all WebSocket connections, subscriptions, and message broadcasting.
//! Uses tokio broadcast channels for efficient pub/sub.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

use super::messages::{ServerMessage, WsEvent};

/// Unique identifier for a WebSocket connection
pub type ConnectionId = String;

/// Manages all WebSocket connections and subscriptions
pub struct ConnectionHub {
    /// Active connections: ConnectionId → ConnectionHandle
    connections: Arc<RwLock<HashMap<ConnectionId, ConnectionHandle>>>,
    /// Topic subscriptions: Topic → Set of ConnectionIds
    subscriptions: Arc<RwLock<HashMap<String, HashSet<ConnectionId>>>>,
    /// Broadcast channel for events (used internally)
    broadcast_tx: broadcast::Sender<WsEvent>,
    /// Configuration
    config: HubConfig,
}

/// Configuration for the connection hub
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    /// Capacity of the broadcast channel
    pub broadcast_capacity: usize,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            broadcast_capacity: 1024,
        }
    }
}

/// Handle for sending messages to a specific connection
pub struct ConnectionHandle {
    /// Channel sender for this connection
    pub sender: mpsc::UnboundedSender<ServerMessage>,
    /// Topics this connection is subscribed to
    pub subscriptions: HashSet<String>,
}

impl ConnectionHub {
    /// Create a new connection hub
    pub fn new(config: HubConfig) -> Self {
        let (broadcast_tx, _) = broadcast::channel(config.broadcast_capacity);

        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
            config,
        }
    }

    /// Register a new WebSocket connection
    ///
    /// Returns the connection ID on success, or an error if the connection
    /// limit has been reached.
    pub async fn register(
        &self,
        sender: mpsc::UnboundedSender<ServerMessage>,
    ) -> Result<ConnectionId, HubError> {
        let connections = self.connections.read().await;
        if connections.len() >= self.config.max_connections {
            return Err(HubError::TooManyConnections);
        }
        drop(connections);

        let id = Uuid::new_v4().to_string();
        let handle = ConnectionHandle {
            sender,
            subscriptions: HashSet::new(),
        };

        self.connections.write().await.insert(id.clone(), handle);

        tracing::info!(connection_id = %id, "WebSocket connected");
        Ok(id)
    }

    /// Unregister a connection and clean up its subscriptions
    pub async fn unregister(&self, id: &str) {
        // Remove from connections
        let handle = self.connections.write().await.remove(id);

        // Remove from all subscriptions
        if let Some(handle) = handle {
            let mut subs = self.subscriptions.write().await;
            for topic in handle.subscriptions {
                if let Some(subscribers) = subs.get_mut(&topic) {
                    subscribers.remove(id);
                    // Clean up empty topic entries
                    if subscribers.is_empty() {
                        subs.remove(&topic);
                    }
                }
            }
        }

        tracing::info!(connection_id = %id, "WebSocket disconnected");
    }

    /// Subscribe a connection to topics
    pub async fn subscribe(
        &self,
        id: &str,
        topics: Vec<String>,
    ) -> Result<Vec<String>, HubError> {
        let mut connections = self.connections.write().await;
        let handle = connections
            .get_mut(id)
            .ok_or(HubError::ConnectionNotFound)?;

        let mut subs = self.subscriptions.write().await;
        let mut subscribed = Vec::new();

        for topic in topics {
            // Validate topic
            if !self.is_valid_topic(&topic) {
                tracing::warn!(topic = %topic, "Invalid topic ignored");
                continue;
            }

            // Add to connection's subscriptions
            handle.subscriptions.insert(topic.clone());

            // Add to topic's subscribers
            subs.entry(topic.clone())
                .or_insert_with(HashSet::new)
                .insert(id.to_string());

            subscribed.push(topic);
        }

        tracing::debug!(
            connection_id = %id,
            topics = ?subscribed,
            "Subscribed to topics"
        );

        Ok(subscribed)
    }

    /// Unsubscribe a connection from topics
    pub async fn unsubscribe(
        &self,
        id: &str,
        topics: Vec<String>,
    ) -> Result<Vec<String>, HubError> {
        let mut connections = self.connections.write().await;
        let handle = connections
            .get_mut(id)
            .ok_or(HubError::ConnectionNotFound)?;

        let mut subs = self.subscriptions.write().await;
        let mut unsubscribed = Vec::new();

        for topic in topics {
            if handle.subscriptions.remove(&topic) {
                unsubscribed.push(topic.clone());

                if let Some(subscribers) = subs.get_mut(&topic) {
                    subscribers.remove(id);
                    if subscribers.is_empty() {
                        subs.remove(&topic);
                    }
                }
            }
        }

        tracing::debug!(
            connection_id = %id,
            topics = ?unsubscribed,
            "Unsubscribed from topics"
        );

        Ok(unsubscribed)
    }

    /// Broadcast an event to all subscribers of its topic
    ///
    /// This is called internally when events are published.
    pub async fn broadcast(&self, event: &WsEvent) {
        let subs = self.subscriptions.read().await;
        let connections = self.connections.read().await;

        // Find direct subscribers for this topic
        let subscriber_ids = subs.get(&event.topic).cloned().unwrap_or_default();

        // Also check for wildcard subscribers (e.g., "metrics.*" matches "metrics.mood")
        let wildcard_topic = event
            .topic
            .split('.')
            .next()
            .map(|p| format!("{}.*", p));
        let wildcard_ids = wildcard_topic
            .and_then(|t| subs.get(&t).cloned())
            .unwrap_or_default();

        // Combine all subscriber IDs
        let all_ids: HashSet<_> = subscriber_ids.union(&wildcard_ids).collect();

        // Send to each subscriber
        let mut sent_count = 0;
        for id in all_ids {
            if let Some(handle) = connections.get(id) {
                if handle.sender.send(event.message.clone()).is_ok() {
                    sent_count += 1;
                }
            }
        }

        if sent_count > 0 {
            tracing::trace!(
                topic = %event.topic,
                subscribers = sent_count,
                "Broadcast event"
            );
        }
    }

    /// Publish an event to the broadcast channel
    ///
    /// This is called from the ingest API to publish data point events.
    pub fn publish(&self, event: WsEvent) {
        // Try to send to broadcast channel (for any internal listeners)
        let _ = self.broadcast_tx.send(event.clone());

        // Also directly broadcast to subscribers
        let hub = self.clone_for_broadcast();
        tokio::spawn(async move {
            hub.broadcast(&event).await;
        });
    }

    /// Send a message directly to a specific connection
    pub async fn send_to(
        &self,
        id: &str,
        message: ServerMessage,
    ) -> Result<(), HubError> {
        let connections = self.connections.read().await;
        let handle = connections.get(id).ok_or(HubError::ConnectionNotFound)?;

        handle
            .sender
            .send(message)
            .map_err(|_| HubError::SendFailed)
    }

    /// Get a receiver for the broadcast channel (internal use)
    pub fn subscribe_broadcast(&self) -> broadcast::Receiver<WsEvent> {
        self.broadcast_tx.subscribe()
    }

    /// Check if a topic is valid
    fn is_valid_topic(&self, topic: &str) -> bool {
        // Valid topics:
        // - metrics.* (wildcard for all metrics)
        // - metrics.{name} (specific metric)
        // - category.{cat} (all metrics in category)
        // - insights (insight updates)
        // - system (system events)
        topic.starts_with("metrics.")
            || topic.starts_with("category.")
            || topic == "insights"
            || topic == "system"
    }

    /// Get the current connection count
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Get subscription count for a topic
    pub async fn subscription_count(&self, topic: &str) -> usize {
        self.subscriptions
            .read()
            .await
            .get(topic)
            .map(|s| s.len())
            .unwrap_or(0)
    }

    /// Clone self for use in async broadcast task
    fn clone_for_broadcast(&self) -> ConnectionHubRef {
        ConnectionHubRef {
            connections: Arc::clone(&self.connections),
            subscriptions: Arc::clone(&self.subscriptions),
        }
    }
}

/// Reference to hub internals for async broadcast
struct ConnectionHubRef {
    connections: Arc<RwLock<HashMap<ConnectionId, ConnectionHandle>>>,
    subscriptions: Arc<RwLock<HashMap<String, HashSet<ConnectionId>>>>,
}

impl ConnectionHubRef {
    async fn broadcast(&self, event: &WsEvent) {
        let subs = self.subscriptions.read().await;
        let connections = self.connections.read().await;

        let subscriber_ids = subs.get(&event.topic).cloned().unwrap_or_default();

        let wildcard_topic = event
            .topic
            .split('.')
            .next()
            .map(|p| format!("{}.*", p));
        let wildcard_ids = wildcard_topic
            .and_then(|t| subs.get(&t).cloned())
            .unwrap_or_default();

        let all_ids: HashSet<_> = subscriber_ids.union(&wildcard_ids).collect();

        for id in all_ids {
            if let Some(handle) = connections.get(id) {
                let _ = handle.sender.send(event.message.clone());
            }
        }
    }
}

/// Errors that can occur in the connection hub
#[derive(Debug, Error)]
pub enum HubError {
    #[error("Too many connections (limit: {0})", 1000)]
    TooManyConnections,

    #[error("Connection not found")]
    ConnectionNotFound,

    #[error("Failed to send message")]
    SendFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HubConfig::default();
        assert_eq!(config.max_connections, 1000);
        assert_eq!(config.broadcast_capacity, 1024);
    }

    #[test]
    fn test_valid_topics() {
        let hub = ConnectionHub::new(HubConfig::default());

        assert!(hub.is_valid_topic("metrics.mood"));
        assert!(hub.is_valid_topic("metrics.*"));
        assert!(hub.is_valid_topic("category.health"));
        assert!(hub.is_valid_topic("insights"));
        assert!(hub.is_valid_topic("system"));

        assert!(!hub.is_valid_topic("invalid"));
        assert!(!hub.is_valid_topic(""));
        assert!(!hub.is_valid_topic("random.topic"));
    }

    #[tokio::test]
    async fn test_register_unregister() {
        let hub = ConnectionHub::new(HubConfig::default());
        let (tx, _rx) = mpsc::unbounded_channel();

        let id = hub.register(tx).await.unwrap();
        assert!(!id.is_empty());
        assert_eq!(hub.connection_count().await, 1);

        hub.unregister(&id).await;
        assert_eq!(hub.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_subscribe_unsubscribe() {
        let hub = ConnectionHub::new(HubConfig::default());
        let (tx, _rx) = mpsc::unbounded_channel();

        let id = hub.register(tx).await.unwrap();

        // Subscribe
        let subscribed = hub
            .subscribe(&id, vec!["metrics.mood".to_string()])
            .await
            .unwrap();
        assert_eq!(subscribed, vec!["metrics.mood"]);
        assert_eq!(hub.subscription_count("metrics.mood").await, 1);

        // Unsubscribe
        let unsubscribed = hub
            .unsubscribe(&id, vec!["metrics.mood".to_string()])
            .await
            .unwrap();
        assert_eq!(unsubscribed, vec!["metrics.mood"]);
        assert_eq!(hub.subscription_count("metrics.mood").await, 0);

        hub.unregister(&id).await;
    }

    #[tokio::test]
    async fn test_connection_limit() {
        let config = HubConfig {
            max_connections: 2,
            broadcast_capacity: 16,
        };
        let hub = ConnectionHub::new(config);

        let (tx1, _) = mpsc::unbounded_channel();
        let (tx2, _) = mpsc::unbounded_channel();
        let (tx3, _) = mpsc::unbounded_channel();

        let id1 = hub.register(tx1).await.unwrap();
        let id2 = hub.register(tx2).await.unwrap();
        let result = hub.register(tx3).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HubError::TooManyConnections));

        hub.unregister(&id1).await;
        hub.unregister(&id2).await;
    }

    #[tokio::test]
    async fn test_broadcast_to_subscribers() {
        let hub = ConnectionHub::new(HubConfig::default());

        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();

        let id1 = hub.register(tx1).await.unwrap();
        let id2 = hub.register(tx2).await.unwrap();

        // Only id1 subscribes to mood
        hub.subscribe(&id1, vec!["metrics.mood".to_string()])
            .await
            .unwrap();

        // Broadcast event
        let event = WsEvent::data_point("mood", 8.0, 1699000000000, HashMap::new());
        hub.broadcast(&event).await;

        // id1 should receive
        let msg = rx1.try_recv();
        assert!(msg.is_ok());

        // id2 should not receive
        let msg = rx2.try_recv();
        assert!(msg.is_err());

        hub.unregister(&id1).await;
        hub.unregister(&id2).await;
    }

    #[tokio::test]
    async fn test_wildcard_subscription() {
        let hub = ConnectionHub::new(HubConfig::default());

        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = hub.register(tx).await.unwrap();

        // Subscribe to wildcard
        hub.subscribe(&id, vec!["metrics.*".to_string()])
            .await
            .unwrap();

        // Broadcast specific metric event
        let event = WsEvent::data_point("energy", 6.5, 1699000000000, HashMap::new());
        hub.broadcast(&event).await;

        // Should receive via wildcard
        let msg = rx.try_recv();
        assert!(msg.is_ok());

        hub.unregister(&id).await;
    }
}
