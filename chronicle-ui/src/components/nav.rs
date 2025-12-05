//! Navigation Component
//!
//! Header navigation bar with logo and links.

use leptos::*;
use leptos_router::*;

/// Navigation header component
#[component]
pub fn Nav() -> impl IntoView {
    view! {
        <nav class="bg-gray-800 border-b border-gray-700">
            <div class="container mx-auto px-4">
                <div class="flex items-center justify-between h-16">
                    // Logo and brand
                    <A href="/" class="flex items-center space-x-3">
                        <span class="text-2xl">"📊"</span>
                        <span class="text-xl font-bold text-white">"Chronicle"</span>
                    </A>

                    // Navigation links
                    <div class="flex items-center space-x-1">
                        <NavLink href="/" label="Dashboard" />
                        <NavLink href="/metrics" label="Metrics" />
                        <NavLink href="/insights" label="Insights" />
                        <NavLink href="/settings" label="Settings" />
                    </div>
                </div>
            </div>
        </nav>
    }
}

/// Individual navigation link
#[component]
fn NavLink(
    href: &'static str,
    label: &'static str,
) -> impl IntoView {
    view! {
        <A
            href=href
            class="px-4 py-2 rounded-lg text-gray-300 hover:text-white hover:bg-gray-700 transition-colors"
            active_class="bg-gray-700 text-white"
        >
            {label}
        </A>
    }
}
