//! Settings Page
//!
//! Application configuration and preferences.

use leptos::*;
use wasm_bindgen::JsCast;

use crate::api;
use crate::state::global::GlobalState;

/// Settings page component
#[component]
pub fn Settings() -> impl IntoView {
    view! {
        <div class="space-y-8">
            // Header
            <div>
                <h1 class="text-3xl font-bold">"Settings"</h1>
                <p class="text-gray-400 mt-1">"Configure your Chronicle dashboard"</p>
            </div>

            // API Connection
            <ApiSettings />

            // Display Settings
            <DisplaySettings />

            // Data Management
            <DataManagement />

            // About
            <AboutSection />
        </div>
    }
}

/// API connection settings
#[component]
fn ApiSettings() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let (api_url, set_api_url) = create_signal(api::get_api_base());
    let (testing, set_testing) = create_signal(false);
    let (test_result, set_test_result) = create_signal(None::<bool>);

    let state_for_test = state.clone();
    let test_connection = move |_| {
        set_testing.set(true);
        set_test_result.set(None);

        let url = api_url.get();
        api::set_api_base(&url);

        let state_clone = state_for_test.clone();
        spawn_local(async move {
            match api::check_health().await {
                Ok(_) => {
                    set_test_result.set(Some(true));
                    state_clone.show_success("Connection successful!");
                }
                Err(e) => {
                    set_test_result.set(Some(false));
                    state_clone.show_error(&format!("Connection failed: {}", e));
                }
            }
            set_testing.set(false);
        });
    };

    let state_for_save = state.clone();
    let save_url = move |_| {
        let url = api_url.get();
        api::set_api_base(&url);
        state_for_save.show_success("API URL saved");
    };

    view! {
        <section class="bg-gray-800 rounded-xl p-6">
            <h2 class="text-xl font-semibold mb-4">"API Connection"</h2>

            <div class="space-y-4">
                // API URL
                <div>
                    <label class="block text-sm text-gray-400 mb-2">"Chronicle API URL"</label>
                    <div class="flex space-x-2">
                        <input
                            type="text"
                            prop:value=move || api_url.get()
                            on:input=move |ev| set_api_url.set(event_target_value(&ev))
                            class="flex-1 bg-gray-700 rounded-lg px-4 py-3
                                   border border-gray-600 focus:border-primary-500 focus:outline-none"
                        />
                        <button
                            on:click=test_connection
                            disabled=move || testing.get()
                            class="px-4 py-3 bg-gray-600 hover:bg-gray-500 disabled:bg-gray-700
                                   rounded-lg font-medium transition-colors"
                        >
                            {move || if testing.get() { "Testing..." } else { "Test" }}
                        </button>
                        <button
                            on:click=save_url
                            class="px-4 py-3 bg-primary-600 hover:bg-primary-700
                                   rounded-lg font-medium transition-colors"
                        >
                            "Save"
                        </button>
                    </div>
                </div>

                // Connection status
                <div class="flex items-center space-x-2">
                    <span class="text-sm text-gray-400">"Status:"</span>
                    {move || {
                        match test_result.get() {
                            Some(true) => view! {
                                <span class="text-green-400">"✓ Connected"</span>
                            }.into_view(),
                            Some(false) => view! {
                                <span class="text-red-400">"✕ Failed"</span>
                            }.into_view(),
                            None => view! {
                                <span class="text-gray-400">"Not tested"</span>
                            }.into_view(),
                        }
                    }}
                </div>

                // WebSocket status
                <div class="flex items-center space-x-2">
                    <span class="text-sm text-gray-400">"WebSocket:"</span>
                    {
                        let ws_connected = state.ws_connected;
                        move || {
                            if ws_connected.get() {
                                view! { <span class="text-green-400">"🟢 Connected"</span> }.into_view()
                            } else {
                                view! { <span class="text-red-400">"🔴 Disconnected"</span> }.into_view()
                            }
                        }
                    }
                </div>
            </div>
        </section>
    }
}

