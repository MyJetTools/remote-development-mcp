use std::sync::Arc;

use mcp_server_middleware::McpMiddleware;

use crate::repo::RepoContext;

use super::{
    ApplyPatchHandler, CleanCargoTargetsHandler, DeletePathHandler, EditFileHandler,
    GetJobOutputHandler, GitHandler, KillJobHandler, ListDirHandler, ListJobsHandler,
    MovePathHandler, ReadFileHandler, RepoInfoHandler, RunCommandHandler, SearchHandler,
    WriteFileHandler,
};

/// Wires this repository's tools into its own MCP endpoint.
///
/// Every handler is handed the same `RepoContext` and holds nothing else, so
/// the endpoint a client connects to is what decides which repository it can
/// reach — there is no repository argument to get wrong or to forge.
pub fn register_tools(mcp: &mut McpMiddleware, repo: &Arc<RepoContext>) {
    // Orientation.
    mcp.register_tool_call(Arc::new(RepoInfoHandler::new(repo.clone())));

    // Running things.
    mcp.register_tool_call(Arc::new(RunCommandHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(GetJobOutputHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(ListJobsHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(KillJobHandler::new(repo.clone())));

    // Version control.
    mcp.register_tool_call(Arc::new(GitHandler::new(repo.clone())));

    // Navigating.
    mcp.register_tool_call(Arc::new(SearchHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(ListDirHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(ReadFileHandler::new(repo.clone())));

    // Changing things.
    mcp.register_tool_call(Arc::new(WriteFileHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(EditFileHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(ApplyPatchHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(MovePathHandler::new(repo.clone())));
    mcp.register_tool_call(Arc::new(DeletePathHandler::new(repo.clone())));

    // Maintenance.
    mcp.register_tool_call(Arc::new(CleanCargoTargetsHandler::new(repo.clone())));
}
