use std::sync::Arc;

use crate::{
    actions::WatchedRun,
    github::{GitHubClient, RepoSlug},
    repo::RepoContext,
};

use super::{git_capture, resolve_working_dir};

/// How many recent runs are fetched when nothing specific was asked for.
const RECENT_RUNS: usize = 20;

/// How many are reported back. Enough to see the last few releases, short enough
/// to read.
const REPORTED_RUNS: usize = 10;

pub struct WatchActionsRequest {
    /// Subfolder holding the repository, when the root holds several.
    pub path: Option<String>,
    /// Follow the run started by this tag — what `create_release` returns.
    pub tag: Option<String>,
    /// Follow one specific run.
    pub run_id: Option<u64>,
}

pub struct WatchedRunReport {
    pub run_id: u64,
    pub workflow: String,
    pub tag: Option<String>,
    pub outcome: String,
    pub finished: bool,
    pub failed_step: Option<String>,
    pub url: Option<String>,
    pub elapsed_sec: f64,
}

pub struct WatchActionsResult {
    pub repository: String,
    pub runs: Vec<WatchedRunReport>,
}

/// Starts following GitHub Actions runs, and reports where they are.
///
/// Calling it arms a background poller that keeps asking GitHub every ten
/// seconds until each run finishes. Two things follow from that: the console
/// shows a build progressing without anyone calling again, and a later call
/// answers from memory instead of waiting on the API.
pub async fn watch_actions(
    repo: &Arc<RepoContext>,
    request: WatchActionsRequest,
) -> Result<WatchActionsResult, String> {
    let token =
        match repo.github_token.as_ref() {
            Some(token) => token.clone(),
            None => return Err(
                "No github_token is configured, so GitHub Actions can not be read. Add one to the \
                 settings and restart"
                    .to_string(),
            ),
        };

    let working_dir = resolve_working_dir(repo, request.path.as_deref())?;
    let slug = read_repo_slug(&working_dir).await?;

    let client = GitHubClient::new(token);

    let found = match request.run_id {
        Some(run_id) => vec![client.get_workflow_run(&slug, run_id).await?],
        None => {
            let runs = client.list_workflow_runs(&slug, RECENT_RUNS).await?;

            match request.tag.as_ref() {
                Some(tag) => {
                    // A run started by pushing a tag carries that tag as its
                    // head_branch, which is what ties a build to its release.
                    let matched: Vec<_> = runs
                        .into_iter()
                        .filter(|run| run.head_branch.as_deref() == Some(tag.as_str()))
                        .collect();

                    if matched.is_empty() {
                        return Err(format!(
                            "No workflow run for tag '{}' yet. GitHub takes a few seconds to start \
                             one after a release is created — try again shortly, and check the tag \
                             matches a workflow's trigger if it never appears",
                            tag
                        ));
                    }

                    matched
                }
                None => runs,
            }
        }
    };

    let mut reports = Vec::new();

    for run in found.iter().take(REPORTED_RUNS) {
        let mut watched = WatchedRun::from_api(&repo.name, &slug, run);

        // Asked for straight away when it is already failed, so the very first
        // answer says which step broke instead of only "failure".
        if run.failed() {
            watched.failed_step = client.failed_step(&slug, run.id).await;
        }

        let now = rust_extensions::date_time::DateTimeAsMicroseconds::now();

        reports.push(WatchedRunReport {
            run_id: watched.run_id,
            workflow: watched.workflow.clone(),
            tag: watched.tag.clone(),
            outcome: watched.outcome.clone(),
            finished: watched.finished,
            failed_step: watched.failed_step.clone(),
            url: watched.url.clone(),
            elapsed_sec: watched.elapsed_sec(now),
        });

        // Only an unfinished run is worth following; a finished one is already
        // its own final answer.
        if !watched.finished {
            repo.watched_runs.watch(watched);
        }
    }

    Ok(WatchActionsResult {
        repository: slug.to_string(),
        runs: reports,
    })
}

async fn read_repo_slug(working_dir: &std::path::Path) -> Result<RepoSlug, String> {
    let output = git_capture(&["remote", "get-url", "origin"], working_dir, None).await?;

    if !output.success {
        return Err(format!(
            "Can not read the git remote 'origin', so there is no way to tell which GitHub \
             repository to watch. {}",
            output.stderr.trim()
        ));
    }

    RepoSlug::parse_remote(&output.stdout).ok_or_else(|| {
        format!(
            "The git remote 'origin' ('{}') is not a GitHub owner/repo URL",
            output.stdout.trim()
        )
    })
}
