use std::{path::PathBuf, sync::Arc};

use ignore::WalkBuilder;
use rust_extensions::date_time::DateTimeAsMicroseconds;

use crate::repo::RepoContext;

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

/// Lists a folder, optionally leaving out whatever git ignores.
///
/// The ignore filtering is done by ripgrep's `ignore` crate rather than by
/// shelling out to `git check-ignore`. That matters beyond tidiness: the walker
/// reads each directory's own `.gitignore` as it descends, so a root that holds
/// several independent repositories — or is not a git repository at all — is
/// handled correctly, where a single `git check-ignore` run at the root would
/// simply fail and silently filter nothing. It also means an ignored tree is
/// never walked into, so `target/` costs nothing instead of consuming the entry
/// budget.
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
        request.max_depth.filter(|depth| *depth > 0)
    } else {
        Some(1)
    };

    let respect_gitignore = request.respect_gitignore;
    let walk_root = root.clone();

    // Walking a large tree is blocking filesystem work.
    let found = tokio::task::spawn_blocking(move || walk(&walk_root, max_depth, respect_gitignore))
        .await
        .map_err(|err| format!("The listing task failed. Err: {}", err))?;

    let mut entries: Vec<DirEntry> = found
        .entries
        .into_iter()
        .map(|entry| DirEntry {
            path: repo.to_relative(&entry.path),
            entry_type: entry.entry_type,
            size_bytes: entry.size_bytes,
            modified: entry.modified,
        })
        .collect();

    entries.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(ListDirResult {
        entries,
        truncated: found.truncated,
    })
}

struct RawEntry {
    path: PathBuf,
    entry_type: EntryType,
    size_bytes: u64,
    modified: Option<String>,
}

struct RawListing {
    entries: Vec<RawEntry>,
    truncated: bool,
}

fn walk(root: &PathBuf, max_depth: Option<usize>, respect_gitignore: bool) -> RawListing {
    let mut builder = WalkBuilder::new(root);

    builder
        .git_ignore(respect_gitignore)
        .git_global(respect_gitignore)
        .git_exclude(respect_gitignore)
        .ignore(respect_gitignore)
        .parents(respect_gitignore)
        // Applied even outside a git repository, so a folder carrying a
        // .gitignore is honoured whether or not it has been `git init`ed.
        .require_git(false)
        // Hidden entries stay out, which also keeps `.git` out of the walk.
        .hidden(true)
        .follow_links(false);

    if let Some(max_depth) = max_depth {
        builder.max_depth(Some(max_depth));
    }

    let mut entries = Vec::new();
    let mut truncated = false;

    for found in builder.build() {
        let found = match found {
            Ok(found) => found,
            // An unreadable subfolder should not sink the whole listing.
            Err(_) => continue,
        };

        // The walk yields the root itself first; it is not one of its entries.
        if found.depth() == 0 {
            continue;
        }

        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }

        let path = found.path().to_path_buf();

        let metadata = match found.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let entry_type = if found.file_type().map(|kind| kind.is_symlink()) == Some(true) {
            EntryType::Symlink
        } else if metadata.is_dir() {
            EntryType::Dir
        } else {
            EntryType::File
        };

        entries.push(RawEntry {
            path,
            entry_type,
            size_bytes: metadata.len(),
            modified: to_rfc3339(&metadata),
        });
    }

    RawListing { entries, truncated }
}

