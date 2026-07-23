use dioxus::prelude::*;
use rest_api_shared::ActionRunModel;

use super::render_duration;

#[component]
pub fn ActionsPanel(actions: Vec<ActionRunModel>) -> Element {
    // The panel costs no room when nobody is watching a build.
    if actions.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: "panel",
            div { class: "panel-head",
                "GitHub Actions"
                span { class: "panel-count", "{actions.len()}" }
            }
            table { class: "grid",
                thead {
                    tr {
                        th { "outcome" }
                        th { "repo" }
                        th { "tag" }
                        th { "workflow" }
                        th { "failed step" }
                        th { "took" }
                        th { "" }
                    }
                }
                tbody {
                    for run in actions {
                        {
                            let outcome_class = format!("status-{}", outcome_style(&run.outcome));
                            let took = render_duration(run.elapsed_sec);
                            let tag = run.tag.clone().unwrap_or_else(|| "—".to_string());
                            let failed_step = run.failed_step.clone().unwrap_or_default();

                            rsx! {
                                tr { key: "{run.run_id}",
                                    td { class: "{outcome_class} nowrap", "{run.outcome}" }
                                    td { class: "nowrap", "{run.repo}" }
                                    td { class: "nowrap", "{tag}" }
                                    td { class: "truncate", "{run.workflow}" }
                                    td { class: "status-failed truncate", "{failed_step}" }
                                    td { class: "nowrap", "{took}" }
                                    td {
                                        if let Some(url) = run.url.as_ref() {
                                            a { href: "{url}", target: "_blank", "open" }
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

fn outcome_style(outcome: &str) -> &'static str {
    match outcome {
        "success" => "exited",
        "queued" | "in_progress" => "running",
        "failure" | "timed_out" => "failed",
        _ => "killed",
    }
}
