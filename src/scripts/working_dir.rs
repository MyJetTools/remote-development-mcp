use std::{path::PathBuf, sync::Arc};

use crate::repo::RepoContext;

/// Resolves the folder an action runs in.
///
/// Every tool that *executes* something does so inside some directory. That
/// directory is the repository root by default, and `path` shifts it — which is
/// what makes a root holding many independent git repositories usable: point the
/// endpoint at the parent folder, and `path: "my-ssh"` runs the action inside
/// that library as if it were the root.
///
/// The shift goes through the ordinary path confinement, so it can only ever
/// move *within* the root — `path: "../elsewhere"` is refused like any other
/// escape.
pub fn resolve_working_dir(repo: &Arc<RepoContext>, path: Option<&str>) -> Result<PathBuf, String> {
    let resolved = match path {
        Some(path) => repo.resolve_path(path)?,
        None => repo.root().to_path_buf(),
    };

    if !resolved.is_dir() {
        return Err(format!(
            "'{}' is not a directory inside the repository",
            repo.to_relative(&resolved)
        ));
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    #[tokio::test]
    async fn no_path_means_the_root() {
        let repo = build_test_repo("workdir_root", TestRepoOptions::default()).await;

        assert_eq!(resolve_working_dir(&repo, None).unwrap(), repo.root());
    }

    #[tokio::test]
    async fn a_path_shifts_into_the_subfolder() {
        let repo = build_test_repo("workdir_shift", TestRepoOptions::default()).await;
        std::fs::create_dir_all(repo.root().join("my-ssh")).unwrap();

        assert_eq!(
            resolve_working_dir(&repo, Some("my-ssh")).unwrap(),
            repo.root().join("my-ssh")
        );
    }

    #[tokio::test]
    async fn a_file_is_not_a_working_directory() {
        let repo = build_test_repo("workdir_file", TestRepoOptions::default()).await;
        std::fs::write(repo.root().join("notes.txt"), b"x").unwrap();

        assert!(resolve_working_dir(&repo, Some("notes.txt")).is_err());
    }

    #[tokio::test]
    async fn it_can_not_be_used_to_leave_the_root() {
        let repo = build_test_repo("workdir_escape", TestRepoOptions::default()).await;

        assert!(resolve_working_dir(&repo, Some("../..")).is_err());
        assert!(resolve_working_dir(&repo, Some("/etc")).is_err());
    }

    #[tokio::test]
    async fn a_missing_folder_is_reported_rather_than_silently_using_the_root() {
        let repo = build_test_repo("workdir_missing", TestRepoOptions::default()).await;

        assert!(resolve_working_dir(&repo, Some("not-there")).is_err());
    }
}
