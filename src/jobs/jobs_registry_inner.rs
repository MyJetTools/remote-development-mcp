use std::path::Path;

use ahash::AHashMap;
use rust_extensions::date_time::DateTimeAsMicroseconds;

use super::{Job, JobStateFilter, JobStatus};

/// Finished jobs are remembered so a client can still collect the result of a
/// build it stopped polling. Past this many, the oldest are forgotten — their
/// log files stay on disk either way.
const MAX_KEPT_FINISHED_JOBS: usize = 200;

/// The stored command line is capped so the registry's retained memory can not
/// be driven by however long a command a caller chooses to send: up to
/// [`MAX_KEPT_FINISHED_JOBS`] of these are held at once.
const MAX_STORED_COMMAND_LINE: usize = 512;

pub(super) struct JobsRegistryInner {
    jobs: AHashMap<String, Job>,
    max_concurrent_jobs: usize,
    next_id: u64,
}

impl JobsRegistryInner {
    pub(super) fn new(max_concurrent_jobs: usize) -> Self {
        Self {
            jobs: AHashMap::new(),
            max_concurrent_jobs,
            next_id: 1,
        }
    }

    /// Reserves a job slot and its identity in one step, so two concurrent
    /// `run_command` calls can not both pass the concurrency check.
    pub(super) fn try_register(
        &mut self,
        command_line: String,
        cwd: String,
        logs_dir: &Path,
        now: DateTimeAsMicroseconds,
        timeout_sec: u64,
    ) -> Result<Job, String> {
        let running = self.running_amount();

        if running >= self.max_concurrent_jobs {
            return Err(format!(
                "Can not start a new job: {} of {} job slots are busy. \
                 Wait for one to finish or kill it with kill_job",
                running, self.max_concurrent_jobs
            ));
        }

        let id = format!("job-{:06}", self.next_id);
        self.next_id += 1;

        let job = Job {
            id: id.clone(),
            command_line: truncate_on_char_boundary(command_line, MAX_STORED_COMMAND_LINE),
            cwd,
            status: JobStatus::Running,
            exit_code: None,
            pid: None,
            kill_requested: false,
            started_at: now,
            finished_at: None,
            timeout_sec,
            stdout_log: logs_dir.join(format!("{}.stdout.log", id)),
            stderr_log: logs_dir.join(format!("{}.stderr.log", id)),
        };

        self.jobs.insert(id, job.clone());

        Ok(job)
    }

    pub(super) fn set_pid(&mut self, id: &str, pid: Option<u32>) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.pid = pid;
        }
    }

    pub(super) fn complete(
        &mut self,
        id: &str,
        status: JobStatus,
        exit_code: Option<i32>,
        now: DateTimeAsMicroseconds,
    ) {
        if let Some(job) = self.jobs.get_mut(id) {
            job.status = status;
            job.exit_code = exit_code;
            job.finished_at = Some(now);
        }

        self.prune_finished();
    }

    pub(super) fn get(&self, id: &str) -> Option<Job> {
        self.jobs.get(id).cloned()
    }

    /// Marks the job as "kill was asked for" and hands back what is needed to
    /// actually signal it. Returns `None` when there is no such job.
    pub(super) fn request_kill(&mut self, id: &str) -> Option<Job> {
        let job = self.jobs.get_mut(id)?;

        if job.status.is_running() {
            job.kill_requested = true;
        }

        Some(job.clone())
    }

    pub(super) fn list(&self, filter: JobStateFilter) -> Vec<Job> {
        let mut result: Vec<Job> = self
            .jobs
            .values()
            .filter(|job| filter.matches(job.status))
            .cloned()
            .collect();

        // By start time, not by id: the zero-padded id stops sorting
        // chronologically once it passes six digits, and start time is what the
        // caller actually means by "oldest first".
        result.sort_by(|left, right| {
            left.started_at
                .unix_microseconds
                .cmp(&right.started_at.unix_microseconds)
                .then_with(|| left.id.cmp(&right.id))
        });

        result
    }

    pub(super) fn running_amount(&self) -> usize {
        self.jobs
            .values()
            .filter(|job| job.status.is_running())
            .count()
    }

    fn prune_finished(&mut self) {
        // Keyed on when the job finished, so the *oldest-finished* are dropped
        // and a job that finished a moment ago — the one a client is most likely
        // still polling — survives. Sorting by id would drop by start order and,
        // past six-digit ids, invert and drop the newest results.
        let mut finished: Vec<(String, i64)> = self
            .jobs
            .values()
            .filter(|job| !job.status.is_running())
            .map(|job| {
                let when = job.finished_at.unwrap_or(job.started_at).unix_microseconds;
                (job.id.clone(), when)
            })
            .collect();

        if finished.len() <= MAX_KEPT_FINISHED_JOBS {
            return;
        }

        finished.sort_by_key(|(_, when)| *when);

        let to_remove = finished.len() - MAX_KEPT_FINISHED_JOBS;

        for (id, _) in finished.iter().take(to_remove) {
            self.jobs.remove(id);
        }
    }
}