fn to_rfc3339(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;

    let moment = DateTimeAsMicroseconds {
        unix_microseconds: since_epoch.as_micros() as i64,
    };

    Some(moment.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    fn request() -> ListDirRequest {
        ListDirRequest {
            path: None,
            recursive: false,
            max_depth: None,
            respect_gitignore: true,
        }
    }

    async fn repo_with_tree(name: &str) -> Arc<RepoContext> {
        let repo = build_test_repo(name, TestRepoOptions::default()).await;
        let root = repo.root();

        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]").unwrap();

        repo
    }

    #[tokio::test]
    async fn lists_the_top_level_only_by_default() {
        let repo = repo_with_tree("list_shallow").await;

        let result = list_dir(&repo, request()).await.unwrap();

        let paths: Vec<&str> = result.entries.iter().map(|e| e.path.as_str()).collect();

        assert!(paths.contains(&"Cargo.toml"));
        assert!(paths.contains(&"src"));
        // Not recursive, so the file inside src is not listed.
        assert!(!paths.contains(&"src/main.rs"));
    }

    #[tokio::test]
    async fn recursive_reaches_nested_files() {
        let repo = repo_with_tree("list_recursive").await;

        let mut recursive = request();
        recursive.recursive = true;

        let result = list_dir(&repo, recursive).await.unwrap();

        assert!(result
            .entries
            .iter()
            .any(|entry| entry.path == "src/main.rs"));
    }

    /// The case that used to fail: the root is not a git repository, so a single
    /// `git check-ignore` there returned "not a repository" and nothing was
    /// filtered at all.
    #[tokio::test]
    async fn gitignore_is_honoured_even_when_the_root_is_not_a_git_repository() {
        let repo = repo_with_tree("list_ignore_no_git").await;
        let root = repo.root();

        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/artifact.bin"), b"x").unwrap();

        let mut recursive = request();
        recursive.recursive = true;

        let result = list_dir(&repo, recursive).await.unwrap();

        assert!(
            !result
                .entries
                .iter()
                .any(|entry| entry.path.starts_with("target")),
            "target/ should be filtered: {:?}",
            result.entries.iter().map(|e| &e.path).collect::<Vec<_>>()
        );
    }

    /// A root holding several independent repositories, each with its own
    /// .gitignore — the layout this change exists for.
    #[tokio::test]
    async fn each_nested_repository_gets_its_own_gitignore_applied() {
        let repo = build_test_repo("list_nested_repos", TestRepoOptions::default()).await;
        let root = repo.root();

        for library in ["my-ssh", "my-json"] {
            std::fs::create_dir_all(root.join(library).join("target")).unwrap();
            std::fs::create_dir_all(root.join(library).join("src")).unwrap();
            std::fs::write(root.join(library).join(".gitignore"), "target/\n").unwrap();
            std::fs::write(root.join(library).join("src/lib.rs"), "// code").unwrap();
            std::fs::write(root.join(library).join("target/build.bin"), b"x").unwrap();
        }

        let mut recursive = request();
        recursive.recursive = true;

        let result = list_dir(&repo, recursive).await.unwrap();

        let paths: Vec<&str> = result.entries.iter().map(|e| e.path.as_str()).collect();

        assert!(paths.contains(&"my-ssh/src/lib.rs"));
        assert!(paths.contains(&"my-json/src/lib.rs"));
        assert!(
            !paths.iter().any(|path| path.contains("target")),
            "each library's own .gitignore should apply: {:?}",
            paths
        );
    }

    #[tokio::test]
    async fn a_path_narrows_the_listing_to_one_library() {
        let repo = build_test_repo("list_one_library", TestRepoOptions::default()).await;
        let root = repo.root();

        std::fs::create_dir_all(root.join("my-ssh/src")).unwrap();
        std::fs::write(root.join("my-ssh/src/lib.rs"), "// code").unwrap();
        std::fs::create_dir_all(root.join("my-json/src")).unwrap();
        std::fs::write(root.join("my-json/src/lib.rs"), "// code").unwrap();

        let mut narrowed = request();
        narrowed.path = Some("my-ssh".to_string());
        narrowed.recursive = true;

        let result = list_dir(&repo, narrowed).await.unwrap();

        assert!(result
            .entries
            .iter()
            .all(|entry| entry.path.starts_with("my-ssh/")));
    }

    #[tokio::test]
    async fn ignored_entries_come_back_when_the_filter_is_off() {
        let repo = repo_with_tree("list_ignore_off").await;
        let root = repo.root();

        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();

        let mut unfiltered = request();
        unfiltered.recursive = true;
        unfiltered.respect_gitignore = false;

        let result = list_dir(&repo, unfiltered).await.unwrap();

        assert!(result.entries.iter().any(|entry| entry.path == "target"));
    }

    #[tokio::test]
    async fn the_git_folder_is_never_listed() {
        let repo = repo_with_tree("list_no_git_folder").await;
        std::fs::create_dir_all(repo.root().join(".git")).unwrap();
        std::fs::write(repo.root().join(".git/config"), "x").unwrap();

        let mut recursive = request();
        recursive.recursive = true;

        let result = list_dir(&repo, recursive).await.unwrap();

        assert!(!result
            .entries
            .iter()
            .any(|entry| entry.path.starts_with(".git")));
    }

    #[tokio::test]
    async fn a_file_is_not_a_directory_to_list() {
        let repo = repo_with_tree("list_not_a_dir").await;

        let mut on_a_file = request();
        on_a_file.path = Some("Cargo.toml".to_string());

        assert!(list_dir(&repo, on_a_file).await.is_err());
    }
}
