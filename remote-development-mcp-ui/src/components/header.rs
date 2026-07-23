use dioxus::prelude::*;
use rest_api_shared::DashboardStateResponse;

use super::render_duration;

#[component]
pub fn Header(state: DashboardStateResponse, stale: Option<String>) -> Element {
    let uptime = render_duration(state.uptime_sec);
    let dot_class = if stale.is_some() {
        "live-dot stale"
    } else {
        "live-dot"
    };

    rsx! {
        div { class: "app-header",
            span { class: dot_class }
            span { class: "app-title", "{state.app_name}" }
            span { class: "dim", "v{state.version}" }
            span { class: "dim", "up {uptime}" }
            span { class: "dim", "{state.bind_addr}" }
            div { class: "spacer" }
            if let Some(err) = stale {
                span { class: "status-failed", "stale — {err}" }
            }
        }
    }
}
