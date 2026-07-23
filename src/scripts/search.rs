use std::{path::PathBuf, sync::Arc};

use ignore::{overrides::OverrideBuilder, WalkBuilder};
use regex::RegexBuilder;

use crate::repo::RepoContext;

pub const DEFAULT_MAX_RESULTS: usize = 200;

/// Ceiling on a caller-supplied `max_results`, so it can not drive an unbounded
/// result set. Narrow the pattern rather than asking for more than this.
const MAX_RESULTS: usize = 5000;

/// Files above this are skipped. Anything larger is a build artefact, a dump or
/// a media file, and reading it would cost more than the match is worth.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// A matching line is returned as context, not as a payload — a minified bundle
/// can hold megabytes on one line, and sending that back helps nobody.
const MAX_LINE_CHARS: usize = 500;

/// How much of a file is inspected for NUL bytes before deciding it is binary.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

pub struct SearchRequest {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    pub max_results: Option<usize>,
    pub ignore_case: bool,
}

#[derive(Debug)]
pub struct SearchMatch {
    pub file: String,
    pub line: u64,
    pub text: String,
}

#[derive(Debug)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

/// Content search across the repository.
///
/// Runs in-process rather than shelling out to `rg`, so the machine hosting this
/// server needs no ripgrep installed. It is still ripgrep's behaviour, because
/// it is ripgrep's own libraries doing the work: `ignore` walks the tree with
/// full gitignore semantics (nested, negated, global and `.git/info/exclude`),
/// and `regex` matches. Binary files and anything git ignores are skipped, which
/// is what keeps results relevant in a large monorepo.
pub async fn search(
    repo: &Arc<RepoContext>,
    request: SearchRequest,
) -> Result<SearchResult, String> {
    if request.pattern.trim().is_empty() {
        return Err("The search pattern is empty".to_string());
    }

    let search_root = match request.path.as_ref() {
        Some(path) => repo.resolve_path(path)?,
        None => repo.root().to_path_buf(),
    };

    let max_results = request
        .max_results
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .clamp(1, MAX_RESULTS);

    // Compiled before the walk starts so a bad pattern is reported as such,
    // rather than after crawling the whole tree.
    let regex = RegexBuilder::new(&request.pattern)
        .case_insensitive(request.ignore_case)
        .build()
        .map_err(|err| {
            format!(
                "'{}' is not a valid regular expression. {}",
                request.pattern, err
            )
        })?;

    let repo_root = repo.root().to_path_buf();
    let glob = request.glob.clone();

    // Walking a monorepo is heavy, blocking filesystem work — off the async
    // runtime it goes.
    let found = tokio::task::spawn_blocking(move || {
        run_search(RunSearch {
            search_root,
            repo_root,
            regex,
            glob,
            max_results,
        })
    })
    .await
    .map_err(|err| format!("The search task failed. Err: {}", err))??;

    Ok(SearchResult {
        matches: found
            .matches
            .into_iter()
            .map(|found| SearchMatch {
                file: repo.to_relative(&found.file),
                line: found.line,
                text: found.text,
            })
            .collect(),
        truncated: found.truncated,
    })
}

struct RunSearch {
    search_root: PathBuf,
    repo_root: PathBuf,
    regex: regex::Regex,
    glob: Option<String>,
    max_results: usize,
}

struct RawMatch {
    file: PathBuf,
    line: u64,
    text: String,
}

struct RawResult {
    matches: Vec<RawMatch>,
    truncated: bool,
}

