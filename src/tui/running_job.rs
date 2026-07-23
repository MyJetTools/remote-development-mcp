use std::sync::Arc;

use rust_extensions::date_time::DateTimeAsMicroseconds;

use crate::{jobs::JobStateFilter, repo::RepoContext};

/// One line of the "Running" pane.
pub struct RunningJob {
    pub repo: String,
    pub job_id: String,
    pub command_line: String,
    pub cwd: String,
    pub elapsed_sec: f64,
}

/// Collects what is running right now, across every repository.
///
/// Read straight from each repository's job registry rather than from a separate
/// list the console would have to keep in step: the registry is already the
/// source of truth the tools answer from, so the console can never disagree with
/// what `list_jobs` reports.
pub fn collect_running(repos: &[Arc<RepoContext>]) -> Vec<RunningJob> {
    let now = DateTimeAsMicroseconds::now();

    let mut result = Vec::new();

    for repo in repos.iter() {
        for job in repo.jobs.list(JobStateFilter::Running) {
            result.push(RunningJob {
                repo: repo.name.clone(),
                job_id: job.id.clone(),
                command_line: job.command_line.clone(),
                cwd: job.cwd.clone(),
                elapsed_sec: job.duration_sec(now),
            });
        }
    }

    // Longest-running first — a build that has been going for ten minutes is
    // what someone watching the console wants to see at the top.
    result.sort_by(|left, right| {
        right
            .elapsed_sec
            .partial_cmp(&left.elapsed_sec)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    result
}

/// `1m 04s` / `12s` — short enough to sit in a narrow column.
pub fn render_elapsed(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;

    if seconds < 60 {
        return format!("{}s", seconds);
    }

    let minutes = seconds / 60;
    let rest = seconds % 60;

    if minutes < 60 {
        return format!("{}m {:02}s", minutes, rest);
    }

    format!("{}h {:02}m", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_is_readable_at_every_scale() {
        assert_eq!(render_elapsed(0.0), "0s");
        assert_eq!(render_elapsed(12.7), "12s");
        assert_eq!(render_elapsed(64.0), "1m 04s");
        assert_eq!(render_elapsed(3600.0), "1h 00m");
        assert_eq!(render_elapsed(7_500.0), "2h 05m");
    }

    #[test]
    fn a_negative_elapsed_does_not_wrap_around() {
        // Clocks can step backwards; the console must not print nonsense.
        assert_eq!(render_elapsed(-5.0), "0s");
    }
}
