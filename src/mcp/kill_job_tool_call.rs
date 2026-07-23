use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{jobs::KillSignal, repo::Endpoint, scripts::kill_job};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct KillJobInputData {
    #[property(description = "Job identifier returned by run_command")]
    pub job_id: String,

    #[property(
        enum: ["TERM", "KILL", "INT"],
        description: "Signal to send. Defaults to TERM, which lets the process clean up. Use KILL only for something that will not stop"
    )]
    pub signal: Option<String>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct KillJobResponse {
    #[property(description = "Job identifier")]
    pub job_id: String,

    #[property(
        enum: ["running", "exited", "killed", "timed_out"],
        description: "State of the job when the signal was sent. A job which was still running takes a moment to actually stop, so poll get_job_output to see it finish"
    )]
    pub status: String,

    #[property(
        description = "False when the job had already finished and there was nothing left to signal"
    )]
    pub signalled: bool,
}

pub struct KillJobHandler {
    endpoint: Arc<Endpoint>,
}

impl KillJobHandler {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self { endpoint }
    }
}

impl ToolDefinition for KillJobHandler {
    const FUNC_NAME: &'static str = "kill_job";

    const DESCRIPTION: &'static str =
        "Stops a running job, together with every process it started — killing a cargo build takes \
         its rustc children with it. The job does not stop instantly: poll get_job_output until its \
         status is no longer 'running'.";
}

#[async_trait::async_trait]
impl McpToolCall<KillJobInputData, KillJobResponse> for KillJobHandler {
    async fn execute_tool_call(&self, model: KillJobInputData) -> Result<KillJobResponse, String> {
        // Routed by the job id's own project prefix — see get_job_output.
        let (repo, job_id) = self.endpoint.resolve_job(&model.job_id)?;

        let signal = KillSignal::parse(model.signal.as_deref())?;

        let result = kill_job(repo, &job_id, signal)?;

        Ok(KillJobResponse {
            job_id: result.job.id,
            status: result.job.status.as_str().to_string(),
            signalled: result.signalled,
        })
    }
}
