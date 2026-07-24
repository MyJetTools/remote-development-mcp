use dioxus::prelude::*;
use dioxus_utils::RenderState;
use rest_api_shared::{FileContentResponse, ListFolderResponse, RepoModel};

use super::FilesState;

/// The icons that actually exist under `public/assets/file-types`, keyed by
/// extension.
///
/// An explicit list rather than "try `<ext>.svg` and hope": a missing file
/// renders as a broken-image glyph, which looks like a bug in the tree rather
/// than like an extension nobody has drawn yet. Adding an icon is adding its
/// file and one line here.
const FILE_ICONS: [&str; 5] = ["html", "md", "pdf", "rs", "toml"];

const ICON_DIR: &str = "/assets/file-types";

/// Which project the tree is showing.
///
/// Falls back to the first configured project instead of writing a default into
/// the state during render — a render must not write, and there is nothing to
/// remember until the reader picks something themselves.
pub fn effective_repo(cs_ra: &FilesState, repos: &[RepoModel]) -> Option<String> {
    if let Some(repo) = cs_ra.repo.as_ref() {
        // A project can disappear from the settings between reloads; falling
        // through to the first one beats showing a tree of errors.
        if repos.iter().any(|itm| &itm.name == repo) {
            return Some(repo.clone());
        }
    }

    // Nothing picked in this session yet — which is every reload — so come back
    // to whatever was being read last time. Checked against the projects the
    // server actually serves, because the stored name can be one that has since
    // been renamed or dropped from the settings.
    if let Some(stored) = crate::web::get_selected_repo() {
        if repos.iter().any(|itm| itm.name == stored) {
            return Some(stored);
        }
    }

    repos.first().map(|repo| repo.name.clone())
}

pub fn file_icon(name: &str) -> String {
    match extension(name) {
        Some(extension) if FILE_ICONS.contains(&extension.as_str()) => {
            format!("{}/{}.svg", ICON_DIR, extension)
        }
        _ => format!("{}/file.svg", ICON_DIR),
    }
}

pub fn folder_icon(expanded: bool) -> String {
    if expanded {
        format!("{}/folder-open.svg", ICON_DIR)
    } else {
        format!("{}/folder.svg", ICON_DIR)
    }
}

fn extension(name: &str) -> Option<String> {
    let (stem, extension) = name.rsplit_once('.')?;

    // `.gitignore` is a name, not an extension of nothing.
    if stem.is_empty() {
        return None;
    }

    Some(extension.to_lowercase())
}

/// One folder's listing, loading it the first time it is asked for.
///
/// Folders are fetched one level at a time, when opened — a repository this
/// server hosts can be a monorepo, and pulling the whole tree to draw the first
/// row would cost tens of thousands of entries nobody has looked at.
pub fn get_folder<'s>(
    mut cs: Signal<FilesState>,
    cs_ra: &'s FilesState,
    repo: &str,
    path: &str,
) -> Result<&'s ListFolderResponse, Element> {
    let state = match cs_ra.folder(path) {
        Some(state) => state.as_ref(),
        None => &RenderState::None,
    };

    match state {
        RenderState::None => {
            let repo = repo.to_string();
            let path = path.to_string();

            spawn(async move {
                cs.write().set_folder_loading(&path);

                // The root is keyed by the empty string here and asked for as
                // "no path" over the wire — the same folder either way.
                let requested = if path.is_empty() {
                    None
                } else {
                    Some(path.clone())
                };

                match crate::api::files::list_folder(repo, requested).await {
                    Ok(listing) => cs.write().set_folder_loaded(&path, listing),
                    Err(err) => cs.write().set_folder_error(&path, err.to_string()),
                }
            });

            Err(render_tree_note("loading…", false))
        }
        RenderState::Loading => Err(render_tree_note("loading…", false)),
        RenderState::Loaded(listing) => Ok(listing),
        RenderState::Error(err) => Err(render_tree_note(err.as_str(), true)),
    }
}

/// The selected file's kind and text.
pub fn get_content<'s>(
    mut cs: Signal<FilesState>,
    cs_ra: &'s FilesState,
    repo: &str,
    path: &str,
) -> Result<&'s FileContentResponse, Element> {
    match cs_ra.content.as_ref() {
        RenderState::None => {
            let repo = repo.to_string();
            let path = path.to_string();

            spawn(async move {
                cs.write().content.set_loading();

                match crate::api::files::get_content(repo, path).await {
                    Ok(content) => cs.write().content.set_value(content),
                    Err(err) => cs.write().content.set_error(err.to_string()),
                }
            });

            Err(render_viewer_note("loading…", false))
        }
        RenderState::Loading => Err(render_viewer_note("loading…", false)),
        RenderState::Loaded(content) => Ok(content),
        RenderState::Error(err) => Err(render_viewer_note(err.as_str(), true)),
    }
}

fn render_tree_note(text: &str, failed: bool) -> Element {
    let class = if failed {
        "tree-note failed"
    } else {
        "tree-note"
    };

    rsx! {
        div { class, "{text}" }
    }
}

fn render_viewer_note(text: &str, failed: bool) -> Element {
    let class = if failed {
        "viewer-note failed"
    } else {
        "viewer-note"
    };

    rsx! {
        div { class, "{text}" }
    }
}

/// `1.2 KB`, `340 B` — enough to tell a stub from a generated file, which is
/// all the tree needs a size for.
pub fn render_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;

    let bytes = bytes as f64;

    if bytes < KB {
        return format!("{} B", bytes as u64);
    }

    if bytes < MB {
        return format!("{:.1} KB", bytes / KB);
    }

    format!("{:.1} MB", bytes / MB)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_the_generic_icon_for_an_extension_nobody_has_drawn() {
        assert_eq!(file_icon("Cargo.toml"), "/assets/file-types/toml.svg");
        assert_eq!(file_icon("main.RS"), "/assets/file-types/rs.svg");

        assert_eq!(file_icon("build.sh"), "/assets/file-types/file.svg");
        assert_eq!(file_icon("Makefile"), "/assets/file-types/file.svg");
        assert_eq!(file_icon(".gitignore"), "/assets/file-types/file.svg");
    }

    #[test]
    fn renders_a_size_a_human_can_read() {
        assert_eq!(render_size(340), "340 B");
        assert_eq!(render_size(1536), "1.5 KB");
        assert_eq!(render_size(3 * 1024 * 1024), "3.0 MB");
    }
}
