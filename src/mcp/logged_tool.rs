use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{de::DeserializeOwned, Serialize};

use crate::activity::{ActivityEvent, ActivityLog};

/// Parameters are echoed to the console, and a `write_file` body can be a whole
/// source file. Past this the line is cut — the console is for watching what is
/// happening, not for holding the payload.
const MAX_LOGGED_PARAMS: usize = 300;

/// Wraps any tool so that every call is announced on the console, with the
/// repository it belongs to and the parameters it was given.
///
/// Done as one wrapper rather than a line inside each of the handlers: there is
/// exactly one place that can fall out of step this way, and adding a tool
/// cannot forget to log itself.
pub struct LoggedTool<TInner> {
    inner: Arc<TInner>,
    repo_name: String,
    activity: Arc<ActivityLog>,
}

impl<TInner> LoggedTool<TInner> {
    pub fn new(inner: TInner, repo_name: String, activity: Arc<ActivityLog>) -> Self {
        Self {
            inner: Arc::new(inner),
            repo_name,
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
        self.activity.push(ActivityEvent::tool_call(
            self.repo_name.clone(),
            TInner::FUNC_NAME.to_string(),
            render_params(&model),
        ));

        let result = self.inner.execute_tool_call(model).await;

        if let Err(err) = result.as_ref() {
            self.activity.push(ActivityEvent::tool_failed(
                self.repo_name.clone(),
                TInner::FUNC_NAME.to_string(),
                clamp(err),
            ));
        }

        result
    }
}

fn render_params<TInput: Serialize>(model: &TInput) -> String {
    match serde_json::to_string(model) {
        Ok(json) => clamp(&json),
        // Nothing here is worth failing a tool call over.
        Err(_) => "<parameters could not be rendered>".to_string(),
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
        let rendered = render_params(&serde_json::json!({"path": "src/main.rs"}));

        assert_eq!(rendered, r#"{"path":"src/main.rs"}"#);
    }

    #[test]
    fn a_large_body_is_cut_so_it_can_not_flood_the_console() {
        let rendered = render_params(&serde_json::json!({"content": "x".repeat(10_000)}));

        assert!(rendered.chars().count() <= MAX_LOGGED_PARAMS + 1);
        assert!(rendered.ends_with('…'));
    }

    #[test]
    fn newlines_are_folded_so_one_call_stays_one_line() {
        let rendered = render_params(&serde_json::json!({"body": "first\nsecond"}));

        assert!(!rendered.contains('\n'));
    }
}
