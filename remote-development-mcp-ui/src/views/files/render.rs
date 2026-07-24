use dioxus::prelude::*;
use rest_api_shared::{
    FileContentResponse, RepoModel, FILE_KIND_BROWSER, FILE_KIND_HTML, FILE_KIND_IMAGE,
    FILE_KIND_MARKDOWN, FILE_KIND_PDF, FILE_KIND_TEXT, FILE_KIND_TOO_BIG,
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

    // The framed kinds — an html page and a pdf — are shown in one column of a
    // split view, and neither was laid out for that, so both get a way out to a
    // tab of their own.
    let is_framed = matches!(
        content.as_ref(),
        Some(Ok(content)) if is_framed_kind(content.kind.as_str())
    );

    // The same url the iframe is pointed at, so "open" shows exactly what the
    // pane is showing rather than a second rendering of it.
    let open_url = selected
        .as_ref()
        .map(|path| crate::api::files::raw_url(&repo, path))
        .unwrap_or_default();

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
                            // Remembered before the state is touched, so a
                            // reload comes back to the project being read
                            // rather than to the first one configured.
                            onchange: move |evt| {
                                let picked = evt.value();
                                crate::web::set_selected_repo(&picked);
                                cs.write().select_repo(picked);
                            },
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
                        if is_framed {
                            div { class: "spacer" }
                            a {
                                class: "viewer-toggle",
                                href: "{open_url}",
                                target: "_blank",
                                "open full screen"
                            }
                        }
                    }
                }
                div { class: "viewer-body", {viewer} }
            }
        }
    }
}

/// The kinds the console does not draw itself — it points a frame at the raw
/// endpoint and lets the browser render them.
///
/// An html page and a pdf because the browser has a viewer for each; `browser`
/// because the file is past the size worth decoding, and streaming it into a
/// frame beats carrying it through json and into the dom in one piece.
fn is_framed_kind(kind: &str) -> bool {
    kind == FILE_KIND_HTML || kind == FILE_KIND_PDF || kind == FILE_KIND_BROWSER
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

    // Only for a plain text file: on markdown the `html` slot holds the rendered
    // document, not a highlighted copy of the source, so reading it here would
    // put the rendering back on screen the moment someone asked for the source.
    if kind == FILE_KIND_TEXT {
        if let Some(html) = content.html.as_ref() {
            return rsx! {
                // Injected markup again, and safe for the same kind of reason as
                // the markdown above: it is generated from this file by the
                // server's highlighter, which escapes the source as it
                // classifies it. It describes the file — it is not the file.
                pre {
                    class: "viewer-text viewer-code",
                    dangerous_inner_html: "{html}",
                }
            };
        }
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

    if is_framed_kind(kind) {
        return rsx! {
            // Deliberately not sandboxed. A preview has to show the page as it
            // actually behaves — script, fetch, storage and all — and every
            // partial sandbox breaks something a real page needs.
            //
            // This is a development console for the repositories on this machine,
            // reached over the local network, so the page in the frame is the
            // reader's own working copy rather than anything hostile. Under a
            // different exposure this is the line to reconsider: a sandbox here
            // would need `allow-scripts` WITHOUT `allow-same-origin`, since the
            // two together let the framed document strip the sandbox itself.
            iframe {
                class: "viewer-frame",
                src: "{raw_url}",
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
