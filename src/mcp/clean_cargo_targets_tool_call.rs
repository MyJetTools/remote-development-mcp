use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::RepoContext,
    scripts::{clean_cargo_targets, format_bytes, CleanCargoTargetsRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct CleanCargoTargetsInputData {
    #[property(
        description = "Subtree to clean under, relative to the repository root. Defaults to the whole repository"
    )]
    pub path: Option<String>,

    #[property(
        description = "List what would be removed, and how much it would free, without deleting anything. Defaults to false"
    )]
    pub dry_run: Option<bool>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct CleanedTargetModel {
    #[property(description = "The target directory, relative to the repository root")]
    pub path: String,

    #[property(description = "Bytes freed by removing it")]
    pub freed_bytes: u64,

    #[property(description = "The same size, human-readable, e.g. '1.5 GB'")]
    pub freed_human: String,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct CleanCargoTargetsResponse {
    #[property(description = "The target directories that were removed, or that would be")]
    pub targets: Vec<CleanedTargetModel>,

    #[property(description = "Total bytes freed across all of them")]
    pub total_freed_bytes: u64,

    #[property(description = "The total freed, human-readable")]
    pub total_freed_human: String,

    #[property(description = "True when nothing was actually deleted because this was a dry run")]
    pub dry_run: bool,

    #[property(
        description = "True when there were more target directories than one run removes. Run it again to continue"
    )]
    pub truncated: bool,
}

pub struct CleanCargoTargetsHandler {
    repo: Arc<RepoContext>,
}

impl CleanCargoTargetsHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for CleanCargoTargetsHandler {
    const FUNC_NAME: &'static str = "clean_cargo_targets";

    const DESCRIPTION: &'static str =
        "Reclaims disk space by removing cargo build output across the repository. Made for a \
         monorepo of many separate crates rather than one workspace: it finds every 'target' \
         directory that sits next to a Cargo.toml and deletes it. A directory merely named \
         'target' with no Cargo.toml beside it is left alone, and symlinks are never followed. \
         Pass dry_run to see what it would remove first. The build output regenerates on the next \
         cargo build.";
}

#[async_trait::async_trait]
impl McpToolCall<CleanCargoTargetsInputData, CleanCargoTargetsResponse>
    for CleanCargoTargetsHandler
{
    async fn execute_tool_call(
        &self,
        model: CleanCargoTargetsInputData,
    ) -> Result<CleanCargoTargetsResponse, String> {
        let result = clean_cargo_targets(
            &self.repo,
            CleanCargoTargetsRequest {
                path: model.path,
                dry_run: model.dry_run.unwrap_or_default(),
            },
        )
        .await?;

        Ok(CleanCargoTargetsResponse {
            targets: result
                .targets
                .into_iter()
                .map(|target| CleanedTargetModel {
                    path: target.path,
                    freed_bytes: target.freed_bytes,
                    freed_human: format_bytes(target.freed_bytes),
                })
                .collect(),
            total_freed_human: format_bytes(result.total_freed_bytes),
            total_freed_bytes: result.total_freed_bytes,
            dry_run: result.dry_run,
            truncated: result.truncated,
        })
    }
}
