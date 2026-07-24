//! The handful of choices that outlive a reload.
//!
//! `localStorage` rather than the url: which project the tree is on is a
//! preference of the reader, not part of the address of a page — putting it in
//! the url would make every link to `/files` carry someone else's project.

use std::collections::HashSet;

/// Which project the file tree was last opened on.
const SELECTED_REPO_KEY: &str = "rdm.files.repo";

/// Per project, because a path from one repository means nothing in another —
/// one shared key would reopen a tree of folders that are not there.
const EXPANDED_KEY_PREFIX: &str = "rdm.files.expanded.";

/// The project to open the tree on, from the last time the reader picked one.
///
/// The caller checks it against the projects the server actually serves: a name
/// here can be one that has since been renamed or dropped from the settings, and
/// this module has no way to know that.
pub fn get_selected_repo() -> Option<String> {
    let stored = dioxus_utils::js::GlobalAppSettings::get_local_storage().get(SELECTED_REPO_KEY)?;

    if stored.is_empty() {
        return None;
    }

    Some(stored)
}

pub fn set_selected_repo(repo: &str) {
    dioxus_utils::js::GlobalAppSettings::get_local_storage().set(SELECTED_REPO_KEY, repo);
}

/// The folders left open in one project's tree.
///
/// Unreadable storage comes back empty rather than failing: a tree that opens
/// collapsed is a small loss, and there is nothing a reader could do about a
/// value they never wrote by hand.
pub fn get_expanded_folders(repo: &str) -> HashSet<String> {
    let stored = match dioxus_utils::js::GlobalAppSettings::get_local_storage().get(&key_of(repo)) {
        Some(stored) => stored,
        None => return HashSet::new(),
    };

    serde_json::from_str::<Vec<String>>(&stored)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

pub fn set_expanded_folders(repo: &str, folders: &HashSet<String>) {
    // Sorted so the stored value only changes when the set does — an unordered
    // dump would rewrite the key on every toggle even when nothing moved.
    let mut folders: Vec<&String> = folders.iter().collect();
    folders.sort();

    if let Ok(json) = serde_json::to_string(&folders) {
        dioxus_utils::js::GlobalAppSettings::get_local_storage().set(&key_of(repo), &json);
    }
}

fn key_of(repo: &str) -> String {
    format!("{}{}", EXPANDED_KEY_PREFIX, repo)
}
