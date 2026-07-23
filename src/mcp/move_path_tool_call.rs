use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{repo::Endpoint, scripts::move_path};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct MovePathInputData {
    #[property(
        description = "Project to work in. Can be omitted only on an endpoint that serves a single project"
    )]
    pub project: Option<String>,

    #[property(description = "File or folder to move, relative to the repository root")]
    pub from: String,

    #[property(description = "Where to move it, relative to the repository root")]
    pub to: String,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct MovePathResponse {
    #[property(description = "Where it was")]
    pub from: String,

    #[property(description = "Where it is now")]
    pub to: String,
}

pub struct MovePathHandler {
    endpoint: Arc<Endpoint>,
}

impl MovePathHandler {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self { endpoint }
    }
}

impl ToolDefinition for MovePathHandler {
    const FUNC_NAME: &'static str = "move_path";

    const DESCRIPTION: &'static str =
        "Moves or renames a file or folder inside the repository. Refuses to overwrite an existing \
         destination — delete it first if that was the intent.";
}

#[async_trait::async_trait]
impl McpToolCall<MovePathInputData, MovePathResponse> for MovePathHandler {
    async fn execute_tool_call(
        &self,
        model: MovePathInputData,
    ) -> Result<MovePathResponse, String> {
        let repo = self.endpoint.resolve(model.project.as_deref())?;

        let result = move_path(repo, &model.from, &model.to).await?;

        Ok(MovePathResponse {
            from: result.from,
            to: result.to,
        })
    }
}
