use std::{process::Stdio, sync::Arc, time::Duration};

use rust_extensions::date_time::DateTimeAsMicroseconds;
use tokio::process::Command;

use crate::{
    audit::{AuditCommandFinished, AuditCommandRefused, AuditCommandStarted},
    jobs::{pump_stream, read_log_at, Job, JobStatus, KillSignal},
    repo::RepoContext,
};

use super::kill_process_group;

/// How long a timed-out job is given to wind down after `TERM` before it gets
/// `KILL`. Long enough for `cargo` to remove its lock file, short enough that a
/// wedged process does not hold a job slot.
const TERM_GRACE: Duration = Duration::from_secs(10);

/// How long the supervisor waits for the log pumps to drain after the child has
/// exited. Normally instant — the child closing its pipes ends the pumps at
/// once. It only matters when a process the command backgrounded inherited a
/// pipe and is holding it open; past this the group is killed to force EOF so
/// the job can never be stuck reporting `running` forever.
const DRAIN_GRACE: Duration = Duration::from_secs(5);

/// Size of the inline preview `run_command` returns. The full output always
/// stays available through `get_job_output`.
const INLINE_PREVIEW_BYTES: u64 = 16 * 1024;

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// `wait_sec` is only an inline-response convenience; anything past a couple of
/// minutes is meaningless and an unclamped value overflows the deadline
/// arithmetic. `timeout_sec` is capped too so a job can not be handed an
/// effectively infinite deadline.
const MAX_WAIT_SEC: u64 = 120;
const MAX_TIMEOUT_SEC: u64 = 24 * 60 * 60;

pub struct EnvVar {
    pub name: String,
    pub value: String,
}

pub struct RunCommandRequest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<EnvVar>,
    pub timeout_sec: Option<u64>,
    /// Seconds to wait inline before returning. A command that finishes inside
    /// it comes back complete in one call; anything longer becomes a job to
    /// poll.
    pub wait_sec: Option<u64>,
}

pub struct RunCommandResult {
    pub job: Job,
    pub stdout: String,
    pub stderr: String,
    pub next_stdout_cursor: u64,
    pub next_stderr_cursor: u64,
    pub has_more: bool,
}

pub async fn run_command(
    repo: &Arc<RepoContext>,
    request: RunCommandRequest,
) -> Result<RunCommandResult, String> {
    let command_line = render_command_line(&request.command, &request.args);

    let refusal = repo.command_policy.check(&request.command).and_then(|_| {
        repo.command_policy
            .check_env(request.env.iter().map(|env| env.name.as_str()))
    });

    if let Err(err) = refusal {
        repo.audit
            .command_refused(AuditCommandRefused {
                repo: &repo.name,
                command_line: &command_line,
                reason: &err,
            })
            .await;

        return Err(err);
    }

    let cwd = match request.cwd.as_ref() {
        Some(cwd) => repo.resolve_path(cwd)?,
        None => repo.root().to_path_buf(),
    };

    if !cwd.is_dir() {
        return Err(format!(
            "Working directory '{}' does not exist inside the repository",
            repo.to_relative(&cwd)
        ));
    }

    let now = DateTimeAsMicroseconds::now();

    let job = repo.jobs.try_register(
        command_line.clone(),
        repo.to_relative(&cwd),
        &repo.logs_dir,
        now,
    )?;

    let mut command = Command::new(&request.command);

    command
        .args(&request.args)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for env in request.env.iter() {
        command.env(&env.name, &env.value);
    }

    // Its own process group, so that killing the job takes the whole tree with
    // it. Signalling `cargo` alone would leave `rustc` running.
    command.process_group(0);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            // Release the slot the registration just took, otherwise a few
            // typos would exhaust the concurrency limit.
            repo.jobs.complete(
                &job.id,
                JobStatus::Exited,
                None,
                DateTimeAsMicroseconds::now(),
            );

            return Err(format!("Can not start '{}'. Err: {}", request.command, err));
        }
    };

    let pid = child.id();
    repo.jobs.set_pid(&job.id, pid);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_pump = stdout.map(|stdout| {
        tokio::spawn(pump_stream(
            stdout,
            job.stdout_log.clone(),
            repo.max_log_bytes,
        ))
    });

    let stderr_pump = stderr.map(|stderr| {
        tokio::spawn(pump_stream(
            stderr,
            job.stderr_log.clone(),
            repo.max_log_bytes,
        ))
    });

    repo.audit
        .command_started(AuditCommandStarted {
            repo: &repo.name,
            job_id: &job.id,
            command_line: &command_line,
            cwd: &job.cwd,
        })
        .await;

    let timeout_sec = match request.timeout_sec {
        Some(timeout_sec) => timeout_sec,
        None => repo.default_timeout_sec,
    }
    .min(MAX_TIMEOUT_SEC);

    tokio::spawn(supervise_job(
        repo.clone(),
        job.id.clone(),
        command_line,
        child,
        timeout_sec,
        SupervisedPumps {
            stdout: stdout_pump,
            stderr: stderr_pump,
        },
    ));

    let job = wait_for_completion(repo, job, request.wait_sec).await;

    read_result(job).await
}

