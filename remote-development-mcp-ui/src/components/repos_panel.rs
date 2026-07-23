use dioxus::prelude::*;
use rest_api_shared::RepoModel;

#[component]
pub fn ReposPanel(repos: Vec<RepoModel>) -> Element {
    rsx! {
        div { class: "panel",
            div { class: "panel-head",
                "Projects"
                span { class: "panel-count", "{repos.len()}" }
            }
            div { class: "repo-cards",
                for repo in repos {
                    div { class: "repo-card", key: "{repo.name}",
                        div { class: "name", "{repo.name}" }
                        // A project can be served by several urls, or by none —
                        // saying so is the point, since a project no endpoint
                        // exposes is unreachable and looks fine otherwise.
                        div { class: "path",
                            if repo.endpoints.is_empty() {
                                "not exposed by any endpoint"
                            } else {
                                "{repo.endpoints.join(\", \")}"
                            }
                        }
                        div { class: "root", "{repo.root}" }
                        if repo.running_jobs > 0 {
                            div { class: "busy", "{repo.running_jobs} running" }
                        }
                    }
                }
            }
        }
    }
}
