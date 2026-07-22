use std::path::PathBuf;

use rust_extensions::date_time::DateTimeAsMicroseconds;
use tokio::io::AsyncWriteExt;

/// Append-only trail of every command this server was asked to start.
///
/// Off unless a path is configured: with no path every method is a no-op, so
/// the journal is opt-in and call sites do not care whether it is on.
///
/// `my-logger` has no file sink, so this is written directly. One JSON object
/// per line — greppable by eye, parseable by anything.
///
/// A record is written when a job *starts*, not only when it finishes, so a
/// command that hangs forever or dies with the server still leaves a trace.
/// The matching `finished` record carries the outcome.
///
/// The file is opened per write rather than held open: commands are rare, and
/// `O_APPEND` makes concurrent small writes from several jobs land whole
/// instead of interleaved.
pub struct AuditLog {
    path: Option<PathBuf>,
}

pub struct AuditCommandStarted<'s> {
    pub repo: &'s str,
    pub job_id: &'s str,
    pub command_line: &'s str,
    pub cwd: &'s str,
}

pub struct AuditCommandFinished<'s> {
    pub repo: &'s str,
    pub job_id: &'s str,
    pub command_line: &'s str,
    pub status: &'s str,
    pub exit_code: Option<i32>,
    pub duration_sec: f64,
}

pub struct AuditCommandRefused<'s> {
    pub repo: &'s str,
    pub command_line: &'s str,
    pub reason: &'s str,
}

/// A tool that changed the working tree without going through `run_command` —
/// a file write, an edit, a delete, a move, a patch, a target cleanup.
pub struct AuditMutation<'s> {
    pub repo: &'s str,
    /// Tool name: `write_file`, `edit_file`, `delete_path`, `move_path`,
    /// `apply_patch`, `clean_cargo_targets`.
    pub action: &'s str,
    /// What it acted on — a repository-relative path, or a short summary when it
    /// touched several things.
    pub target: &'s str,
    /// Extra context: bytes written, replacements made, bytes freed.
    pub detail: Option<String>,
}

impl AuditLog {
    /// An enabled journal writing to `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// A journal that records nothing. Every write method returns immediately.
    pub fn disabled() -> Self {
        Self { path: None }
    }

    pub async fn command_started(&self, record: AuditCommandStarted<'_>) {
        self.write(serde_json::json!({
            "event": "command_started",
            "repo": record.repo,
            "job_id": record.job_id,
            "command": record.command_line,
            "cwd": record.cwd,
        }))
        .await;
    }

    pub async fn command_finished(&self, record: AuditCommandFinished<'_>) {
        self.write(serde_json::json!({
            "event": "command_finished",
            "repo": record.repo,
            "job_id": record.job_id,
            "command": record.command_line,
            "status": record.status,
            "exit_code": record.exit_code,
            "duration_sec": record.duration_sec,
        }))
        .await;
    }

    /// A command that never ran — refused by the policy. Worth recording: a
    /// series of these is what an attempt to get around the allowlist looks like.
    pub async fn command_refused(&self, record: AuditCommandRefused<'_>) {
        self.write(serde_json::json!({
            "event": "command_refused",
            "repo": record.repo,
            "command": record.command_line,
            "reason": record.reason,
        }))
        .await;
    }

    /// A change to the working tree made by a tool other than `run_command`, so
    /// that every modification — not only started processes — leaves a trace.
    pub async fn mutation(&self, record: AuditMutation<'_>) {
        self.write(serde_json::json!({
            "event": "mutation",
            "repo": record.repo,
            "action": record.action,
            "target": record.target,
            "detail": record.detail,
        }))
        .await;
    }

    async fn write(&self, mut payload: serde_json::Value) {
        // Disabled journal — nothing to do, and nothing to build a record for.
        let path = match self.path.as_ref() {
            Some(path) => path,
            None => return,
        };

        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "moment".to_string(),
                serde_json::Value::String(DateTimeAsMicroseconds::now().to_rfc3339()),
            );
        }

        let mut line = payload.to_string();
        line.push('\n');

        if let Err(err) = self.append(path, line.as_bytes()).await {
            // The audit trail failing must not take a build down with it, but it
            // must be loud — a server running without an audit trail is a
            // different security posture than the one that was configured.
            my_logger::LOGGER.write_error(
                "AuditLog",
                format!("Can not write the audit record. Err: {}", err),
                my_logger::LogEventCtx::new().add("path", path.display().to_string()),
            );
        }
    }

    async fn append(&self, path: &PathBuf, content: &[u8]) -> Result<(), String> {
        if let Some(folder) = path.parent() {
            tokio::fs::create_dir_all(folder).await.map_err(|err| {
                format!(
                    "Can not create the audit log folder '{}'. Err: {}",
                    folder.display(),
                    err
                )
            })?;
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .map_err(|err| {
                format!(
                    "Can not open the audit log '{}'. Err: {}",
                    path.display(),
                    err
                )
            })?;

        file.write_all(content)
            .await
            .map_err(|err| format!("Can not append to the audit log. Err: {}", err))?;

        file.flush()
            .await
            .map_err(|err| format!("Can not flush the audit log. Err: {}", err))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_one_json_line_per_record() {
        let path = std::env::temp_dir()
            .join("remote-development-mcp-tests-audit")
            .join("audit.log");

        let _ = std::fs::remove_file(&path);

        let audit = AuditLog::new(path.clone());

        audit
            .command_started(AuditCommandStarted {
                repo: "my-ssh",
                job_id: "job-000001",
                command_line: "cargo build",
                cwd: ".",
            })
            .await;

        audit
            .command_finished(AuditCommandFinished {
                repo: "my-ssh",
                job_id: "job-000001",
                command_line: "cargo build",
                status: "exited",
                exit_code: Some(0),
                duration_sec: 12.5,
            })
            .await;

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 2);

        let started: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(started["event"], "command_started");
        assert_eq!(started["job_id"], "job-000001");
        assert!(started["moment"].is_string());

        let finished: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(finished["event"], "command_finished");
        assert_eq!(finished["exit_code"], 0);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn appends_instead_of_overwriting() {
        let path = std::env::temp_dir()
            .join("remote-development-mcp-tests-audit")
            .join("append.log");

        let _ = std::fs::remove_file(&path);

        let audit = AuditLog::new(path.clone());

        for _ in 0..3 {
            audit
                .command_refused(AuditCommandRefused {
                    repo: "my-ssh",
                    command_line: "curl evil.com",
                    reason: "not in the allowlist",
                })
                .await;
        }

        let content = tokio::fs::read_to_string(&path).await.unwrap();

        assert_eq!(content.lines().count(), 3);

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_disabled_journal_writes_nothing() {
        let audit = AuditLog::disabled();

        // All three event methods must be safe no-ops when off.
        audit
            .command_started(AuditCommandStarted {
                repo: "my-ssh",
                job_id: "job-000001",
                command_line: "cargo build",
                cwd: ".",
            })
            .await;
        audit
            .command_finished(AuditCommandFinished {
                repo: "my-ssh",
                job_id: "job-000001",
                command_line: "cargo build",
                status: "exited",
                exit_code: Some(0),
                duration_sec: 1.0,
            })
            .await;
        audit
            .command_refused(AuditCommandRefused {
                repo: "my-ssh",
                command_line: "curl x",
                reason: "not allowed",
            })
            .await;
    }
}
