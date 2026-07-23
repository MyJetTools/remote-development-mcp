use dioxus::prelude::*;
use rest_api_shared::HistoryEntryModel;

#[component]
pub fn HistoryPanel(history: Vec<HistoryEntryModel>) -> Element {
    rsx! {
        div { class: "panel history",
            div { class: "panel-head",
                "History"
                span { class: "panel-count", "{history.len()}" }
            }

            if history.is_empty() {
                div { class: "panel-empty", "nothing yet" }
            } else {
                div { class: "history-scroll",
                    table { class: "grid",
                        tbody {
                            for (index , entry) in history.iter().enumerate() {
                                {
                                    // A panic is the one thing here nobody should have
                                    // to spot — the whole row carries the colour.
                                    let row_class = format!("row-{}", entry.kind);
                                    let kind_class = format!("kind kind-{}", entry.kind);

                                    rsx! {
                                        tr { key: "{index}-{entry.moment}", class: "{row_class}",
                                            td { class: "dim nowrap", "{entry.time_of_day}" }
                                            td { class: "{kind_class} nowrap", "{entry.kind}" }
                                            td { class: "dim nowrap", "{entry.repo}" }
                                            td { class: "nowrap", "{entry.subject}" }
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
