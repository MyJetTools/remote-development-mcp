use dioxus::prelude::*;
use rest_api_shared::HistoryEntryModel;
use rust_extensions::date_time::TimeZone;

use super::render_precise_duration;

#[component]
pub fn HistoryPanel(history: Vec<HistoryEntryModel>, tz: TimeZone) -> Element {
    rsx! {
        div { class: "panel",
            div { class: "panel-head",
                "History"
                span { class: "panel-count", "{history.len()}" }
            }

            div { class: "panel-body",
            if history.is_empty() {
                div { class: "panel-empty", "nothing yet" }
            } else {
                    table { class: "grid",
                        tbody {
                        for (index , entry) in history.iter().enumerate() {
                            {
                                // A panic is the one thing here nobody should have
                                // to spot — the whole row carries the colour.
                                let row_class = format!("row-{}", entry.kind);
                                let kind_class = format!("kind kind-{}", entry.kind);
                                let duration = match entry.duration_sec {
                                    Some(seconds) => render_precise_duration(seconds),
                                    None => "—".to_string(),
                                };
                                // Localised here from the UTC instant rather than
                                // shown as the server's own `time_of_day`.
                                let time = crate::time::local_time_of_day(entry.moment, tz);
                                let when = crate::time::local_date_time(entry.moment, tz);

                                rsx! {
                                    tr { key: "{index}-{entry.moment}", class: "{row_class}",
                                        td { class: "dim nowrap", title: "{when}", "{time}" }
                                        td { class: "{kind_class} nowrap", "{entry.kind}" }
                                        td { class: "dim nowrap", "{entry.repo}" }
                                        td { class: "nowrap", "{entry.subject}" }
                                        td { class: "dim nowrap", "{duration}" }
                                        td { class: "detail-cell truncate", title: "{entry.detail}", "{entry.detail}" }
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
}