struct SupervisedPumps {
    stdout: Option<tokio::task::JoinHandle<()>>,
    stderr: Option<tokio::task::JoinHandle<()>>,
}

/// Owns the child for the rest of its life: waits for it, enforces the timeout,
/// then records the outcome. Runs detached, which is what lets a ten-minute
/// build outlive the tool call that started it.
async fn supervise_job(
    repo: Arc<RepoContext>,
    job_id: String,
    command_line: String,
    mut child: tokio::process::Child,
    timeout_sec: u64,
    pumps: SupervisedPumps,
) {
    // Captured before the child is reaped, because `child.id()` returns None
    // afterwards and the drain path may still need to signal the group.
    let group_pid = child.id();

    let timed_out = match tokio::time::timeout(Duration::from_secs(timeout_sec), child.wait()).await
    {
        Ok(_) => false,
        Err(_) => {
            terminate(&mut child, &job_id).await;
            true
        }
    };

    let exit_status = match child.try_wait() {
        Ok(exit_status) => exit_status,
        Err(err) => {
            my_logger::LOGGER.write_error(
                "supervise_job",
                format!("Can not read the job exit status. Err: {:?}", err),
                my_logger::LogEventCtx::new()
                    .add("repo", repo.name.clone())
                    .add("job_id", job_id.clone()),
            );
            None
        }
    };

    // The child's pid is reaped now; stop advertising it so a late kill_job can
    // not fire a signal at a pid the OS may already have recycled.
    repo.jobs.set_pid(&job_id, None);

    // The logs should be complete before the job is reported finished — a client
    // that sees `exited` reads the log once and stops polling. But the wait is
    // bounded: a process the command backgrounded can inherit the pipe and keep
    // it open after the child exits, and an unbounded await there is what would
    // otherwise pin the job at `running` and leak its slot forever.
    drain_pumps(pumps, group_pid).await;

    let kill_requested = match repo.jobs.get(&job_id) {
        Some(job) => job.kill_requested,
        None => false,
    };

    let status = if timed_out {
        JobStatus::TimedOut
    } else if kill_requested {
        JobStatus::Killed
    } else {
        JobStatus::Exited
    };

    let exit_code = exit_status.and_then(|exit_status| exit_status.code());

    let now = DateTimeAsMicroseconds::now();

    repo.jobs.complete(&job_id, status, exit_code, now);

    let duration_sec = match repo.jobs.get(&job_id) {
        Some(job) => job.duration_sec(now),
        None => 0.0,
    };

    repo.audit
        .command_finished(AuditCommandFinished {
            repo: &repo.name,
            job_id: &job_id,
            command_line: &command_line,
            status: status.as_str(),
            exit_code,
            duration_sec,
        })
        .await;
}

/// Waits for the log pumps to drain, but not forever.
///
/// A pump ends only when every copy of its pipe's write end is closed, which the
/// direct child exiting normally achieves at once. If a backgrounded descendant
/// inherited the pipe, that never happens on its own — so past `DRAIN_GRACE` the
/// process group is killed to force EOF, and the (detached) pump tasks finish
/// and flush by themselves once the pipe closes.
async fn drain_pumps(pumps: SupervisedPumps, group_pid: Option<u32>) {
    let joined = async {
        if let Some(stdout_pump) = pumps.stdout {
            let _ = stdout_pump.await;
        }
        if let Some(stderr_pump) = pumps.stderr {
            let _ = stderr_pump.await;
        }
    };

    if tokio::time::timeout(DRAIN_GRACE, joined).await.is_err() {
        if let Some(pid) = group_pid {
            let _ = kill_process_group(pid, KillSignal::Kill);
        }
    }
}

/// `TERM` first so the process can clean up, `KILL` if it will not go.
async fn terminate(child: &mut tokio::process::Child, job_id: &str) {
    let pid = match child.id() {
        Some(pid) => pid,
        None => return,
    };

    if let Err(err) = kill_process_group(pid, KillSignal::Term) {
        my_logger::LOGGER.write_warning(
            "terminate",
            format!("Can not send TERM to the job. Err: {}", err),
            my_logger::LogEventCtx::new().add("job_id", job_id.to_string()),
        );
    }

    if tokio::time::timeout(TERM_GRACE, child.wait()).await.is_ok() {
        return;
    }

    if let Err(err) = kill_process_group(pid, KillSignal::Kill) {
        my_logger::LOGGER.write_warning(
            "terminate",
            format!("Can not send KILL to the job. Err: {}", err),
            my_logger::LogEventCtx::new().add("job_id", job_id.to_string()),
        );
    }

    let _ = child.wait().await;
}

