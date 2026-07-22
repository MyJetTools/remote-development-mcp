use crate::jobs::KillSignal;

/// Signals the job's whole process group.
///
/// The negative pid is the point: jobs are spawned with `process_group(0)`, so
/// their pgid equals their pid, and `kill(-pgid, sig)` reaches `cargo` together
/// with every `rustc` it started. Signalling the pid alone would leave the
/// compiler processes running and the build still burning CPU.
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
