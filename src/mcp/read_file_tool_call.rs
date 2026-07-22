use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{repo::RepoContext, scripts::read_file};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ReadFileInputData {
    #[property(description = "File to read, relative to the repository root")]
    pub path: String,

    #[property(
        description = "First line to return, counting from 1. Use it with limit to page through a large file"
    )]
    pub offset: Option<u64>,

    #[property(description = "How many lines to return. Defaults to 2000")]
    pub limit: Option<u64>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct ReadFileResponse {
    #[property(description = "The requested lines, joined by newlines")]
    pub content: String,

    #[property(description = "How many lines the whole file has")]
    pub total_lines: u64,

    #[property(
        description = "True when the returned window does not cover the whole file. Page on with offset and limit"
    )]
    pub truncated: bool,
}

pub struct ReadFileHandler {
    repo: Arc<RepoContext>,
}

impl ReadFileHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for ReadFileHandler {
    const FUNC_NAME: &'static str = "read_file";

    const DESCRIPTION: &'static str =
        "Reads a text file from the repository, optionally a window of it by line. For finding \
         something across many files use search instead — it is far cheaper than reading them.";
}

#[async_trait::async_trait]
impl McpToolCall<ReadFileInputData, ReadFileResponse> for ReadFileHandler {
    async fn execute_tool_call(
        &self,
        model: ReadFileInputData,
    ) -> Result<ReadFileResponse, String> {
        let result = read_file(
            &self.repo,
            &model.path,
            model.offset.map(|offset| offset as usize),
            model.limit.map(|limit| limit as usize),
        )
        .await?;

        Ok(ReadFileResponse {
            content: result.content,
            total_lines: result.total_lines as u64,
            truncated: result.truncated,
        })
    }
}
