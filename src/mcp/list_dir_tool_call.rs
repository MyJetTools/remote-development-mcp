use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::RepoContext,
    scripts::{list_dir, ListDirRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ListDirInputData {
    #[property(
        description = "Folder to list, relative to the repository root. Defaults to the root itself"
    )]
    pub path: Option<String>,

    #[property(description = "Descend into subfolders. Defaults to false")]
    pub recursive: Option<bool>,

    #[property(description = "How many levels deep to descend when recursive is set")]
    pub max_depth: Option<u64>,

    #[property(
        description = "Leave out anything git ignores, such as target and node_modules. Defaults to true"
    )]
    pub respect_gitignore: Option<bool>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct DirEntryModel {
    #[property(description = "Path relative to the repository root, usable with the other tools")]
    pub path: String,

    #[property(enum: ["file", "dir", "symlink"], description: "What this entry is")]
    pub entry_type: String,

    #[property(description = "Size in bytes")]
    pub size_bytes: u64,

    #[property(description = "When it was last modified")]
    pub modified: Option<String>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ListDirResponse {
    #[property(description = "Entries, sorted by path")]
    pub entries: Vec<DirEntryModel>,

    #[property(
        description = "True when there were more entries than could be returned. Narrow the listing with path or max_depth"
    )]
    pub truncated: bool,
}

pub struct ListDirHandler {
    repo: Arc<RepoContext>,
}

impl ListDirHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for ListDirHandler {
    const FUNC_NAME: &'static str = "list_dir";

    const DESCRIPTION: &'static str =
        "Lists the contents of a folder in the repository. The .git folder is always left out, and \
         by default so is everything git ignores. In a large repository, prefer search to find \
         things — a recursive listing of the whole tree is rarely what is wanted.";
}

#[async_trait::async_trait]
impl McpToolCall<ListDirInputData, ListDirResponse> for ListDirHandler {
    async fn execute_tool_call(&self, model: ListDirInputData) -> Result<ListDirResponse, String> {
        let request = ListDirRequest {
            path: model.path,
            recursive: model.recursive.unwrap_or_default(),
            max_depth: model.max_depth.map(|max_depth| max_depth as usize),
            respect_gitignore: model.respect_gitignore.unwrap_or(true),
        };

        let result = list_dir(&self.repo, request).await?;

        Ok(ListDirResponse {
            entries: result
                .entries
                .into_iter()
                .map(|entry| DirEntryModel {
                    path: entry.path,
                    entry_type: entry.entry_type.as_str().to_string(),
                    size_bytes: entry.size_bytes,
                    modified: entry.modified,
                })
                .collect(),
            truncated: result.truncated,
        })
    }
}
