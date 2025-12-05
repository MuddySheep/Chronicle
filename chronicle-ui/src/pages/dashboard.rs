//! Dashboard Page
//!
//! Main dashboard view showing metrics overview, charts, and quick entry.

use leptos::*;

use crate::api;
use crate::components::{Chart, DataEntry, InsightCard, MetricCard};
use crate::state::global::GlobalState;

/// Dashboard page component
#[component]
pub fn Dashboard() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    // Fetch initial data on mount
    let state_for_effect = state.clone();
    create_effect(move |_| {
        let state = state_for_effect.clone();
        spawn_local(async move {
            state.loading.set(true);

            // Fetch metrics list
            match api::fetch_metrics().await {
                Ok(metrics) => {
                    state.metrics.set(metrics);
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Failed to fetch metrics: {}", e).into());
                }
            }

            // Fetch chart data
            let range = state.time_range.get();
            let selected = state.selected_metrics.get();

            match api::fetch_chart_data(&selected, range.start, range.end).await {
                Ok(data) => {
                    state.chart_data.set(data);
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Failed to fetch chart data: {}", e).into());
                }
            }

            state.loading.set(false);
        });
    });

    view! {
        <div class="space-y-8">
            // Page header
            <div class="flex items-center justify-between">
                <div>
                    <h1 class="text-3xl font-bold">"Dashboard"</h1>
                    <p class="text-gray-400 mt-1">"Your personal metrics at a glance"</p>
                </div>

                // Time range display
                <div class="text-sm text-gray-400">
                    {move || state.time_range.get().label}
                </div>
            </div>

            // Summary row - key metrics from Apple Health
            <section>
                <h2 class="text-lg font-semibold mb-4">"Health Snapshot"</h2>
                <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
                    <MetricCard name="heart_rate".to_string() unit="bpm".to_string() />
                    <MetricCard name="steps".to_string() />
                    <MetricCard name="calories_active".to_string() unit="cal".to_string() />
                    <MetricCard name="resting_heart_rate".to_string() unit="bpm".to_string() />
                </div>
            </section>

            // Main chart
            <section class="bg-gray-800 rounded-xl p-6">
                <h2 class="text-xl font-semibold mb-4">"Your Trends"</h2>

                // Loading state
                {move || {
                    if state.loading.get() {
                        view! {
                            <div class="h-64 flex items-center justify-center">
                                <div class="loading-spinner w-8 h-8" />
                            </div>
                        }.into_view()
                    } else {
                        view! { <Chart /> }.into_view()
                    }
                }}
            </section>

            // Two column layout for entry and insights
            <div class="grid md:grid-cols-2 gap-8">
                // Quick entry section
                <section class="bg-gray-800 rounded-xl p-6">
                    <h2 class="text-xl font-semibold mb-4">"Log Entry"</h2>
                    <DataEntry />
                </section>

                // Insights section
                <section class="bg-gray-800 rounded-xl p-6">
                    <h2 class="text-xl font-semibold mb-4">"Insights"</h2>
                    <InsightCard />
                </section>
            </div>

            // Recent activity (optional)
            <RecentActivity />
        </div>
    }
}

/// Recent activity component
#[component]
fn RecentActivity() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    view! {
        <section class="bg-gray-800 rounded-xl p-6">
            <h2 class="text-xl font-semibold mb-4">"Recent Activity"</h2>

            <div class="space-y-2">
                {move || {
                    let data = state.chart_data.get();
                    let mut all_points: Vec<_> = data.iter()
                        .flat_map(|(metric, points)| {
                            points.iter().map(|p| (metric.clone(), p.clone()))
                        })
                        .collect();

                    // Sort by timestamp descending
                    all_points.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));

                    // Take last 5
                    let recent: Vec<_> = all_points.into_iter().take(5).collect();

                    if recent.is_empty() {
                        view! {
                            <p class="text-gray-400 text-sm">"No recent activity"</p>
                        }.into_view()
                    } else {
                        recent.into_iter().map(|(metric, point)| {
                            let time = chrono::DateTime::from_timestamp_millis(point.timestamp)
                                .map(|dt| dt.format("%b %d, %H:%M").to_string())
                                .unwrap_or_default();

                            view! {
                                <div class="flex items-center justify-between py-2 border-b border-gray-700 last:border-0">
                                    <div class="flex items-center space-x-3">
                                        <span class="text-2xl">{metric_icon(&metric)}</span>
                                        <div>
                                            <span class="capitalize">{metric}</span>
                                            <span class="text-gray-400 text-sm ml-2">{time}</span>
                                        </div>
                                    </div>
                                    <span class="font-semibold">{format!("{:.1}", point.value)}</span>
                                </div>
                            }
                        }).collect_view()
                    }
                }}
            </div>
        </section>
    }
}

/// Get icon for metric type
fn metric_icon(metric: &str) -> &'static str {
    match metric {
        "mood" => "😊",
        "energy" => "⚡",
        "sleep_hours" | "sleep" => "😴",
        "steps" => "🚶",
        "weight" => "⚖️",
        "anxiety" => "😰",
        "focus" => "🎯",
        "productivity" => "📈",
        "exercise" | "workout" => "🏃",
        "water" | "hydration" => "💧",
        "caffeine" | "coffee" => "☕",
        "alcohol" => "🍺",
        "meditation" => "🧘",
        _ => "📊",
    }
}