/// Truncates a string to at most `max_bytes`, never splitting a character, and
/// appends an ellipsis when it had to cut.
fn truncate_on_char_boundary(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }

    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    text.truncate(end);
    text.push('…');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logs_dir() -> std::path::PathBuf {
        std::path::PathBuf::from("/tmp/remote-development-mcp-tests-registry")
    }

    fn register(inner: &mut JobsRegistryInner) -> Result<Job, String> {
        inner.try_register(
            "cargo build".to_string(),
            ".".to_string(),
            &logs_dir(),
            DateTimeAsMicroseconds::now(),
            3600,
        )
    }

    #[test]
    fn ids_are_sequential_and_sortable() {
        let mut inner = JobsRegistryInner::new(10);

        let first = register(&mut inner).unwrap();
        let second = register(&mut inner).unwrap();

        assert_eq!(first.id, "job-000001");
        assert_eq!(second.id, "job-000002");
        assert!(first.id < second.id);
    }

    #[test]
    fn concurrency_limit_is_enforced() {
        let mut inner = JobsRegistryInner::new(2);

        register(&mut inner).unwrap();
        register(&mut inner).unwrap();

        let err = register(&mut inner).unwrap_err();

        assert!(err.contains("job slots are busy"), "{}", err);
    }

    #[test]
    fn finishing_a_job_frees_its_slot() {
        let mut inner = JobsRegistryInner::new(1);

        let job = register(&mut inner).unwrap();

        assert!(register(&mut inner).is_err());

        inner.complete(
            &job.id,
            JobStatus::Exited,
            Some(0),
            DateTimeAsMicroseconds::now(),
        );

        assert!(register(&mut inner).is_ok());
    }

    #[test]
    fn completed_job_keeps_its_result() {
        let mut inner = JobsRegistryInner::new(4);

        let job = register(&mut inner).unwrap();

        inner.complete(
            &job.id,
            JobStatus::TimedOut,
            None,
            DateTimeAsMicroseconds::now(),
        );

        let stored = inner.get(&job.id).unwrap();

        assert_eq!(stored.status, JobStatus::TimedOut);
        assert!(stored.exit_code.is_none());
        assert!(stored.finished_at.is_some());
    }

    #[test]
    fn list_filters_by_state() {
        let mut inner = JobsRegistryInner::new(4);

        let finished = register(&mut inner).unwrap();
        let running = register(&mut inner).unwrap();

        inner.complete(
            &finished.id,
            JobStatus::Exited,
            Some(0),
            DateTimeAsMicroseconds::now(),
        );

        let all = inner.list(JobStateFilter::All);
        assert_eq!(all.len(), 2);

        let still_running = inner.list(JobStateFilter::Running);
        assert_eq!(still_running.len(), 1);
        assert_eq!(still_running[0].id, running.id);

        let done = inner.list(JobStateFilter::Finished);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].id, finished.id);
    }

    #[test]
    fn old_finished_jobs_are_pruned_and_running_ones_are_kept() {
        let mut inner = JobsRegistryInner::new(usize::MAX);

        let kept_running = register(&mut inner).unwrap();

        for _ in 0..(MAX_KEPT_FINISHED_JOBS + 50) {
            let job = register(&mut inner).unwrap();
            inner.complete(
                &job.id,
                JobStatus::Exited,
                Some(0),
                DateTimeAsMicroseconds::now(),
            );
        }

        assert_eq!(
            inner.list(JobStateFilter::Finished).len(),
            MAX_KEPT_FINISHED_JOBS
        );
        assert!(inner.get(&kept_running.id).is_some());
    }
}
