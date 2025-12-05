//! State Management
//!
//! Global application state and WebSocket connection management.

pub mod global;
pub mod websocket;

pub use global::{provide_global_state, GlobalState, DataPoint, Metric, TimeRange};
pub use websocket::{WebSocketClient, WsMessage};
