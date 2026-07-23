use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{audit::AuditMutation, repo::RepoContext};

/// A monorepo without a workspace has one `target` per crate; a few thousand is
/// already pathological, and the cap keeps a run bounded either way.
const MAX_TARGETS: usize = 4000;

pub struct CleanCargoTargetsRequest {
    /// Subtree to clean, relative to the repository root. Defaults to the whole
    /// repository.
    pub path: Option<String>,
    /// List what would be removed without removing anything.
    pub dry_run: bool,
}

pub struct CleanedTarget {
    /// Relative to the repository root.
    pub path: String,
    pub freed_bytes: u64,
}

pub struct CleanCargoTargetsResult {
    pub targets: Vec<CleanedTarget>,
    pub total_freed_bytes: u64,
    pub dry_run: bool,
    /// True when there were more target directories than the cap allowed in one
    /// run. Run it again to continue.
    pub truncated: bool,
}

/// Finds and removes cargo `target` directories throughout the repository.
///
/// Made for a monorepo that has many independent crates rather than one cargo
/// workspace, so there is a `target` next to every `Cargo.toml`. A directory is
/// only treated as build output — and therefore removable — when it is named
/// `target` and sits beside a `Cargo.toml`; a directory that merely happens to
/// be called `target` is left alone and walked through.
///
/// The walk never follows symlinks, so a `target` symlinked out of the tree can
/// not be used to delete something elsewhere. The heavy filesystem work runs on
/// a blocking thread rather than the async runtime.
pub async fn clean_cargo_targets(
    repo: &Arc<RepoContext>,
    request: CleanCargoTargetsRequest,
) -> Result<CleanCargoTargetsResult, String> {
    let root = match request.path.as_ref() {
        Some(path) => repo.resolve_path(path)?,
        None => repo.root().to_path_buf(),
    };

    if !root.is_dir() {
        return Err(format!("'{}' is not a directory", repo.to_relative(&root)));
    }

    let repo_root = repo.root().to_path_buf();
    let dry_run = request.dry_run;

    let (cleaned, truncated) = tokio::task::spawn_blocking(move || {
        let (targets, truncated) = find_target_dirs(&root);

        let mut cleaned: Vec<(PathBuf, u64)> = Vec::with_capacity(targets.len());

        for target in targets {
            // The walk starts inside the root and never follows symlinks, so
            // this always holds — checked anyway, because the next line deletes.
            if !target.starts_with(&repo_root) {
                continue;
            }

            let freed = dir_size(&target);

            if !dry_run {
                std::fs::remove_dir_all(&target).map_err(|err| {
                    format!("Can not delete '{}'. Err: {}", target.display(), err)
                })?;
            }

            cleaned.push((target, freed));
        }

        Ok::<(Vec<(PathBuf, u64)>, bool), String>((cleaned, truncated))
    })
    .await
    .map_err(|err| format!("clean_cargo_targets task failed. Err: {}", err))??;

    let mut total_freed_bytes = 0u64;
    let mut targets = Vec::with_capacity(cleaned.len());

    for (path, freed_bytes) in cleaned {
        total_freed_bytes += freed_bytes;
        targets.push(CleanedTarget {
            path: repo.to_relative(&path),
            freed_bytes,
        });
    }

    targets.sort_by(|left, right| left.path.cmp(&right.path));

    // A dry run changes nothing, so there is nothing to record; a real run that
    // removed at least one target leaves a summary trace.
    if !dry_run && !targets.is_empty() {
        repo.audit
            .mutation(AuditMutation {
                repo: &repo.name,
                action: "clean_cargo_targets",
                target: &format!("{} target dir(s)", targets.len()),
                detail: Some(format!("freed {}", format_bytes(total_freed_bytes))),
            })
            .await;
    }

    Ok(CleanCargoTargetsResult {
        targets,
        total_freed_bytes,
        dry_run,
        truncated,
    })
}

/// Collects every `target` directory that sits beside a `Cargo.toml`.
///
/// Symlinks are skipped outright: `DirEntry::file_type` reports the link itself,
/// not its destination, so `is_dir()` is false for a symlink and the walk never
/// steps through one. A cargo `target` is never descended into — there are no
/// nested crates to find inside build output.
fn find_target_dirs(root: &Path) -> (Vec<PathBuf>, bool) {
    let mut result = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut truncated = false;

    while let Some(dir) = stack.pop() {
        let has_cargo = dir.join("Cargo.toml").is_file();

        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };

            // Files, and symlinks of any kind, are not walked.
            if !file_type.is_dir() {
                continue;
            }

            let name = entry.file_name();

            if name == ".git" {
                continue;
            }

            let path = entry.path();

            if name == "target" && has_cargo {
                if result.len() >= MAX_TARGETS {
                    truncated = true;
                } else {
                    result.push(path);
                }

                // Do not descend into build output.
                continue;
            }

            stack.push(path);
        }
    }

    (result, truncated)
}

/// Sums the sizes of the regular files under `dir`, without following symlinks.
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };

            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    total += metadata.len();
                }
            }
        }
    }

    total
}

