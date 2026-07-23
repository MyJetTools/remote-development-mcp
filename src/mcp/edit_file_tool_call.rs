use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::Endpoint,
    scripts::{edit_file, EditFileRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct EditFileInputData {
    #[property(
        description = "Project to work in. Can be omitted only on an endpoint that serves a single project"
    )]
    pub project: Option<String>,

    #[property(description = "File to edit, relative to the repository root")]
    pub path: String,

    #[property(
        description = "Exact text to replace, including indentation. It must appear exactly once unless replace_all is set"
    )]
    pub old_string: String,

    #[property(description = "Text to put in its place")]
    pub new_string: String,

    #[property(
        description = "Replace every occurrence instead of requiring a unique one. Defaults to false"
    )]
    pub replace_all: Option<bool>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct EditFileResponse {
    #[property(description = "How many occurrences were replaced")]
    pub replaced: u64,

    #[property(description = "Path that was edited, relative to the repository root")]
    pub path: String,
}

pub struct EditFileHandler {
    endpoint: Arc<Endpoint>,
}

impl EditFileHandler {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self { endpoint }
    }
}

impl ToolDefinition for EditFileHandler {
    const FUNC_NAME: &'static str = "edit_file";

    const DESCRIPTION: &'static str =
        "Replaces an exact piece of text in a file, without resending the rest of it. If old_string \
         is not unique the edit is refused rather than guessing which occurrence was meant — extend \
         it with surrounding lines until it is unique, or pass replace_all. For a change spanning \
         several files use apply_patch.";
}

#[async_trait::async_trait]
impl McpToolCall<EditFileInputData, EditFileResponse> for EditFileHandler {
    async fn execute_tool_call(
        &self,
        model: EditFileInputData,
    ) -> Result<EditFileResponse, String> {
        let repo = self.endpoint.resolve(model.project.as_deref())?;

        let replaced = edit_file(
            repo,
            EditFileRequest {
                path: &model.path,
                old_string: &model.old_string,
                new_string: &model.new_string,
                replace_all: model.replace_all.unwrap_or_default(),
            },
        )
        .await?;

        Ok(EditFileResponse {
            replaced: replaced as u64,
            path: model.path,
        })
    }
}
