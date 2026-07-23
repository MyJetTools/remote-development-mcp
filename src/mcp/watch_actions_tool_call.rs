use std::sync::Arc;

use mcp_server_middleware::*;
use serde::{Deserialize, Serialize};

use crate::{
    repo::Endpoint,
    scripts::{watch_actions, WatchActionsRequest},
};

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct WatchActionsInputData {
    #[property(
        description = "Project to work in. Can be omitted only on an endpoint that serves a single project"
    )]
    pub project: Option<String>,

    #[property(
        description = "Subfolder holding the repository, relative to the root. Use it when the root holds several independent git repositories. Defaults to the root itself"
    )]
    pub path: Option<String>,

    #[property(
        description = "Follow the build started by this tag — pass the tag create_release returned. A run triggered by a tag carries that tag, which is how a release is matched to its build"
    )]
    pub tag: Option<String>,

    #[property(description = "Follow one specific run, by its GitHub run id")]
    pub run_id: Option<u64>,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct WatchedRunModel {
    #[property(description = "GitHub run id — pass it back as run_id to follow this one")]
    pub run_id: u64,

    #[property(description = "Workflow name, such as 'Release App'")]
    pub workflow: String,

    #[property(description = "The tag that started it, when it was started by one")]
    pub tag: Option<String>,

    #[property(
        enum: ["queued", "in_progress", "success", "failure", "cancelled", "skipped", "timed_out", "completed"],
        description: "Where the run is: queued or in_progress while it works, otherwise how it ended"
    )]
    pub outcome: String,

    #[property(description = "True once the run has ended, whatever the outcome")]
    pub finished: bool,

    #[property(
        description = "Which job and step broke, for a run that failed — 'build / Run cargo test'. This is the part worth reading before opening the web UI"
    )]
    pub failed_step: Option<String>,

    #[property(description = "Link to the run on GitHub")]
    pub url: Option<String>,

    #[property(description = "How long it has been running, or how long it took")]
    pub elapsed_sec: f64,
}

#[derive(ApplyJsonSchema, Debug, Serialize, Deserialize)]
pub struct WatchActionsResponse {
    #[property(description = "The GitHub repository these runs belong to, as owner/repo")]
    pub repository: String,

    #[property(description = "The runs, newest first")]
    pub runs: Vec<WatchedRunModel>,
}

pub struct WatchActionsHandler {
    endpoint: Arc<Endpoint>,
}

impl WatchActionsHandler {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self { endpoint }
    }
}

impl ToolDefinition for WatchActionsHandler {
    const FUNC_NAME: &'static str = "watch_actions";

    const DESCRIPTION: &'static str =
        "Follows GitHub Actions builds. Call it with the tag create_release returned and it finds \
         that build, reports where it is, and starts watching it — the server then re-checks every \
         ten seconds on its own, so calling again answers instantly and the operator sees the build \
         progress on the server console meanwhile. Call it with nothing to see the recent runs. A \
         failed run comes back naming the job and step that broke, not just 'failure'.";
}

#[async_trait::async_trait]
impl McpToolCall<WatchActionsInputData, WatchActionsResponse> for WatchActionsHandler {
    async fn execute_tool_call(
        &self,
        model: WatchActionsInputData,
    ) -> Result<WatchActionsResponse, String> {
        let repo = self.endpoint.resolve(model.project.as_deref())?;

        let result = watch_actions(
            repo,
            WatchActionsRequest {
                path: model.path,
                tag: model.tag,
                run_id: model.run_id,
            },
        )
        .await?;

        Ok(WatchActionsResponse {
            repository: result.repository,
            runs: result
                .runs
                .into_iter()
                .map(|run| WatchedRunModel {
                    run_id: run.run_id,
                    workflow: run.workflow,
                    tag: run.tag,
                    outcome: run.outcome,
                    finished: run.finished,
                    failed_step: run.failed_step,
                    url: run.url,
                    elapsed_sec: run.elapsed_sec,
                })
                .collect(),
        })
    }
}
