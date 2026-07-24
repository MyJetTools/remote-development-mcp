use std::collections::{HashMap, HashSet};

use dioxus_utils::DataState;
use rest_api_shared::{FileContentResponse, ListFolderResponse, RepoModel};

/// The path of the project root inside this state. The server takes an empty
/// `path` to mean the root, so the same string keys the root's own listing.
pub const ROOT_PATH: &str = "";

/// The file browser, for one project at a time.
///
/// The tree is kept flat — a map from folder path to that folder's listing —
/// rather than as nested nodes. A nested tree would mean walking it to find the
/// folder a click landed on and rebuilding the branch above it on every change;
/// flat, opening a folder is one insert and rendering is a lookup per level.
///
/// Which file is *selected* is deliberately not here: it lives in the url, so a
/// file can be linked to and comes back after a reload. What is below is the
/// state that has no business being in an address.
#[derive(Default)]
pub struct FilesState {
    /// Which project is being browsed. `None` only when none is configured.
    pub repo: Option<String>,
    /// One entry per folder that has been opened at least once. Kept when the
    /// folder is collapsed, so re-opening it is instant and does not re-fetch.
    folders: HashMap<String, DataState<ListFolderResponse>>,
    /// Which folders are drawn open. Separate from `folders` for exactly that
    /// reason: "loaded" and "open" are different questions. Mirrored into
    /// storage on every change, so a reload comes back to the same tree.
    expanded: HashSet<String>,
    pub content: DataState<FileContentResponse>,
    /// Which file `content` holds. The selected path lives in the url and can
    /// change without this state being touched, so the two are compared rather
    /// than assumed to agree — without it the pane would show the previous
    /// file's text under the new file's name.
    content_path: Option<String>,
    /// Markdown is shown rendered; this is the reader asking for the file it was
    /// rendered from. Kept across files on purpose — someone reading sources
    /// stays reading sources.
    pub show_source: bool,
}

impl FilesState {
    /// The tree as it should look the moment the page has loaded.
    ///
    /// Decided here rather than mended afterwards because a render must not
    /// write: by the time the first row is drawn, the project and the open
    /// folders are already what they should be.
    pub fn new(repos: &[RepoModel], selected: &str) -> Self {
        let repo = restore_repo(repos);

        let mut expanded = match repo.as_deref() {
            Some(repo) => crate::web::get_expanded_folders(repo),
            None => HashSet::new(),
        };

        // Whatever the url points at has to be reachable, so every folder on the
        // way down to it is opened whether or not it was open last time. This is
        // what makes a link to a file land on the file instead of on a collapsed
        // tree with the file somewhere inside it.
        expanded.extend(ancestors_of(selected));

        Self {
            repo,
            expanded,
            ..Default::default()
        }
    }

    /// Switches projects, dropping everything that belonged to the old one —
    /// paths from one repository mean nothing in another. The open folders are
    /// not cleared but swapped for the ones this project was left with.
    pub fn select_repo(&mut self, repo: String) {
        if self.repo.as_deref() == Some(repo.as_str()) {
            return;
        }

        self.expanded = crate::web::get_expanded_folders(&repo);
        self.repo = Some(repo);
        self.folders.clear();
        self.content = DataState::default();
        self.content_path = None;
    }

    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// A second click closes what the first opened, and drops what was loaded
    /// for it.
    ///
    /// Dropping it costs a fetch when the folder is opened again, and buys the
    /// only refresh gesture the tree has: these are working copies being edited
    /// while the console is open, so a listing kept forever would show a folder
    /// as it was when it was first opened and never say so. Close and re-open
    /// is how you ask again.
    pub fn toggle(&mut self, path: &str) {
        if self.expanded.remove(path) {
            self.folders.remove(path);
        } else {
            self.expanded.insert(path.to_string());
        }

        self.persist_expanded();
    }

    pub fn folder(&self, path: &str) -> Option<&DataState<ListFolderResponse>> {
        self.folders.get(path)
    }

    pub fn set_folder_loading(&mut self, path: &str) {
        self.folders
            .entry(path.to_string())
            .or_default()
            .set_loading();
    }

    pub fn set_folder_loaded(&mut self, path: &str, listing: ListFolderResponse) {
        self.folders
            .entry(path.to_string())
            .or_default()
            .set_value(listing);
    }

    pub fn set_folder_error(&mut self, path: &str, err: String) {
        self.folders
            .entry(path.to_string())
            .or_default()
            .set_error(err);
    }

    /// Whether `content` holds this file, rather than one read before it.
    pub fn content_is_for(&self, path: &str) -> bool {
        self.content_path.as_deref() == Some(path)
    }

    /// Claims `content` for a file and marks it in flight — both in one write,
    /// so no render sees the slot claimed while it still holds the old file.
    pub fn begin_content_load(&mut self, path: &str) {
        self.content_path = Some(path.to_string());
        self.content.set_loading();
    }

    pub fn toggle_source(&mut self) {
        self.show_source = !self.show_source;
    }

    fn persist_expanded(&self) {
        if let Some(repo) = self.repo.as_deref() {
            crate::web::set_expanded_folders(repo, &self.expanded);
        }
    }
}

/// The project to open on: the one last read, when the server still serves it.
///
/// Checked against the configured projects because the stored name can be one
/// that has since been renamed or dropped from the settings, and a tree rooted
/// at a project that is not there shows nothing but errors.
fn restore_repo(repos: &[RepoModel]) -> Option<String> {
    if let Some(stored) = crate::web::get_selected_repo() {
        if repos.iter().any(|repo| repo.name == stored) {
            return Some(stored);
        }
    }

    repos.first().map(|repo| repo.name.clone())
}

/// Every folder on the way down to a file — `a` and `a/b` for `a/b/c.rs`. The
/// file itself is not one of them, and neither is the root, which is always
/// drawn open.
fn ancestors_of(path: &str) -> Vec<String> {
    path.char_indices()
        .filter(|(_, character)| *character == '/')
        .map(|(at, _)| path[..at].to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_names_every_folder_above_it() {
        assert_eq!(
            ancestors_of("src/views/files/render.rs"),
            vec!["src", "src/views", "src/views/files"]
        );
    }

    #[test]
    fn a_file_at_the_root_names_none() {
        // The root is always drawn open, so it is never in the set.
        assert!(ancestors_of("README.md").is_empty());
        assert!(ancestors_of("").is_empty());
    }
}
