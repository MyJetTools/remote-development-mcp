use rust_extensions::date_time::DateTimeAsMicroseconds;

use crate::github::{RepoSlug, WorkflowRun};

/// One GitHub Actions run the server is keeping an eye on.
///
/// Held rather than fetched on demand so the console can show a build
/// progressing without anyone asking: the poller refreshes these, the console
/// renders them, and the tool reads the same entries — one state, three views.
#[derive(Debug, Clone)]
pub struct WatchedRun {
    /// Repository endpoint this was started from.
    pub repo: String,
    /// `owner/repo` on GitHub — the poller needs it to refresh.
    pub owner: String,
    pub repo_name: String,
    pub run_id: u64,
    /// Workflow name, e.g. "Release App".
    pub workflow: String,
    /// For a tag-triggered run this is the tag, which is how a build is tied
    /// back to the release that started it.
    pub tag: Option<String>,
    /// `queued`, `in_progress`, or the conclusion once finished.
    pub outcome: String,
    pub finished: bool,
    /// Which job and step broke, once one has.
    pub failed_step: Option<String>,
    pub url: Option<String>,
    pub first_seen: DateTimeAsMicroseconds,
    pub last_checked: DateTimeAsMicroseconds,
}

impl WatchedRun {
    pub fn from_api(repo: &str, slug: &RepoSlug, run: &WorkflowRun) -> Self {
        let now = DateTimeAsMicroseconds::now();

        Self {
            repo: repo.to_string(),
            owner: slug.owner.clone(),
            repo_name: slug.repo.clone(),
            run_id: run.id,
            workflow: run.name.clone().unwrap_or_else(|| "workflow".to_string()),
            tag: run.head_branch.clone(),
            outcome: run.outcome().to_string(),
            finished: run.is_finished(),
            failed_step: None,
            url: run.html_url.clone(),
            first_seen: now,
            last_checked: now,
        }
    }

    pub fn slug(&self) -> RepoSlug {
        RepoSlug {
            owner: self.owner.clone(),
            repo: self.repo_name.clone(),
        }
    }

    /// Applies a fresh reading. Returns true when the outcome actually moved,
    /// which is what decides whether the change is worth an entry in the feed.
    pub fn apply(&mut self, run: &WorkflowRun) -> bool {
        let changed = self.outcome != run.outcome();

        self.outcome = run.outcome().to_string();
        self.finished = run.is_finished();
        self.url = run.html_url.clone().or_else(|| self.url.take());
        self.last_checked = DateTimeAsMicroseconds::now();

        changed
    }

    /// How long it has been watched — close enough to the build's duration, and
    /// it is what the console shows ticking.
    pub fn elapsed_sec(&self, now: DateTimeAsMicroseconds) -> f64 {
        let until = if self.finished {
            self.last_checked
        } else {
            now
        };

        (until.unix_microseconds - self.first_seen.unix_microseconds) as f64 / 1_000_000.0
    }

    /// What the run is called in one short string — the tag when there is one,
    /// since that is what a person asked for.
    pub fn label(&self) -> String {
        match self.tag.as_ref() {
            Some(tag) => tag.clone(),
            None => self.workflow.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_run(status: &str, conclusion: Option<&str>) -> WorkflowRun {
        WorkflowRun {
            id: 7,
            name: Some("Release App".to_string()),
            display_title: None,
            head_branch: Some("margin-engine-0.1.5".to_string()),
            status: status.to_string(),
            conclusion: conclusion.map(|value| value.to_string()),
            event: None,
            run_number: None,
            run_started_at: None,
            updated_at: None,
            html_url: Some("https://github.com/x/y/actions/runs/7".to_string()),
        }
    }

    fn slug() -> RepoSlug {
        RepoSlug {
            owner: "org".to_string(),
            repo: "mono".to_string(),
        }
    }

    #[test]
    fn takes_its_shape_from_the_api_run() {
        let watched = WatchedRun::from_api("mt-risks", &slug(), &api_run("in_progress", None));

        assert_eq!(watched.run_id, 7);
        assert_eq!(watched.outcome, "in_progress");
        assert!(!watched.finished);
        assert_eq!(watched.label(), "margin-engine-0.1.5");
    }

    #[test]
    fn a_moved_outcome_is_reported_as_a_change() {
        let mut watched = WatchedRun::from_api("r", &slug(), &api_run("queued", None));

        assert!(watched.apply(&api_run("in_progress", None)));
        // The same reading again is not a change — the feed must not repeat it.
        assert!(!watched.apply(&api_run("in_progress", None)));

        assert!(watched.apply(&api_run("completed", Some("success"))));
        assert!(watched.finished);
        assert_eq!(watched.outcome, "success");
    }

    #[test]
    fn the_label_falls_back_to_the_workflow_when_there_is_no_tag() {
        let mut run = api_run("queued", None);
        run.head_branch = None;

        assert_eq!(
            WatchedRun::from_api("r", &slug(), &run).label(),
            "Release App"
        );
    }

    #[test]
    fn a_finished_run_stops_accumulating_elapsed_time() {
        let mut watched = WatchedRun::from_api("r", &slug(), &api_run("queued", None));
        watched.apply(&api_run("completed", Some("success")));

        let much_later = DateTimeAsMicroseconds {
            unix_microseconds: watched.last_checked.unix_microseconds + 60_000_000,
        };

        // Frozen at completion, not still counting up.
        assert!(watched.elapsed_sec(much_later) < 1.0);
    }
}
