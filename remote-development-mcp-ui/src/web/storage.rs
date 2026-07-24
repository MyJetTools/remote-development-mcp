//! The handful of choices that outlive a reload.
//!
//! `localStorage` rather than the url: which project the tree is on is a
//! preference of the reader, not part of the address of a page — putting it in
//! the url would make every link to `/files` carry someone else's project.

/// Which project the file tree was last opened on.
const SELECTED_REPO_KEY: &str = "rdm.files.repo";

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
