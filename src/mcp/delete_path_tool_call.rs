use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{repo::Endpoint, scripts::delete_path};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct DeletePathInputData {
    #[property(
        description = "Project to work in. Can be omitted only on an endpoint that serves a single project"
    )]
    pub project: Option<String>,

    #[property(description = "File or folder to delete, relative to the repository root")]
    pub path: String,

    #[property(
        description = "Delete a folder together with everything inside it. Defaults to false, so a non-empty folder is refused"
    )]
    pub recursive: Option<bool>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct DeletePathResponse {
    #[property(description = "What was deleted, relative to the repository root")]
    pub path: String,

    #[property(description = "True when the deleted entry was a folder")]
    pub was_directory: bool,
}

pub struct DeletePathHandler {
    endpoint: Arc<Endpoint>,
}

impl DeletePathHandler {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self { endpoint }
    }
}

impl ToolDefinition for DeletePathHandler {
    const FUNC_NAME: &'static str = "delete_path";

    const DESCRIPTION: &'static str =
        "Deletes a file or folder from the repository. This is the one tool here that can not be \
         undone, so it is turned off unless the repository is configured to allow it.";
}

#[async_trait::async_trait]
impl McpToolCall<DeletePathInputData, DeletePathResponse> for DeletePathHandler {
    async fn execute_tool_call(
        &self,
        model: DeletePathInputData,
    ) -> Result<DeletePathResponse, String> {
        let repo = self.endpoint.resolve(model.project.as_deref())?;

        let result = delete_path(repo, &model.path, model.recursive.unwrap_or_default()).await?;

        Ok(DeletePathResponse {
            path: result.path,
            was_directory: result.was_directory,
        })
    }
}
