use dioxus::prelude::*;
use rest_api_shared::{
    FileContentResponse, RepoModel, FILE_KIND_HTML, FILE_KIND_IMAGE, FILE_KIND_MARKDOWN,
    FILE_KIND_TEXT, FILE_KIND_TOO_BIG,
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
    let show_source = cs_ra.show_source;

    let content = selected
        .as_ref()
        .map(|path| get_content(cs, &cs_ra, &repo, path));

    // Only markdown has two ways of being read, so only markdown gets the
    // switch.
    let is_markdown =
        matches!(content.as_ref(), Some(Ok(content)) if content.kind == FILE_KIND_MARKDOWN);

    let viewer = match content {
        Some(Ok(content)) => render_content(&repo, content, show_source),
        Some(Err(note)) => note,
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
                        select {
                            class: "tree-repo-select",
                            value: "{repo}",
                            onchange: move |evt| cs.write().select_repo(evt.value()),
                            for project in repos.iter() {
                                option {
                                    key: "{project.name}",
                                    value: "{project.name}",
                                    "{project.name}"
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
                        if is_markdown {
                            div { class: "spacer" }
                            button {
                                class: "viewer-toggle",
                                onclick: move |_| cs.write().toggle_source(),
                                if show_source { "rendered" } else { "source" }
                            }
                        }
                    }
                }
                div { class: "viewer-body", {viewer} }
            }
        }
    }
}

fn render_content(repo: &str, content: &FileContentResponse, show_source: bool) -> Element {
    let kind = content.kind.as_str();

    if kind == FILE_KIND_MARKDOWN && !show_source {
        // `None` only if the server contradicted itself — an empty page is the
        // honest reading of "markdown, with no markup".
        let html = content.html.clone().unwrap_or_default();

        return rsx! {
            // The one place this console injects markup it did not write. It is
            // safe because of what produced the string, not because of anything
            // here: the server renders it with markdown's raw-html passthrough
            // switched off and every non-http destination neutralised, so there
            // is no script and no `javascript:` href left to inject. Point this
            // at anything else and that stops being true.
            div { class: "viewer-markdown", dangerous_inner_html: "{html}" }
        };
    }

    if kind == FILE_KIND_TEXT || kind == FILE_KIND_MARKDOWN {
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