/// Display settings
#[component]
fn DisplaySettings() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let selected_metrics = state.selected_metrics;

    let toggle_metric = move |metric: String| {
        selected_metrics.update(|m| {
            if m.contains(&metric) {
                m.retain(|x| x != &metric);
            } else {
                m.push(metric);
            }
        });
    };

    let default_metrics = ["mood", "energy", "sleep_hours", "steps", "weight", "anxiety", "focus"];

    view! {
        <section class="bg-gray-800 rounded-xl p-6">
            <h2 class="text-xl font-semibold mb-4">"Display Settings"</h2>

            <div class="space-y-4">
                // Default metrics to show
                <div>
                    <label class="block text-sm text-gray-400 mb-2">"Metrics to Display on Dashboard"</label>
                    <div class="flex flex-wrap gap-2">
                        {default_metrics.into_iter().map(|metric| {
                            let m = metric.to_string();
                            let m_clone = m.clone();

                            view! {
                                <button
                                    on:click=move |_| toggle_metric(m_clone.clone())
                                    class=move || {
                                        let base = "px-3 py-2 rounded-lg text-sm font-medium transition-colors capitalize";
                                        if selected_metrics.get().contains(&m) {
                                            format!("{} bg-primary-600 text-white", base)
                                        } else {
                                            format!("{} bg-gray-700 text-gray-400 hover:bg-gray-600", base)
                                        }
                                    }
                                >
                                    {metric}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                </div>

                // Default time range
                <div>
                    <label class="block text-sm text-gray-400 mb-2">"Default Time Range"</label>
                    <select class="bg-gray-700 rounded-lg px-4 py-3 w-full max-w-xs
                                   border border-gray-600 focus:border-primary-500 focus:outline-none">
                        <option value="7">"Last 7 days"</option>
                        <option value="30">"Last 30 days"</option>
                        <option value="90">"Last 90 days"</option>
                    </select>
                </div>
            </div>
        </section>
    }
}

/// Data management section
#[component]
fn DataManagement() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    let (exporting, set_exporting) = create_signal(false);
    let (syncing, set_syncing) = create_signal(false);
    let (importing, set_importing) = create_signal(false);
    let (import_status, set_import_status) = create_signal(String::new());

    let state_for_export = state.clone();
    let export_data = move |_| {
        set_exporting.set(true);

        let state_clone = state_for_export.clone();
        spawn_local(async move {
            match api::export_data("json", None, None, None).await {
                Ok(data) => {
                    // Create download
                    if let Some(window) = web_sys::window() {
                        let blob = web_sys::Blob::new_with_str_sequence(
                            &js_sys::Array::of1(&data.into()),
                        ).ok();

                        if let Some(blob) = blob {
                            let url = web_sys::Url::create_object_url_with_blob(&blob).ok();
                            if let Some(url) = url {
                                let document = window.document().unwrap();
                                let a = document.create_element("a").unwrap();
                                let _ = a.set_attribute("href", &url);
                                let _ = a.set_attribute("download", "chronicle-export.json");
                                let _ = a.dyn_ref::<web_sys::HtmlElement>().unwrap().click();
                                let _ = web_sys::Url::revoke_object_url(&url);
                            }
                        }
                    }
                    state_clone.show_success("Data exported successfully");
                }
                Err(e) => {
                    state_clone.show_error(&e);
                }
            }
            set_exporting.set(false);
        });
    };

    let state_for_sync = state.clone();
    let sync_memmachine = move |_| {
        set_syncing.set(true);

        let state_clone = state_for_sync.clone();
        spawn_local(async move {
            match api::trigger_sync().await {
                Ok(_) => {
                    state_clone.show_success("Sync triggered successfully");
                }
                Err(e) => {
                    state_clone.show_error(&e);
                }
            }
            set_syncing.set(false);
        });
    };

    // Apple Health import handler
    let state_for_import = state;
    let handle_file_upload = move |ev: web_sys::Event| {
        let input: web_sys::HtmlInputElement = ev.target().unwrap().dyn_into().unwrap();

        if let Some(files) = input.files() {
            if let Some(file) = files.get(0) {
                set_importing.set(true);
                set_import_status.set("Reading file...".to_string());

                let state_clone = state_for_import.clone();
                let file_reader = web_sys::FileReader::new().unwrap();

                let onload = {
                    let file_reader = file_reader.clone();
                    wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                        if let Ok(result) = file_reader.result() {
                            if let Some(array_buffer) = result.dyn_ref::<js_sys::ArrayBuffer>() {
                                let uint8_array = js_sys::Uint8Array::new(array_buffer);
                                let data: Vec<u8> = uint8_array.to_vec();

                                set_import_status.set("Uploading to server...".to_string());

                                let state_inner = state_clone.clone();
                                spawn_local(async move {
                                    match api::import_apple_health(&data).await {
                                        Ok(result) => {
                                            set_import_status.set(format!("Imported {} data points!", result.imported_count));
                                            state_inner.show_success(&format!("Successfully imported {} Apple Health records", result.imported_count));
                                        }
                                        Err(e) => {
                                            set_import_status.set(format!("Error: {}", e));
                                            state_inner.show_error(&format!("Import failed: {}", e));
                                        }
                                    }
                                    set_importing.set(false);
                                });
                            }
                        }
                    }) as Box<dyn FnMut(_)>)
                };

                file_reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                onload.forget();

                let _ = file_reader.read_as_array_buffer(&file);
            }
        }
    };

    view! {
        <section class="bg-gray-800 rounded-xl p-6">
            <h2 class="text-xl font-semibold mb-4">"Data Management"</h2>

            <div class="space-y-4">
                // Export data
                <div class="flex items-center justify-between p-4 bg-gray-700 rounded-lg">
                    <div>
                        <h3 class="font-medium">"Export Data"</h3>
                        <p class="text-sm text-gray-400">"Download all your data as JSON"</p>
                    </div>
                    <button
                        on:click=export_data
                        disabled=move || exporting.get()
                        class="px-4 py-2 bg-gray-600 hover:bg-gray-500 disabled:bg-gray-700
                               rounded-lg font-medium transition-colors"
                    >
                        {move || if exporting.get() { "Exporting..." } else { "Export" }}
                    </button>
                </div>

                // Sync with MemMachine
                <div class="flex items-center justify-between p-4 bg-gray-700 rounded-lg">
                    <div>
                        <h3 class="font-medium">"Sync with MemMachine"</h3>
                        <p class="text-sm text-gray-400">"Push latest data to MemMachine for insights"</p>
                    </div>
                    <button
                        on:click=sync_memmachine
                        disabled=move || syncing.get()
                        class="px-4 py-2 bg-primary-600 hover:bg-primary-700 disabled:bg-gray-600
                               rounded-lg font-medium transition-colors"
                    >
                        {move || if syncing.get() { "Syncing..." } else { "Sync Now" }}
                    </button>
                </div>

                // Apple Health Import
                <div class="p-4 bg-gray-700 rounded-lg">
                    <div class="flex items-center justify-between mb-3">
                        <div>
                            <h3 class="font-medium flex items-center gap-2">
                                <span>"🍎"</span>
                                "Import Apple Health"
                            </h3>
                            <p class="text-sm text-gray-400">"Import your Apple Health export ZIP file"</p>
                        </div>
                    </div>

                    <div class="space-y-3">
                        <div class="flex items-center gap-3">
                            <label
                                class="flex-1 flex items-center justify-center px-4 py-3 bg-gray-600
                                       hover:bg-gray-500 rounded-lg cursor-pointer transition-colors
                                       border-2 border-dashed border-gray-500 hover:border-primary-500"
                            >
                                <input
                                    type="file"
                                    accept=".zip"
                                    class="hidden"
                                    on:change=handle_file_upload
                                    disabled=move || importing.get()
                                />
                                <span class="flex items-center gap-2">
                                    {move || if importing.get() {
                                        view! { <span class="loading-spinner w-4 h-4"></span> }.into_view()
                                    } else {
                                        view! { <span>"📁"</span> }.into_view()
                                    }}
                                    {move || if importing.get() {
                                        "Processing..."
                                    } else {
                                        "Choose ZIP file or drag here"
                                    }}
                                </span>
                            </label>
                        </div>

                        {move || {
                            let status = import_status.get();
                            if !status.is_empty() {
                                view! {
                                    <div class="text-sm p-2 bg-gray-800 rounded">
                                        {status}
                                    </div>
                                }.into_view()
                            } else {
                                view! {}.into_view()
                            }
                        }}

                        <div class="text-xs text-gray-500">
                            <p>"How to export from Apple Health:"</p>
                            <ol class="list-decimal list-inside mt-1 space-y-1">
                                <li>"Open the Health app on your iPhone"</li>
                                <li>"Tap your profile picture → Export All Health Data"</li>
                                <li>"Save the ZIP file and upload it here"</li>
                            </ol>
                        </div>
                    </div>
                </div>
            </div>
        </section>
    }
}

