use dioxus::prelude::*;
use rest_api_shared::FolderEntryModel;

use super::{file_icon, folder_icon, get_folder, render_size, FilesState};

/// How far one level is pushed in, in pixels.
const INDENT: usize = 14;

/// Everything inside one folder.
///
/// Split from [`FileTreeRow`] so the recursion goes through two components
/// rather than one calling itself — and so a folder's listing is fetched by
/// whoever draws its contents, which means it is fetched exactly when the
/// folder is first opened and never before.
#[component]
pub fn FolderChildren(cs: Signal<FilesState>, repo: String, path: String, depth: usize) -> Element {
    let cs_ra = cs.read();

    let listing = match get_folder(cs, &cs_ra, &repo, &path) {
        Ok(listing) => listing,
        Err(note) => {
            // Indented to sit under the folder it belongs to, rather than at the
            // left edge where it would read as the whole tree failing.
            let indent = format!("padding-left: {}px", (depth + 1) * INDENT);

            return rsx! {
                div { style: "{indent}", {note} }
            };
        }
    };

    // Cloned out before the rsx: every row hands owned values to its click
    // handler, and the read guard can not be held across them.
    let entries = listing.entries.clone();
    let truncated = listing.truncated;
    let indent = format!("padding-left: {}px", (depth + 1) * INDENT);

    drop(cs_ra);

    rsx! {
        if entries.is_empty() {
            div { style: "{indent}",
                div { class: "tree-note", "empty" }
            }
        }

        for entry in entries {
            FileTreeRow {
                key: "{entry.path}",
                cs,
                repo: repo.clone(),
                entry,
                depth,
            }
        }

        // Saying so matters: without it a folder the server cut short looks like
        // a folder that simply holds less than it does.
        if truncated {
            div { style: "{indent}",
                div { class: "tree-note failed", "…too many entries to list" }
            }
        }
    }
}

/// One row — a folder that opens and closes, or a file that can be selected.
#[component]
pub fn FileTreeRow(
    mut cs: Signal<FilesState>,
    repo: String,
    entry: FolderEntryModel,
    depth: usize,
) -> Element {
    let cs_ra = cs.read();

    let expanded = entry.is_dir && cs_ra.is_expanded(&entry.path);
    let selected = !entry.is_dir && cs_ra.selected.as_deref() == Some(entry.path.as_str());

    drop(cs_ra);

    let icon = if entry.is_dir {
        folder_icon(expanded)
    } else {
        file_icon(&entry.name)
    };

    let row_class = if selected {
        "tree-row selected"
    } else {
        "tree-row"
    };
    let indent = format!("padding-left: {}px", depth * INDENT + 6);

    let path = entry.path.clone();
    let is_dir = entry.is_dir;

    rsx! {
        div {
            class: row_class,
            style: "{indent}",
            onclick: move |_| {
                let mut w = cs.write();
                if is_dir {
                    w.toggle(&path);
                } else {
                    w.select_file(path.clone());
                }
            },

            img { class: "tree-icon", src: "{icon}" }
            span { class: "tree-name truncate", "{entry.name}" }
            if !entry.is_dir {
                span { class: "tree-size dim", "{render_size(entry.size_bytes)}" }
            }
        }

        // Only mounted while open, so collapsing a folder stops its children
        // rendering — and, for a folder never opened, nothing is ever fetched.
        if expanded {
            FolderChildren {
                cs,
                repo,
                path: entry.path.clone(),
                depth: depth + 1,
            }
        }
    }
}
