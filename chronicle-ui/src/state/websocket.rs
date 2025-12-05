//! WebSocket Client
//!
//! Real-time connection to Chronicle API for live updates.

use leptos::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use super::global::GlobalState;

/// WebSocket message types from server
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    Connected {
        connection_id: String,
    },
    DataPoint {
        metric: String,
        value: f64,
        timestamp: i64,
        #[serde(default)]
        tags: std::collections::HashMap<String, String>,
    },
    Subscribed {
        topics: Vec<String>,
    },
    Unsubscribed {
        topics: Vec<String>,
    },
    Pong,
    Error {
        message: String,
    },
}

/// WebSocket client message types
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
    Ping,
}

/// WebSocket client for real-time updates
pub struct WebSocketClient {
    ws: Rc<RefCell<Option<WebSocket>>>,
    url: String,
    reconnect_attempts: Rc<RefCell<u32>>,
    max_reconnect_attempts: u32,
}

impl WebSocketClient {
    /// Create a new WebSocket client
    pub fn new(url: &str) -> Self {
        Self {
            ws: Rc::new(RefCell::new(None)),
            url: url.to_string(),
            reconnect_attempts: Rc::new(RefCell::new(0)),
            max_reconnect_attempts: 5,
        }
    }

    /// Connect to the WebSocket server
    pub fn connect(&self, state: GlobalState) {
        let ws_result = WebSocket::new(&self.url);

        match ws_result {
            Ok(ws) => {
                self.setup_handlers(&ws, state);
                *self.ws.borrow_mut() = Some(ws);
            }
            Err(e) => {
                web_sys::console::error_1(&format!("WebSocket connection failed: {:?}", e).into());
                self.schedule_reconnect(state);
            }
        }
    }

