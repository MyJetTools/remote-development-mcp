use std::sync::Arc;

use mcp_server_middleware::McpMiddleware;

use crate::repo::RepoContext;

use super::{
    ApplyPatchHandler, CleanCargoTargetsHandler, DeletePathHandler, EditFileHandler,
    GetJobOutputHandler, GitHandler, KillJobHandler, ListDirHandler, ListJobsHandler, LoggedTool,
    MovePathHandler, ReadFileHandler, RepoInfoHandler, RunCommandHandler, SearchHandler,
    WriteFileHandler,
};

/// Wires this repository's tools into its own MCP endpoint.
///
/// Every handler is handed the same `RepoContext` and holds nothing else, so
/// the endpoint a client connects to is what decides which repository it can
/// reach — there is no repository argument to get wrong or to forge.
///
/// Each one is wrapped in [`LoggedTool`], so every call is announced on the
/// console with its parameters.
pub fn register_tools(mcp: &mut McpMiddleware, repo: &Arc<RepoContext>) {
    // Orientation.
    register(mcp, repo, RepoInfoHandler::new(repo.clone()));

    // Running things.
    register(mcp, repo, RunCommandHandler::new(repo.clone()));
    register(mcp, repo, GetJobOutputHandler::new(repo.clone()));
    register(mcp, repo, ListJobsHandler::new(repo.clone()));
    register(mcp, repo, KillJobHandler::new(repo.clone()));

    // Version control.
    register(mcp, repo, GitHandler::new(repo.clone()));

    // Navigating.
    register(mcp, repo, SearchHandler::new(repo.clone()));
    register(mcp, repo, ListDirHandler::new(repo.clone()));
    register(mcp, repo, ReadFileHandler::new(repo.clone()));

    // Changing things.
    register(mcp, repo, WriteFileHandler::new(repo.clone()));
    register(mcp, repo, EditFileHandler::new(repo.clone()));
    register(mcp, repo, ApplyPatchHandler::new(repo.clone()));
    register(mcp, repo, MovePathHandler::new(repo.clone()));
    register(mcp, repo, DeletePathHandler::new(repo.clone()));

    // Maintenance.
    register(mcp, repo, CleanCargoTargetsHandler::new(repo.clone()));
}

fn register<InputData, OutputData, THandler>(
    mcp: &mut McpMiddleware,
    repo: &Arc<RepoContext>,
    handler: THandler,
) where
    InputData: mcp_server_middleware::json_schema::JsonTypeDescription
        + serde::Serialize
        + serde::de::DeserializeOwned
        + Sized
        + Send
        + Sync
        + 'static,
    OutputData: mcp_server_middleware::json_schema::JsonTypeDescription
        + serde::Serialize
        + serde::de::DeserializeOwned
        + Sized
        + Send
        + Sync
        + 'static,
    THandler: mcp_server_middleware::McpToolCall<InputData, OutputData>
        + mcp_server_middleware::ToolDefinition
        + Send
        + Sync
        + 'static,
{
    mcp.register_tool_call(Arc::new(LoggedTool::new(
        handler,
        repo.name.clone(),
        repo.activity.clone(),
    )));
}
