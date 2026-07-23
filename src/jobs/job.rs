use std::path::PathBuf;

use rust_extensions::date_time::DateTimeAsMicroseconds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// The process is still alive.
    Running,
    /// The process finished on its own — successfully or not, `exit_code` tells.
    Exited,
    /// A `kill_job` call ended it.
    Killed,
    /// Its timeout elapsed and the server ended it.
    TimedOut,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Running => "running",
            JobStatus::Exited => "exited",
            JobStatus::Killed => "killed",
            JobStatus::TimedOut => "timed_out",
        }
    }

    pub fn is_running(&self) -> bool {
        match self {
            JobStatus::Running => true,
            JobStatus::Exited => false,
            JobStatus::Killed => false,
            JobStatus::TimedOut => false,
        }
    }
}

/// Which jobs `list_jobs` should return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStateFilter {
    All,
    Running,
    Finished,
}

impl JobStateFilter {
    pub fn parse(src: Option<&str>) -> Result<Self, String> {
        let src = match src {
            Some(src) => src.trim(),
            None => return Ok(JobStateFilter::All),
        };

        match src {
            "" => Ok(JobStateFilter::All),
            "all" => Ok(JobStateFilter::All),
            "running" => Ok(JobStateFilter::Running),
            "finished" => Ok(JobStateFilter::Finished),
            _ => Err(format!(
                "Unknown job state filter '{}'. Expected: all, running, finished",
                src
            )),
        }
    }

    pub fn matches(&self, status: JobStatus) -> bool {
        match self {
            JobStateFilter::All => true,
            JobStateFilter::Running => status.is_running(),
            JobStateFilter::Finished => !status.is_running(),
        }
    }
}

/// Which of a job's two streams `get_job_output` should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
    Both,
}

impl OutputStream {
    pub fn parse(src: Option<&str>) -> Result<Self, String> {
        let src = match src {
            Some(src) => src.trim().to_lowercase(),
            None => return Ok(OutputStream::Both),
        };

        match src.as_str() {
            "" => Ok(OutputStream::Both),
            "both" => Ok(OutputStream::Both),
            "stdout" => Ok(OutputStream::Stdout),
            "stderr" => Ok(OutputStream::Stderr),
            _ => Err(format!(
                "Unknown stream '{}'. Expected: stdout, stderr or both",
                src
            )),
        }
    }

    pub fn reads_stdout(&self) -> bool {
        match self {
            OutputStream::Stdout => true,
            OutputStream::Stderr => false,
            OutputStream::Both => true,
        }
    }

    pub fn reads_stderr(&self) -> bool {
        match self {
            OutputStream::Stdout => false,
            OutputStream::Stderr => true,
            OutputStream::Both => true,
        }
    }
}

/// Signal `kill_job` sends to the job's process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillSignal {
    Term,
    Kill,
    Int,
}

impl KillSignal {
    pub fn parse(src: Option<&str>) -> Result<Self, String> {
        let src = match src {
            Some(src) => src.trim().to_uppercase(),
            None => return Ok(KillSignal::Term),
        };

        match src.as_str() {
            "" => Ok(KillSignal::Term),
            "TERM" | "SIGTERM" => Ok(KillSignal::Term),
            "KILL" | "SIGKILL" => Ok(KillSignal::Kill),
            "INT" | "SIGINT" => Ok(KillSignal::Int),
            _ => Err(format!(
                "Unknown signal '{}'. Expected: TERM, KILL or INT",
                src
            )),
        }
    }

    pub fn as_libc(&self) -> i32 {
        match self {
            KillSignal::Term => libc::SIGTERM,
            KillSignal::Kill => libc::SIGKILL,
            KillSignal::Int => libc::SIGINT,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            KillSignal::Term => "TERM",
            KillSignal::Kill => "KILL",
            KillSignal::Int => "INT",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    /// Rendered command line, as it appears in `list_jobs` and the audit log.
    pub command_line: String,
    /// Working directory, relative to the repository root.
    pub cwd: String,
    pub status: JobStatus,
    pub exit_code: Option<i32>,
    /// Also the process group id — the job is spawned into its own group.
    pub pid: Option<u32>,
    /// Set by `kill_job`. A signalled process still exits through the normal
    /// path, so without this the supervisor could not tell "killed on request"
    /// from "died on its own".
    pub kill_requested: bool,
    pub started_at: DateTimeAsMicroseconds,
    pub finished_at: Option<DateTimeAsMicroseconds>,
    /// The deadline this job was given, in seconds. Carried on the job rather
    /// than only inside the supervisor so every answer about a job can say how
    /// long it is allowed to run — otherwise a caller watching a long build has
    /// no way to tell whether it is about to be killed.
    pub timeout_sec: u64,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
}

impl Job {
    pub fn duration_sec(&self, now: DateTimeAsMicroseconds) -> f64 {
        let until = match self.finished_at {
            Some(finished_at) => finished_at,
            None => now,
        };

        let micros = until.unix_microseconds - self.started_at.unix_microseconds;

        micros as f64 / 1_000_000.0
    }

    /// Seconds left before the job is killed, or `None` once it has finished.
    ///
    /// What a caller polling a long build actually wants to know: not only how
    /// long it has been running, but how long it still may.
    pub fn remaining_sec(&self, now: DateTimeAsMicroseconds) -> Option<f64> {
        if !self.status.is_running() {
            return None;
        }

        let remaining = self.timeout_sec as f64 - self.duration_sec(now);

        Some(remaining.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_signal_defaults_to_term() {
        assert_eq!(KillSignal::parse(None).unwrap(), KillSignal::Term);
        assert_eq!(KillSignal::parse(Some("")).unwrap(), KillSignal::Term);
    }

    #[test]
    fn kill_signal_accepts_both_spellings_and_any_case() {
        assert_eq!(KillSignal::parse(Some("term")).unwrap(), KillSignal::Term);
        assert_eq!(
            KillSignal::parse(Some("SIGKILL")).unwrap(),
            KillSignal::Kill
        );
        assert_eq!(KillSignal::parse(Some("Int")).unwrap(), KillSignal::Int);
    }

    #[test]
    fn unknown_kill_signal_is_refused() {
        assert!(KillSignal::parse(Some("HUP")).is_err());
    }

    #[test]
    fn job_state_filter_parses_and_matches() {
        assert_eq!(JobStateFilter::parse(None).unwrap(), JobStateFilter::All);

        let running = JobStateFilter::parse(Some("running")).unwrap();

        assert!(running.matches(JobStatus::Running));
        assert!(!running.matches(JobStatus::Exited));

        let finished = JobStateFilter::parse(Some("finished")).unwrap();

        assert!(!finished.matches(JobStatus::Running));
        assert!(finished.matches(JobStatus::TimedOut));
        assert!(finished.matches(JobStatus::Killed));
    }

    #[test]
    fn unknown_job_state_filter_is_refused() {
        assert!(JobStateFilter::parse(Some("zombie")).is_err());
    }
}
