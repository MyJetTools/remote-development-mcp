use std::sync::Arc;

use crate::{audit::AuditMutation, repo::RepoContext};

pub struct EditFileRequest<'s> {
    pub path: &'s str,
    pub old_string: &'s str,
    pub new_string: &'s str,
    pub replace_all: bool,
}

/// Replaces an exact substring in a file.
///
/// Refusing an ambiguous match is the whole point: an `old_string` that occurs
/// three times almost always means the caller was thinking of one specific
/// place, and silently editing the first one is how a refactor quietly corrupts
/// a file.
pub async fn edit_file(
    repo: &Arc<RepoContext>,
    request: EditFileRequest<'_>,
) -> Result<usize, String> {
    if request.old_string.is_empty() {
        return Err(
            "old_string is empty. Use write_file to create or replace a whole file".to_string(),
        );
    }

    if request.old_string == request.new_string {
        return Err(
            "old_string and new_string are identical, so there is nothing to do".to_string(),
        );
    }

    let resolved = repo.resolve_writable_path(request.path)?;

    let content = tokio::fs::read(&resolved).await.map_err(|err| {
        format!(
            "Can not read '{}'. Err: {}",
            repo.to_relative(&resolved),
            err
        )
    })?;

    let content = String::from_utf8(content).map_err(|_| {
        format!(
            "'{}' is not valid UTF-8, so it can not be edited as text",
            repo.to_relative(&resolved)
        )
    })?;

    let occurrences = content.matches(request.old_string).count();

    if occurrences == 0 {
        return Err(format!(
            "old_string was not found in '{}'",
            repo.to_relative(&resolved)
        ));
    }

    if occurrences > 1 && !request.replace_all {
        return Err(format!(
            "old_string occurs {} times in '{}'. Extend it until it is unique, or pass \
             replace_all=true to replace every occurrence",
            occurrences,
            repo.to_relative(&resolved)
        ));
    }

    let updated = if request.replace_all {
        content.replace(request.old_string, request.new_string)
    } else {
        content.replacen(request.old_string, request.new_string, 1)
    };

    tokio::fs::write(&resolved, updated.as_bytes())
        .await
        .map_err(|err| {
            format!(
                "Can not write '{}'. Err: {}",
                repo.to_relative(&resolved),
                err
            )
        })?;

    let replaced = if request.replace_all { occurrences } else { 1 };

    repo.audit
        .mutation(AuditMutation {
            repo: &repo.name,
            action: "edit_file",
            target: &repo.to_relative(&resolved),
            detail: Some(format!("{} replacement(s)", replaced)),
        })
        .await;

    Ok(replaced)
}
