//! Chronicle Dashboard
//!
//! Personal Time-Series Intelligence Dashboard built with Leptos (WASM).
//!
//! # Features
//!
//! - Real-time metrics visualization
//! - Data entry for manual tracking
//! - AI-powered insights via MemMachine
//! - WebSocket live updates
//!
//! # Architecture
//!
//! This is a client-side rendered (CSR) Leptos application that compiles to
//! WebAssembly. It communicates with the Chronicle API via HTTP and WebSocket.

use leptos::*;

mod api;
mod app;
mod components;
mod pages;
mod state;

fn main() {
    // Set up panic hook for better error messages in WASM
    console_error_panic_hook::set_once();

    // Mount the app to the document body
    mount_to_body(|| view! { <app::App /> });
}
