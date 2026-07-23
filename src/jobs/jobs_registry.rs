use std::path::Path;

use parking_lot::RwLock;
use rust_extensions::date_time::DateTimeAsMicroseconds;

use super::{Job, JobStateFilter, JobStatus, JobsRegistryInner};

/// Every project keeps its own registry, so the concurrency limit is per
/// project. Job ids carry their project as a prefix (`my-ssh:job-000001`), which
/// makes them unambiguous across the whole server — an endpoint serving several
/// projects can route a job id back to its own registry without being told which
/// project it belongs to.
///
/// `parking_lot` rather than `tokio::sync`: nothing is awaited under the lock —
/// the critical section only touches the map. The guard being `!Send` is a
/// feature here, since it turns "held a lock across the spawn of a job" into a
/// compile error rather than a deadlock.
pub struct JobsRegistry {
    inner: RwLock<JobsRegistryInner>,
}

impl JobsRegistry {
    pub fn new(max_concurrent_jobs: usize, project_id: String) -> Self {
        Self {
            inner: RwLock::new(JobsRegistryInner::new(max_concurrent_jobs, project_id)),
        }
    }

    pub fn try_register(
        &self,
        command_line: String,
        cwd: String,
        logs_dir: &Path,
        now: DateTimeAsMicroseconds,
        timeout_sec: u64,
    ) -> Result<Job, String> {
        self.inner
            .write()
            .try_register(command_line, cwd, logs_dir, now, timeout_sec)
    }

    pub fn set_pid(&self, id: &str, pid: Option<u32>) {
        self.inner.write().set_pid(id, pid);
    }

    pub fn complete(
        &self,
        id: &str,
        status: JobStatus,
        exit_code: Option<i32>,
        now: DateTimeAsMicroseconds,
    ) {
        self.inner.write().complete(id, status, exit_code, now);
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.inner.read().get(id)
    }

    pub fn request_kill(&self, id: &str) -> Option<Job> {
        self.inner.write().request_kill(id)
    }

    pub fn list(&self, filter: JobStateFilter) -> Vec<Job> {
        self.inner.read().list(filter)
    }
}
