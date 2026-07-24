use dioxus::prelude::*;
use rest_api_shared::JobModel;

use crate::states::AppState;

use super::render_duration;

#[component]
pub fn JobsPanel(jobs: Vec<JobModel>) -> Element {
    let mut app_state = consume_context::<Signal<AppState>>();

    let running = jobs.iter().filter(|job| job.status == "running").count();

    rsx! {
        div { class: "panel",
            div { class: "panel-head",
                "Processes"
                span { class: "panel-count", "{running} running / {jobs.len()} total" }
            }

            div { class: "panel-body",
            if jobs.is_empty() {
                div { class: "panel-empty", "nothing has been started yet" }
            } else {
                table { class: "grid",
                    thead {
                        tr {
                            th { "status" }
                            th { "repo" }
                            th { "command" }
                            th { "cwd" }
                            th { "took" }
                            th { "left" }
                        }
                    }
                    tbody {
                        for job in jobs {
                            {
                                let status_class = format!("status-{}", status_style(&job));
                                let took = render_duration(job.duration_sec);
                                let left = match job.remaining_sec {
                                    Some(left) => render_duration(left),
                                    None => match job.exit_code {
                                        Some(code) => format!("exit {}", code),
                                        None => "—".to_string(),
                                    },
                                };
                                let repo = job.repo.clone();
                                let job_id = job.job_id.clone();
                                let command_line = job.command_line.clone();

                                rsx! {
                                    tr {
                                        key: "{job.repo}/{job.job_id}",
                                        class: "clickable",
                                        title: "open output",
                                        onclick: move |_| {
                                            app_state
                                                .write()
                                                .open_job_output(
                                                    repo.clone(),
                                                    job_id.clone(),
                                                    command_line.clone(),
                                                );
                                        },
                                        td { class: "{status_class} nowrap", "{job.status}" }
                                        td { class: "nowrap", "{job.repo}" }
                                        td { class: "truncate", "{job.command_line}" }
                                        td { class: "dim truncate", "{job.cwd}" }
                                        td { class: "nowrap", "{took}" }
                                        td { class: "dim nowrap", "{left}" }
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

/// A command which exited non-zero is a failure, however calmly it ended — the
/// status alone would call it "exited" and hide that.
fn status_style(job: &JobModel) -> &'static str {
    match job.status.as_str() {
        "running" => "running",
        "killed" => "killed",
        "timed_out" => "timed_out",
        _ => match job.exit_code {
            Some(0) => "exited",
            _ => "failed",
        },
    }
}
