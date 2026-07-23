use std::sync::Arc;

use mcp_server_middleware::McpMiddleware;

use crate::repo::Endpoint;

use super::{
    ApplyPatchHandler, CleanCargoTargetsHandler, CreateReleaseHandler, DeletePathHandler,
    EditFileHandler, GetJobOutputHandler, GitHandler, KillJobHandler, ListDirHandler,
    ListJobsHandler, LoggedTool, MovePathHandler, ReadFileHandler, RepoInfoHandler,
    RunCommandHandler, SearchHandler, WatchActionsHandler, WriteFileHandler,
};

/// Wires the tool set into one MCP endpoint.
///
/// Registered once per endpoint rather than once per repository, which is what
/// keeps a client from paying for these schemas again for every project it can
/// reach. Every handler is handed the same `Endpoint` and holds nothing else,
/// so the url a client connects to still fixes the set of projects it can name:
/// the `project` argument selects among them, it can not widen them.
///
/// Each one is wrapped in [`LoggedTool`], so every call is announced on the
/// console with its parameters.
pub fn register_tools(mcp: &mut McpMiddleware, endpoint: &Arc<Endpoint>) {
    // Orientation.
    register(mcp, endpoint, RepoInfoHandler::new(endpoint.clone()));

    // Running things.
    register(mcp, endpoint, RunCommandHandler::new(endpoint.clone()));
    register(mcp, endpoint, GetJobOutputHandler::new(endpoint.clone()));
    register(mcp, endpoint, ListJobsHandler::new(endpoint.clone()));
    register(mcp, endpoint, KillJobHandler::new(endpoint.clone()));

    // Version control.
    register(mcp, endpoint, GitHandler::new(endpoint.clone()));

    // Navigating.
    register(mcp, endpoint, SearchHandler::new(endpoint.clone()));
    register(mcp, endpoint, ListDirHandler::new(endpoint.clone()));
    register(mcp, endpoint, ReadFileHandler::new(endpoint.clone()));

    // Changing things.
    register(mcp, endpoint, WriteFileHandler::new(endpoint.clone()));
    register(mcp, endpoint, EditFileHandler::new(endpoint.clone()));
    register(mcp, endpoint, ApplyPatchHandler::new(endpoint.clone()));
    register(mcp, endpoint, MovePathHandler::new(endpoint.clone()));
    register(mcp, endpoint, DeletePathHandler::new(endpoint.clone()));

    // Releasing.
    register(mcp, endpoint, CreateReleaseHandler::new(endpoint.clone()));
    register(mcp, endpoint, WatchActionsHandler::new(endpoint.clone()));

    // Maintenance.
    register(
        mcp,
        endpoint,
        CleanCargoTargetsHandler::new(endpoint.clone()),
    );
}

fn register<InputData, OutputData, THandler>(
    mcp: &mut McpMiddleware,
    endpoint: &Arc<Endpoint>,
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
    // Any project of this endpoint carries the same `ActivityLog` — it is one
    // shared feed for the whole server, and the console tells the lines apart by
    // the project each call named.
    let activity = endpoint.projects()[0].activity.clone();

    mcp.register_tool_call(Arc::new(LoggedTool::new(
        handler,
        endpoint.clone(),
        activity,
    )));
}
