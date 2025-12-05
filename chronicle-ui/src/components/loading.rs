//! Loading Component
//!
//! Loading spinners and skeleton states.

use leptos::*;

/// Full-page loading spinner
#[component]
pub fn Loading() -> impl IntoView {
    view! {
        <div class="flex items-center justify-center py-12">
            <div class="loading-spinner w-8 h-8" />
        </div>
    }
}

/// Inline loading spinner
#[component]
pub fn InlineLoading() -> impl IntoView {
    view! {
        <span class="inline-block loading-spinner w-4 h-4" />
    }
}

/// Skeleton loader for cards
#[component]
pub fn CardSkeleton() -> impl IntoView {
    view! {
        <div class="bg-gray-800 rounded-lg p-4 animate-pulse">
            <div class="h-4 bg-gray-700 rounded w-1/3 mb-4" />
            <div class="h-8 bg-gray-700 rounded w-1/2 mb-2" />
            <div class="h-4 bg-gray-700 rounded w-2/3" />
        </div>
    }
}

/// Skeleton loader for chart
#[component]
pub fn ChartSkeleton() -> impl IntoView {
    view! {
        <div class="bg-gray-800 rounded-lg p-6 animate-pulse">
            <div class="h-6 bg-gray-700 rounded w-1/4 mb-4" />
            <div class="h-64 bg-gray-700 rounded" />
        </div>
    }
}

/// Skeleton loader for list items
#[component]
pub fn ListSkeleton(
    #[prop(default = 3)]
    count: usize,
) -> impl IntoView {
    view! {
        <div class="space-y-3 animate-pulse">
            {(0..count).map(|_| view! {
                <div class="bg-gray-700 rounded h-12" />
            }).collect_view()}
        </div>
    }
}

/// Loading overlay for forms
#[component]
pub fn LoadingOverlay(
    #[prop(into)]
    loading: Signal<bool>,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="relative">
            {children()}

            {move || {
                if loading.get() {
                    view! {
                        <div class="absolute inset-0 bg-gray-900/50 flex items-center justify-center rounded-lg">
                            <div class="loading-spinner w-8 h-8" />
                        </div>
                    }.into_view()
                } else {
                    view! {}.into_view()
                }
            }}
        </div>
    }
}
