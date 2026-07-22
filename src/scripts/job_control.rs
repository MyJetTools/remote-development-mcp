use std::sync::Arc;

use crate::{
    jobs::{clamp_read_bytes, read_log_at, Job, JobStateFilter, KillSignal, OutputStream},
    repo::RepoContext,
};

use super::kill_process_group;

pub struct JobOutputRequest {
    pub job_id: String,
    pub stream: OutputStream,
    pub stdout_cursor: u64,
    pub stderr_cursor: u64,
    pub max_bytes: Option<u64>,
}

pub struct JobOutputResult {
    pub job: Job,
    pub stdout: String,
    pub stderr: String,
    pub next_stdout_cursor: u64,
    pub next_stderr_cursor: u64,
    pub has_more: bool,
}

pub struct KillJobResult {
    pub job: Job,
    /// False when the job had already finished, so nothing was signalled.
    pub signalled: bool,
}

pub async fn get_job_output(
    repo: &Arc<RepoContext>,
    request: JobOutputRequest,
) -> Result<JobOutputResult, String> {
    let job = get_job(repo, &request.job_id)?;

    let max_bytes = clamp_read_bytes(request.max_bytes);

    // Each stream carries its own offset because they are separate files. One
    // shared cursor could not address both without losing or repeating output.
    let stdout = if request.stream.reads_stdout() {
        read_log_at(&job.stdout_log, request.stdout_cursor, max_bytes).await?
    } else {
        skipped(request.stdout_cursor)
    };

    let stderr = if request.stream.reads_stderr() {
        read_log_at(&job.stderr_log, request.stderr_cursor, max_bytes).await?
    } else {
        skipped(request.stderr_cursor)
    };

    Ok(JobOutputResult {
        job,
        stdout: stdout.text,
        stderr: stderr.text,
        next_stdout_cursor: stdout.next_cursor,
        next_stderr_cursor: stderr.next_cursor,
        has_more: stdout.has_more || stderr.has_more,
    })
}

pub fn list_jobs(repo: &Arc<RepoContext>, filter: JobStateFilter) -> Vec<Job> {
    repo.jobs.list(filter)
}

pub fn kill_job(
    repo: &Arc<RepoContext>,
    job_id: &str,
    signal: KillSignal,
) -> Result<KillJobResult, String> {
    let job = match repo.jobs.request_kill(job_id) {
        Some(job) => job,
        None => return Err(not_found(repo, job_id)),
    };

    if !job.status.is_running() {
        return Ok(KillJobResult {
            job,
            signalled: false,
        });
    }

    // No pid on a running job means one of two brief windows: the process is
    // still being started, or it has just been reaped and is being finalized
    // (the supervisor clears the pid before it flips the status). Either way
    // `request_kill` above has recorded the intent, so the supervisor will mark
    // it `killed` — there is simply nothing to signal right now. Reporting this
    // as an error would be misleading, and signalling a stale pid would be
    // dangerous, since the OS may already have recycled it.
    let pid = match job.pid {
        Some(pid) => pid,
        None => {
            return Ok(KillJobResult {
                job,
                signalled: false,
            })
        }
    };

    kill_process_group(pid, signal)?;

    Ok(KillJobResult {
        job,
        signalled: true,
    })
}

fn get_job(repo: &Arc<RepoContext>, job_id: &str) -> Result<Job, String> {
    match repo.jobs.get(job_id) {
        Some(job) => Ok(job),
        None => Err(not_found(repo, job_id)),
    }
}

fn not_found(repo: &Arc<RepoContext>, job_id: &str) -> String {
    format!(
        "There is no job '{}' in repository '{}'. Old finished jobs are eventually forgotten — \
         use list_jobs to see what is still known",
        job_id, repo.name
    )
}

fn skipped(cursor: u64) -> crate::jobs::JobLogChunk {
    crate::jobs::JobLogChunk {
        text: String::new(),
        next_cursor: cursor,
        has_more: false,
    }
}
