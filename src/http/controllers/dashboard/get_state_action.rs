use std::sync::Arc;

use my_http_server::macros::*;
use my_http_server::*;
use rest_api_shared::DashboardStateResponse;

use crate::app::AppContext;

#[http_route(
    method: "GET",
    route: "/api/dashboard/v1/state",
    controller: "Dashboard",
    summary: "Everything the console shows",
    description: "Repositories, running and finished jobs, the activity feed and the GitHub Actions runs being followed — one consistent snapshot, so a polling page can not show a job as running in one pane and finished in another.",
    result: [
        {status_code: 200, description: "Current state", model: DashboardStateResponse},
    ]
)]
pub struct GetStateAction {
    app: Arc<AppContext>,
}

impl GetStateAction {
    pub fn new(app: Arc<AppContext>) -> Self {
        Self { app }
    }
}

async fn handle_request(
    action: &GetStateAction,
    _ctx: &HttpContext,
) -> Result<HttpOkResult, HttpFailResult> {
    let state = crate::scripts::read_dashboard_state(&action.app);

    HttpOutput::as_json(state).into_ok_result(true)
}