/// About section
#[component]
fn AboutSection() -> impl IntoView {
    view! {
        <section class="bg-gray-800 rounded-xl p-6">
            <h2 class="text-xl font-semibold mb-4">"About Chronicle"</h2>

            <div class="space-y-4 text-gray-300">
                <p>
                    "Chronicle is a personal time-series intelligence system. "
                    "Track your metrics, discover patterns, and get personalized insights."
                </p>

                <div class="grid md:grid-cols-2 gap-4 text-sm">
                    <div class="p-4 bg-gray-700 rounded-lg">
                        <h3 class="font-medium text-white mb-2">"Built With"</h3>
                        <ul class="space-y-1 text-gray-400">
                            <li>"• Rust (Backend & Frontend)"</li>
                            <li>"• Leptos (WASM UI Framework)"</li>
                            <li>"• Axum (Web Server)"</li>
                            <li>"• WebSocket (Real-time)"</li>
                        </ul>
                    </div>

                    <div class="p-4 bg-gray-700 rounded-lg">
                        <h3 class="font-medium text-white mb-2">"Features"</h3>
                        <ul class="space-y-1 text-gray-400">
                            <li>"• Time-series storage"</li>
                            <li>"• Real-time updates"</li>
                            <li>"• AI-powered insights"</li>
                            <li>"• Correlation analysis"</li>
                        </ul>
                    </div>
                </div>

                <p class="text-sm text-gray-400">
                    "Version 0.1.0 • Made with 🧡 using Rust"
                </p>
            </div>
        </section>
    }
}
