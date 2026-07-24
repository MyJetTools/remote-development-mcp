use std::sync::Arc;

use my_http_server::macros::*;
use my_http_server::*;
use rest_api_shared::{FolderEntryModel, ListFolderRequestModel, ListFolderResponse};

use crate::{
    app::AppContext,
    scripts::{list_dir, EntryType, ListDirRequest},
};

#[http_route(
    method: "GET",
    route: "/api/files/v1/folder",
    controller: "Files",
    summary: "List one folder of a project",
    description: "One level only — the console's tree loads a folder when it is opened rather than pulling the whole repository up front, which for a monorepo would be a hundred thousand entries nobody asked for. Filtered exactly the way the MCP tools see the tree: whatever git ignores, and hidden entries, stay out.",
    input_data: "ListFolderRequestModel",
    result: [
        {status_code: 200, description: "The folder's entries", model: ListFolderResponse},
        {status_code: 404, description: "No such project"},
        {status_code: 400, description: "The path is not a folder, or resolves outside the project"},
    ]
)]
pub struct ListFolderAction {
    app: Arc<AppContext>,
}

impl ListFolderAction {
    pub fn new(app: Arc<AppContext>) -> Self {
        Self { app }
    }
}

async fn handle_request(
    action: &ListFolderAction,
    input_data: ListFolderRequestModel,
    _ctx: &HttpContext,
) -> Result<HttpOkResult, HttpFailResult> {
    let repo = crate::http::find_project(&action.app, &input_data.repo)?;

    // An empty string is what the console sends for the root, and it means the
    // same thing as sending nothing at all.
    let path = input_data
        .path
        .filter(|path| !path.trim().is_empty())
        .map(|path| path.trim().to_string());

    let result = list_dir(
        repo,
        ListDirRequest {
            path: path.clone(),
            recursive: false,
            max_depth: None,
            respect_gitignore: true,
        },
    )
    .await;

    let result = match result {
        Ok(result) => result,
        Err(err) => return crate::http::bad_request(err).into_err(),
    };

    let mut entries: Vec<FolderEntryModel> = result
        .entries
        .into_iter()
        .map(|entry| FolderEntryModel {
            name: leaf_of(&entry.path).to_string(),
            path: entry.path,
            // A symlink is shown as the file it stands for. Following it to find
            // out whether it points at a folder would mean a stat per row, and
            // opening one is answered by listing it anyway.
            is_dir: entry.entry_type == EntryType::Dir,
            size_bytes: entry.size_bytes,
        })
        .collect();

    // Folders first, then by name — the order a file tree is read in. `list_dir`
    // sorts by path alone, which interleaves them.
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    let response = ListFolderResponse {
        path: path.unwrap_or_default(),
        entries,
        truncated: result.truncated,
    };

    HttpOutput::as_json(response).into_ok_result(true)
}

fn leaf_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((_, leaf)) => leaf,
        None => path,
    }
}
