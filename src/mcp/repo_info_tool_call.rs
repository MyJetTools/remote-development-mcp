use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{repo::RepoContext, scripts::repo_info};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct RepoInfoInputData {}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct WorkspaceMemberModel {
    #[property(description = "Crate name, as cargo knows it")]
    pub member_name: String,

    #[property(description = "Its Cargo.toml, relative to the repository root")]
    pub manifest_path: String,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct RepoInfoResponse {
    #[property(description = "Name this repository is served under")]
    pub root: String,

    #[property(description = "Branch currently checked out")]
    pub git_branch: Option<String>,

    #[property(description = "True when there are uncommitted changes")]
    pub git_dirty: bool,

    #[property(description = "Output of 'git status --porcelain'")]
    pub git_status_short: Vec<String>,

    #[property(description = "True when the status listing above was cut short")]
    pub git_status_truncated: bool,

    #[property(
        description = "Crates in this Rust workspace. Build or test one of them on its own with run_command instead of the whole workspace — on a large repository that is the difference between seconds and many minutes"
    )]
    pub workspace_members: Vec<WorkspaceMemberModel>,

    #[property(description = "Why the workspace list is empty, when it is")]
    pub workspace_note: Option<String>,
}

pub struct RepoInfoHandler {
    repo: Arc<RepoContext>,
}

impl RepoInfoHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for RepoInfoHandler {
    const FUNC_NAME: &'static str = "repo_info";

    const DESCRIPTION: &'static str =
        "Describes the repository: branch, whether the tree is dirty, and — for a Rust workspace — \
         which crates it contains. Worth calling first: knowing the workspace members lets you \
         build or test one crate rather than everything.";
}

#[async_trait::async_trait]
impl McpToolCall<RepoInfoInputData, RepoInfoResponse> for RepoInfoHandler {
    async fn execute_tool_call(
        &self,
        _model: RepoInfoInputData,
    ) -> Result<RepoInfoResponse, String> {
        let info = repo_info(&self.repo).await?;

        Ok(RepoInfoResponse {
            root: info.root,
            git_branch: info.git_branch,
            git_dirty: info.git_dirty,
            git_status_short: info.git_status_short,
            git_status_truncated: info.git_status_truncated,
            workspace_members: info
                .workspace_members
                .into_iter()
                .map(|member| WorkspaceMemberModel {
                    member_name: member.member_name,
                    manifest_path: member.manifest_path,
                })
                .collect(),
            workspace_note: info.workspace_note,
        })
    }
}
