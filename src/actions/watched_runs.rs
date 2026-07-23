use ahash::AHashMap;
use parking_lot::Mutex;

use super::WatchedRun;

/// A finished run stays visible for a while so the console still shows what a
/// build ended as, then makes room.
const MAX_KEPT: usize = 50;

/// The runs the server is following.
///
/// `parking_lot` because nothing is awaited under the lock — the poller does its
/// HTTP work outside it and only comes back to write the result.
pub struct WatchedRuns {
    runs: Mutex<AHashMap<u64, WatchedRun>>,
}

impl WatchedRuns {
    pub fn new() -> Self {
        Self {
            runs: Mutex::new(AHashMap::new()),
        }
    }

    /// Starts following a run, or refreshes one already followed.
    pub fn watch(&self, run: WatchedRun) {
        let mut runs = self.runs.lock();

        match runs.get_mut(&run.run_id) {
            // Keep the original first_seen so the elapsed time stays honest.
            Some(existing) => {
                existing.outcome = run.outcome;
                existing.finished = run.finished;
                existing.url = run.url;
                existing.last_checked = run.last_checked;
            }
            None => {
                runs.insert(run.run_id, run);
            }
        }

        prune(&mut runs);
    }

    /// The runs still being built — what the poller has to ask about.
    pub fn unfinished(&self) -> Vec<WatchedRun> {
        self.runs
            .lock()
            .values()
            .filter(|run| !run.finished)
            .cloned()
            .collect()
    }

    /// Applies a fresh reading. Returns the updated run when its outcome moved,
    /// so the caller knows whether to announce it.
    pub fn apply(
        &self,
        run_id: u64,
        fresh: &crate::github::WorkflowRun,
        failed_step: Option<String>,
    ) -> Option<WatchedRun> {
        let mut runs = self.runs.lock();

        let watched = runs.get_mut(&run_id)?;

        let changed = watched.apply(fresh);

        if failed_step.is_some() {
            watched.failed_step = failed_step;
        }

        if changed {
            return Some(watched.clone());
        }

        None
    }

    /// Everything followed, newest first.
    pub fn all(&self) -> Vec<WatchedRun> {
        let mut result: Vec<WatchedRun> = self.runs.lock().values().cloned().collect();

        result.sort_by(|left, right| {
            right
                .first_seen
                .unix_microseconds
                .cmp(&left.first_seen.unix_microseconds)
        });

        result
    }
}

fn prune(runs: &mut AHashMap<u64, WatchedRun>) {
    if runs.len() <= MAX_KEPT {
        return;
    }

    // Only finished runs are candidates — dropping one still being built would
    // stop the poller following it.
    let mut finished: Vec<(u64, i64)> = runs
        .values()
        .filter(|run| run.finished)
        .map(|run| (run.run_id, run.first_seen.unix_microseconds))
        .collect();

    if finished.is_empty() {
        return;
    }

    finished.sort_by_key(|(_, when)| *when);

    let to_remove = runs.len() - MAX_KEPT;

    for (run_id, _) in finished.iter().take(to_remove) {
        runs.remove(run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::github::{RepoSlug, WorkflowRun};

    fn api_run(id: u64, status: &str, conclusion: Option<&str>) -> WorkflowRun {
        WorkflowRun {
            id,
            name: Some("Release App".to_string()),
            display_title: None,
            head_branch: Some(format!("svc-0.1.{}", id)),
            status: status.to_string(),
            conclusion: conclusion.map(|value| value.to_string()),
            event: None,
            run_number: None,
            run_started_at: None,
            updated_at: None,
            html_url: None,
        }
    }

    fn slug() -> RepoSlug {
        RepoSlug {
            owner: "org".to_string(),
            repo: "mono".to_string(),
        }
    }

    fn watched(id: u64, status: &str, conclusion: Option<&str>) -> WatchedRun {
        WatchedRun::from_api("r", &slug(), &api_run(id, status, conclusion))
    }

    #[test]
    fn only_unfinished_runs_are_polled() {
        let runs = WatchedRuns::new();

        runs.watch(watched(1, "in_progress", None));
        runs.watch(watched(2, "completed", Some("success")));

        let pending = runs.unfinished();

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].run_id, 1);
    }

    #[test]
    fn a_change_is_reported_once_and_not_repeated() {
        let runs = WatchedRuns::new();
        runs.watch(watched(1, "queued", None));

        assert!(runs
            .apply(1, &api_run(1, "in_progress", None), None)
            .is_some());
        assert!(runs
            .apply(1, &api_run(1, "in_progress", None), None)
            .is_none());

        let done = runs
            .apply(
                1,
                &api_run(1, "completed", Some("failure")),
                Some("build / test".to_string()),
            )
            .unwrap();

        assert_eq!(done.outcome, "failure");
        assert_eq!(done.failed_step.as_deref(), Some("build / test"));
    }

    #[test]
    fn applying_to_an_unknown_run_is_harmless() {
        let runs = WatchedRuns::new();

        assert!(runs
            .apply(99, &api_run(99, "completed", Some("success")), None)
            .is_none());
    }

    #[test]
    fn watching_the_same_run_again_keeps_when_it_was_first_seen() {
        let runs = WatchedRuns::new();

        runs.watch(watched(1, "queued", None));
        let first_seen = runs.all()[0].first_seen.unix_microseconds;

        runs.watch(watched(1, "in_progress", None));

        assert_eq!(runs.all().len(), 1);
        assert_eq!(runs.all()[0].first_seen.unix_microseconds, first_seen);
        assert_eq!(runs.all()[0].outcome, "in_progress");
    }

    #[test]
    fn the_registry_stays_bounded_but_never_drops_a_running_build() {
        let runs = WatchedRuns::new();

        runs.watch(watched(1, "in_progress", None));

        for id in 2..(MAX_KEPT as u64 + 30) {
            runs.watch(watched(id, "completed", Some("success")));
        }

        assert!(runs.all().len() <= MAX_KEPT);
        // The one still building survived the pruning.
        assert!(runs.all().iter().any(|run| run.run_id == 1));
    }
}
