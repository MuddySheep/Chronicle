//! Chart Component
//!
//! Time-series chart using HTML5 Canvas.

use leptos::*;
use std::collections::HashMap;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::state::global::{DataPoint, GlobalState, TimeRange};

/// Chart colors for different series
const SERIES_COLORS: [&str; 6] = [
    "#FF9800", // Orange (primary)
    "#4CAF50", // Green
    "#2196F3", // Blue
    "#9C27B0", // Purple
    "#F44336", // Red
    "#00BCD4", // Cyan
];

/// Time-series chart component
#[component]
pub fn Chart() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");
    let canvas_ref = create_node_ref::<html::Canvas>();

    // Redraw chart when data or time range changes
    create_effect(move |_| {
        let data = state.chart_data.get();
        let selected = state.selected_metrics.get();
        let range = state.time_range.get();

        if let Some(canvas) = canvas_ref.get() {
            draw_chart(&canvas, &data, &selected, &range);
        }
    });

    view! {
        <div class="relative">
            <canvas
                node_ref=canvas_ref
                width="800"
                height="400"
                class="w-full h-64 md:h-96 rounded-lg"
            />

            // Legend
            <ChartLegend />

            // Time range selector
            <div class="flex justify-center space-x-2 mt-4">
                <TimeRangeButton label="7D" days=7 />
                <TimeRangeButton label="30D" days=30 />
                <TimeRangeButton label="90D" days=90 />
                <TimeRangeButton label="1Y" days=365 />
            </div>
        </div>
    }
}

