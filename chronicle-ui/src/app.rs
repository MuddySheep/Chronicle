//! App Root Component
//!
//! Main application component with routing and global providers.

use leptos::*;
use leptos_router::*;

use crate::api;
use crate::components::{Nav, Toast};
use crate::pages::{Dashboard, Insights, Metrics, Settings};
use crate::state::global::{provide_global_state, GlobalState};
use crate::state::websocket::init_websocket;

/// Root application component
#[component]
pub fn App() -> impl IntoView {
    // Provide global state to all components
    provide_global_state();

    // Initialize WebSocket connection
    let state = use_context::<GlobalState>().expect("GlobalState not found");
    init_websocket(state.clone(), &api::get_api_base());

    view! {
        <Router>
            <div class="min-h-screen bg-gray-900 text-white flex flex-col">
                // Navigation header
                <Nav />

                // Main content area
                <main class="flex-1 container mx-auto px-4 py-8 pb-24">
                    <Routes>
                        <Route path="/" view=Dashboard />
                        <Route path="/metrics" view=Metrics />
                        <Route path="/insights" view=Insights />
                        <Route path="/settings" view=Settings />
                        <Route path="/*any" view=NotFound />
                    </Routes>
                </main>

                // Footer with connection status
                <Footer />

                // Toast notifications
                <Toast />
            </div>
        </Router>
    }
}

/// Footer component showing connection status
#[component]
fn Footer() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    view! {
        <footer class="fixed bottom-0 left-0 right-0 bg-gray-800 border-t border-gray-700 py-3 px-4">
            <div class="container mx-auto flex items-center justify-between text-sm">
                // WebSocket status
                <div class="flex items-center space-x-2">
                    {move || {
                        if state.ws_connected.get() {
                            view! {
                                <span class="flex items-center space-x-1 text-green-400">
                                    <span class="w-2 h-2 bg-green-400 rounded-full pulse" />
                                    <span>"Connected"</span>
                                </span>
                            }.into_view()
                        } else {
                            view! {
                                <span class="flex items-center space-x-1 text-red-400">
                                    <span class="w-2 h-2 bg-red-400 rounded-full" />
                                    <span>"Disconnected"</span>
                                </span>
                            }.into_view()
                        }
                    }}
                </div>

                // Last sync time
                <div class="text-gray-400">
                    {move || {
                        state.last_sync.get()
                            .and_then(|ts| chrono::DateTime::from_timestamp_millis(ts))
                            .map(|dt| format!("Last sync: {}", dt.format("%H:%M:%S")))
                            .unwrap_or_else(|| "Not synced".to_string())
                    }}
                </div>

                // Loading indicator
                {move || {
                    if state.loading.get() {
                        view! {
                            <div class="flex items-center space-x-2 text-primary-400">
                                <div class="loading-spinner w-4 h-4" />
                                <span>"Loading..."</span>
                            </div>
                        }.into_view()
                    } else {
                        view! {}.into_view()
                    }
                }}
            </div>
        </footer>
    }
}

/// 404 Not Found page
#[component]
fn NotFound() -> impl IntoView {
    view! {
        <div class="flex flex-col items-center justify-center min-h-[60vh] text-center">
            <div class="text-6xl mb-4">"🔍"</div>
            <h1 class="text-3xl font-bold mb-2">"Page Not Found"</h1>
            <p class="text-gray-400 mb-6">"The page you're looking for doesn't exist."</p>
            <A
                href="/"
                class="px-6 py-3 bg-primary-600 hover:bg-primary-700 rounded-lg font-medium transition-colors"
            >
                "Go to Dashboard"
            </A>
        </div>
    }
}
