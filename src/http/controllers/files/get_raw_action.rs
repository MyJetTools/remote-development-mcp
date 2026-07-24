use std::sync::Arc;

use my_http_server::macros::*;
use my_http_server::*;
use rest_api_shared::FileRequestModel;

use crate::app::AppContext;

#[http_route(
    method: "GET",
    route: "/api/files/v1/raw",
    controller: "Files",
    summary: "One file's bytes, for an <img> or an <iframe>",
    description: "The only endpoint the browser fetches by itself rather than through the console's own code, because that is what an <img src> and an <iframe src> do. Served with a 'Content-Security-Policy: sandbox' header, so an html file out of a repository can not run script against this origin even when opened directly.",
    input_data: "FileRequestModel",
    result: [
        {status_code: 200, description: "The file's bytes"},
        {status_code: 404, description: "No such project"},
        {status_code: 400, description: "No such file, it is a folder, it is too large, or it resolves outside the project"},
    ]
)]
pub struct GetRawAction {
    app: Arc<AppContext>,
}

impl GetRawAction {
    pub fn new(app: Arc<AppContext>) -> Self {
        Self { app }
    }
}

async fn handle_request(
    action: &GetRawAction,
    input_data: FileRequestModel,
    _ctx: &HttpContext,
) -> Result<HttpOkResult, HttpFailResult> {
    let repo = crate::http::find_project(&action.app, &input_data.repo)?;

    let result = crate::scripts::read_file_bytes(repo, &input_data.path).await;

    let result = match result {
        Ok(result) => result,
        Err(err) => return crate::http::bad_request(err).into_err(),
    };

    let content_type = result
        .content_type
        .map(|content_type| WebContentType::Raw(content_type.to_string()));

    HttpOutput::from_builder()
        .set_content(result.bytes)
        .set_content_type_opt(content_type)
        // The file comes out of a repository, which is content this server does
        // not vouch for. `sandbox` drops it into an opaque origin with scripts
        // off, so an html page here can not read the console's origin — the
        // iframe carries the same restriction, this covers opening the url
        // directly.
        .add_header("Content-Security-Policy", "sandbox")
        // Without a content type the browser would otherwise sniff one, which is
        // how a text file gets executed as html.
        .add_header("X-Content-Type-Options", "nosniff")
        .into_ok_result(false)
}
