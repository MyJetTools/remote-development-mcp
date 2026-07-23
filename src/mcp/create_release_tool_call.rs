use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::RepoContext,
    scripts::{create_release, CreateReleaseRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct CreateReleaseInputData {
    #[property(
        description = "Service to release, when the repository holds several — the tag becomes '{service}-{version}'. Leave it out when the repository is one service, and the tag is the bare version"
    )]
    pub service: Option<String>,

    #[property(
        description = "Version to release, such as 0.1.4. LEAVE THIS EMPTY to release the next version: the highest already-released version with its last number raised. Numbers and dots only — a hyphen is refused, because the build workflow reads the version out of the tag as everything after the last hyphen"
    )]
    pub version: Option<String>,

    #[property(
        description = "Work out the tag and report it without creating anything. Use it to ask 'what would the next version be?'. Defaults to false"
    )]
    pub dry_run: Option<bool>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct CreateReleaseResponse {
    #[property(description = "The git tag, which is what the build workflow triggers on")]
    pub tag: String,

    #[property(
        description = "The version inside that tag — what the docker image will be tagged with"
    )]
    pub version: String,

    #[property(description = "The version this one follows, if the service had any releases")]
    pub previous_version: Option<String>,

    #[property(description = "The GitHub repository the release was created in, as owner/repo")]
    pub repository: String,

    #[property(description = "True when the release was actually created")]
    pub created: bool,

    #[property(description = "True when nothing was created because this was a dry run")]
    pub dry_run: bool,

    #[property(description = "Link to the created release")]
    pub release_url: Option<String>,
}

pub struct CreateReleaseHandler {
    repo: Arc<RepoContext>,
}

impl CreateReleaseHandler {
    pub fn new(repo: Arc<RepoContext>) -> Self {
        Self { repo }
    }
}

impl ToolDefinition for CreateReleaseHandler {
    const FUNC_NAME: &'static str = "create_release";

    const DESCRIPTION: &'static str =
        "Releases a service by creating its GitHub release, which creates the tag and triggers the \
         build that publishes the docker image. Leave version empty and it releases the NEXT one — \
         it reads the tags already on GitHub, takes the highest version for this service and raises \
         the last number, so there is nothing to remember and no way to guess wrong. Pass dry_run \
         to see which version that would be without creating anything. Tag naming follows the house \
         convention: '{service}-{version}' when the repository holds several services, the bare \
         version when it holds one.";
}

#[async_trait::async_trait]
impl McpToolCall<CreateReleaseInputData, CreateReleaseResponse> for CreateReleaseHandler {
    async fn execute_tool_call(
        &self,
        model: CreateReleaseInputData,
    ) -> Result<CreateReleaseResponse, String> {
        let result = create_release(
            &self.repo,
            CreateReleaseRequest {
                service: model.service,
                version: model.version,
                dry_run: model.dry_run.unwrap_or_default(),
            },
        )
        .await?;

        Ok(CreateReleaseResponse {
            tag: result.tag,
            version: result.version,
            previous_version: result.previous_version,
            repository: result.repository,
            created: result.created,
            dry_run: result.dry_run,
            release_url: result.release_url,
        })
    }
}
