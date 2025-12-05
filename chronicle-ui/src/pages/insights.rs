//! Insights Page
//!
//! Ask questions and explore correlations in your data.

use leptos::*;

use crate::api;
use crate::components::insight_card::CorrelationCard;
use crate::state::global::GlobalState;

/// Insights page component
#[component]
pub fn Insights() -> impl IntoView {
    view! {
        <div class="space-y-8">
            // Header
            <div>
                <h1 class="text-3xl font-bold">"Insights"</h1>
                <p class="text-gray-400 mt-1">"Discover patterns in your data with AI"</p>
            </div>

            // Main ask question section
            <AskQuestionSection />

            // Two column layout
            <div class="grid lg:grid-cols-2 gap-8">
                // Correlations
                <section class="bg-gray-800 rounded-xl p-6">
                    <CorrelationCard />
                </section>

                // Suggested questions
                <section class="bg-gray-800 rounded-xl p-6">
                    <SuggestedQuestions />
                </section>
            </div>

            // Recent insights
            <RecentInsights />
        </div>
    }
}

/// Main ask question section
#[component]
fn AskQuestionSection() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let (question, set_question) = create_signal(String::new());
    let (answer, set_answer) = create_signal(None::<String>);
    let (loading, set_loading) = create_signal(false);
    let (context_days, set_context_days) = create_signal(30_i64);

    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        let q = question.get();
        if q.is_empty() {
            return;
        }

        set_loading.set(true);
        set_answer.set(None);

        let days = context_days.get();
        let state_clone = state.clone();
        spawn_local(async move {
            match api::fetch_insight(&q, days).await {
                Ok(text) => {
                    set_answer.set(Some(text));
                }
                Err(e) => {
                    state_clone.show_error(&e);
                    set_answer.set(Some(format!("Unable to generate insight: {}", e)));
                }
            }
            set_loading.set(false);
        });
    };

    view! {
        <section class="bg-gray-800 rounded-xl p-6">
            <h2 class="text-xl font-semibold mb-4">"Ask About Your Data"</h2>

            <form on:submit=on_submit class="space-y-4">
                // Question input
                <div>
                    <textarea
                        placeholder="What would you like to know about your data?"
                        prop:value=move || question.get()
                        on:input=move |ev| set_question.set(event_target_value(&ev))
                        rows="3"
                        class="w-full bg-gray-700 rounded-lg px-4 py-3
                               border border-gray-600 focus:border-primary-500 focus:outline-none
                               resize-none"
                    />
                </div>

                // Context days selector
                <div class="flex items-center space-x-4">
                    <span class="text-sm text-gray-400">"Analyze data from:"</span>
                    <select
                        on:change=move |ev| {
                            if let Ok(days) = event_target_value(&ev).parse() {
                                set_context_days.set(days);
                            }
                        }
                        class="bg-gray-700 rounded px-3 py-2 text-sm
                               border border-gray-600 focus:border-primary-500 focus:outline-none"
                    >
                        <option value="7">"Last 7 days"</option>
                        <option value="30" selected>"Last 30 days"</option>
                        <option value="90">"Last 90 days"</option>
                        <option value="365">"Last year"</option>
                    </select>

                    <button
                        type="submit"
                        disabled=move || loading.get() || question.get().is_empty()
                        class="px-6 py-2 bg-primary-600 hover:bg-primary-700 disabled:bg-gray-600
                               rounded-lg font-medium transition-colors ml-auto"
                    >
                        {move || if loading.get() {
                            "Thinking..."
                        } else {
                            "Ask"
                        }}
                    </button>
                </div>
            </form>

            // Answer display
            {move || {
                if loading.get() {
                    view! {
                        <div class="mt-6 animate-pulse">
                            <div class="h-4 bg-gray-700 rounded w-3/4 mb-2" />
                            <div class="h-4 bg-gray-700 rounded w-full mb-2" />
                            <div class="h-4 bg-gray-700 rounded w-5/6" />
                        </div>
                    }.into_view()
                } else if let Some(text) = answer.get() {
                    view! {
                        <div class="mt-6 bg-gray-700 rounded-lg p-4">
                            <div class="flex items-start space-x-3">
                                <span class="text-2xl">"💡"</span>
                                <p class="text-gray-200 leading-relaxed whitespace-pre-wrap">{text}</p>
                            </div>
                        </div>
                    }.into_view()
                } else {
                    view! {}.into_view()
                }
            }}
        </section>
    }
}

