use dioxus::prelude::*;
use rest_api_shared::SessionModel;

use super::render_duration;

#[component]
pub fn SessionsPanel(sessions: Vec<SessionModel>) -> Element {
    rsx! {
        div { class: "panel",
            div { class: "panel-head",
                "Sessions"
                span { class: "panel-count", "{sessions.len()}" }
            }

            if sessions.is_empty() {
                div { class: "panel-empty", "nobody is connected" }
            } else {
                table { class: "grid",
                    thead {
                        tr {
                            th { "country" }
                            th { "ip" }
                            th { "client" }
                            th { "proto" }
                            th { "endpoint" }
                            th { "session id" }
                            th { "connected" }
                            th { "last seen" }
                        }
                    }
                    tbody {
                        for session in sessions {
                            {
                                let country = session.country.clone().unwrap_or_else(|| "—".to_string());
                                let client = session.client.clone().unwrap_or_else(|| "—".to_string());
                                let connected = render_duration(session.age_sec);
                                // Read live from the middleware on every poll, so
                                // this counts up while a session sits idle and
                                // drops back to zero the moment anything arrives —
                                // a ping included.
                                let last_seen = render_duration(session.idle_sec);

                                rsx! {
                                    tr { key: "{session.endpoint}/{session.session_id}",
                                        td { class: "nowrap", "{country}" }
                                        td { class: "nowrap", "{session.ip}" }
                                        td { class: "truncate", "{client}" }
                                        td { class: "dim nowrap", "{session.protocol_version}" }
                                        td { class: "nowrap", "{session.endpoint}" }
                                        td { class: "dim truncate", "{session.session_id}" }
                                        td { class: "dim nowrap", "{connected} ago" }
                                        td { class: "dim nowrap", "{last_seen} ago" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