fn run_search(request: RunSearch) -> Result<RawResult, String> {
    let mut builder = WalkBuilder::new(&request.search_root);

    builder
        // Full gitignore behaviour, the same set of sources ripgrep consults.
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        // ripgrep only applies .gitignore inside an actual git repository. Here
        // it applies always: a folder carrying a .gitignore means it, whether or
        // not it has been `git init`ed yet, and the alternative is a search that
        // silently changes behaviour based on something the caller can not see.
        .require_git(false)
        // Hidden files stay out, which also keeps `.git` out of the walk.
        .hidden(true)
        .follow_links(false);

    if let Some(glob) = request.glob.as_ref() {
        // Overrides are anchored at the repository root so a glob like
        // `src/**/*.rs` means what the caller expects.
        let mut overrides = OverrideBuilder::new(&request.repo_root);

        overrides
            .add(glob)
            .map_err(|err| format!("'{}' is not a valid glob. {}", glob, err))?;

        let overrides = overrides
            .build()
            .map_err(|err| format!("Can not use the glob '{}'. {}", glob, err))?;

        builder.overrides(overrides);
    }

    let mut matches = Vec::new();
    let mut truncated = false;

    'walk: for entry in builder.build() {
        // An unreadable directory should not sink the whole search.
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let path = entry.path();

        let content = match read_text_file(path) {
            Some(content) => content,
            None => continue,
        };

        for (index, line) in content.lines().enumerate() {
            if !request.regex.is_match(line) {
                continue;
            }

            if matches.len() >= request.max_results {
                truncated = true;
                break 'walk;
            }

            matches.push(RawMatch {
                file: path.to_path_buf(),
                line: index as u64 + 1,
                text: clamp_line(line),
            });
        }
    }

    Ok(RawResult { matches, truncated })
}

