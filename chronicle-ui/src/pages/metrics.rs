//! Metrics Page
//!
//! Manage and view all tracked metrics.

use leptos::*;

use crate::api;
use crate::state::global::{GlobalState, Metric};

/// Metrics management page
#[component]
pub fn Metrics() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");
    let (show_create, set_show_create) = create_signal(false);

    // Extract the signals we need
    let metrics_signal = state.metrics;

    // Fetch metrics on mount
    let state_for_effect = state.clone();
    create_effect(move |_| {
        let state = state_for_effect.clone();
        spawn_local(async move {
            match api::fetch_metrics().await {
                Ok(metrics) => {
                    state.metrics.set(metrics);
                }
                Err(e) => {
                    state.show_error(&e);
                }
            }
        });
    });

    view! {
        <div class="space-y-8">
            // Header
            <div class="flex items-center justify-between">
                <div>
                    <h1 class="text-3xl font-bold">"Metrics"</h1>
                    <p class="text-gray-400 mt-1">"Manage your tracked metrics"</p>
                </div>

                <button
                    on:click=move |_| set_show_create.set(true)
                    class="px-4 py-2 bg-primary-600 hover:bg-primary-700 rounded-lg font-medium transition-colors"
                >
                    "+ New Metric"
                </button>
            </div>

            // Create metric modal
            {move || {
                if show_create.get() {
                    view! {
                        <CreateMetricModal on_close=move || set_show_create.set(false) />
                    }.into_view()
                } else {
                    view! {}.into_view()
                }
            }}

            // Metrics list
            <div class="grid md:grid-cols-2 lg:grid-cols-3 gap-4">
                {move || {
                    let metrics = metrics_signal.get();
                    if metrics.is_empty() {
                        view! {
                            <div class="col-span-full text-center py-12">
                                <p class="text-gray-400">"No metrics yet. Create your first one!"</p>
                            </div>
                        }.into_view()
                    } else {
                        metrics.into_iter().map(|metric| {
                            view! { <MetricListItem metric=metric /> }
                        }).collect_view()
                    }
                }}
            </div>

            // Default metrics info
            <section class="bg-gray-800 rounded-xl p-6">
                <h2 class="text-xl font-semibold mb-4">"Default Metrics"</h2>
                <p class="text-gray-400 mb-4">
                    "These metrics are available by default. You can log data for them anytime."
                </p>

                <div class="grid md:grid-cols-2 lg:grid-cols-4 gap-4">
                    <DefaultMetricCard name="Mood" unit="1-10" description="Daily mood rating" />
                    <DefaultMetricCard name="Energy" unit="1-10" description="Energy level" />
                    <DefaultMetricCard name="Sleep" unit="hours" description="Hours of sleep" />
                    <DefaultMetricCard name="Steps" unit="count" description="Daily step count" />
                </div>
            </section>
        </div>
    }
}

/// Single metric list item
#[component]
fn MetricListItem(metric: Metric) -> impl IntoView {
    let category_color = match metric.category.as_str() {
        "mood" => "bg-yellow-500",
        "health" => "bg-green-500",
        "activity" => "bg-blue-500",
        "sleep" => "bg-purple-500",
        "custom" => "bg-gray-500",
        _ => "bg-gray-500",
    };

    view! {
        <div class="bg-gray-800 rounded-xl p-4 border border-gray-700 hover:border-gray-600 transition-colors">
            <div class="flex items-start justify-between">
                <div>
                    <div class="flex items-center space-x-2">
                        <h3 class="font-semibold capitalize">{&metric.name}</h3>
                        <span class=format!("{} text-xs px-2 py-0.5 rounded-full text-white capitalize", category_color)>
                            {&metric.category}
                        </span>
                    </div>
                    <p class="text-gray-400 text-sm mt-1">
                        {metric.description.clone().unwrap_or_else(|| format!("Unit: {}", metric.unit))}
                    </p>
                </div>

                <span class="text-gray-500 text-sm">
                    "ID: "{metric.id}
                </span>
            </div>

            <div class="flex items-center space-x-4 mt-4 text-sm text-gray-400">
                <span>"Unit: "{&metric.unit}</span>
                {metric.min_value.map(|min| view! {
                    <span>"Min: "{min}</span>
                })}
                {metric.max_value.map(|max| view! {
                    <span>"Max: "{max}</span>
                })}
            </div>
        </div>
    }
}

/// Default metric card
#[component]
fn DefaultMetricCard(
    name: &'static str,
    unit: &'static str,
    description: &'static str,
) -> impl IntoView {
    view! {
        <div class="bg-gray-700 rounded-lg p-4">
            <h4 class="font-medium">{name}</h4>
            <p class="text-gray-400 text-sm mt-1">{description}</p>
            <p class="text-gray-500 text-xs mt-2">"Unit: "{unit}</p>
        </div>
    }
}

