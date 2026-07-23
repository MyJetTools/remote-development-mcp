use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::RepoContext,
    scripts::{run_command, EnvVar, RunCommandRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct EnvVarModel {
    #[property(description = "Environment variable name")]
    pub env_name: String,
    #[property(description = "Environment variable value")]
    pub env_value: String,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct RunCommandInputData {
    #[property(
        description = "Binary to start, for example 'cargo'. Unless the repository runs in passthrough mode this must be a bare name with no path"
    )]
    pub command: String,

    #[property(description = "Arguments for the command, one per element")]
    pub args: Option<Vec<String>>,

    #[property(
        description = "Working directory relative to the repository root. Defaults to the root itself"
    )]
    pub cwd: Option<String>,

    #[property(
        description = "Extra environment variables, added on top of the server environment. In allowlist mode PATH and the LD_/DYLD_ loader variables are refused, because they would change which binary actually runs"
    )]
    pub env: Option<Vec<EnvVarModel>>,

    #[property(
        description = "Kill the job after this many seconds, counted from when it starts. RAISE THIS FOR A LONG BUILD — the default is only one hour, and a job that overruns is killed together with every process it spawned. The ceiling is 86400 (24 hours). The value actually applied comes back as timeout_sec in the response"
    )]
    pub timeout_sec: Option<u64>,

    #[property(
        description = "Wait up to this many seconds for the command to finish before returning. A command that finishes within it comes back complete in this one call. Defaults to 0, which returns immediately"
    )]
    pub wait_sec: Option<u64>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct RunCommandResponse {
    #[property(description = "Identifier to poll with get_job_output and to stop with kill_job")]
    pub job_id: String,

    #[property(
        enum: ["running", "exited", "killed", "timed_out"],
        description: "State of the job at the moment this call returned"
    )]
    pub status: String,

    #[property(description = "Exit code, present once the job has finished")]
    pub exit_code: Option<i64>,

    #[property(
        description = "Beginning of stdout. The whole log stays available via get_job_output"
    )]
    pub stdout: String,

    #[property(description = "Beginning of stderr")]
    pub stderr: String,

    #[property(
        description = "Pass this back as stdout_cursor to get_job_output to continue reading"
    )]
    pub next_stdout_cursor: u64,

    #[property(
        description = "Pass this back as stderr_cursor to get_job_output to continue reading"
    )]
    pub next_stderr_cursor: u64,

    #[property(
        description = "True when there is more output beyond what is returned here. Read the rest with get_job_output"
    )]
    pub truncated: bool,

    #[property(
        description = "The deadline this job actually got, in seconds — either what was asked for, or the server default. It is killed if it runs longer"
    )]
    pub timeout_sec: u64,
}

pub struct RunCommandHandler {
    repo: Arc<RepoContext>,
}

impl RunCommandHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for RunCommandHandler {
    const FUNC_NAME: &'static str = "run_command";

    const DESCRIPTION: &'static str =
        "Starts a command inside the repository and returns a job to poll. Builds and tests take \
         minutes, so the command keeps running after this call returns: use the returned job_id \
         with get_job_output to follow it, and kill_job to stop it. Set wait_sec to have a short \
         command return its result inline instead.";
}

#[async_trait::async_trait]
impl McpToolCall<RunCommandInputData, RunCommandResponse> for RunCommandHandler {
    async fn execute_tool_call(
        &self,
        model: RunCommandInputData,
    ) -> Result<RunCommandResponse, String> {
        let env = match model.env {
            Some(env) => env
                .into_iter()
                .map(|itm| EnvVar {
                    name: itm.env_name,
                    value: itm.env_value,
                })
                .collect(),
            None => Vec::new(),
        };

        let request = RunCommandRequest {
            command: model.command,
            args: model.args.unwrap_or_default(),
            cwd: model.cwd,
            env,
            timeout_sec: model.timeout_sec,
            wait_sec: model.wait_sec,
        };

        let result = run_command(&self.repo, request).await?;

        Ok(RunCommandResponse {
            job_id: result.job.id,
            status: result.job.status.as_str().to_string(),
            exit_code: result.job.exit_code.map(|exit_code| exit_code as i64),
            stdout: result.stdout,
            stderr: result.stderr,
            next_stdout_cursor: result.next_stdout_cursor,
            next_stderr_cursor: result.next_stderr_cursor,
            truncated: result.has_more,
            timeout_sec: result.job.timeout_sec,
        })
    }
}