    /// Set up WebSocket event handlers
    fn setup_handlers(&self, ws: &WebSocket, state: GlobalState) {
        let reconnect_attempts = Rc::clone(&self.reconnect_attempts);
        let ws_ref = Rc::clone(&self.ws);
        let url = self.url.clone();

        // On open
        let state_clone = state.clone();
        let reconnect_clone = Rc::clone(&reconnect_attempts);
        let on_open = Closure::wrap(Box::new(move |_: JsValue| {
            web_sys::console::log_1(&"WebSocket connected".into());
            state_clone.ws_connected.set(true);
            *reconnect_clone.borrow_mut() = 0;

            // Update last sync time
            state_clone.last_sync.set(Some(chrono::Utc::now().timestamp_millis()));
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();

        // On message
        let state_clone = state.clone();
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                let text_str: String = text.into();
                handle_message(&text_str, &state_clone);
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        // On close
        let state_clone = state.clone();
        let ws_clone = Rc::clone(&ws_ref);
        let url_clone = url.clone();
        let reconnect_clone = Rc::clone(&reconnect_attempts);
        let on_close = Closure::wrap(Box::new(move |event: CloseEvent| {
            web_sys::console::log_1(&format!("WebSocket closed: code={}, reason={}", event.code(), event.reason()).into());
            state_clone.ws_connected.set(false);

            // Schedule reconnect
            let attempts = *reconnect_clone.borrow();
            if attempts < 5 {
                let delay = (2_u32.pow(attempts) * 1000).min(30000);
                *reconnect_clone.borrow_mut() = attempts + 1;

                let state_inner = state_clone.clone();
                let url_inner = url_clone.clone();
                let ws_inner = Rc::clone(&ws_clone);
                let reconnect_inner = Rc::clone(&reconnect_clone);

                gloo_timers::callback::Timeout::new(delay, move || {
                    web_sys::console::log_1(&format!("Attempting reconnect (attempt {})", reconnect_inner.borrow()).into());
                    let client = WebSocketClient {
                        ws: ws_inner,
                        url: url_inner,
                        reconnect_attempts: reconnect_inner,
                        max_reconnect_attempts: 5,
                    };
                    client.connect(state_inner);
                }).forget();
            }
        }) as Box<dyn FnMut(CloseEvent)>);
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();

        // On error
        let on_error = Closure::wrap(Box::new(move |e: JsValue| {
            web_sys::console::error_1(&format!("WebSocket error: {:?}", e).into());
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    }

    /// Schedule a reconnect attempt
    fn schedule_reconnect(&self, state: GlobalState) {
        let attempts = *self.reconnect_attempts.borrow();
        if attempts >= self.max_reconnect_attempts {
            web_sys::console::error_1(&"Max reconnect attempts reached".into());
            return;
        }

        let delay = (2_u32.pow(attempts) * 1000).min(30000);
        *self.reconnect_attempts.borrow_mut() = attempts + 1;

        let ws_ref = Rc::clone(&self.ws);
        let url = self.url.clone();
        let reconnect_attempts = Rc::clone(&self.reconnect_attempts);
        let max_attempts = self.max_reconnect_attempts;

        gloo_timers::callback::Timeout::new(delay, move || {
            let client = WebSocketClient {
                ws: ws_ref,
                url,
                reconnect_attempts,
                max_reconnect_attempts: max_attempts,
            };
            client.connect(state);
        }).forget();
    }

    /// Send a message to the server
    pub fn send(&self, message: &ClientMessage) -> Result<(), String> {
        let ws_guard = self.ws.borrow();
        let ws = ws_guard.as_ref().ok_or("WebSocket not connected")?;

        let json = serde_json::to_string(message).map_err(|e| e.to_string())?;
        ws.send_with_str(&json).map_err(|e| format!("{:?}", e))
    }

    /// Subscribe to topics
    pub fn subscribe(&self, topics: Vec<String>) -> Result<(), String> {
        self.send(&ClientMessage::Subscribe { topics })
    }

    /// Unsubscribe from topics
    pub fn unsubscribe(&self, topics: Vec<String>) -> Result<(), String> {
        self.send(&ClientMessage::Unsubscribe { topics })
    }

    /// Send a ping
    pub fn ping(&self) -> Result<(), String> {
        self.send(&ClientMessage::Ping)
    }

    /// Close the connection
    pub fn close(&self) {
        if let Some(ws) = self.ws.borrow().as_ref() {
            let _ = ws.close();
        }
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.ws.borrow()
            .as_ref()
            .map(|ws| ws.ready_state() == WebSocket::OPEN)
            .unwrap_or(false)
    }
}

/// Handle incoming WebSocket message
fn handle_message(text: &str, state: &GlobalState) {
    match serde_json::from_str::<WsMessage>(text) {
        Ok(msg) => {
            match msg {
                WsMessage::Connected { connection_id } => {
                    web_sys::console::log_1(&format!("Connected with ID: {}", connection_id).into());
                }
                WsMessage::DataPoint { metric, value, timestamp, tags } => {
                    // Add new data point to chart data
                    let point = super::global::DataPoint {
                        timestamp,
                        value,
                        tags,
                    };
                    state.add_data_point(&metric, point);
                    state.last_sync.set(Some(chrono::Utc::now().timestamp_millis()));

                    web_sys::console::log_1(&format!("Received data point: {}={}", metric, value).into());
                }
                WsMessage::Subscribed { topics } => {
                    web_sys::console::log_1(&format!("Subscribed to: {:?}", topics).into());
                }
                WsMessage::Unsubscribed { topics } => {
                    web_sys::console::log_1(&format!("Unsubscribed from: {:?}", topics).into());
                }
                WsMessage::Pong => {
                    // Connection alive
                }
                WsMessage::Error { message } => {
                    web_sys::console::error_1(&format!("Server error: {}", message).into());
                    state.show_error(&message);
                }
            }
        }
        Err(e) => {
            web_sys::console::error_1(&format!("Failed to parse WebSocket message: {}", e).into());
        }
    }
}

/// Initialize WebSocket connection (call from app root)
pub fn init_websocket(state: GlobalState, api_base: &str) {
    // Convert HTTP URL to WebSocket URL
    // api_base already contains /api/v1, so just append /ws
    let ws_url = api_base.replace("http://", "ws://").replace("https://", "wss://");
    let ws_url = format!("{}/ws", ws_url);

    let client = WebSocketClient::new(&ws_url);
    client.connect(state.clone());

    // Subscribe to selected metrics - use get_untracked to avoid reactive warning
    let selected = state.selected_metrics.get_untracked();
    let topics: Vec<String> = selected.iter()
        .map(|m| format!("metrics.{}", m))
        .collect();

    // Small delay to ensure connection is established
    let client_clone = WebSocketClient::new(&ws_url);
    gloo_timers::callback::Timeout::new(500, move || {
        if client_clone.is_connected() {
            let _ = client_clone.subscribe(topics);
        }
    }).forget();
}
