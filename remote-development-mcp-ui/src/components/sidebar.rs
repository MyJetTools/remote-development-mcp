use dioxus::prelude::*;
use rest_api_shared::DashboardStateResponse;

use crate::states::{AppState, Section};

use super::render_duration;

#[component]
pub fn Sidebar(state: DashboardStateResponse, active: Section, stale: Option<String>) -> Element {
    let mut app_state = consume_context::<Signal<AppState>>();

    let uptime = render_duration(state.uptime_sec);
    let dot_class = if stale.is_some() {
        "live-dot stale"
    } else {
        "live-dot"
    };

    let running_jobs = state
        .jobs
        .iter()
        .filter(|job| job.status == "running")
        .count();

    rsx! {
        div { class: "sidebar",
            div { class: "sidebar-head",
                div { class: "sidebar-title",
                    span { class: dot_class }
                    "{state.app_name}"
                }
                div { class: "sidebar-sub", "v{state.version}" }
            }

            nav { class: "sidebar-nav",
                for section in Section::ALL {
                    {
                        let count = badge_count(&state, section, running_jobs);
                        let item_class = if section == active {
                            "nav-item active"
                        } else {
                            "nav-item"
                        };

                        rsx! {
                            button {
                                key: "{section.label()}",
                                class: item_class,
                                onclick: move |_| app_state.write().select_section(section),
                                span { class: "nav-label", "{section.label()}" }
                                if let Some(count) = count {
                                    span { class: "nav-badge", "{count}" }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "sidebar-foot",
                div { "up {uptime}" }
                div { "{state.bind_addr}" }
                if let Some(err) = stale {
                    div { class: "stale", title: "{err}", "stale — reconnecting" }
                }
            }
        }
    }
}

/// The number next to a menu item — how much is behind it right now. A section
/// with nothing to count (or nothing in it) shows no badge rather than a `0`.
fn badge_count(
    state: &DashboardStateResponse,
    section: Section,
    running_jobs: usize,
) -> Option<usize> {
    let count = match section {
        Section::Projects => state.repos.len(),
        Section::Sessions => state.sessions.len(),
        // What is live is what is worth a badge — a finished job is history.
        Section::Tasks => running_jobs,
    };

    if count == 0 {
        None
    } else {
        Some(count)
    }
}
