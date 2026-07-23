use std::sync::Arc;

use crate::{audit::AuditMutation, repo::RepoContext};

use super::{git_capture, resolve_working_dir};

pub struct ApplyPatchResult {
    pub files_changed: Vec<String>,
    /// What git said when it refused. Populated only on failure.
    pub rejected: Option<String>,
}

/// Applies a unified diff through `git apply`.
///
/// Handing this to git rather than parsing diffs here is deliberate: git
/// already knows about context matching, renames, mode bits and binary hunks,
/// and it refuses paths that escape the working tree. A hand-rolled applier
/// would be a second, worse implementation of all of that — including the
/// confinement.
///
/// The patch is validated with `--check` first, so a diff that only partly
/// applies leaves nothing half-written behind. The list of changed files comes
/// from git (`--numstat`), not from re-parsing the diff, so renames, mode-only
/// changes and binary hunks are reported correctly.
pub async fn apply_patch(
    repo: &Arc<RepoContext>,
    patch: &str,
    path: Option<&str>,
) -> Result<ApplyPatchResult, String> {
    if patch.trim().is_empty() {
        return Err("The patch is empty".to_string());
    }

    // The folder git applies in. Paths inside the patch are relative to it, so
    // with a root holding several repositories this is what says which one.
    let working_dir = resolve_working_dir(repo, path)?;

    let patch = normalize_trailing_newline(patch);

    let check = git_capture(
        &["apply", "--check", "-"],
        &working_dir,
        Some(patch.as_bytes()),
    )
    .await?;

    if !check.success {
        return Ok(rejected(collect_git_complaint(
            &check.stderr,
            &check.stdout,
        )));
    }

    // Ask git which files the patch touches, rather than re-deriving them from
    // the `+++` headers (which git omits for pure renames, mode changes and
    // binary hunks, and which a crafted content line can spoof).
    let numstat = git_capture(
        &["apply", "--numstat", "-"],
        &working_dir,
        Some(patch.as_bytes()),
    )
    .await?;

    let files_changed = parse_numstat(&numstat.stdout);

    let applied = git_capture(&["apply", "-"], &working_dir, Some(patch.as_bytes())).await?;

    if !applied.success {
        return Ok(rejected(collect_git_complaint(
            &applied.stderr,
            &applied.stdout,
        )));
    }

    // git apply run from inside a larger working tree silently skips any hunk
    // whose path falls outside the current directory and still exits 0. Treat a
    // reported skip as a refusal, so the tool never claims a change it did not
    // make.
    if applied.stderr.contains("Skipped patch") || check.stderr.contains("Skipped patch") {
        return Ok(rejected(
            "git skipped part of this patch — the repository root is inside a larger git working \
             tree, so some paths resolved outside it. Point the repository at the git top level."
                .to_string(),
        ));
    }

    repo.audit
        .mutation(AuditMutation {
            repo: &repo.name,
            action: "apply_patch",
            target: &summarize_files(&files_changed),
            detail: Some(files_changed.join(", ")),
        })
        .await;

    Ok(ApplyPatchResult {
        files_changed,
        rejected: None,
    })
}

fn summarize_files(files: &[String]) -> String {
    match files.len() {
        0 => "no files".to_string(),
        1 => files[0].clone(),
        n => format!("{} files", n),
    }
}

fn rejected(reason: String) -> ApplyPatchResult {
    ApplyPatchResult {
        files_changed: Vec::new(),
        rejected: Some(reason),
    }
}

/// `git apply` rejects a patch whose last line has no newline.
fn normalize_trailing_newline(patch: &str) -> String {
    if patch.ends_with('\n') {
        return patch.to_string();
    }

    format!("{}\n", patch)
}

fn collect_git_complaint(stderr: &str, stdout: &str) -> String {
    let stderr = stderr.trim();

    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = stdout.trim();

    if !stdout.is_empty() {
        return stdout.to_string();
    }

    "git apply refused the patch without explaining why".to_string()
}

/// Reads the changed paths from `git apply --numstat` output.
///
/// Each line is `<added>\t<deleted>\t<path>`, with `-` for the counts of a
/// binary file. A rename is rendered as `{old => new}` inside the path; the new
/// path is the one that ends up on disk, so that is what is reported.
fn parse_numstat(stdout: &str) -> Vec<String> {
    let mut result = Vec::new();

    for line in stdout.lines() {
        let path = match line.splitn(3, '\t').nth(2) {
            Some(path) => path.trim(),
            None => continue,
        };

        if path.is_empty() {
            continue;
        }

        let path = rename_target(path);

        if !result.contains(&path) {
            result.push(path);
        }
    }

    result
}

/// `{src => dst}/file` and `old => new` both denote a rename in numstat output;
/// return the destination, which is the path that exists afterwards.
fn rename_target(path: &str) -> String {
    if let (Some(open), Some(close)) = (path.find('{'), path.find('}')) {
        if let Some(arrow) = path[open..close].find("=>") {
            let before = &path[..open];
            let after_arrow = &path[open + arrow + 2..close];
            let after_brace = &path[close + 1..];
            return format!("{}{}{}", before, after_arrow.trim(), after_brace);
        }
    }

    if let Some(arrow) = path.find("=>") {
        return path[arrow + 2..].trim().to_string();
    }

    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_changed_files_from_numstat() {
        let numstat = "1\t0\tsrc/main.rs\n3\t2\tsrc/lib.rs\n";

        assert_eq!(parse_numstat(numstat), vec!["src/main.rs", "src/lib.rs"]);
    }

    #[test]
    fn reads_a_binary_file_from_numstat() {
        let numstat = "-\t-\tassets/logo.png\n";

        assert_eq!(parse_numstat(numstat), vec!["assets/logo.png"]);
    }

    #[test]
    fn reports_the_destination_of_a_rename() {
        assert_eq!(rename_target("src/{old => new}.rs"), "src/new.rs");
        assert_eq!(rename_target("old_name.rs => new_name.rs"), "new_name.rs");
        assert_eq!(rename_target("plain.rs"), "plain.rs");
    }

    #[test]
    fn does_not_repeat_a_file() {
        let numstat = "1\t0\tsrc/main.rs\n2\t2\tsrc/main.rs\n";

        assert_eq!(parse_numstat(numstat), vec!["src/main.rs"]);
    }

    #[test]
    fn adds_the_trailing_newline_git_insists_on() {
        assert_eq!(normalize_trailing_newline("a"), "a\n");
        assert_eq!(normalize_trailing_newline("a\n"), "a\n");
    }
}
