use std::sync::Arc;

use my_http_server::macros::*;
use my_http_server::*;
use rest_api_shared::{JobOutputRequestModel, JobOutputResponse};

use crate::{
    app::AppContext,
    jobs::OutputStream,
    scripts::{get_job_output, JobOutputRequest},
};

#[http_route(
    method: "GET",
    route: "/api/jobs/v1/output",
    controller: "Jobs",
    summary: "Read a job's output",
    description: "Resumes from the cursors the previous call returned, so watching a long build in the browser reads each byte exactly once — the same hole-free cursor the MCP tool uses.",
    input_data: "JobOutputRequestModel",
    result: [
        {status_code: 200, description: "Output since the cursors", model: JobOutputResponse},
        {status_code: 404, description: "No such repository or job"},
    ]
)]
pub struct GetOutputAction {
    app: Arc<AppContext>,
}

impl GetOutputAction {
    pub fn new(app: Arc<AppContext>) -> Self {
        Self { app }
    }
}

async fn handle_request(
    action: &GetOutputAction,
    input_data: JobOutputRequestModel,
    _ctx: &HttpContext,
) -> Result<HttpOkResult, HttpFailResult> {
    let repo = action
        .app
        .repos
        .iter()
        .find(|repo| repo.name == input_data.repo);

    let repo = match repo {
        Some(repo) => repo,
        None => {
            return HttpFailResult::as_not_found(
                format!("No repository endpoint named '{}'", input_data.repo),
                false,
            )
            .into_err()
        }
    };

    let result = get_job_output(
        repo,
        JobOutputRequest {
            job_id: input_data.job_id,
            stream: OutputStream::Both,
            stdout_cursor: input_data.stdout_cursor,
            stderr_cursor: input_data.stderr_cursor,
            max_bytes: None,
        },
    )
    .await;

    let result = match result {
        Ok(result) => result,
        Err(err) => return HttpFailResult::as_not_found(err, false).into_err(),
    };

    let response = JobOutputResponse {
        job_id: result.job.id.clone(),
        command_line: result.job.command_line.clone(),
        status: result.job.status.as_str().to_string(),
        exit_code: result.job.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
        next_stdout_cursor: result.next_stdout_cursor,
        next_stderr_cursor: result.next_stderr_cursor,
        has_more: result.has_more,
    };

    HttpOutput::as_json(response).into_ok_result(true)
}
