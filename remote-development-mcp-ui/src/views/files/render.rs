use dioxus::prelude::*;
use rest_api_shared::{
    FileContentResponse, RepoModel, FILE_KIND_HTML, FILE_KIND_IMAGE, FILE_KIND_TEXT,
    FILE_KIND_TOO_BIG,
};

use super::{effective_repo, get_content, render_size, FilesState, FolderChildren, ROOT_PATH};

/// The file browser: the tree on the left, whatever is selected on the right.
#[component]
pub fn RenderFiles(repos: Vec<RepoModel>) -> Element {
    let mut cs = use_signal(FilesState::default);
    let cs_ra = cs.read();

    let repo = match effective_repo(&cs_ra, &repos) {
        Some(repo) => repo,
        None => {
            return rsx! {
                div { class: "viewer-note", "no projects are configured" }
            }
        }
    };

    let selected = cs_ra.selected.clone();

    let viewer = match selected.as_ref() {
        Some(path) => match get_content(cs, &cs_ra, &repo, path) {
            Ok(content) => render_content(&repo, content),
            Err(note) => note,
        },
        None => rsx! {
            div { class: "viewer-note", "select a file" }
        },
    };

    drop(cs_ra);

    rsx! {
        div { class: "files-layout",
            div { class: "files-tree",
                // Only worth a picker when there is something to pick between —
                // a machine serving one project should not spend a row saying so.
                if repos.len() > 1 {
                    div { class: "tree-repos",
                        for project in repos.iter() {
                            {
                                let name = project.name.clone();
                                let picked = name == repo;
                                let class = if picked { "tree-repo picked" } else { "tree-repo" };

                                rsx! {
                                    button {
                                        key: "{name}",
                                        class,
                                        onclick: move |_| cs.write().select_repo(name.clone()),
                                        "{project.name}"
                                    }
                                }
                            }
                        }
                    }
                }

                // Keyed by project: switching projects mounts a fresh tree
                // rather than re-using the rows of the previous one.
                FolderChildren {
                    key: "{repo}",
                    cs,
                    repo: repo.clone(),
                    path: ROOT_PATH.to_string(),
                    depth: 0,
                }
            }

            div { class: "files-viewer",
                if let Some(path) = selected {
                    div { class: "viewer-head",
                        span { class: "truncate", "{path}" }
                    }
                }
                div { class: "viewer-body", {viewer} }
            }
        }
    }
}

fn render_content(repo: &str, content: &FileContentResponse) -> Element {
    let kind = content.kind.as_str();

    if kind == FILE_KIND_TEXT {
        // `None` only if the server contradicted itself; showing the file as
        // empty is the honest reading of "text, with no text".
        let text = content.text.clone().unwrap_or_default();

        return rsx! {
            pre { class: "viewer-text", "{text}" }
        };
    }

    let raw_url = crate::api::files::raw_url(repo, &content.path);

    if kind == FILE_KIND_IMAGE {
        return rsx! {
            div { class: "viewer-image",
                img { src: "{raw_url}", alt: "{content.path}" }
            }
        };
    }

    if kind == FILE_KIND_HTML {
        return rsx! {
            iframe {
                class: "viewer-frame",
                src: "{raw_url}",
                // Every restriction on: the page comes out of a repository this
                // console does not vouch for, and without this it would run its
                // own script against this origin — where it could call the api
                // with whatever authenticates the reader. Scripts inside the
                // preview therefore do not run; markup and styling do.
                // Quoted because dioxus has no typed `sandbox` on iframe; an
                // empty value is the attribute at its most restrictive.
                "sandbox": "",
            }
        };
    }

    let reason = if kind == FILE_KIND_TOO_BIG {
        format!(
            "{} — too large to show here",
            render_size(content.size_bytes)
        )
    } else {
        format!("binary file — {}", render_size(content.size_bytes))
    };

    rsx! {
        div { class: "viewer-note",
            div { "{reason}" }
            a { href: "{raw_url}", target: "_blank", "download" }
        }
    }
}