/// Gives a short command the chance to come back inline.
///
/// The supervisor owns the child, so completion is observed through the
/// registry rather than by awaiting the process. `job` is the state as
/// registered, and is what gets returned if the registry ever stops knowing
/// about it.
async fn wait_for_completion(repo: &Arc<RepoContext>, job: Job, wait_sec: Option<u64>) -> Job {
    let wait_sec = wait_sec.unwrap_or_default().min(MAX_WAIT_SEC);

    // `checked_add` rather than `+`: the plain operator panics on overflow, and
    // although wait_sec is clamped above, the fallback keeps the arithmetic
    // honest regardless.
    let deadline = tokio::time::Instant::now()
        .checked_add(Duration::from_secs(wait_sec))
        .unwrap_or_else(tokio::time::Instant::now);

    let mut current = job;

    while let Some(latest) = repo.jobs.get(&current.id) {
        current = latest;

        if !current.status.is_running() {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            break;
        }

        tokio::time::sleep(WAIT_POLL_INTERVAL).await;
    }

    current
}

async fn read_result(job: Job) -> Result<RunCommandResult, String> {
    let stdout = read_log_at(&job.stdout_log, 0, INLINE_PREVIEW_BYTES).await?;
    let stderr = read_log_at(&job.stderr_log, 0, INLINE_PREVIEW_BYTES).await?;

    Ok(RunCommandResult {
        job,
        stdout: stdout.text,
        stderr: stderr.text,
        next_stdout_cursor: stdout.next_cursor,
        next_stderr_cursor: stderr.next_cursor,
        has_more: stdout.has_more || stderr.has_more,
    })
}

