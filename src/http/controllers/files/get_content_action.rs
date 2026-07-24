use std::sync::Arc;

use my_http_server::macros::*;
use my_http_server::*;
use rest_api_shared::{
    FileContentResponse, FileRequestModel, FILE_KIND_BINARY, FILE_KIND_HTML, FILE_KIND_IMAGE,
    FILE_KIND_MARKDOWN, FILE_KIND_PDF, FILE_KIND_TEXT, FILE_KIND_TOO_BIG,
};

use crate::{app::AppContext, scripts::FilePreview};

#[http_route(
    method: "GET",
    route: "/api/files/v1/content",
    controller: "Files",
    summary: "How one file should be shown, and its text when it is text",
    description: "Whether a file is text is answered by decoding its bytes as UTF-8 here, not by guessing from its name in the browser. Images and html come back as a kind only — the console points an <img> or an <iframe> at the raw endpoint for those, rather than carrying bytes through this JSON.",
    input_data: "FileRequestModel",
    result: [
        {status_code: 200, description: "The file's kind, and its text when it has one", model: FileContentResponse},
        {status_code: 404, description: "No such project"},
        {status_code: 400, description: "No such file, it is a folder, or it resolves outside the project"},
    ]
)]
pub struct GetContentAction {
    app: Arc<AppContext>,
}

impl GetContentAction {
    pub fn new(app: Arc<AppContext>) -> Self {
        Self { app }
    }
}

async fn handle_request(
    action: &GetContentAction,
    input_data: FileRequestModel,
    _ctx: &HttpContext,
) -> Result<HttpOkResult, HttpFailResult> {
    let repo = crate::http::find_project(&action.app, &input_data.repo)?;

    let result = crate::scripts::preview_file(repo, &input_data.path).await;

    let result = match result {
        Ok(result) => result,
        Err(err) => return crate::http::bad_request(err).into_err(),
    };

    let (kind, text, html) = match result.preview {
        FilePreview::Text { source, html } => (FILE_KIND_TEXT, Some(source), html),
        FilePreview::Markdown { source, html } => (FILE_KIND_MARKDOWN, Some(source), Some(html)),
        FilePreview::Image => (FILE_KIND_IMAGE, None, None),
        FilePreview::Html => (FILE_KIND_HTML, None, None),
        FilePreview::Pdf => (FILE_KIND_PDF, None, None),
        FilePreview::Binary => (FILE_KIND_BINARY, None, None),
        FilePreview::TooBig => (FILE_KIND_TOO_BIG, None, None),
    };

    let response = FileContentResponse {
        path: result.path,
        size_bytes: result.size_bytes,
        kind: kind.to_string(),
        text,
        html,
    };

    HttpOutput::as_json(response).into_ok_result(true)
}