/// Chart legend showing series colors
#[component]
fn ChartLegend() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");
    let selected = state.selected_metrics;

    view! {
        <div class="flex justify-center flex-wrap gap-4 mt-4">
            {move || {
                selected.get()
                    .into_iter()
                    .enumerate()
                    .map(|(idx, metric)| {
                        let color = SERIES_COLORS[idx % SERIES_COLORS.len()];
                        view! {
                            <div class="flex items-center space-x-2">
                                <div
                                    class="w-3 h-3 rounded-full"
                                    style=format!("background-color: {}", color)
                                />
                                <span class="text-sm text-gray-300 capitalize">{metric}</span>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        </div>
    }
}

/// Time range selection button
#[component]
fn TimeRangeButton(
    label: &'static str,
    days: i64,
) -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let state_for_memo = state.clone();
    let is_active = create_memo(move |_| {
        state_for_memo.time_range.get().duration_days() == days
    });

    let state_for_click = state;
    let on_click = move |_| {
        state_for_click.time_range.set(TimeRange::last_days(days));

        // Trigger data refetch
        let selected = state_for_click.selected_metrics.get();
        let range = state_for_click.time_range.get();

        let state_for_async = state_for_click.clone();
        spawn_local(async move {
            state_for_async.loading.set(true);
            match crate::api::fetch_chart_data(&selected, range.start, range.end).await {
                Ok(data) => {
                    state_for_async.chart_data.set(data);
                }
                Err(e) => {
                    state_for_async.show_error(&e);
                }
            }
            state_for_async.loading.set(false);
        });
    };

    view! {
        <button
            on:click=on_click
            class=move || {
                let base = "px-4 py-2 rounded-lg text-sm font-medium transition-colors";
                if is_active.get() {
                    format!("{} bg-primary-600 text-white", base)
                } else {
                    format!("{} bg-gray-700 text-gray-300 hover:bg-gray-600", base)
                }
            }
        >
            {label}
        </button>
    }
}

/// Draw the chart on canvas
fn draw_chart(
    canvas: &HtmlCanvasElement,
    data: &HashMap<String, Vec<DataPoint>>,
    selected: &[String],
    range: &TimeRange,
) {
    let ctx = match canvas.get_context("2d") {
        Ok(Some(ctx)) => match ctx.dyn_into::<CanvasRenderingContext2d>() {
            Ok(ctx) => ctx,
            Err(_) => return,
        },
        _ => return,
    };

    let width = canvas.width() as f64;
    let height = canvas.height() as f64;

    // Margins
    let margin_left = 60.0;
    let margin_right = 20.0;
    let margin_top = 20.0;
    let margin_bottom = 40.0;

    let chart_width = width - margin_left - margin_right;
    let chart_height = height - margin_top - margin_bottom;

    // Clear canvas
    ctx.set_fill_style(&"#1f2937".into()); // gray-800
    ctx.fill_rect(0.0, 0.0, width, height);

    // Find global min/max for y-axis
    let mut global_min = f64::INFINITY;
    let mut global_max = f64::NEG_INFINITY;

    for metric in selected {
        if let Some(points) = data.get(metric) {
            for point in points {
                global_min = global_min.min(point.value);
                global_max = global_max.max(point.value);
            }
        }
    }

    // Add padding to y range
    let y_range = global_max - global_min;
    let y_padding = if y_range > 0.0 { y_range * 0.1 } else { 1.0 };
    global_min -= y_padding;
    global_max += y_padding;

    if global_min == global_max {
        global_min -= 1.0;
        global_max += 1.0;
    }

    // Draw grid lines
    ctx.set_stroke_style(&"#374151".into()); // gray-700
    ctx.set_line_width(1.0);

    // Horizontal grid lines (5 lines)
    for i in 0..=5 {
        let y = margin_top + (i as f64 / 5.0) * chart_height;
        ctx.begin_path();
        ctx.move_to(margin_left, y);
        ctx.line_to(width - margin_right, y);
        ctx.stroke();

        // Y-axis labels
        let value = global_max - (i as f64 / 5.0) * (global_max - global_min);
        ctx.set_fill_style(&"#9ca3af".into()); // gray-400
        ctx.set_font("12px sans-serif");
        let _ = ctx.fill_text(&format!("{:.1}", value), 5.0, y + 4.0);
    }

    // Draw each data series
    for (idx, metric) in selected.iter().enumerate() {
        if let Some(points) = data.get(metric) {
            if points.is_empty() {
                continue;
            }

            let color = SERIES_COLORS[idx % SERIES_COLORS.len()];
            ctx.set_stroke_style(&color.into());
            ctx.set_line_width(2.0);
            ctx.begin_path();

            let time_range_ms = (range.end - range.start) as f64;

            for (i, point) in points.iter().enumerate() {
                // Scale x to chart area
                let x = margin_left + ((point.timestamp - range.start) as f64 / time_range_ms) * chart_width;

                // Scale y to chart area (inverted because canvas y grows downward)
                let y = margin_top + ((global_max - point.value) / (global_max - global_min)) * chart_height;

                if i == 0 {
                    ctx.move_to(x, y);
                } else {
                    ctx.line_to(x, y);
                }
            }

            ctx.stroke();

            // Draw points
            ctx.set_fill_style(&color.into());
            for point in points {
                let x = margin_left + ((point.timestamp - range.start) as f64 / time_range_ms) * chart_width;
                let y = margin_top + ((global_max - point.value) / (global_max - global_min)) * chart_height;

                ctx.begin_path();
                let _ = ctx.arc(x, y, 3.0, 0.0, std::f64::consts::PI * 2.0);
                ctx.fill();
            }
        }
    }

    // Draw x-axis labels
    ctx.set_fill_style(&"#9ca3af".into());
    ctx.set_font("12px sans-serif");

    let num_labels = 5;
    for i in 0..=num_labels {
        let timestamp = range.start + (i as i64 * (range.end - range.start) / num_labels as i64);
        let x = margin_left + (i as f64 / num_labels as f64) * chart_width;

        // Format date
        let date = chrono::DateTime::from_timestamp_millis(timestamp)
            .map(|dt| dt.format("%m/%d").to_string())
            .unwrap_or_default();

        let _ = ctx.fill_text(&date, x - 15.0, height - 10.0);
    }

    // Draw "No data" message if empty
    if selected.iter().all(|m| data.get(m).map(|p| p.is_empty()).unwrap_or(true)) {
        ctx.set_fill_style(&"#6b7280".into());
        ctx.set_font("16px sans-serif");
        let _ = ctx.fill_text("No data for selected range", width / 2.0 - 80.0, height / 2.0);
    }
}
