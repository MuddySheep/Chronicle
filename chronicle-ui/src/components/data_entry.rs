//! Data Entry Component
//!
//! Form for logging new data points.

use leptos::*;
use std::collections::HashMap;

use crate::api;
use crate::state::global::GlobalState;

/// Data entry form component
#[component]
pub fn DataEntry() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let (metric, set_metric) = create_signal("mood".to_string());
    let (value, set_value) = create_signal(5.0);
    let (submitting, set_submitting) = create_signal(false);
    let (mode, set_mode) = create_signal(EntryMode::Quick);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        let m = metric.get();
        let v = value.get();

        set_submitting.set(true);

        let state_clone = state.clone();
        spawn_local(async move {
            match api::submit_data_point(&m, v, None).await {
                Ok(_response) => {
                    state_clone.show_success(&format!("Logged {} = {:.1}", m, v));

                    // Add point to chart data for immediate feedback
                    let point = crate::state::global::DataPoint {
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        value: v,
                        tags: HashMap::new(),
                    };
                    state_clone.add_data_point(&m, point);

                    // Reset value to midpoint
                    set_value.set(5.0);
                }
                Err(e) => {
                    state_clone.show_error(&e);
                }
            }
            set_submitting.set(false);
        });
    };

    view! {
        <div class="space-y-4">
            // Mode toggle
            <div class="flex space-x-2">
                <ModeButton
                    label="Quick"
                    current=mode
                    target=EntryMode::Quick
                    on_click=move |_| set_mode.set(EntryMode::Quick)
                />
                <ModeButton
                    label="Full"
                    current=mode
                    target=EntryMode::Full
                    on_click=move |_| set_mode.set(EntryMode::Full)
                />
            </div>

            <form on:submit=on_submit class="space-y-4">
                // Metric selector
                <MetricSelector metric=metric set_metric=set_metric />

                // Value input
                <ValueInput value=value set_value=set_value metric=metric />

                // Tags (full mode only)
                {move || {
                    if mode.get() == EntryMode::Full {
                        view! { <TagsInput /> }.into_view()
                    } else {
                        view! {}.into_view()
                    }
                }}

                // Submit button
                <button
                    type="submit"
                    disabled=move || submitting.get()
                    class="w-full bg-primary-600 hover:bg-primary-700 disabled:bg-gray-600
                           disabled:cursor-not-allowed rounded-lg py-3 font-semibold
                           transition-colors flex items-center justify-center space-x-2"
                >
                    {move || if submitting.get() {
                        view! {
                            <div class="loading-spinner w-5 h-5" />
                            <span>"Saving..."</span>
                        }.into_view()
                    } else {
                        view! {
                            <span>"Log Entry"</span>
                        }.into_view()
                    }}
                </button>
            </form>
        </div>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EntryMode {
    Quick,
    Full,
}

#[component]
fn ModeButton(
    label: &'static str,
    current: ReadSignal<EntryMode>,
    target: EntryMode,
    on_click: impl Fn(web_sys::MouseEvent) + 'static,
) -> impl IntoView {
    view! {
        <button
            type="button"
            on:click=on_click
            class=move || {
                let base = "px-4 py-2 rounded-lg text-sm font-medium transition-colors";
                if current.get() == target {
                    format!("{} bg-gray-600 text-white", base)
                } else {
                    format!("{} bg-gray-700 text-gray-400 hover:text-white", base)
                }
            }
        >
            {label}
        </button>
    }
}

#[component]
fn MetricSelector(
    metric: ReadSignal<String>,
    set_metric: WriteSignal<String>,
) -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    view! {
        <div>
            <label class="block text-sm text-gray-400 mb-2">"Metric"</label>
            <select
                on:change=move |ev| set_metric.set(event_target_value(&ev))
                prop:value=move || metric.get()
                class="w-full bg-gray-700 rounded-lg px-4 py-3 text-white
                       border border-gray-600 focus:border-primary-500 focus:outline-none"
            >
                // Default options
                <option value="mood">"Mood"</option>
                <option value="energy">"Energy"</option>
                <option value="sleep_hours">"Sleep (hours)"</option>
                <option value="steps">"Steps"</option>
                <option value="weight">"Weight"</option>
                <option value="anxiety">"Anxiety"</option>
                <option value="focus">"Focus"</option>
                <option value="productivity">"Productivity"</option>

                // Dynamic options from API
                {move || {
                    state.metrics.get()
                        .into_iter()
                        .filter(|m| !["mood", "energy", "sleep_hours", "steps", "weight", "anxiety", "focus", "productivity"].contains(&m.name.as_str()))
                        .map(|m| view! {
                            <option value=m.name.clone()>{m.name}</option>
                        })
                        .collect_view()
                }}
            </select>
        </div>
    }
}

