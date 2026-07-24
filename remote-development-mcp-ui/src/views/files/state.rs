use std::collections::{HashMap, HashSet};

use dioxus_utils::DataState;
use rest_api_shared::{FileContentResponse, ListFolderResponse};

/// The path of the project root inside this state. The server takes an empty
/// `path` to mean the root, so the same string keys the root's own listing.
pub const ROOT_PATH: &str = "";

/// The file browser, for one project at a time.
///
/// The tree is kept flat — a map from folder path to that folder's listing —
/// rather than as nested nodes. A nested tree would mean walking it to find the
/// folder a click landed on and rebuilding the branch above it on every change;
/// flat, opening a folder is one insert and rendering is a lookup per level.
#[derive(Default)]
pub struct FilesState {
    /// Which project is being browsed. `None` until the first snapshot names
    /// one — the tree can not be loaded before there is a root to load.
    pub repo: Option<String>,
    /// One entry per folder that has been opened at least once. Kept when the
    /// folder is collapsed, so re-opening it is instant and does not re-fetch.
    folders: HashMap<String, DataState<ListFolderResponse>>,
    /// Which folders are drawn open. Separate from `folders` for exactly that
    /// reason: "loaded" and "open" are different questions.
    expanded: HashSet<String>,
    /// The file shown on the right, relative to the project root.
    pub selected: Option<String>,
    pub content: DataState<FileContentResponse>,
    /// Markdown is shown rendered; this is the reader asking for the file it was
    /// rendered from. Kept across files on purpose — someone reading sources
    /// stays reading sources.
    pub show_source: bool,
}

impl FilesState {
    /// Switches projects, dropping everything that belonged to the old one —
    /// paths from one repository mean nothing in another.
    pub fn select_repo(&mut self, repo: String) {
        if self.repo.as_deref() == Some(repo.as_str()) {
            return;
        }

        self.repo = Some(repo);
        self.folders.clear();
        self.expanded.clear();
        self.selected = None;
        self.content = DataState::default();
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
            return;
        }

        self.expanded.insert(path.to_string());
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

    /// Clicking a file both selects it and drops whatever was loaded for the
    /// previous one, so the pane can not show the old file's text under the new
    /// file's name while the request is in flight.
    pub fn select_file(&mut self, path: String) {
        self.selected = Some(path);
        self.content = DataState::default();
    }

    pub fn toggle_source(&mut self) {
        self.show_source = !self.show_source;
    }
}
