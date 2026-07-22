use std::sync::Arc;

use crate::{audit::AuditMutation, repo::RepoContext};

pub struct DeletePathResult {
    pub path: String,
    pub was_directory: bool,
}

/// Off unless the repository turns it on.
///
/// It is the one irreversible tool here — everything else can be undone by
/// writing the file back or by git — so it stays behind an explicit
/// per-repository flag rather than the command allowlist, which does not govern
/// file tools.
pub async fn delete_path(
    repo: &Arc<RepoContext>,
    path: &str,
    recursive: bool,
) -> Result<DeletePathResult, String> {
    if !repo.allow_delete {
        return Err(format!(
            "Deleting is turned off for repository '{}'. Set allow_delete: true for it in the \
             settings if this is really wanted",
            repo.name
        ));
    }

    // Resolve the leaf as itself, so a symlink is unlinked rather than followed
    // to its target — and so a dangling link stays removable.
    let resolved = repo.resolve_symlink_path(path)?;

    if resolved == repo.root() {
        return Err("Refusing to delete the repository root".to_string());
    }

    let metadata = tokio::fs::symlink_metadata(&resolved)
        .await
        .map_err(|err| {
            format!(
                "Can not read '{}'. Err: {}",
                repo.to_relative(&resolved),
                err
            )
        })?;

    // A symlink — even one pointing at a directory — is removed as the link, not
    // followed. Otherwise `delete_path` would destroy the target and leave the
    // link behind.
    let (was_directory, kind) = if metadata.file_type().is_symlink() {
        remove_file(repo, &resolved).await?;
        (false, "symlink")
    } else if metadata.is_dir() {
        if !recursive {
            let mut reader = tokio::fs::read_dir(&resolved).await.map_err(|err| {
                format!(
                    "Can not read '{}'. Err: {}",
                    repo.to_relative(&resolved),
                    err
                )
            })?;

            if reader.next_entry().await.ok().flatten().is_some() {
                return Err(format!(
                    "'{}' is not empty. Pass recursive=true to delete it with everything inside",
                    repo.to_relative(&resolved)
                ));
            }
        }

        tokio::fs::remove_dir_all(&resolved).await.map_err(|err| {
            format!(
                "Can not delete '{}'. Err: {}",
                repo.to_relative(&resolved),
                err
            )
        })?;

        (true, "directory")
    } else {
        remove_file(repo, &resolved).await?;
        (false, "file")
    };

    let relative = repo.to_relative(&resolved);

    repo.audit
        .mutation(AuditMutation {
            repo: &repo.name,
            action: "delete_path",
            target: &relative,
            detail: Some(kind.to_string()),
        })
        .await;

    Ok(DeletePathResult {
        path: relative,
        was_directory,
    })
}

async fn remove_file(repo: &Arc<RepoContext>, resolved: &std::path::Path) -> Result<(), String> {
    tokio::fs::remove_file(resolved).await.map_err(|err| {
        format!(
            "Can not delete '{}'. Err: {}",
            repo.to_relative(resolved),
            err
        )
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    #[tokio::test]
    async fn deleting_a_symlink_removes_the_link_not_its_target() {
        let repo = build_test_repo(
            "delete_symlink",
            TestRepoOptions {
                allow_delete: true,
                ..Default::default()
            },
        )
        .await;

        let target = repo.root().join("real.txt");
        std::fs::write(&target, b"keep me").unwrap();
        std::os::unix::fs::symlink(&target, repo.root().join("link.txt")).unwrap();

        let result = delete_path(&repo, "link.txt", false).await.unwrap();

        assert!(!result.was_directory);
        // The link is gone; the file it pointed at is untouched.
        assert!(!repo.root().join("link.txt").exists());
        assert!(target.exists());
    }

    #[tokio::test]
    async fn a_dangling_symlink_can_be_deleted() {
        let repo = build_test_repo(
            "delete_dangling",
            TestRepoOptions {
                allow_delete: true,
                ..Default::default()
            },
        )
        .await;

        std::os::unix::fs::symlink(
            repo.root().join("nonexistent"),
            repo.root().join("dangling"),
        )
        .unwrap();

        // resolve_path would refuse this as a broken link; resolve_symlink_path
        // lets it be removed as the link it is.
        delete_path(&repo, "dangling", false).await.unwrap();

        assert!(tokio::fs::symlink_metadata(repo.root().join("dangling"))
            .await
            .is_err());
    }
}
