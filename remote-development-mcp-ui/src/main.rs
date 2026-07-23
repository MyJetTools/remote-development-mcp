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
    use_context_provider(|| Signal::new(AppState::default()));

    rsx! {
        div { id: "main-panel",
            crate::views::dashboard::RenderDashboard {}
        }
        crate::dialogs::RenderDialog {}
    }
}
