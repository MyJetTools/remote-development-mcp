use std::{sync::Arc, time::Duration};

use crate::{
    activity::{ActivityEvent, ActivityLog},
    github::GitHubClient,
};

use super::WatchedRuns;

/// How often GitHub is asked. Ten seconds is what a person watching a build
/// wants, and it costs one request per unfinished run — nothing next to the
/// 5000/hour the API allows.
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Stops following a build that never ends, so a stuck or deleted run can not
/// be polled for the lifetime of the process.
const GIVE_UP_AFTER: Duration = Duration::from_secs(6 * 60 * 60);

/// Keeps the watched runs fresh in the background.
///
/// Runs whether or not anyone is asking: that is what lets the console show a
/// build finishing on its own, and what makes the tool's answer instant rather
/// than a round trip to GitHub.
pub async fn run_poller(runs: Arc<WatchedRuns>, activity: Arc<ActivityLog>, token: Option<String>) {
    // With no token there is nothing to poll with — every call would fail, so
    // the loop would be pure noise.
    let token = match token {
        Some(token) => token,
        None => return,
    };

    let client = GitHubClient::new(token);

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        for watched in runs.unfinished() {
            let now = rust_extensions::date_time::DateTimeAsMicroseconds::now();

            if watched.elapsed_sec(now) > GIVE_UP_AFTER.as_secs() as f64 {
                continue;
            }

            let fresh = match client
                .get_workflow_run(&watched.slug(), watched.run_id)
                .await
            {
                Ok(fresh) => fresh,
                // A transient API failure must not end the watch — the next tick
                // tries again.
                Err(_) => continue,
            };

            // Only asked for once the run has actually failed, so a healthy build
            // costs one request per tick rather than two.
            let failed_step = if fresh.failed() {
                client.failed_step(&watched.slug(), watched.run_id).await
            } else {
                None
            };

            let changed = runs.apply(watched.run_id, &fresh, failed_step);

            if let Some(changed) = changed {
                activity.push(ActivityEvent::action_run(
                    changed.repo.clone(),
                    changed.label(),
                    match changed.failed_step.as_ref() {
                        Some(step) => format!("{} — {}", changed.outcome, step),
                        None => changed.outcome.clone(),
                    },
                ));
            }
        }
    }
}
