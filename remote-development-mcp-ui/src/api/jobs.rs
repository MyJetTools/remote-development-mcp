use flurl::{FlUrl, HttpVerb};
use rest_api_shared::{JobOutputRequestModel, JobOutputResponse};

use crate::models::RequestError;

pub async fn get_output(
    repo: String,
    job_id: String,
    stdout_cursor: u64,
    stderr_cursor: u64,
) -> Result<JobOutputResponse, RequestError> {
    let request = JobOutputRequestModel {
        repo,
        job_id,
        stdout_cursor,
        stderr_cursor,
    };

    let response = FlUrl::new("/api/jobs/v1/output")
        .execute_request(HttpVerb::Get, request)
        .await;

    super::handle_http_response(response).await
}
