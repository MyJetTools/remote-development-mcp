use std::sync::Arc;

use crate::{audit::AuditMutation, repo::RepoContext};

#[derive(Debug)]
pub struct MovePathResult {
    pub from: String,
    pub to: String,
}

/// Both ends are resolved through the repository confinement, so a rename can
/// not be used to walk a file out of the tree.
pub async fn move_path(
    repo: &Arc<RepoContext>,
    from: &str,
    to: &str,
) -> Result<MovePathResult, String> {
    // Resolve leaves as themselves: moving a symlink should move the link, and
    // a case-only rename on a case-insensitive filesystem must not be mistaken
    // for the destination already existing.
    let resolved_from = repo.resolve_symlink_path(from)?;
    let resolved_to = repo.resolve_symlink_path(to)?;

    if tokio::fs::symlink_metadata(&resolved_from).await.is_err() {
        return Err(format!(
            "'{}' does not exist",
            repo.to_relative(&resolved_from)
        ));
    }

    // On a case-insensitive filesystem `Foo.rs` and `foo.rs` are the same file,
    // so a plain exists() check would refuse a case-only rename that rename(2)
    // handles fine. Allow the move when the destination is really just the
    // source under a different spelling.
    if resolved_to != resolved_from && is_existing_other_path(&resolved_from, &resolved_to) {
        return Err(format!(
            "'{}' already exists. Delete it first if you meant to replace it",
            repo.to_relative(&resolved_to)
        ));
    }

    if let Some(folder) = resolved_to.parent() {
        tokio::fs::create_dir_all(folder).await.map_err(|err| {
            format!(
                "Can not create folder '{}'. Err: {}",
                repo.to_relative(folder),
                err
            )
        })?;
    }

    tokio::fs::rename(&resolved_from, &resolved_to)
        .await
        .map_err(|err| {
            format!(
                "Can not move '{}' to '{}'. Err: {}",
                repo.to_relative(&resolved_from),
                repo.to_relative(&resolved_to),
                err
            )
        })?;

    let from_relative = repo.to_relative(&resolved_from);
    let to_relative = repo.to_relative(&resolved_to);

    repo.audit
        .mutation(AuditMutation {
            repo: &repo.name,
            action: "move_path",
            target: &from_relative,
            detail: Some(format!("-> {}", to_relative)),
        })
        .await;

    Ok(MovePathResult {
        from: repo.to_relative(&resolved_from),
        to: repo.to_relative(&resolved_to),
    })
}

/// True when `to` exists and is a genuinely different file from `from` — as
/// opposed to the same file reached by a case-only difference on a
/// case-insensitive filesystem, which is what a case-only rename looks like.
fn is_existing_other_path(from: &std::path::Path, to: &std::path::Path) -> bool {
    if std::fs::symlink_metadata(to).is_err() {
        return false;
    }

    // Both exist; if they canonicalize to the same path they are the same file
    // (a case-only spelling), so the move is allowed to proceed.
    match (from.canonicalize(), to.canonicalize()) {
        (Ok(from_real), Ok(to_real)) => from_real != to_real,
        // If either can not be canonicalized (e.g. a dangling link), fall back
        // to treating a present destination as a real collision.
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    #[tokio::test]
    async fn refuses_to_overwrite_a_genuinely_different_destination() {
        let repo = build_test_repo("move_collision", TestRepoOptions::default()).await;

        std::fs::write(repo.root().join("a.txt"), b"a").unwrap();
        std::fs::write(repo.root().join("b.txt"), b"b").unwrap();

        let err = move_path(&repo, "a.txt", "b.txt").await.unwrap_err();

        assert!(err.contains("already exists"), "{}", err);
    }

    #[tokio::test]
    async fn allows_a_case_only_rename() {
        let repo = build_test_repo("move_case_only", TestRepoOptions::default()).await;

        std::fs::write(repo.root().join("Foo.rs"), b"content").unwrap();

        // On a case-insensitive filesystem the destination "exists" as a case
        // variant of the source; the rename must still be allowed. On a
        // case-sensitive one there is simply no collision. Either way it works.
        move_path(&repo, "Foo.rs", "foo.rs").await.unwrap();

        assert_eq!(
            std::fs::read_to_string(repo.root().join("foo.rs")).unwrap(),
            "content"
        );
    }
}