fn render_command_line(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        return command.to_string();
    }

    format!("{} {}", command, args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        audit::AuditLog,
        jobs::OutputStream,
        scripts::{get_job_output, kill_job, JobOutputRequest},
        settings::{CommandMode, RepoSettings, SettingsModel},
    };

    /// Builds a throwaway repository backed by a temp folder.
    ///
    /// Passthrough mode on purpose: these tests are about the job machinery, and
    /// they need a shell to produce output slowly enough to be polled. The
    /// allowlist itself is covered by the command policy tests.
    async fn test_repo(name: &str) -> Arc<RepoContext> {
        build_repo(name, CommandMode::Passthrough, Vec::new()).await
    }

    async fn build_repo(
        name: &str,
        command_mode: CommandMode,
        command_allowlist: Vec<String>,
    ) -> Arc<RepoContext> {
        let base = std::env::temp_dir()
            .join("remote-development-mcp-tests-jobs")
            .join(name);

        let _ = std::fs::remove_dir_all(&base);

        let root = base.join("repo");
        std::fs::create_dir_all(&root).unwrap();

        let settings = SettingsModel {
            bind_addr: "127.0.0.1:0".to_string(),
            auth_token: "test-token".to_string(),
            repos: Vec::new(),
            command_mode,
            command_allowlist,
            max_concurrent_jobs: 4,
            default_timeout_sec: 60,
            max_log_bytes: 1024 * 1024,
            logs_path: base.join("logs").to_string_lossy().to_string(),
            audit_log_path: None,
        };

        let repo_settings = RepoSettings {
            mcp_path: format!("/{}", name),
            root: root.to_string_lossy().to_string(),
            description: None,
            command_mode: None,
            command_allowlist: None,
            allow_delete: false,
        };

        let audit = Arc::new(AuditLog::disabled());

        Arc::new(
            RepoContext::new(&settings, &repo_settings, audit)
                .await
                .unwrap(),
        )
    }

    fn shell(script: &str) -> RunCommandRequest {
        RunCommandRequest {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            cwd: None,
            env: Vec::new(),
            timeout_sec: None,
            wait_sec: None,
        }
    }

    /// Polls exactly the way a client is meant to: carry the cursors forward,
    /// keep going while the job runs, then drain whatever is left.
    async fn poll_to_completion(repo: &Arc<RepoContext>, job_id: &str) -> (Job, String) {
        let mut stdout_cursor = 0u64;
        let mut collected = String::new();

        loop {
            let result = get_job_output(
                repo,
                JobOutputRequest {
                    job_id: job_id.to_string(),
                    stream: OutputStream::Both,
                    stdout_cursor,
                    stderr_cursor: 0,
                    max_bytes: Some(crate::jobs::MIN_READ_BYTES),
                },
            )
            .await
            .unwrap();

            collected.push_str(&result.stdout);
            stdout_cursor = result.next_stdout_cursor;

            if !result.job.status.is_running() && !result.has_more {
                return (result.job, collected);
            }

            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn a_long_command_is_polled_to_its_exit_code_without_losing_output() {
        let repo = test_repo("polled_to_exit_code").await;

        // Output arrives in instalments, so the polling loop genuinely has to
        // resume from a cursor several times rather than reading it all at once.
        let started = run_command(
            &repo,
            shell("echo one; sleep 0.2; echo two; sleep 0.2; echo three; exit 7"),
        )
        .await
        .unwrap();

        assert_eq!(started.job.status, JobStatus::Running);

        let (job, collected) = poll_to_completion(&repo, &started.job.id).await;

        assert_eq!(job.status, JobStatus::Exited);
        assert_eq!(job.exit_code, Some(7));
        assert_eq!(collected, "one\ntwo\nthree\n");
    }

    #[tokio::test]
    async fn a_command_finishing_inside_wait_sec_comes_back_inline() {
        let repo = test_repo("inline_result").await;

        let mut request = shell("echo done");
        request.wait_sec = Some(10);

        let result = run_command(&repo, request).await.unwrap();

        assert_eq!(result.job.status, JobStatus::Exited);
        assert_eq!(result.job.exit_code, Some(0));
        assert_eq!(result.stdout, "done\n");
    }

    #[tokio::test]
    async fn stdout_and_stderr_are_captured_separately() {
        let repo = test_repo("separate_streams").await;

        let mut request = shell("echo to-out; echo to-err >&2");
        request.wait_sec = Some(10);

        let result = run_command(&repo, request).await.unwrap();

        assert_eq!(result.stdout, "to-out\n");
        assert_eq!(result.stderr, "to-err\n");
    }

    #[tokio::test]
    async fn a_job_which_overruns_its_timeout_is_reported_as_timed_out() {
        let repo = test_repo("timed_out").await;

        let mut request = shell("sleep 30");
        request.timeout_sec = Some(1);

        let started = run_command(&repo, request).await.unwrap();

        let (job, _) = poll_to_completion(&repo, &started.job.id).await;

        assert_eq!(job.status, JobStatus::TimedOut);
    }

    #[tokio::test]
    async fn a_killed_job_is_reported_as_killed_rather_than_merely_exited() {
        let repo = test_repo("killed").await;

        let started = run_command(&repo, shell("sleep 30")).await.unwrap();

        let killed = kill_job(&repo, &started.job.id, KillSignal::Term).unwrap();
        assert!(killed.signalled);

        let (job, _) = poll_to_completion(&repo, &started.job.id).await;

        assert_eq!(job.status, JobStatus::Killed);
    }

    #[tokio::test]
    async fn a_command_outside_the_allowlist_never_starts() {
        let repo = build_repo(
            "allowlist_refusal",
            CommandMode::Allowlist,
            vec!["cargo".to_string()],
        )
        .await;

        let err = match run_command(&repo, shell("echo hello")).await {
            Ok(_) => panic!("A command outside the allowlist must not run"),
            Err(err) => err,
        };

        assert!(err.contains("not in the allowlist"), "{}", err);

        // Refused before registration, so it must not have taken a job slot.
        assert!(repo.jobs.list(crate::jobs::JobStateFilter::All).is_empty());
    }

    /// The bypass this closes, end to end: `PATH` given to the child is what
    /// resolves a bare program name, so without the check a caller could build
    /// their own binary, call it `cargo`, and have the allowlist wave it through.
    #[tokio::test]
    async fn path_can_not_be_used_to_redirect_an_allowlisted_command() {
        let repo = build_repo(
            "path_redirect",
            CommandMode::Allowlist,
            vec!["cargo".to_string()],
        )
        .await;

        // A binary named `cargo` which is not cargo, inside the repository.
        let impostor_dir = repo.root().join("bin");
        std::fs::create_dir_all(&impostor_dir).unwrap();
        let impostor = impostor_dir.join("cargo");
        std::fs::write(&impostor, "#!/bin/sh\necho IMPOSTOR\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&impostor, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let request = RunCommandRequest {
            command: "cargo".to_string(),
            args: vec!["--version".to_string()],
            cwd: None,
            env: vec![EnvVar {
                name: "PATH".to_string(),
                value: impostor_dir.to_string_lossy().to_string(),
            }],
            timeout_sec: None,
            wait_sec: Some(10),
        };

        let err = match run_command(&repo, request).await {
            Ok(result) => panic!(
                "PATH override must be refused, but it ran: {}",
                result.stdout
            ),
            Err(err) => err,
        };

        assert!(err.contains("is refused in allowlist mode"), "{}", err);
        assert!(repo.jobs.list(crate::jobs::JobStateFilter::All).is_empty());
    }

    #[test]
    fn renders_the_command_line() {
        assert_eq!(render_command_line("cargo", &[]), "cargo");

        assert_eq!(
            render_command_line("cargo", &["build".to_string(), "--release".to_string()]),
            "cargo build --release"
        );
    }
}
