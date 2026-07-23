use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    activity::{ActivityEvent, ActivityLog},
    repo::Endpoint,
};

/// Parameters are echoed to the console, and a `write_file` body can be a whole
/// source file. Past this the line is cut — the console is for watching what is
/// happening, not for holding the payload.
const MAX_LOGGED_PARAMS: usize = 300;

/// Wraps any tool so that every call is announced on the console, with the
/// project it landed in and the parameters it was given.
///
/// Done as one wrapper rather than a line inside each of the handlers: there is
/// exactly one place that can fall out of step this way, and adding a tool
/// cannot forget to log itself.
pub struct LoggedTool<TInner> {
    inner: Arc<TInner>,
    endpoint: Arc<Endpoint>,
    activity: Arc<ActivityLog>,
}

impl<TInner> LoggedTool<TInner> {
    pub fn new(inner: TInner, endpoint: Arc<Endpoint>, activity: Arc<ActivityLog>) -> Self {
        Self {
            inner: Arc::new(inner),
            endpoint,
            activity,
        }
    }
}

/// Forwarded verbatim, so the wrapper is invisible to the MCP client — it sees
/// the same tool name and description as if the handler were registered directly.
impl<TInner: ToolDefinition> ToolDefinition for LoggedTool<TInner> {
    const FUNC_NAME: &'static str = TInner::FUNC_NAME;
    const DESCRIPTION: &'static str = TInner::DESCRIPTION;
}

#[async_trait::async_trait]
impl<InputData, OutputData, TInner> McpToolCall<InputData, OutputData> for LoggedTool<TInner>
where
    InputData: json_schema::JsonTypeDescription
        + Serialize
        + DeserializeOwned
        + Sized
        + Send
        + Sync
        + 'static,
    OutputData: json_schema::JsonTypeDescription + Sized + Send + Sync + 'static,
    TInner: McpToolCall<InputData, OutputData> + ToolDefinition + Send + Sync + 'static,
{
    async fn execute_tool_call(&self, model: InputData) -> Result<OutputData, String> {
        // Serialized once and used for both, since the console line and the
        // project label are read out of the same payload.
        let params = serde_json::to_value(&model).ok();

        let project = self.project_of(params.as_ref());

        self.activity.push(ActivityEvent::tool_call(
            project.clone(),
            TInner::FUNC_NAME.to_string(),
            render_params(params.as_ref()),
        ));

        let result = self.inner.execute_tool_call(model).await;

        if let Err(err) = result.as_ref() {
            self.activity.push(ActivityEvent::tool_failed(
                project,
                TInner::FUNC_NAME.to_string(),
                clamp(err),
            ));
        }

        result
    }
}

impl<TInner> LoggedTool<TInner> {
    /// Which project the console should file this call under.
    ///
    /// Read from the call rather than fixed at registration, because one
    /// endpoint now serves several projects. The job tools carry no `project`
    /// of their own — their project is the prefix of the job id — so both are
    /// looked at here, and the handler stays free of console concerns.
    fn project_of(&self, params: Option<&serde_json::Value>) -> String {
        let named = params
            .and_then(|params| params.get("project"))
            .and_then(|project| project.as_str())
            .map(|project| project.trim())
            .filter(|project| !project.is_empty());

        if let Some(named) = named {
            return named.to_string();
        }

        let from_job = params
            .and_then(|params| params.get("job_id"))
            .and_then(|job_id| job_id.as_str())
            .and_then(|job_id| job_id.split_once(':'))
            .map(|(project, _)| project.trim())
            .filter(|project| !project.is_empty());

        if let Some(from_job) = from_job {
            return from_job.to_string();
        }

        match self.endpoint.projects() {
            [only] => only.name.clone(),
            // Nothing named a project, so the call is about to be refused. The
            // url is the honest label for a line the console still has to show.
            _ => self.endpoint.url.to_string(),
        }
    }
}

fn render_params(params: Option<&serde_json::Value>) -> String {
    match params.map(|params| params.to_string()) {
        Some(json) => clamp(&json),
        // Nothing here is worth failing a tool call over.
        None => "<parameters could not be rendered>".to_string(),
    }
}

fn clamp(text: &str) -> String {
    let text = text.replace('\n', " ");

    if text.chars().count() <= MAX_LOGGED_PARAMS {
        return text;
    }

    let clamped: String = text.chars().take(MAX_LOGGED_PARAMS).collect();

    format!("{}…", clamped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_parameters_are_left_alone() {
        let rendered = render_params(Some(&serde_json::json!({"path": "src/main.rs"})));

        assert_eq!(rendered, r#"{"path":"src/main.rs"}"#);
    }

    #[test]
    fn a_large_body_is_cut_so_it_can_not_flood_the_console() {
        let rendered = render_params(Some(&serde_json::json!({"content": "x".repeat(10_000)})));

        assert!(rendered.chars().count() <= MAX_LOGGED_PARAMS + 1);
        assert!(rendered.ends_with('…'));
    }

    #[test]
    fn newlines_are_folded_so_one_call_stays_one_line() {
        let rendered = render_params(Some(&serde_json::json!({"body": "first\nsecond"})));

        assert!(!rendered.contains('\n'));
    }
}
