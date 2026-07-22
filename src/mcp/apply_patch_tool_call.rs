use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{repo::RepoContext, scripts::apply_patch};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ApplyPatchInputData {
    #[property(
        description = "Unified diff, as produced by 'git diff'. It may touch several files. Paths are relative to the repository root"
    )]
    pub patch: String,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ApplyPatchResponse {
    #[property(description = "True when the whole patch was applied")]
    pub applied: bool,

    #[property(description = "Files the patch touched")]
    pub files_changed: Vec<String>,

    #[property(
        description = "Why the patch was refused, when it was. Nothing is applied in that case — the patch is checked in full first, so it never lands half way"
    )]
    pub rejected: Option<String>,
}

pub struct ApplyPatchHandler {
    repo: Arc<RepoContext>,
}

impl ApplyPatchHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for ApplyPatchHandler {
    const FUNC_NAME: &'static str = "apply_patch";

    const DESCRIPTION: &'static str =
        "Applies a unified diff to the repository — the way to make one coherent change across \
         several files. The patch is validated before anything is written, so it either applies \
         completely or not at all.";
}

#[async_trait::async_trait]
impl McpToolCall<ApplyPatchInputData, ApplyPatchResponse> for ApplyPatchHandler {
    async fn execute_tool_call(
        &self,
        model: ApplyPatchInputData,
    ) -> Result<ApplyPatchResponse, String> {
        let result = apply_patch(&self.repo, &model.patch).await?;

        Ok(ApplyPatchResponse {
            applied: result.rejected.is_none(),
            files_changed: result.files_changed,
            rejected: result.rejected,
        })
    }
}
