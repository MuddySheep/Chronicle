//! Insight Card Component
//!
//! Displays AI-generated insights from MemMachine.

use leptos::*;

use crate::api;

/// Insight card component
#[component]
pub fn InsightCard() -> impl IntoView {
    let (insight, set_insight) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(true);
    let (error, set_error) = create_signal(None::<String>);

    // Fetch insight on mount
    create_effect(move |_| {
        spawn_local(async move {
            match api::fetch_insight("What patterns do you see in my recent data?", 30).await {
                Ok(text) => {
                    set_insight.set(Some(text));
                    set_error.set(None);
                }
                Err(e) => {
                    set_error.set(Some(e));
                    set_insight.set(Some("Unable to generate insights right now. Make sure MemMachine is connected.".to_string()));
                }
            }
            set_loading.set(false);
        });
    });

    view! {
        <div class="space-y-4">
            {move || {
                if loading.get() {
                    view! { <InsightSkeleton /> }.into_view()
                } else {
                    view! {
                        <div class="bg-gray-700 rounded-lg p-4">
                            <div class="flex items-start space-x-3">
                                <span class="text-2xl">"💡"</span>
                                <div class="flex-1">
                                    <p class="text-gray-200 leading-relaxed">
                                        {move || insight.get().unwrap_or_default()}
                                    </p>

                                    {move || error.get().map(|_| view! {
                                        <p class="text-yellow-400 text-sm mt-2">
                                            "⚠️ Connect MemMachine for personalized insights"
                                        </p>
                                    })}
                                </div>
                            </div>
                        </div>
                    }.into_view()
                }
            }}

            // Ask a question button
            <AskQuestion />
        </div>
    }
}

/// Loading skeleton for insight
#[component]
fn InsightSkeleton() -> impl IntoView {
    view! {
        <div class="bg-gray-700 rounded-lg p-4 animate-pulse">
            <div class="flex items-start space-x-3">
                <div class="w-8 h-8 bg-gray-600 rounded" />
                <div class="flex-1 space-y-2">
                    <div class="h-4 bg-gray-600 rounded w-3/4" />
                    <div class="h-4 bg-gray-600 rounded w-full" />
                    <div class="h-4 bg-gray-600 rounded w-5/6" />
                </div>
            </div>
        </div>
    }
}

/// Ask a question component
#[component]
fn AskQuestion() -> impl IntoView {
    let (expanded, set_expanded) = create_signal(false);
    let (question, set_question) = create_signal(String::new());
    let (answer, set_answer) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(false);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        let q = question.get();
        if q.is_empty() {
            return;
        }

        set_loading.set(true);

        spawn_local(async move {
            match api::fetch_insight(&q, 30).await {
                Ok(text) => {
                    set_answer.set(Some(text));
                }
                Err(e) => {
                    set_answer.set(Some(format!("Error: {}", e)));
                }
            }
            set_loading.set(false);
        });
    };

    view! {
        <div>
            {move || {
                if expanded.get() {
                    view! {
                        <form on:submit=on_submit class="space-y-3">
                            <div class="flex space-x-2">
                                <input
                                    type="text"
                                    placeholder="Ask about your data..."
                                    prop:value=move || question.get()
                                    on:input=move |ev| set_question.set(event_target_value(&ev))
                                    class="flex-1 bg-gray-700 rounded-lg px-4 py-2
                                           border border-gray-600 focus:border-primary-500 focus:outline-none"
                                />
                                <button
                                    type="submit"
                                    disabled=move || loading.get() || question.get().is_empty()
                                    class="px-4 py-2 bg-primary-600 hover:bg-primary-700 disabled:bg-gray-600
                                           rounded-lg font-medium transition-colors"
                                >
                                    {move || if loading.get() { "..." } else { "Ask" }}
                                </button>
                            </div>

                            // Quick question suggestions
                            <div class="flex flex-wrap gap-2">
                                <QuickQuestion
                                    text="How is my sleep affecting my mood?"
                                    on_click=move |q| set_question.set(q)
                                />
                                <QuickQuestion
                                    text="What improves my energy?"
                                    on_click=move |q| set_question.set(q)
                                />
                                <QuickQuestion
                                    text="When am I most productive?"
                                    on_click=move |q| set_question.set(q)
                                />
                            </div>

                            // Answer display
                            {move || answer.get().map(|a| view! {
                                <div class="bg-gray-700 rounded-lg p-4 mt-4">
                                    <p class="text-gray-200">{a}</p>
                                </div>
                            })}

                            <button
                                type="button"
                                on:click=move |_| {
                                    set_expanded.set(false);
                                    set_answer.set(None);
                                    set_question.set(String::new());
                                }
                                class="text-gray-400 hover:text-white text-sm"
                            >
                                "← Back"
                            </button>
                        </form>
                    }.into_view()
                } else {
                    view! {
                        <button
                            on:click=move |_| set_expanded.set(true)
                            class="text-primary-400 hover:text-primary-300 text-sm font-medium
                                   flex items-center space-x-1"
                        >
                            <span>"Ask a question"</span>
                            <span>"→"</span>
                        </button>
                    }.into_view()
                }
            }}
        </div>
    }
}

/// Quick question suggestion button
#[component]
fn QuickQuestion(
    text: &'static str,
    on_click: impl Fn(String) + 'static,
) -> impl IntoView {
    let text_string = text.to_string();

    view! {
        <button
            type="button"
            on:click=move |_| on_click(text_string.clone())
            class="px-3 py-1 bg-gray-600 hover:bg-gray-500 rounded-full text-xs text-gray-300 transition-colors"
        >
            {text}
        </button>
    }
}

/// Correlation display component
#[component]
pub fn CorrelationCard() -> impl IntoView {
    let (correlations, set_correlations) = create_signal(Vec::new());
    let (loading, set_loading) = create_signal(true);

    // Fetch correlations on mount
    create_effect(move |_| {
        spawn_local(async move {
            match api::fetch_correlations(30).await {
                Ok(data) => {
                    set_correlations.set(data);
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Failed to fetch correlations: {}", e).into());
                }
            }
            set_loading.set(false);
        });
    });

    view! {
        <div class="space-y-3">
            <h3 class="text-lg font-semibold">"Correlations"</h3>

            {move || {
                if loading.get() {
                    view! {
                        <div class="space-y-2 animate-pulse">
                            <div class="h-12 bg-gray-700 rounded" />
                            <div class="h-12 bg-gray-700 rounded" />
                        </div>
                    }.into_view()
                } else if correlations.get().is_empty() {
                    view! {
                        <p class="text-gray-400 text-sm">"No correlations found yet. Add more data!"</p>
                    }.into_view()
                } else {
                    view! {
                        <div class="space-y-2">
                            {correlations.get().into_iter().take(5).map(|corr| {
                                let strength = (corr.correlation.abs() * 100.0) as i32;
                                let color = if corr.correlation > 0.0 { "bg-green-500" } else { "bg-red-500" };

                                view! {
                                    <div class="bg-gray-700 rounded-lg p-3">
                                        <div class="flex justify-between items-center mb-2">
                                            <span class="text-sm capitalize">
                                                {corr.metric_a}" ↔ "{corr.metric_b}
                                            </span>
                                            <span class="text-sm font-medium">
                                                {format!("{:.2}", corr.correlation)}
                                            </span>
                                        </div>
                                        <div class="w-full bg-gray-600 rounded-full h-2">
                                            <div
                                                class=format!("{} rounded-full h-2 transition-all", color)
                                                style=format!("width: {}%", strength)
                                            />
                                        </div>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }.into_view()
                }
            }}
        </div>
    }
}
