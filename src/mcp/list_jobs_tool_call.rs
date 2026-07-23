use std::sync::Arc;

use mcp_server_middleware::*;
use rust_extensions::date_time::DateTimeAsMicroseconds;
use serde::{Deserialize, Serialize};

use crate::{jobs::JobStateFilter, repo::Endpoint, scripts::list_jobs};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ListJobsInputData {
    #[property(
        description = "Narrow to one project. Omit to list the jobs of every project on this endpoint"
    )]
    pub project: Option<String>,

    #[property(
        enum: ["all", "running", "finished"],
        description: "Which jobs to return. Defaults to all"
    )]
    pub state: Option<String>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct JobModel {
    #[property(description = "Job identifier")]
    pub job_id: String,

    #[property(description = "Command line this job was started with")]
    pub command_line: String,

    #[property(description = "Working directory, relative to the repository root")]
    pub cwd: String,

    #[property(
        enum: ["running", "exited", "killed", "timed_out"],
        description: "State of the job"
    )]
    pub status: String,

    #[property(description = "Exit code, present once the job has finished")]
    pub exit_code: Option<i64>,

    #[property(description = "When the job was started")]
    pub started_at: String,

    #[property(description = "When the job finished, if it has")]
    pub finished_at: Option<String>,

    #[property(description = "How long the job has been running, or how long it ran")]
    pub duration_sec: f64,

    #[property(description = "The deadline this job was given, in seconds")]
    pub timeout_sec: u64,

    #[property(description = "Seconds left before it is killed. Absent once it has finished")]
    pub remaining_sec: Option<f64>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ListJobsResponse {
    #[property(
        description = "Known jobs, oldest first. Each job_id carries the project it belongs to"
    )]
    pub jobs: Vec<JobModel>,
}

pub struct ListJobsHandler {
    endpoint: Arc<Endpoint>,
}

impl ListJobsHandler {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self { endpoint }
    }
}

impl ToolDefinition for ListJobsHandler {
    const FUNC_NAME: &'static str = "list_jobs";

    const DESCRIPTION: &'static str =
        "Lists known jobs, running and recently finished, across every project of this endpoint \
         unless narrowed to one. Useful to pick up a build started earlier, or to see what is \
         occupying the concurrency limit when run_command reports there is no free slot.";
}

#[async_trait::async_trait]
impl McpToolCall<ListJobsInputData, ListJobsResponse> for ListJobsHandler {
    async fn execute_tool_call(
        &self,
        model: ListJobsInputData,
    ) -> Result<ListJobsResponse, String> {
        // The one tool where leaving `project` out is not ambiguous: listing is
        // read-only, and "what is running right now" is usually asked about the
        // whole endpoint rather than one repository.
        let projects = match model.project.as_deref() {
            Some(project) => vec![self.endpoint.resolve(Some(project))?],
            None => self.endpoint.projects().iter().collect(),
        };

        let filter = JobStateFilter::parse(model.state.as_deref())?;

        let now = DateTimeAsMicroseconds::now();

        let mut jobs: Vec<crate::jobs::Job> = projects
            .into_iter()
            .flat_map(|project| list_jobs(project, filter))
            .collect();

        // Merged from several registries, so re-sorted the way one registry
        // would have returned them: oldest first.
        jobs.sort_by_key(|job| job.started_at.unix_microseconds);

        let jobs = jobs
            .into_iter()
            .map(|job| JobModel {
                job_id: job.id.clone(),
                command_line: job.command_line.clone(),
                cwd: job.cwd.clone(),
                status: job.status.as_str().to_string(),
                exit_code: job.exit_code.map(|exit_code| exit_code as i64),
                started_at: job.started_at.to_rfc3339(),
                finished_at: job.finished_at.map(|finished_at| finished_at.to_rfc3339()),
                duration_sec: job.duration_sec(now),
                timeout_sec: job.timeout_sec,
                remaining_sec: job.remaining_sec(now),
            })
            .collect();

        Ok(ListJobsResponse { jobs })
    }
}
