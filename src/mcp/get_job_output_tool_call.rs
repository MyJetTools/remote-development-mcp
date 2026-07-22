use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    jobs::OutputStream,
    repo::RepoContext,
    scripts::{get_job_output, JobOutputRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct GetJobOutputInputData {
    #[property(description = "Job identifier returned by run_command")]
    pub job_id: String,

    #[property(
        enum: ["stdout", "stderr", "both"],
        description: "Which stream to read. Defaults to both"
    )]
    pub stream: Option<String>,

    #[property(
        description = "Byte offset to continue stdout from. Use next_stdout_cursor from the previous call, or omit to start at the beginning"
    )]
    pub stdout_cursor: Option<u64>,

    #[property(description = "Byte offset to continue stderr from. Use next_stderr_cursor")]
    pub stderr_cursor: Option<u64>,

    #[property(
        description = "Largest amount of bytes to return per stream. Defaults to 65536, capped at 4194304"
    )]
    pub max_bytes: Option<u64>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct GetJobOutputResponse {
    #[property(
        enum: ["running", "exited", "killed", "timed_out"],
        description: "State of the job"
    )]
    pub status: String,

    #[property(description = "Exit code, present once the job has finished")]
    pub exit_code: Option<i64>,

    #[property(description = "Output written since stdout_cursor")]
    pub stdout: String,

    #[property(description = "Output written since stderr_cursor")]
    pub stderr: String,

    #[property(description = "Pass this back as stdout_cursor on the next call")]
    pub next_stdout_cursor: u64,

    #[property(description = "Pass this back as stderr_cursor on the next call")]
    pub next_stderr_cursor: u64,

    #[property(
        description = "True when output beyond this response is already waiting, so it is worth calling again straight away rather than sleeping"
    )]
    pub truncated: bool,

    #[property(description = "How long the job has been running, or how long it ran")]
    pub duration_sec: f64,
}

pub struct GetJobOutputHandler {
    repo: Arc<RepoContext>,
}

impl GetJobOutputHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for GetJobOutputHandler {
    const FUNC_NAME: &'static str = "get_job_output";

    const DESCRIPTION: &'static str =
        "Reads a job's output from a byte offset onwards, and reports whether it is still running. \
         This is how a long build is followed: call it with the cursors from the previous call and \
         each response continues exactly where the last one stopped, so nothing is missed however \
         far behind the polling falls. Keep calling while status is 'running'.";
}

#[async_trait::async_trait]
impl McpToolCall<GetJobOutputInputData, GetJobOutputResponse> for GetJobOutputHandler {
    async fn execute_tool_call(
        &self,
        model: GetJobOutputInputData,
    ) -> Result<GetJobOutputResponse, String> {
        let stream = OutputStream::parse(model.stream.as_deref())?;

        let request = JobOutputRequest {
            job_id: model.job_id,
            stream,
            stdout_cursor: model.stdout_cursor.unwrap_or_default(),
            stderr_cursor: model.stderr_cursor.unwrap_or_default(),
            max_bytes: model.max_bytes,
        };

        let result = get_job_output(&self.repo, request).await?;

        let now = rust_extensions::date_time::DateTimeAsMicroseconds::now();

        Ok(GetJobOutputResponse {
            status: result.job.status.as_str().to_string(),
            exit_code: result.job.exit_code.map(|exit_code| exit_code as i64),
            stdout: result.stdout,
            stderr: result.stderr,
            next_stdout_cursor: result.next_stdout_cursor,
            next_stderr_cursor: result.next_stderr_cursor,
            truncated: result.has_more,
            duration_sec: result.job.duration_sec(now),
        })
    }
}
