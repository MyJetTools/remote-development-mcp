use std::sync::Arc;

use crate::{
    audit::{AuditCommandRefused, AuditMutation},
    repo::RepoContext,
};

use super::{git_capture, resolve_working_dir};

pub struct GitCommandRequest {
    pub args: Vec<String>,
    /// Subdirectory to run in, relative to the repository root. Defaults to the
    /// root.
    pub cwd: Option<String>,
}

#[derive(Debug)]
pub struct GitCommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub success: bool,
}

/// Runs an arbitrary `git` command inside the repository.
///
/// This is full git — it can commit, rewrite history, change config and reach
/// remotes. That is not a new capability: `git` is a general-purpose binary and
/// is already reachable through `run_command` when it is allowlisted, so this
/// tool grants nothing more. It is the same power, more conveniently, and run
/// through the hardened [`git_capture`] (hooks, `core.fsmonitor` and the `ext::`
/// transport disabled) rather than a bare spawn.
///
/// Gated on the command policy: if an operator has removed `git` from the
/// allowlist, this refuses too, so there is no way around that decision here.
/// No caller-supplied environment is accepted, which keeps `GIT_SSH_COMMAND` and
/// the other env-based execution vectors closed.
pub async fn run_git(
    repo: &Arc<RepoContext>,
    request: GitCommandRequest,
) -> Result<GitCommandResult, String> {
    if request.args.is_empty() {
        return Err(
            "No git arguments were given, for example [\"status\", \"--short\"]".to_string(),
        );
    }

    if let Err(err) = repo.command_policy.check("git") {
        repo.audit
            .command_refused(AuditCommandRefused {
                repo: &repo.name,
                command_line: &format!("git {}", request.args.join(" ")),
                reason: &err,
            })
            .await;

        return Err(err);
    }

    let cwd = resolve_working_dir(repo, request.cwd.as_deref())?;

    // Not gated on the directory already being a git working tree — that would
    // block the very commands that create one (`init`, `clone`). Running git
    // somewhere that is not a repository simply returns git's own clear
    // "fatal: not a git repository" in stderr.
    let arg_refs: Vec<&str> = request.args.iter().map(|arg| arg.as_str()).collect();

    let output = git_capture(&arg_refs, &cwd, None).await?;

    // Every git command that actually ran goes in the trail — many of them
    // change the tree, and even the read-only ones are useful to see there.
    repo.audit
        .mutation(AuditMutation {
            repo: &repo.name,
            action: "git",
            target: &request.args.join(" "),
            detail: Some(match output.exit_code {
                Some(code) => format!("exit {}", code),
                None => "killed".to_string(),
            }),
        })
        .await;

    Ok(GitCommandResult {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
        success: output.success,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    fn git_allowed() -> TestRepoOptions {
        TestRepoOptions {
            command_allowlist: vec!["git".to_string()],
            ..Default::default()
        }
    }

    fn request(args: &[&str]) -> GitCommandRequest {
        GitCommandRequest {
            args: args.iter().map(|arg| arg.to_string()).collect(),
            cwd: None,
        }
    }

    #[tokio::test]
    async fn runs_an_arbitrary_git_command() {
        let repo = build_test_repo("git_status", git_allowed()).await;

        // init is allowed even though the directory is not yet a repository.
        run_git(&repo, request(&["init"])).await.unwrap();

        let status = run_git(&repo, request(&["status", "--short"]))
            .await
            .unwrap();

        assert!(status.success, "stderr: {}", status.stderr);
        assert_eq!(status.exit_code, Some(0));
    }

    #[tokio::test]
    async fn refused_when_git_is_not_in_the_allowlist() {
        // Default test options ship an empty allowlist, so git is not permitted.
        let repo = build_test_repo("git_refused", TestRepoOptions::default()).await;

        let err = run_git(&repo, request(&["status"])).await.unwrap_err();

        assert!(err.contains("not in the allowlist"), "{}", err);
    }

    #[tokio::test]
    async fn empty_args_are_refused() {
        let repo = build_test_repo("git_empty", git_allowed()).await;

        assert!(run_git(&repo, request(&[])).await.is_err());
    }
}