#[component]
fn ValueInput(
    value: ReadSignal<f64>,
    set_value: WriteSignal<f64>,
    metric: ReadSignal<String>,
) -> impl IntoView {
    // Determine range based on metric
    let (min, max, step) = create_memo(move |_| {
        match metric.get().as_str() {
            "sleep_hours" => (0.0, 16.0, 0.5),
            "steps" => (0.0, 30000.0, 500.0),
            "weight" => (50.0, 300.0, 0.1),
            _ => (0.0, 10.0, 0.5), // Default 1-10 scale
        }
    }).get();

    view! {
        <div>
            <label class="block text-sm text-gray-400 mb-2">
                "Value: "
                <span class="text-white font-medium">{move || format!("{:.1}", value.get())}</span>
            </label>

            // Slider
            <input
                type="range"
                min=min
                max=max
                step=step
                prop:value=move || value.get().to_string()
                on:input=move |ev| {
                    if let Ok(v) = event_target_value(&ev).parse() {
                        set_value.set(v);
                    }
                }
                class="w-full"
            />

            // Quick value buttons
            <div class="flex justify-between mt-2">
                {[1.0, 3.0, 5.0, 7.0, 9.0].into_iter().map(|v| {
                    let adjusted = if max > 10.0 {
                        v / 10.0 * max
                    } else {
                        v
                    };
                    view! {
                        <button
                            type="button"
                            on:click=move |_| set_value.set(adjusted)
                            class="px-3 py-1 bg-gray-700 hover:bg-gray-600 rounded text-sm transition-colors"
                        >
                            {format!("{:.0}", adjusted)}
                        </button>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

#[component]
fn TagsInput() -> impl IntoView {
    let (tag_key, set_tag_key) = create_signal(String::new());
    let (tag_value, set_tag_value) = create_signal(String::new());
    let (tags, set_tags) = create_signal(Vec::<(String, String)>::new());

    let add_tag = move |_| {
        let k = tag_key.get();
        let v = tag_value.get();
        if !k.is_empty() && !v.is_empty() {
            set_tags.update(|t| t.push((k, v)));
            set_tag_key.set(String::new());
            set_tag_value.set(String::new());
        }
    };

    view! {
        <div>
            <label class="block text-sm text-gray-400 mb-2">"Tags (optional)"</label>

            // Existing tags
            <div class="flex flex-wrap gap-2 mb-2">
                {move || {
                    tags.get().into_iter().enumerate().map(|(idx, (k, v))| {
                        view! {
                            <span class="bg-gray-700 px-2 py-1 rounded text-sm flex items-center space-x-1">
                                <span>{k}"="{v}</span>
                                <button
                                    type="button"
                                    on:click=move |_| set_tags.update(|t| { t.remove(idx); })
                                    class="text-gray-400 hover:text-white"
                                >
                                    "×"
                                </button>
                            </span>
                        }
                    }).collect_view()
                }}
            </div>

            // Add tag inputs
            <div class="flex space-x-2">
                <input
                    type="text"
                    placeholder="Key"
                    prop:value=move || tag_key.get()
                    on:input=move |ev| set_tag_key.set(event_target_value(&ev))
                    class="flex-1 bg-gray-700 rounded px-3 py-2 text-sm
                           border border-gray-600 focus:border-primary-500 focus:outline-none"
                />
                <input
                    type="text"
                    placeholder="Value"
                    prop:value=move || tag_value.get()
                    on:input=move |ev| set_tag_value.set(event_target_value(&ev))
                    class="flex-1 bg-gray-700 rounded px-3 py-2 text-sm
                           border border-gray-600 focus:border-primary-500 focus:outline-none"
                />
                <button
                    type="button"
                    on:click=add_tag
                    class="px-3 py-2 bg-gray-600 hover:bg-gray-500 rounded text-sm transition-colors"
                >
                    "Add"
                </button>
            </div>
        </div>
    }
}

/// Quick entry widget for dashboard
#[component]
pub fn QuickEntry() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let (value, set_value) = create_signal(5.0);
    let (submitting, set_submitting) = create_signal(false);

    let on_submit = move |_| {
        let v = value.get();
        set_submitting.set(true);

        let state_clone = state.clone();
        spawn_local(async move {
            match api::submit_data_point("mood", v, None).await {
                Ok(_) => {
                    state_clone.show_success(&format!("Mood logged: {:.1}", v));
                    set_value.set(5.0);
                }
                Err(e) => {
                    state_clone.show_error(&e);
                }
            }
            set_submitting.set(false);
        });
    };

    view! {
        <div class="flex items-center space-x-4">
            <span class="text-gray-400">"How's your mood?"</span>
            <input
                type="range"
                min="1"
                max="10"
                step="0.5"
                prop:value=move || value.get().to_string()
                on:input=move |ev| {
                    if let Ok(v) = event_target_value(&ev).parse() {
                        set_value.set(v);
                    }
                }
                class="flex-1"
            />
            <span class="text-lg font-bold w-8">{move || format!("{:.1}", value.get())}</span>
            <button
                on:click=on_submit
                disabled=move || submitting.get()
                class="px-4 py-2 bg-primary-600 hover:bg-primary-700 disabled:bg-gray-600
                       rounded-lg font-medium transition-colors"
            >
                {move || if submitting.get() { "..." } else { "Log" }}
            </button>
        </div>
    }
}
