use flurl::{EmptyRequestModel, FlUrl, HttpVerb};
use rest_api_shared::DashboardStateResponse;

use crate::models::RequestError;

pub async fn get_state() -> Result<DashboardStateResponse, RequestError> {
    let response = FlUrl::new("/api/dashboard/v1/state")
        .execute_request(HttpVerb::Get, EmptyRequestModel)
        .await;

    super::handle_http_response(response).await
}
