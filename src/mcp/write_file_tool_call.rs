use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{repo::RepoContext, scripts::write_file};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct WriteFileInputData {
    #[property(description = "File to write, relative to the repository root")]
    pub path: String,

    #[property(description = "Full contents to write. Any existing file is replaced")]
    pub content: String,

    #[property(
        description = "Create missing parent folders. Defaults to false, so a typo in the folder name fails instead of creating one"
    )]
    pub create_dirs: Option<bool>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct WriteFileResponse {
    #[property(description = "How many bytes were written")]
    pub bytes_written: u64,

    #[property(description = "Path that was written, relative to the repository root")]
    pub path: String,
}

pub struct WriteFileHandler {
    repo: Arc<RepoContext>,
}

impl WriteFileHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for WriteFileHandler {
    const FUNC_NAME: &'static str = "write_file";

    const DESCRIPTION: &'static str =
        "Creates a file or replaces one entirely. To change part of an existing file use edit_file, \
         which does not require sending the whole content back.";
}

#[async_trait::async_trait]
impl McpToolCall<WriteFileInputData, WriteFileResponse> for WriteFileHandler {
    async fn execute_tool_call(
        &self,
        model: WriteFileInputData,
    ) -> Result<WriteFileResponse, String> {
        let bytes_written = write_file(
            &self.repo,
            &model.path,
            &model.content,
            model.create_dirs.unwrap_or_default(),
        )
        .await?;

        Ok(WriteFileResponse {
            bytes_written: bytes_written as u64,
            path: model.path,
        })
    }
}
