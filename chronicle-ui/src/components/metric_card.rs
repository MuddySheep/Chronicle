//! Metric Card Component
//!
//! Displays a single metric with current value and trend.

use leptos::*;

use crate::state::global::GlobalState;

/// Metric card component
#[component]
pub fn MetricCard(
    /// Metric name to display
    #[prop(into)]
    name: String,
    /// Optional custom unit label
    #[prop(optional)]
    unit: Option<String>,
) -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");
    let metric_name = name.clone();
    let metric_name_display = name.clone();
    let metric_name_trend = name.clone();

    // Get current value
    let current_value = create_memo(move |_| {
        state.chart_data.get()
            .get(&metric_name)
            .and_then(|points| points.last())
            .map(|p| p.value)
    });

    // Calculate trend vs average
    let trend = create_memo(move |_| {
        let data = state.chart_data.get();
        let points = data.get(&metric_name_trend)?;
        if points.len() < 2 {
            return None;
        }

        let avg: f64 = points.iter().map(|p| p.value).sum::<f64>() / points.len() as f64;
        let current = points.last()?.value;
        let diff = current - avg;

        Some((diff, diff / avg * 100.0))
    });

    view! {
        <div class="bg-gray-800 rounded-lg p-4 hover:bg-gray-750 transition cursor-pointer border border-gray-700 hover:border-gray-600">
            // Header with metric name
            <div class="flex items-center justify-between">
                <span class="text-gray-400 text-sm capitalize">{metric_name_display}</span>
                {move || unit.clone().map(|u| view! {
                    <span class="text-gray-500 text-xs">{u}</span>
                })}
            </div>

            // Current value
            <div class="text-3xl font-bold mt-2">
                {move || {
                    current_value.get()
                        .map(|v| format!("{:.1}", v))
                        .unwrap_or_else(|| "—".to_string())
                }}
            </div>

            // Trend indicator
            <div class="mt-2">
                {move || {
                    match trend.get() {
                        Some((diff, _percent)) => {
                            let (arrow, color) = if diff > 0.1 {
                                ("↑", "text-green-400")
                            } else if diff < -0.1 {
                                ("↓", "text-red-400")
                            } else {
                                ("→", "text-gray-400")
                            };

                            view! {
                                <span class=format!("text-sm {}", color)>
                                    {arrow}
                                    " "
                                    {format!("{:+.1}", diff)}
                                    " vs avg"
                                </span>
                            }.into_view()
                        }
                        None => view! {
                            <span class="text-sm text-gray-500">"No trend data"</span>
                        }.into_view()
                    }
                }}
            </div>

            // Mini sparkline (simplified bar chart)
            <MiniSparkline name=name.clone() />
        </div>
    }
}

/// Mini sparkline showing recent trend
#[component]
fn MiniSparkline(
    #[prop(into)]
    name: String,
) -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    view! {
        <div class="flex items-end space-x-1 h-8 mt-3">
            {move || {
                let data = state.chart_data.get();
                let points = data.get(&name).cloned().unwrap_or_default();

                // Take last 7 points
                let recent: Vec<_> = points.iter().rev().take(7).rev().collect();

                if recent.is_empty() {
                    return view! {
                        <div class="flex-1 bg-gray-700 rounded h-2"></div>
                    }.into_view();
                }

                let min = recent.iter().map(|p| p.value).fold(f64::INFINITY, f64::min);
                let max = recent.iter().map(|p| p.value).fold(f64::NEG_INFINITY, f64::max);
                let range = if (max - min).abs() < 0.01 { 1.0 } else { max - min };

                recent.iter().map(|point| {
                    let height_percent = ((point.value - min) / range * 80.0 + 20.0) as i32;
                    view! {
                        <div
                            class="flex-1 bg-primary-500 rounded-t opacity-70"
                            style=format!("height: {}%", height_percent)
                        />
                    }
                }).collect_view()
            }}
        </div>
    }
}

/// Compact metric card for summaries
#[component]
pub fn CompactMetricCard(
    #[prop(into)]
    name: String,
) -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");
    let metric_name = name.clone();

    let current_value = create_memo(move |_| {
        state.chart_data.get()
            .get(&metric_name)
            .and_then(|points| points.last())
            .map(|p| p.value)
    });

    view! {
        <div class="bg-gray-700 rounded px-3 py-2 inline-flex items-center space-x-2">
            <span class="text-gray-400 text-xs capitalize">{name}</span>
            <span class="font-semibold">
                {move || current_value.get().map(|v| format!("{:.1}", v)).unwrap_or("—".to_string())}
            </span>
        </div>
    }
}
