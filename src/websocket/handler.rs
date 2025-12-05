//! WebSocket Handler
//!
//! Handles WebSocket upgrade requests and manages the connection lifecycle.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::hub::ConnectionHub;
use super::messages::{ClientMessage, ServerMessage};
use crate::api::AppState;

/// WebSocket upgrade handler
///
/// This is the entry point for WebSocket connections.
/// It upgrades the HTTP connection to WebSocket and starts message handling.
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Response {
    let hub = Arc::clone(&state.ws_hub);
    ws.on_upgrade(move |socket| handle_socket(socket, hub))
}

/// Handle an established WebSocket connection
async fn handle_socket(socket: WebSocket, hub: Arc<ConnectionHub>) {
    let (mut sender, mut receiver) = socket.split();

    // Create channel for sending messages to this connection
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Register with hub
    let connection_id = match hub.register(tx).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "Failed to register WebSocket connection");
            // Send error message before closing
            let error_msg = ServerMessage::Error {
                message: e.to_string(),
            };
            let _ = sender
                .send(Message::Text(serde_json::to_string(&error_msg).unwrap()))
                .await;
            return;
        }
    };

    // Send connected message with connection ID
    let connected_msg = ServerMessage::Connected {
        connection_id: connection_id.clone(),
    };
    if sender
        .send(Message::Text(serde_json::to_string(&connected_msg).unwrap()))
        .await
        .is_err()
    {
        tracing::error!(connection_id = %connection_id, "Failed to send connected message");
        hub.unregister(&connection_id).await;
        return;
    }

    let _hub_for_send = Arc::clone(&hub);
    let conn_id_for_send = connection_id.clone();

    // Task to forward messages from channel to WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(text) => {
                    if sender.send(Message::Text(text)).await.is_err() {
                        tracing::debug!(
                            connection_id = %conn_id_for_send,
                            "WebSocket send failed, closing connection"
                        );
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to serialize message");
                }
            }
        }
    });

    let hub_for_recv = Arc::clone(&hub);
    let conn_id_for_recv = connection_id.clone();

    // Task to receive messages from WebSocket and handle them
    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = receiver.next().await {
            match result {
                Ok(msg) => {
                    if !handle_ws_message(&hub_for_recv, &conn_id_for_recv, msg).await {
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        connection_id = %conn_id_for_recv,
                        error = %e,
                        "WebSocket receive error"
                    );
                    break;
                }
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }

    // Cleanup: unregister from hub
    hub.unregister(&connection_id).await;
}

/// Handle a received WebSocket message
///
/// Returns false if the connection should be closed.
async fn handle_ws_message(
    hub: &Arc<ConnectionHub>,
    connection_id: &str,
    message: Message,
) -> bool {
    match message {
        Message::Text(text) => {
            match serde_json::from_str::<ClientMessage>(&text) {
                Ok(client_msg) => {
                    handle_client_message(hub, connection_id, client_msg).await;
                }
                Err(e) => {
                    tracing::debug!(
                        connection_id = %connection_id,
                        error = %e,
                        text = %text,
                        "Invalid client message"
                    );
                    // Send error but keep connection open
                    let error_msg = ServerMessage::Error {
                        message: format!("Invalid message format: {}", e),
                    };
                    let _ = hub.send_to(connection_id, error_msg).await;
                }
            }
            true
        }
        Message::Binary(_) => {
            // We don't support binary messages
            let error_msg = ServerMessage::Error {
                message: "Binary messages not supported".to_string(),
            };
            let _ = hub.send_to(connection_id, error_msg).await;
            true
        }
        Message::Ping(_) => {
            // Axum handles ping/pong automatically
            true
        }
        Message::Pong(_) => {
            // Received pong, connection is alive
            true
        }
        Message::Close(_) => {
            tracing::debug!(connection_id = %connection_id, "Client requested close");
            false
        }
    }
}

/// Handle a parsed client message
async fn handle_client_message(
    hub: &Arc<ConnectionHub>,
    connection_id: &str,
    message: ClientMessage,
) {
    match message {
        ClientMessage::Subscribe { topics } => {
            match hub.subscribe(connection_id, topics).await {
                Ok(subscribed) => {
                    let response = ServerMessage::Subscribed { topics: subscribed };
                    let _ = hub.send_to(connection_id, response).await;
                }
                Err(e) => {
                    tracing::error!(
                        connection_id = %connection_id,
                        error = %e,
                        "Subscribe error"
                    );
                    let error_msg = ServerMessage::Error {
                        message: e.to_string(),
                    };
                    let _ = hub.send_to(connection_id, error_msg).await;
                }
            }
        }
        ClientMessage::Unsubscribe { topics } => {
            match hub.unsubscribe(connection_id, topics).await {
                Ok(unsubscribed) => {
                    let response = ServerMessage::Unsubscribed {
                        topics: unsubscribed,
                    };
                    let _ = hub.send_to(connection_id, response).await;
                }
                Err(e) => {
                    tracing::error!(
                        connection_id = %connection_id,
                        error = %e,
                        "Unsubscribe error"
                    );
                    let error_msg = ServerMessage::Error {
                        message: e.to_string(),
                    };
                    let _ = hub.send_to(connection_id, error_msg).await;
                }
            }
        }
        ClientMessage::Ping => {
            let response = ServerMessage::Pong;
            let _ = hub.send_to(connection_id, response).await;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_handler_module_compiles() {
        // Integration tests would be in separate test file
    }
}
