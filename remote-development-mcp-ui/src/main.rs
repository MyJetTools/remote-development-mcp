use dioxus::prelude::*;

mod api;
mod components;
mod dialogs;
mod models;
mod states;
mod time;
mod views;

use states::AppState;

fn main() {
    dioxus::LaunchBuilder::new().launch(|| {
        rsx! {
            document::Link { rel: "icon", href: asset!("/public/favicon.ico") }
            document::Meta {
                name: "viewport",
                content: "width=device-width, initial-scale=1.0",
            }
            App {}
        }
    });
}

#[component]
fn App() -> Element {
    let app_state = use_context_provider(|| Signal::new(AppState::default()));

    // The one place the palette is chosen. Everything below reads its colours
    // from variables, so the whole console changes with this class and nothing
    // else has to know a theme exists. No class at all means the stylesheet's
    // own `prefers-color-scheme` default is left to decide.
    let theme_class = app_state.read().theme.class();

    rsx! {
        // Wraps the dialog too, not just the panel: the dialog is a fixed
        // overlay outside the panel's box, and left outside this it would keep
        // the system palette while everything behind it changed.
        div { class: "app-root {theme_class}",
            div { id: "main-panel",
                crate::views::dashboard::RenderDashboard {}
            }
            crate::dialogs::RenderDialog {}
        }
    }
}
