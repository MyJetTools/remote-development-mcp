use rust_extensions::date_time::DateTimeAsMicroseconds;

/// What happened. Kept as an enum rather than a formatted string so the console
/// can colour and align each kind, and so a reader can tell a refusal from a
/// completed build without parsing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    /// A tool was called.
    ToolCall,
    /// A tool returned an error.
    ToolFailed,
    /// A command finished — arrives long after the call that started it.
    JobFinished,
    /// A GitHub Actions run changed state.
    ActionRun,
    /// Something panicked. Only reachable here: with the console running, the
    /// default panic report goes to a screen nobody can see.
    Panicked,
}

impl ActivityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActivityKind::ToolCall => "call",
            ActivityKind::ToolFailed => "fail",
            ActivityKind::JobFinished => "done",
            ActivityKind::ActionRun => "CI",
            ActivityKind::Panicked => "PANIC",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub moment: DateTimeAsMicroseconds,
    pub kind: ActivityKind,
    pub repo: String,
    /// Tool name, or the job id for a completion.
    pub subject: String,
    /// Parameters, or the outcome.
    pub detail: String,
    /// How long the thing took, once it is known. A tool call and a job carry
    /// it; a panic or a state change of a remote CI run have no duration to
    /// speak of and leave it `None`.
    pub duration_sec: Option<f64>,
}

impl ActivityEvent {
    pub fn tool_call(
        repo: String,
        tool: String,
        params: String,
        duration_sec: Option<f64>,
    ) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::ToolCall,
            repo,
            subject: tool,
            detail: params,
            duration_sec,
        }
    }

    pub fn tool_failed(
        repo: String,
        tool: String,
        error: String,
        duration_sec: Option<f64>,
    ) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::ToolFailed,
            repo,
            subject: tool,
            detail: error,
            duration_sec,
        }
    }

    pub fn job_finished(repo: String, job_id: String, outcome: String, duration_sec: f64) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::JobFinished,
            repo,
            subject: job_id,
            detail: outcome,
            duration_sec: Some(duration_sec),
        }
    }

    pub fn action_run(repo: String, label: String, outcome: String) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::ActionRun,
            repo,
            subject: label,
            detail: outcome,
            duration_sec: None,
        }
    }

    pub fn panicked(location: String, message: String, frames: String) -> Self {
        let detail = if frames.is_empty() {
            message
        } else {
            format!("{}  ⟵ {}", message, frames)
        };

        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::Panicked,
            repo: String::new(),
            subject: location,
            detail,
            duration_sec: None,
        }
    }
}