/// Suggested questions section
#[component]
fn SuggestedQuestions() -> impl IntoView {
    let suggestions = [
        ("Sleep & Mood", "How does my sleep affect my mood the next day?"),
        ("Best Days", "What patterns do my best days have in common?"),
        ("Energy Factors", "What factors seem to influence my energy levels?"),
        ("Weekly Patterns", "Are there any weekly patterns in my data?"),
        ("Improvement", "What one change would have the biggest impact?"),
        ("Correlations", "What are the strongest correlations in my data?"),
    ];

    let state = use_context::<GlobalState>().expect("GlobalState not found");
    let (loading, set_loading) = create_signal(false);
    let (answer, set_answer) = create_signal(None::<(String, String)>);

    view! {
        <div>
            <h3 class="text-lg font-semibold mb-4">"Suggested Questions"</h3>

            <div class="space-y-2">
                {suggestions.into_iter().map(|(label, question)| {
                    let q = question.to_string();
                    let l = label.to_string();
                    let state_clone = state.clone();

                    let on_click = move |_| {
                        set_loading.set(true);
                        set_answer.set(None);

                        let q_clone = q.clone();
                        let l_clone = l.clone();
                        let state_inner = state_clone.clone();

                        spawn_local(async move {
                            match api::fetch_insight(&q_clone, 30).await {
                                Ok(text) => {
                                    set_answer.set(Some((l_clone, text)));
                                }
                                Err(e) => {
                                    state_inner.show_error(&e);
                                }
                            }
                            set_loading.set(false);
                        });
                    };

                    view! {
                        <button
                            on:click=on_click
                            disabled=move || loading.get()
                            class="w-full text-left px-4 py-3 bg-gray-700 hover:bg-gray-600
                                   disabled:opacity-50 rounded-lg transition-colors"
                        >
                            <div class="font-medium">{label}</div>
                            <div class="text-sm text-gray-400">{question}</div>
                        </button>
                    }
                }).collect_view()}
            </div>

            // Answer for suggested question
            {move || {
                if loading.get() {
                    view! {
                        <div class="mt-4 animate-pulse">
                            <div class="h-4 bg-gray-700 rounded w-full mb-2" />
                            <div class="h-4 bg-gray-700 rounded w-3/4" />
                        </div>
                    }.into_view()
                } else if let Some((label, text)) = answer.get() {
                    view! {
                        <div class="mt-4 bg-gray-700 rounded-lg p-4">
                            <div class="text-sm text-gray-400 mb-2">{label}</div>
                            <p class="text-gray-200">{text}</p>
                        </div>
                    }.into_view()
                } else {
                    view! {}.into_view()
                }
            }}
        </div>
    }
}

/// Recent insights section
#[component]
fn RecentInsights() -> impl IntoView {
    // In a full implementation, this would load from local storage or API
    view! {
        <section class="bg-gray-800 rounded-xl p-6">
            <h2 class="text-xl font-semibold mb-4">"How Insights Work"</h2>

            <div class="grid md:grid-cols-3 gap-6">
                <div class="text-center">
                    <div class="text-4xl mb-2">"📊"</div>
                    <h3 class="font-medium mb-1">"Data Collection"</h3>
                    <p class="text-sm text-gray-400">
                        "Chronicle collects your personal metrics and stores them securely."
                    </p>
                </div>

                <div class="text-center">
                    <div class="text-4xl mb-2">"🔍"</div>
                    <h3 class="font-medium mb-1">"Pattern Analysis"</h3>
                    <p class="text-sm text-gray-400">
                        "MemMachine analyzes your data to find correlations and patterns."
                    </p>
                </div>

                <div class="text-center">
                    <div class="text-4xl mb-2">"💡"</div>
                    <h3 class="font-medium mb-1">"Personalized Insights"</h3>
                    <p class="text-sm text-gray-400">
                        "Get actionable insights tailored to your unique patterns."
                    </p>
                </div>
            </div>

            <div class="mt-6 p-4 bg-gray-700 rounded-lg">
                <p class="text-sm text-gray-300">
                    <span class="font-medium">"Tip:"</span>
                    " The more data you log consistently, the better insights you'll get. "
                    "Try to log your mood and energy at least once a day for best results."
                </p>
            </div>
        </section>
    }
}
