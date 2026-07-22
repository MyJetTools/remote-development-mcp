use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::RepoContext,
    scripts::{run_git, GitCommandRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct GitInputData {
    #[property(
        description = "Git arguments, one per element — the subcommand and its flags. For example [\"status\", \"--short\"], [\"log\", \"-5\", \"--oneline\"], or [\"commit\", \"-m\", \"message\"]"
    )]
    pub args: Vec<String>,

    #[property(
        description = "Subdirectory to run in, relative to the repository root. Defaults to the root"
    )]
    pub cwd: Option<String>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct GitResponse {
    #[property(description = "Standard output of the git command")]
    pub stdout: String,

    #[property(description = "Standard error of the git command")]
    pub stderr: String,

    #[property(description = "Exit code; 0 means success")]
    pub exit_code: Option<i64>,

    #[property(description = "True when the command exited with code 0")]
    pub success: bool,
}

pub struct GitHandler {
    repo: Arc<RepoContext>,
}

impl GitHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for GitHandler {
    const FUNC_NAME: &'static str = "git";

    const DESCRIPTION: &'static str =
        "Runs any git command in the repository and returns its output. Use it for the whole git \
         workflow — status, diff, log, add, commit, branch, checkout, stash, and so on. It runs \
         synchronously and is meant for the quick commands git usually is; for a long clone or \
         fetch, use run_command instead so it becomes a job you can poll. This is full git: it can \
         commit, rewrite history and reach remotes, so treat it as you would running git locally.";
}

#[async_trait::async_trait]
impl McpToolCall<GitInputData, GitResponse> for GitHandler {
    async fn execute_tool_call(&self, model: GitInputData) -> Result<GitResponse, String> {
        let result = run_git(
            &self.repo,
            GitCommandRequest {
                args: model.args,
                cwd: model.cwd,
            },
        )
        .await?;

        Ok(GitResponse {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code.map(|code| code as i64),
            success: result.success,
        })
    }
}
