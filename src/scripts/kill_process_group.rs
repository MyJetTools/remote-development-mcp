use crate::jobs::KillSignal;

/// Signals the job's whole process group.
///
/// The negative pid is the point: jobs are spawned with `process_group(0)`, so
/// their pgid equals their pid, and `kill(-pgid, sig)` reaches `cargo` together
/// with every `rustc` it started. Signalling the pid alone would leave the
/// compiler processes running and the build still burning CPU.
#[cfg(unix)]
pub fn kill_process_group(pid: u32, signal: KillSignal) -> Result<(), String> {
    let result = unsafe { libc::kill(-(pid as i32), signal.as_libc()) };

    if result == 0 {
        return Ok(());
    }

    let err = std::io::Error::last_os_error();

    // ESRCH — nothing left to signal. The job finished between being looked up
    // and being signalled, which is a race the caller does not need to hear
    // about.
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }

    Err(format!(
        "Can not send {} to the process group {}. Err: {}",
        signal.as_str(),
        pid,
        err
    ))
}

/// Kills the job's whole process tree.
///
/// Windows has no POSIX signals and no process groups to signal, so every
/// `KillSignal` variant collapses into the same forced tree kill: `taskkill /T`
/// walks the parent-child links and takes every `rustc` down with its `cargo`,
/// which is what the negative-pgid kill achieves on unix. `output()` rather
/// than `status()` so taskkill's chatter is captured instead of landing on the
/// server's console.
#[cfg(windows)]
pub fn kill_process_group(pid: u32, signal: KillSignal) -> Result<(), String> {
    let output = std::process::Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .output();

    let output = match output {
        Ok(output) => output,
        Err(err) => {
            return Err(format!(
                "Can not run taskkill for the process tree {}. Err: {}",
                pid, err
            ));
        }
    };

    if output.status.success() {
        return Ok(());
    }

    // 128 — no such process. The job finished between being looked up and being
    // killed, which is a race the caller does not need to hear about.
    if output.status.code() == Some(128) {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    Err(format!(
        "Can not kill the process tree {} ({} requested). taskkill exit {:?}: {}",
        pid,
        signal.as_str(),
        output.status.code(),
        stderr.trim()
    ))
}
