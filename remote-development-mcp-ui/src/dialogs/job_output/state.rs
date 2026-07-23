use rest_api_shared::JobOutputResponse;

/// What the output dialog is holding.
///
/// The two cursors are the point: each poll asks only for what has appeared
/// since the last one, and the text is appended. That is what makes watching a
/// long build cheap, and what guarantees no line is shown twice or skipped.
#[derive(Default)]
pub struct JobOutputState {
    pub stdout: String,
    pub stderr: String,
    pub stdout_cursor: u64,
    pub stderr_cursor: u64,
    pub status: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub polling_started: bool,
    pub loaded_once: bool,
}

impl JobOutputState {
    pub fn append(&mut self, chunk: JobOutputResponse) {
        self.stdout.push_str(&chunk.stdout);
        self.stderr.push_str(&chunk.stderr);
        self.stdout_cursor = chunk.next_stdout_cursor;
        self.stderr_cursor = chunk.next_stderr_cursor;
        self.status = chunk.status;
        self.exit_code = chunk.exit_code;
        self.error = None;
        self.loaded_once = true;
    }

    pub fn set_error(&mut self, err: String) {
        self.error = Some(err);
        self.loaded_once = true;
    }

    /// A finished job has nothing more to say, so the dialog stops asking.
    pub fn is_finished(&self) -> bool {
        !self.status.is_empty() && self.status != "running"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(stdout: &str, next: u64, status: &str) -> JobOutputResponse {
        JobOutputResponse {
            job_id: "j".to_string(),
            command_line: "cargo build".to_string(),
            status: status.to_string(),
            exit_code: None,
            stdout: stdout.to_string(),
            stderr: String::new(),
            next_stdout_cursor: next,
            next_stderr_cursor: 0,
            has_more: false,
        }
    }

    #[test]
    fn chunks_are_appended_and_the_cursor_moves_on() {
        let mut state = JobOutputState::default();

        state.append(chunk("first\n", 6, "running"));
        state.append(chunk("second\n", 13, "running"));

        assert_eq!(state.stdout, "first\nsecond\n");
        assert_eq!(state.stdout_cursor, 13);
        assert!(!state.is_finished());
    }

    #[test]
    fn a_job_which_ended_stops_being_polled() {
        let mut state = JobOutputState::default();

        state.append(chunk("done\n", 5, "exited"));

        assert!(state.is_finished());
    }
}
