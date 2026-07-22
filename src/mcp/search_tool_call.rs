use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::RepoContext,
    scripts::{search, SearchRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct SearchInputData {
    #[property(description = "Regular expression to look for")]
    pub pattern: String,

    #[property(
        description = "Folder or file to search in, relative to the repository root. Defaults to the whole repository"
    )]
    pub path: Option<String>,

    #[property(description = "Only search files matching this glob, for example '*.rs'")]
    pub glob: Option<String>,

    #[property(description = "Largest number of matches to return. Defaults to 200")]
    pub max_results: Option<u64>,

    #[property(description = "Match case-insensitively. Defaults to false")]
    pub ignore_case: Option<bool>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct SearchMatchModel {
    #[property(description = "File the match is in, relative to the repository root")]
    pub file: String,

    #[property(description = "Line number, counting from 1")]
    pub line: u64,

    #[property(description = "The matching line")]
    pub text: String,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    #[property(description = "Matches found")]
    pub matches: Vec<SearchMatchModel>,

    #[property(
        description = "True when there were more matches than max_results. Narrow the pattern or the glob"
    )]
    pub truncated: bool,
}

pub struct SearchHandler {
    repo: Arc<RepoContext>,
}

impl SearchHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for SearchHandler {
    const FUNC_NAME: &'static str = "search";

    const DESCRIPTION: &'static str =
        "Searches file contents by regular expression, the main way to find your way around a large \
         repository. Ignores what git ignores and skips binaries, so results stay relevant.";
}

#[async_trait::async_trait]
impl McpToolCall<SearchInputData, SearchResponse> for SearchHandler {
    async fn execute_tool_call(&self, model: SearchInputData) -> Result<SearchResponse, String> {
        let request = SearchRequest {
            pattern: model.pattern,
            path: model.path,
            glob: model.glob,
            max_results: model.max_results.map(|max_results| max_results as usize),
            ignore_case: model.ignore_case.unwrap_or_default(),
        };

        let result = search(&self.repo, request).await?;

        Ok(SearchResponse {
            matches: result
                .matches
                .into_iter()
                .map(|found| SearchMatchModel {
                    file: found.file,
                    line: found.line,
                    text: found.text,
                })
                .collect(),
            truncated: result.truncated,
        })
    }
}
