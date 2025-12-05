//! WebSocket Real-Time Streaming
//!
//! Provides real-time data streaming to dashboard clients via WebSocket.
//!
//! ## Architecture
//!
//! - **ConnectionHub**: Manages all active connections and subscriptions
//! - **Handler**: Handles WebSocket upgrade and message processing
//! - **Messages**: Defines client and server message formats
//!
//! ## Usage
//!
//! Clients connect to `/ws` and can subscribe to topics:
//! - `metrics.*` - All metric updates
//! - `metrics.{name}` - Specific metric (e.g., `metrics.mood`)
//! - `category.{cat}` - All metrics in category
//! - `insights` - Insight updates
//! - `system` - System events
//!
//! ## Example
//!
//! ```javascript
//! // Browser
//! const ws = new WebSocket('ws://localhost:8082/ws');
//!
//! ws.onopen = () => {
//!   ws.send(JSON.stringify({type: 'subscribe', topics: ['metrics.mood']}));
//! };
//!
//! ws.onmessage = (event) => {
//!   const msg = JSON.parse(event.data);
//!   console.log('Received:', msg);
//! };
//! ```

mod handler;
mod hub;
mod messages;

pub use handler::websocket_handler;
pub use hub::{ConnectionHub, HubConfig, HubError};
pub use messages::{ClientMessage, ServerMessage, WsEvent};