/// Human-readable byte count for the tool response, e.g. `1.5 GB`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        audit::AuditLog,
        settings::{CommandMode, ProjectSettings, SettingsModel},
    };

    async fn test_repo(name: &str) -> Arc<RepoContext> {
        let base = std::env::temp_dir()
            .join("remote-development-mcp-tests-clean")
            .join(name);

        let _ = std::fs::remove_dir_all(&base);

        let root = base.join("repo");
        std::fs::create_dir_all(&root).unwrap();

        let settings = SettingsModel {
            bind_addr: "127.0.0.1:0".to_string(),
            auth_token: Some("test-token".to_string()),
            projects: Vec::new(),
            endpoints: Vec::new(),
            command_mode: CommandMode::Allowlist,
            command_allowlist: Vec::new(),
            max_concurrent_jobs: 1,
            default_timeout_sec: 60,
            max_log_bytes: 1024,
            logs_path: base.join("logs").to_string_lossy().to_string(),
            audit_log_path: None,
            github_token: None,
        };

        let repo_settings = ProjectSettings {
            id: name.to_string(),
            root: root.to_string_lossy().to_string(),
            description: None,
            command_mode: None,
            command_allowlist: None,
            allow_delete: false,
        };

        let audit = Arc::new(AuditLog::disabled());

        Arc::new(
            RepoContext::new(
                &settings,
                &repo_settings,
                audit,
                std::sync::Arc::new(crate::activity::ActivityLog::new()),
                std::sync::Arc::new(crate::actions::WatchedRuns::new()),
            )
            .await
            .unwrap(),
        )
    }

    /// A crate directory with a `Cargo.toml` and a `target` holding one file.
    fn make_crate(root: &Path, rel: &str, target_file_bytes: usize) {
        let crate_dir = root.join(rel);
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(crate_dir.join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(crate_dir.join("src").join("main.rs"), "fn main() {}").unwrap();

        let target = crate_dir.join("target").join("debug");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("artifact.bin"), vec![0u8; target_file_bytes]).unwrap();
    }

    #[tokio::test]
    async fn removes_every_cargo_target_across_the_tree() {
        let repo = test_repo("removes_targets").await;
        let root = repo.root().to_path_buf();

        make_crate(&root, "crate-a", 1000);
        make_crate(&root, "nested/crate-b", 2000);

        let result = clean_cargo_targets(
            &repo,
            CleanCargoTargetsRequest {
                path: None,
                dry_run: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.targets.len(), 2);
        assert_eq!(result.total_freed_bytes, 3000);
        assert!(!result.truncated);

        assert!(!root.join("crate-a").join("target").exists());
        assert!(!root.join("nested").join("crate-b").join("target").exists());

        // The crates themselves stay.
        assert!(root.join("crate-a").join("Cargo.toml").exists());
        assert!(root.join("nested").join("crate-b").join("src").exists());
    }

    #[tokio::test]
    async fn dry_run_reports_without_deleting() {
        let repo = test_repo("dry_run").await;
        let root = repo.root().to_path_buf();

        make_crate(&root, "crate-a", 500);

        let result = clean_cargo_targets(
            &repo,
            CleanCargoTargetsRequest {
                path: None,
                dry_run: true,
            },
        )
        .await
        .unwrap();

        assert!(result.dry_run);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.total_freed_bytes, 500);

        // Nothing was actually removed.
        assert!(root.join("crate-a").join("target").exists());
    }

    #[tokio::test]
    async fn leaves_a_target_directory_which_is_not_cargo_output() {
        let repo = test_repo("non_cargo_target").await;
        let root = repo.root().to_path_buf();

        // A directory called `target` with no Cargo.toml beside it — for example
        // a build output of some other tool, or just a coincidental name.
        let stray = root.join("assets").join("target");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(stray.join("keep.txt"), b"keep me").unwrap();

        let result = clean_cargo_targets(
            &repo,
            CleanCargoTargetsRequest {
                path: None,
                dry_run: false,
            },
        )
        .await
        .unwrap();

        assert!(result.targets.is_empty());
        assert!(stray.join("keep.txt").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn does_not_follow_a_symlinked_target_out_of_the_tree() {
        let repo = test_repo("symlink_target").await;
        let root = repo.root().to_path_buf();

        // Something outside the repository we must not touch.
        let outside = std::env::temp_dir()
            .join("remote-development-mcp-tests-clean")
            .join("symlink_target_outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious.txt"), b"do not delete").unwrap();

        // A crate whose `target` is a symlink pointing at that outside folder.
        let crate_dir = root.join("crate-a");
        std::fs::create_dir_all(&crate_dir).unwrap();
        std::fs::write(crate_dir.join("Cargo.toml"), "[package]").unwrap();
        std::os::unix::fs::symlink(&outside, crate_dir.join("target")).unwrap();

        let result = clean_cargo_targets(
            &repo,
            CleanCargoTargetsRequest {
                path: None,
                dry_run: false,
            },
        )
        .await
        .unwrap();

        // The symlink is not treated as a cargo target, and the outside folder
        // is untouched.
        assert!(result.targets.is_empty());
        assert!(outside.join("precious.txt").exists());

        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn formats_bytes_readably() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }
}
