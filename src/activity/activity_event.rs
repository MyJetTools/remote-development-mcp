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
}

impl ActivityEvent {
    pub fn tool_call(repo: String, tool: String, params: String) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::ToolCall,
            repo,
            subject: tool,
            detail: params,
        }
    }

    pub fn tool_failed(repo: String, tool: String, error: String) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::ToolFailed,
            repo,
            subject: tool,
            detail: error,
        }
    }

    pub fn job_finished(repo: String, job_id: String, outcome: String) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::JobFinished,
            repo,
            subject: job_id,
            detail: outcome,
        }
    }

    pub fn action_run(repo: String, label: String, outcome: String) -> Self {
        Self {
            moment: DateTimeAsMicroseconds::now(),
            kind: ActivityKind::ActionRun,
            repo,
            subject: label,
            detail: outcome,
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
        }
    }

    /// `HH:MM:SS` — the console shows a live feed, so the date is noise.
    pub fn time_of_day(&self) -> String {
        let rfc3339 = self.moment.to_rfc3339();

        match rfc3339.get(11..19) {
            Some(time) => time.to_string(),
            None => rfc3339,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_only_the_time_of_day() {
        let event =
            ActivityEvent::tool_call("r".to_string(), "search".to_string(), "{}".to_string());

        let time = event.time_of_day();

        assert_eq!(time.len(), 8, "expected HH:MM:SS, got {}", time);
        assert_eq!(time.chars().filter(|c| *c == ':').count(), 2);
    }
}
