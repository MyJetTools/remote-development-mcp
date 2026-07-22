use std::{collections::HashSet, path::PathBuf, sync::Arc};

use rust_extensions::date_time::DateTimeAsMicroseconds;

use crate::repo::RepoContext;

use super::git_capture;

/// A monorepo listing can otherwise return a hundred thousand entries and
/// bury the caller.
const MAX_ENTRIES: usize = 5000;

pub struct DirEntry {
    /// Relative to the repository root, so it can be passed straight back into
    /// any other tool.
    pub path: String,
    pub entry_type: EntryType,
    pub size_bytes: u64,
    pub modified: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    File,
    Dir,
    Symlink,
}

impl EntryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryType::File => "file",
            EntryType::Dir => "dir",
            EntryType::Symlink => "symlink",
        }
    }
}

pub struct ListDirRequest {
    pub path: Option<String>,
    pub recursive: bool,
    pub max_depth: Option<usize>,
    pub respect_gitignore: bool,
}

pub struct ListDirResult {
    pub entries: Vec<DirEntry>,
    pub truncated: bool,
}

/// One directory's worth of children, before any of them are added to the
/// result — so the ignore filter can be applied here and the entry cap only
/// ever counts what is actually kept.
struct Child {
    path: PathBuf,
    relative: String,
    entry_type: EntryType,
    size_bytes: u64,
    modified: Option<String>,
}

pub async fn list_dir(
    repo: &Arc<RepoContext>,
    request: ListDirRequest,
) -> Result<ListDirResult, String> {
    let root = match request.path.as_ref() {
        Some(path) => repo.resolve_path(path)?,
        None => repo.root().to_path_buf(),
    };

    if !root.is_dir() {
        return Err(format!("'{}' is not a directory", repo.to_relative(&root)));
    }

    let max_depth = if request.recursive {
        request.max_depth.unwrap_or(usize::MAX).max(1)
    } else {
        1
    };

    let mut entries = Vec::new();
    let mut truncated = false;

    let mut queue: Vec<(PathBuf, usize)> = vec![(root.clone(), 1)];

    'walk: while let Some((folder, depth)) = queue.pop() {
        let children = match read_children(repo, &folder).await {
            Ok(children) => children,
            Err(err) => {
                // An unreadable subfolder should not sink the whole listing;
                // only a failure to read the requested root is fatal.
                if folder == root {
                    return Err(err);
                }

                continue;
            }
        };

        // Filter out what git ignores *before* the cap and the descent, so a
        // large ignored tree (target/, node_modules/) is never walked into and
        // never eats the entry budget.
        let ignored = if request.respect_gitignore {
            check_ignored(repo, &children).await
        } else {
            HashSet::new()
        };

        for child in children {
            if ignored.contains(&child.relative) {
                continue;
            }

            if entries.len() >= MAX_ENTRIES {
                truncated = true;
                break 'walk;
            }

            if child.entry_type == EntryType::Dir && depth < max_depth {
                queue.push((child.path.clone(), depth + 1));
            }

            entries.push(DirEntry {
                path: child.relative,
                entry_type: child.entry_type,
                size_bytes: child.size_bytes,
                modified: child.modified,
            });
        }
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(ListDirResult { entries, truncated })
}

async fn read_children(repo: &Arc<RepoContext>, folder: &PathBuf) -> Result<Vec<Child>, String> {
    let mut reader = tokio::fs::read_dir(folder)
        .await
        .map_err(|err| format!("Can not read '{}'. Err: {}", repo.to_relative(folder), err))?;

    let mut children = Vec::new();

    loop {
        let entry = match reader.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(_) => break,
        };

        let path = entry.path();

        // `.git` is never interesting to a caller and is enormous.
        if path.file_name().map(|name| name == ".git").unwrap_or(false) {
            continue;
        }

        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let is_symlink = tokio::fs::symlink_metadata(&path)
            .await
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false);

        let entry_type = if is_symlink {
            EntryType::Symlink
        } else if metadata.is_dir() {
            EntryType::Dir
        } else {
            EntryType::File
        };

        children.push(Child {
            relative: repo.to_relative(&path),
            path,
            entry_type,
            size_bytes: metadata.len(),
            modified: to_rfc3339(&metadata),
        });
    }

    Ok(children)
}

fn to_rfc3339(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;

    let moment = DateTimeAsMicroseconds {
        unix_microseconds: since_epoch.as_micros() as i64,
    };

    Some(moment.to_rfc3339())
}

/// Returns which of one directory's children git ignores.
///
/// Asks git rather than re-implementing `.gitignore` semantics, which nest,
/// negate and come from several files at once. One call per directory keeps the
/// output small enough to never deadlock the pipe, and a repository without git
/// simply yields no ignores.
async fn check_ignored(repo: &Arc<RepoContext>, children: &[Child]) -> HashSet<String> {
    let mut result = HashSet::new();

    if children.is_empty() {
        return result;
    }

    let mut stdin = Vec::new();

    for child in children.iter() {
        stdin.extend_from_slice(child.relative.as_bytes());
        stdin.push(0);
    }

    let output = match git_capture(
        &["check-ignore", "-z", "--stdin"],
        repo.root(),
        Some(&stdin),
    )
    .await
    {
        Ok(output) => output,
        Err(_) => return result,
    };

    // Exit 0 means some paths matched; 1 means none did (a normal answer);
    // anything else (128 — not a git repository) means we simply do not know.
    match output.exit_code {
        Some(0) => {}
        Some(_) => return result,
        None => return result,
    }

    for ignored in output.stdout.split('\0') {
        if !ignored.is_empty() {
            result.insert(ignored.to_string());
        }
    }

    result
}
