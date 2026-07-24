use dioxus::prelude::*;
use rest_api_shared::DashboardStateResponse;

use crate::states::{AppState, Section};

use super::render_duration;

/// The one horizontal strip above the content: what this server is, the tabs,
/// and how it is doing. Horizontal on purpose — the tables below are wide, and a
/// side menu spends width that the content wants.
#[component]
pub fn TopBar(state: DashboardStateResponse, active: Section, stale: Option<String>) -> Element {
    let mut app_state = consume_context::<Signal<AppState>>();

    let theme = app_state.read().theme;

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
        div { class: "topbar",
            div { class: "topbar-brand",
                span { class: dot_class }
                span { class: "topbar-title", "{state.app_name}" }
                span { class: "dim", "v{state.version}" }
            }

            nav { class: "tabs",
                for section in Section::ALL {
                    {
                        let count = badge_count(&state, section, running_jobs);
                        let tab_class = if section == active {
                            "tab active"
                        } else {
                            "tab"
                        };

                        rsx! {
                            button {
                                key: "{section.label()}",
                                class: tab_class,
                                onclick: move |_| app_state.write().select_section(section),
                                span { "{section.label()}" }
                                if let Some(count) = count {
                                    span { class: "tab-badge", "{count}" }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "topbar-spacer" }

            div { class: "topbar-meta",
                if let Some(err) = stale {
                    span { class: "stale", title: "{err}", "stale — reconnecting" }
                }
                span { "up {uptime}" }
                span { "{state.bind_addr}" }

                // One control for all three settings rather than three: the
                // list is short and the current one is written on the button.
                button {
                    class: "theme-toggle",
                    title: "theme — follows the machine until you pick one",
                    onclick: move |_| app_state.write().cycle_theme(),
                    "{theme.label()}"
                }
            }
        }
    }
}

/// The number next to a tab — how much is behind it right now. A tab with
/// nothing to count shows no badge rather than a `0`.
fn badge_count(
    state: &DashboardStateResponse,
    section: Section,
    running_jobs: usize,
) -> Option<usize> {
    let count = match section {
        Section::Projects => state.repos.len(),
        // Nothing to count — the tree is one project at a time, and how many
        // files are in it is exactly what the reader opened the tab to find out.
        Section::Files => 0,
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
