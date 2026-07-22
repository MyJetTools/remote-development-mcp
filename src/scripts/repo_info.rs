use std::sync::Arc;

use crate::repo::RepoContext;

use super::{exec_capture, git_capture};

/// `git status --porcelain` on a large dirty tree can be long; the count is
/// what matters, the listing is a sample.
const MAX_STATUS_LINES: usize = 100;

pub struct WorkspaceMember {
    pub member_name: String,
    /// Relative to the repository root.
    pub manifest_path: String,
}

pub struct RepoInfo {
    pub root: String,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub git_status_short: Vec<String>,
    pub git_status_truncated: bool,
    pub workspace_members: Vec<WorkspaceMember>,
    /// Why the workspace member list is empty, when it is.
    pub workspace_note: Option<String>,
}

/// What a client needs to orient itself before doing anything else: which
/// branch, how dirty, and — for a Rust workspace — which crates can be built or
/// tested on their own instead of building the whole tree.
pub async fn repo_info(repo: &Arc<RepoContext>) -> Result<RepoInfo, String> {
    let branch = read_branch(repo).await;
    let status = read_status(repo).await;
    let workspace = read_workspace_members(repo).await;

    Ok(RepoInfo {
        // The name, not the host path: a client has no use for the layout of
        // the machine, and every tool takes repository-relative paths anyway.
        root: repo.name.clone(),
        git_branch: branch,
        git_dirty: !status.lines.is_empty(),
        git_status_short: status.lines,
        git_status_truncated: status.truncated,
        workspace_members: workspace.members,
        workspace_note: workspace.note,
    })
}

async fn read_branch(repo: &Arc<RepoContext>) -> Option<String> {
    let output = git_capture(&["rev-parse", "--abbrev-ref", "HEAD"], repo.root(), None)
        .await
        .ok()?;

    if !output.success {
        return None;
    }

    let branch = output.stdout.trim();

    // On a detached HEAD `--abbrev-ref` prints the literal "HEAD"; that is the
    // absence of a branch, which `Option` already models, not a branch called
    // HEAD.
    if branch.is_empty() || branch == "HEAD" {
        return None;
    }

    Some(branch.to_string())
}

struct GitStatus {
    lines: Vec<String>,
    truncated: bool,
}

async fn read_status(repo: &Arc<RepoContext>) -> GitStatus {
    let output = git_capture(&["status", "--porcelain"], repo.root(), None).await;

    let output = match output {
        Ok(output) => output,
        Err(_) => {
            return GitStatus {
                lines: Vec::new(),
                truncated: false,
            }
        }
    };

    if !output.success {
        return GitStatus {
            lines: Vec::new(),
            truncated: false,
        };
    }

    let all: Vec<String> = output
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect();

    let truncated = all.len() > MAX_STATUS_LINES;

    GitStatus {
        lines: all.into_iter().take(MAX_STATUS_LINES).collect(),
        truncated,
    }
}

struct Workspace {
    members: Vec<WorkspaceMember>,
    note: Option<String>,
}

/// Reads the workspace layout from `cargo metadata`.
///
/// `--no-deps` keeps it to this workspace's own crates and, more importantly,
/// stops cargo from touching the network to resolve the dependency graph.
async fn read_workspace_members(repo: &Arc<RepoContext>) -> Workspace {
    if !repo.root().join("Cargo.toml").exists() {
        return Workspace {
            members: Vec::new(),
            note: Some("Not a Rust workspace — there is no Cargo.toml at the root".to_string()),
        };
    }

    let output = exec_capture(
        "cargo",
        &["metadata", "--no-deps", "--format-version", "1"],
        repo.root(),
        None,
    )
    .await;

    let output = match output {
        Ok(output) => output,
        Err(err) => {
            return Workspace {
                members: Vec::new(),
                note: Some(err),
            }
        }
    };

    if !output.success {
        return Workspace {
            members: Vec::new(),
            note: Some(format!("cargo metadata failed: {}", output.stderr.trim())),
        };
    }

    let parsed: serde_json::Value = match serde_json::from_str(&output.stdout) {
        Ok(parsed) => parsed,
        Err(err) => {
            return Workspace {
                members: Vec::new(),
                note: Some(format!("Can not read the cargo metadata output: {}", err)),
            }
        }
    };

    let packages = match parsed
        .get("packages")
        .and_then(|packages| packages.as_array())
    {
        Some(packages) => packages,
        None => {
            return Workspace {
                members: Vec::new(),
                note: Some("cargo metadata returned no packages".to_string()),
            }
        }
    };

    let mut members = Vec::new();

    for package in packages.iter() {
        let member_name = match package.get("name").and_then(|name| name.as_str()) {
            Some(member_name) => member_name.to_string(),
            None => continue,
        };

        let manifest_path = package
            .get("manifest_path")
            .and_then(|manifest_path| manifest_path.as_str())
            .map(|manifest_path| repo.to_relative(std::path::Path::new(manifest_path)))
            .unwrap_or_default();

        members.push(WorkspaceMember {
            member_name,
            manifest_path,
        });
    }

    members.sort_by(|left, right| left.member_name.cmp(&right.member_name));

    Workspace {
        members,
        note: None,
    }
}