/// Reads a file as text, or `None` when it is too big or looks binary.
fn read_text_file(path: &std::path::Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;

    if metadata.len() > MAX_FILE_BYTES {
        return None;
    }

    let bytes = std::fs::read(path).ok()?;

    if looks_binary(&bytes) {
        return None;
    }

    // Lossy rather than strict: a file with a stray invalid byte is still worth
    // searching, and the replacement character only affects that one spot.
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// A NUL byte near the start is the same heuristic ripgrep uses to call a file
/// binary — text formats do not contain one.
fn looks_binary(bytes: &[u8]) -> bool {
    let window = &bytes[..std::cmp::min(bytes.len(), BINARY_SNIFF_BYTES)];

    window.contains(&0)
}

fn clamp_line(line: &str) -> String {
    let line = line.trim_end_matches('\r');

    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }

    let clamped: String = line.chars().take(MAX_LINE_CHARS).collect();

    format!("{}…", clamped)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    async fn repo_with_files(name: &str) -> Arc<RepoContext> {
        let repo = build_test_repo(name, TestRepoOptions::default()).await;
        let root = repo.root();

        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    let needle = 1;\n    println!(\"NEEDLE upper\");\n}\n",
        )
        .unwrap();
        std::fs::write(root.join("src/lib.rs"), "// nothing to find here\n").unwrap();
        std::fs::write(root.join("notes.txt"), "a needle in the text file\n").unwrap();

        repo
    }

    fn request(pattern: &str) -> SearchRequest {
        SearchRequest {
            pattern: pattern.to_string(),
            path: None,
            glob: None,
            max_results: None,
            ignore_case: false,
        }
    }

    #[tokio::test]
    async fn finds_matches_with_file_and_line() {
        let repo = repo_with_files("search_basic").await;

        let result = search(&repo, request("needle")).await.unwrap();

        assert!(!result.truncated);

        let main_hit = result
            .matches
            .iter()
            .find(|found| found.file == "src/main.rs")
            .expect("expected a hit in src/main.rs");

        assert_eq!(main_hit.line, 2);
        assert!(main_hit.text.contains("let needle = 1;"));

        assert!(result.matches.iter().any(|found| found.file == "notes.txt"));
    }

    #[tokio::test]
    async fn is_case_sensitive_unless_asked_otherwise() {
        let repo = repo_with_files("search_case").await;

        let sensitive = search(&repo, request("NEEDLE")).await.unwrap();
        assert_eq!(sensitive.matches.len(), 1);

        let mut insensitive = request("NEEDLE");
        insensitive.ignore_case = true;

        let insensitive = search(&repo, insensitive).await.unwrap();
        assert!(insensitive.matches.len() > 1);
    }

    #[tokio::test]
    async fn a_glob_narrows_the_files_searched() {
        let repo = repo_with_files("search_glob").await;

        let mut with_glob = request("needle");
        with_glob.glob = Some("*.txt".to_string());

        let result = search(&repo, with_glob).await.unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].file, "notes.txt");
    }

    #[tokio::test]
    async fn a_path_narrows_the_subtree_searched() {
        let repo = repo_with_files("search_path").await;

        let mut in_src = request("needle");
        in_src.path = Some("src".to_string());

        let result = search(&repo, in_src).await.unwrap();

        assert!(result
            .matches
            .iter()
            .all(|found| found.file.starts_with("src/")));
    }

    #[tokio::test]
    async fn what_git_ignores_is_not_searched() {
        let repo = repo_with_files("search_gitignore").await;
        let root = repo.root();

        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/generated.rs"), "the needle is here\n").unwrap();

        let result = search(&repo, request("needle")).await.unwrap();

        assert!(
            !result
                .matches
                .iter()
                .any(|found| found.file.starts_with("target/")),
            "ignored files must not be searched: {:?}",
            result.matches.iter().map(|m| &m.file).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn binary_files_are_skipped() {
        let repo = repo_with_files("search_binary").await;

        // Contains the pattern, but a NUL byte marks it binary.
        std::fs::write(repo.root().join("blob.bin"), b"needle\0needle").unwrap();

        let result = search(&repo, request("needle")).await.unwrap();

        assert!(!result.matches.iter().any(|found| found.file == "blob.bin"));
    }

    #[tokio::test]
    async fn max_results_caps_and_reports_truncation() {
        let repo = build_test_repo("search_truncate", TestRepoOptions::default()).await;

        let many = (0..50)
            .map(|index| format!("needle {}", index))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(repo.root().join("many.txt"), many).unwrap();

        let mut capped = request("needle");
        capped.max_results = Some(10);

        let result = search(&repo, capped).await.unwrap();

        assert_eq!(result.matches.len(), 10);
        assert!(result.truncated);
    }

    #[tokio::test]
    async fn an_exact_hit_count_is_not_reported_as_truncated() {
        let repo = repo_with_files("search_exact").await;

        // Exactly one match in the tree, asking for exactly one.
        let mut capped = request("NEEDLE");
        capped.max_results = Some(1);

        let result = search(&repo, capped).await.unwrap();

        assert_eq!(result.matches.len(), 1);
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn a_regular_expression_works_not_just_a_literal() {
        let repo = repo_with_files("search_regex").await;

        let result = search(&repo, request(r"^fn \w+\(\)")).await.unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].file, "src/main.rs");
        assert_eq!(result.matches[0].line, 1);
    }

    #[tokio::test]
    async fn an_invalid_pattern_is_reported_clearly() {
        let repo = repo_with_files("search_bad_regex").await;

        let err = search(&repo, request("(unclosed")).await.unwrap_err();

        assert!(err.contains("not a valid regular expression"), "{}", err);
    }

    #[tokio::test]
    async fn an_empty_pattern_is_refused() {
        let repo = repo_with_files("search_empty").await;

        assert!(search(&repo, request("   ")).await.is_err());
    }

    #[test]
    fn a_very_long_line_is_clamped() {
        let long = "x".repeat(MAX_LINE_CHARS + 100);

        let clamped = clamp_line(&long);

        assert!(clamped.chars().count() <= MAX_LINE_CHARS + 1);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn binary_detection_looks_only_at_the_start() {
        assert!(looks_binary(b"abc\0def"));
        assert!(!looks_binary(b"plain text"));
        assert!(!looks_binary(b""));
    }
}
