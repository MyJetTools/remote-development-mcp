use std::{path::Path, process::Stdio, time::Duration};

use tokio::io::AsyncWriteExt;

use crate::jobs::KillSignal;

use super::kill_process_group;

/// Bounds one of the server's own helper invocations. These are `git`, `rg` and
/// `cargo metadata` — quick by nature, but a hostile repository could wedge one,
/// and nothing that holds a request open should be allowed to hang forever.
const EXEC_TIMEOUT: Duration = Duration::from_secs(120);

/// Runs a short-lived helper process and buffers its whole output.
///
/// This is for the server's *own* helpers — `git`, `rg` — not for user jobs.
/// Their output is bounded and they finish in moments, so buffering is right
/// here, whereas a user's `cargo build` gets the streaming job machinery
/// instead.
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub success: bool,
}

/// Runs one of the server's own `git` helpers with hostile-repository hardening.
///
/// Even with `.git` writes blocked, this is defense in depth against a
/// repository that was already hostile before the server was pointed at it: git
/// executes `core.fsmonitor` and the hooks named by `core.hooksPath`, and the
/// `ext::` transport runs an arbitrary command. Forcing them off here means the
/// server's own `git status` / `git apply` can not be turned into code
/// execution by config that lives in the working tree.
pub async fn git_capture(
    args: &[&str],
    cwd: &Path,
    stdin: Option<&[u8]>,
) -> Result<ExecOutput, String> {
    let mut full: Vec<&str> = vec![
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "protocol.ext.allow=never",
    ];

    full.extend_from_slice(args);

    exec_capture("git", &full, cwd, stdin).await
}

pub async fn exec_capture(
    program: &str,
    args: &[&str],
    cwd: &Path,
    stdin: Option<&[u8]>,
) -> Result<ExecOutput, String> {
    let mut command = tokio::process::Command::new(program);

    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(match stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        // kill_on_drop so a dropped future never leaves the direct child
        // running.
        .kill_on_drop(true);

    // Its own group so the timeout path can take out anything it spawned. On
    // Windows the timeout path uses `taskkill /T` instead, which needs no group.
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            return format!(
                "'{}' is not installed on the machine running this server, so this tool can not \
                 work. Install it and try again",
                program
            );
        }

        format!("Can not start '{}'. Err: {}", program, err)
    })?;

    let pid = child.id();

    // Feed stdin from its own task, concurrently with draining the output
    // below. Writing it all first and only then reading — as this used to do —
    // deadlocks the moment the child's answer fills its stdout pipe before it
    // has consumed its input, which `git check-ignore --stdin` does routinely.
    let stdin_writer = match stdin {
        Some(content) => child.stdin.take().map(|mut handle| {
            let content = content.to_vec();
            tokio::spawn(async move {
                let _ = handle.write_all(&content).await;
                // Dropping the handle closes the pipe — the child's EOF.
            })
        }),
        None => None,
    };

    let output = match tokio::time::timeout(EXEC_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            if let Some(writer) = stdin_writer {
                writer.abort();
            }
            return Err(format!("Can not run '{}'. Err: {}", program, err));
        }
        Err(_) => {
            // The dropped future kills the direct child (kill_on_drop); take the
            // rest of the group with it in case the helper spawned anything.
            if let Some(writer) = stdin_writer {
                writer.abort();
            }
            if let Some(pid) = pid {
                let _ = kill_process_group(pid, KillSignal::Kill);
            }
            return Err(format!(
                "'{}' did not finish within {} seconds and was stopped",
                program,
                EXEC_TIMEOUT.as_secs()
            ));
        }
    };

    if let Some(writer) = stdin_writer {
        let _ = writer.await;
    }

    Ok(ExecOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
        success: output.status.success(),
    })
}
