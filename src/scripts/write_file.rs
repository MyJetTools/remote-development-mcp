use std::sync::Arc;

use crate::{audit::AuditMutation, repo::RepoContext};

pub async fn write_file(
    repo: &Arc<RepoContext>,
    path: &str,
    content: &str,
    create_dirs: bool,
) -> Result<usize, String> {
    let resolved = repo.resolve_writable_path(path)?;

    if resolved.is_dir() {
        return Err(format!(
            "'{}' is a directory and can not be overwritten with a file",
            repo.to_relative(&resolved)
        ));
    }

    if let Some(folder) = resolved.parent() {
        if !folder.exists() {
            if !create_dirs {
                return Err(format!(
                    "Folder '{}' does not exist. Pass create_dirs=true to create it",
                    repo.to_relative(folder)
                ));
            }

            tokio::fs::create_dir_all(folder).await.map_err(|err| {
                format!(
                    "Can not create folder '{}'. Err: {}",
                    repo.to_relative(folder),
                    err
                )
            })?;
        }
    }

    tokio::fs::write(&resolved, content.as_bytes())
        .await
        .map_err(|err| {
            format!(
                "Can not write '{}'. Err: {}",
                repo.to_relative(&resolved),
                err
            )
        })?;

    let bytes = content.len();

    repo.audit
        .mutation(AuditMutation {
            repo: &repo.name,
            action: "write_file",
            target: &repo.to_relative(&resolved),
            detail: Some(format!("{} bytes", bytes)),
        })
        .await;

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    #[tokio::test]
    async fn a_write_is_recorded_in_the_journal_when_it_is_on() {
        let audit_path = std::env::temp_dir()
            .join("remote-development-mcp-tests-audit")
            .join("write_file_mutation.log");
        let _ = std::fs::remove_file(&audit_path);

        let repo = build_test_repo(
            "audit_write_file",
            TestRepoOptions {
                audit_path: Some(audit_path.clone()),
                ..Default::default()
            },
        )
        .await;

        write_file(&repo, "src/new.rs", "fn main() {}", true)
            .await
            .unwrap();

        let content = tokio::fs::read_to_string(&audit_path).await.unwrap();
        let record: serde_json::Value =
            serde_json::from_str(content.lines().next().unwrap()).unwrap();

        assert_eq!(record["event"], "mutation");
        assert_eq!(record["action"], "write_file");
        assert_eq!(record["target"], "src/new.rs");

        let _ = std::fs::remove_file(&audit_path);
    }

    #[tokio::test]
    async fn a_write_records_nothing_when_the_journal_is_off() {
        let repo = build_test_repo("audit_write_off", TestRepoOptions::default()).await;

        // Disabled journal — the call must simply succeed and write nothing.
        write_file(&repo, "src/new.rs", "x", true).await.unwrap();
    }
}
