//! Toast Notification Component
//!
//! Shows success and error messages.

use leptos::*;

use crate::state::global::GlobalState;

/// Toast notification container
#[component]
pub fn Toast() -> impl IntoView {
    let state = use_context::<GlobalState>().expect("GlobalState not found");

    view! {
        <div class="fixed bottom-20 right-4 z-50 space-y-2">
            // Success toast
            {move || {
                state.success.get().map(|msg| view! {
                    <ToastMessage message=msg variant=ToastVariant::Success />
                })
            }}

            // Error toast
            {move || {
                state.error.get().map(|msg| view! {
                    <ToastMessage message=msg variant=ToastVariant::Error />
                })
            }}
        </div>
    }
}

#[derive(Clone, Copy)]
enum ToastVariant {
    Success,
    Error,
}

#[component]
fn ToastMessage(
    #[prop(into)]
    message: String,
    variant: ToastVariant,
) -> impl IntoView {
    let (icon, bg_class) = match variant {
        ToastVariant::Success => ("✓", "bg-green-600"),
        ToastVariant::Error => ("✕", "bg-red-600"),
    };

    view! {
        <div class=format!(
            "flex items-center space-x-3 {} text-white px-4 py-3 rounded-lg shadow-lg \
             transform transition-all duration-300 ease-out animate-slide-in",
            bg_class
        )>
            <span class="text-lg">{icon}</span>
            <span class="text-sm font-medium">{message}</span>
        </div>
    }
}

/// Standalone toast for specific use cases
#[component]
pub fn StandaloneToast(
    #[prop(into)]
    message: String,
    #[prop(default = "success")]
    variant: &'static str,
    #[prop(into)]
    visible: Signal<bool>,
) -> impl IntoView {
    let (icon, bg_class) = match variant {
        "success" => ("✓", "bg-green-600"),
        "error" => ("✕", "bg-red-600"),
        "warning" => ("⚠", "bg-yellow-600"),
        "info" => ("ℹ", "bg-blue-600"),
        _ => ("•", "bg-gray-600"),
    };

    view! {
        {move || {
            if visible.get() {
                view! {
                    <div class=format!(
                        "fixed bottom-20 right-4 z-50 flex items-center space-x-3 {} text-white \
                         px-4 py-3 rounded-lg shadow-lg",
                        bg_class
                    )>
                        <span class="text-lg">{icon}</span>
                        <span class="text-sm font-medium">{message.clone()}</span>
                    </div>
                }.into_view()
            } else {
                view! {}.into_view()
            }
        }}
    }
}
