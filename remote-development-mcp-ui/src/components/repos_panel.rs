use dioxus::prelude::*;
use rest_api_shared::RepoModel;

#[component]
pub fn ReposPanel(repos: Vec<RepoModel>) -> Element {
    rsx! {
        div { class: "panel",
            div { class: "panel-head",
                "Repositories"
                span { class: "panel-count", "{repos.len()}" }
            }
            div { class: "repo-cards",
                for repo in repos {
                    div { class: "repo-card", key: "{repo.mcp_path}",
                        div { class: "name", "{repo.name}" }
                        div { class: "path", "{repo.mcp_path}" }
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