/// Create metric modal
#[component]
fn CreateMetricModal(on_close: impl Fn() + 'static + Clone) -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let (name, set_name) = create_signal(String::new());
    let (unit, set_unit) = create_signal(String::new());
    let (category, set_category) = create_signal("custom".to_string());
    let (aggregation, set_aggregation) = create_signal("average".to_string());
    let (submitting, set_submitting) = create_signal(false);

    // Clone on_close for each place it's used
    let on_close_for_submit = on_close.clone();
    let on_close_for_x = on_close.clone();
    let on_close_for_cancel = on_close;

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        let n = name.get();
        let u = unit.get();
        let c = category.get();
        let a = aggregation.get();

        if n.is_empty() || u.is_empty() {
            state.show_error("Name and unit are required");
            return;
        }

        set_submitting.set(true);

        let state_clone = state.clone();
        let on_close_inner = on_close_for_submit.clone();
        spawn_local(async move {
            match api::create_metric(&n, &u, &c, &a).await {
                Ok(metric) => {
                    state_clone.metrics.update(|m| m.push(metric));
                    state_clone.show_success("Metric created successfully");
                    on_close_inner();
                }
                Err(e) => {
                    state_clone.show_error(&e);
                }
            }
            set_submitting.set(false);
        });
    };

    view! {
        <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
            <div class="bg-gray-800 rounded-xl p-6 w-full max-w-md mx-4">
                <div class="flex items-center justify-between mb-6">
                    <h2 class="text-xl font-semibold">"Create Metric"</h2>
                    <button
                        on:click=move |_| on_close_for_x()
                        class="text-gray-400 hover:text-white"
                    >
                        "✕"
                    </button>
                </div>

                <form on:submit=on_submit class="space-y-4">
                    // Name
                    <div>
                        <label class="block text-sm text-gray-400 mb-2">"Name"</label>
                        <input
                            type="text"
                            placeholder="e.g., caffeine"
                            prop:value=move || name.get()
                            on:input=move |ev| set_name.set(event_target_value(&ev))
                            class="w-full bg-gray-700 rounded-lg px-4 py-3
                                   border border-gray-600 focus:border-primary-500 focus:outline-none"
                        />
                    </div>

                    // Unit
                    <div>
                        <label class="block text-sm text-gray-400 mb-2">"Unit"</label>
                        <input
                            type="text"
                            placeholder="e.g., mg, cups, 1-10"
                            prop:value=move || unit.get()
                            on:input=move |ev| set_unit.set(event_target_value(&ev))
                            class="w-full bg-gray-700 rounded-lg px-4 py-3
                                   border border-gray-600 focus:border-primary-500 focus:outline-none"
                        />
                    </div>

                    // Category
                    <div>
                        <label class="block text-sm text-gray-400 mb-2">"Category"</label>
                        <select
                            on:change=move |ev| set_category.set(event_target_value(&ev))
                            prop:value=move || category.get()
                            class="w-full bg-gray-700 rounded-lg px-4 py-3
                                   border border-gray-600 focus:border-primary-500 focus:outline-none"
                        >
                            <option value="custom">"Custom"</option>
                            <option value="mood">"Mood"</option>
                            <option value="health">"Health"</option>
                            <option value="activity">"Activity"</option>
                            <option value="sleep">"Sleep"</option>
                        </select>
                    </div>

                    // Aggregation
                    <div>
                        <label class="block text-sm text-gray-400 mb-2">"Aggregation"</label>
                        <select
                            on:change=move |ev| set_aggregation.set(event_target_value(&ev))
                            prop:value=move || aggregation.get()
                            class="w-full bg-gray-700 rounded-lg px-4 py-3
                                   border border-gray-600 focus:border-primary-500 focus:outline-none"
                        >
                            <option value="average">"Average"</option>
                            <option value="sum">"Sum"</option>
                            <option value="min">"Minimum"</option>
                            <option value="max">"Maximum"</option>
                            <option value="last">"Last Value"</option>
                        </select>
                    </div>

                    // Buttons
                    <div class="flex space-x-3 pt-4">
                        <button
                            type="button"
                            on:click=move |_| on_close_for_cancel()
                            class="flex-1 px-4 py-3 bg-gray-700 hover:bg-gray-600 rounded-lg font-medium transition-colors"
                        >
                            "Cancel"
                        </button>
                        <button
                            type="submit"
                            disabled=move || submitting.get()
                            class="flex-1 px-4 py-3 bg-primary-600 hover:bg-primary-700 disabled:bg-gray-600
                                   rounded-lg font-medium transition-colors"
                        >
                            {move || if submitting.get() { "Creating..." } else { "Create" }}
                        </button>
                    </div>
                </form>
            </div>
        </div>
    }
}
