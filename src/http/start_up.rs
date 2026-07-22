use std::{net::SocketAddr, sync::Arc};

use mcp_server_middleware::McpMiddleware;
use my_http_server::MyHttpServer;

use crate::{
    app::{AppContext, APP_NAME, APP_VERSION},
    repo::RepoContext,
};

use super::AuthMiddleware;

pub async fn start(app: &Arc<AppContext>) {
    let addr: SocketAddr = app
        .bind_addr
        .parse()
        .unwrap_or_else(|_| panic!("bind_addr '{}' is not a valid address", app.bind_addr));

    let mut http_server = MyHttpServer::new(addr);

    // First, so that nothing below it ever sees an unauthenticated request.
    http_server.add_middleware(Arc::new(AuthMiddleware::new(app.auth_token.clone())));

    for repo in app.repos.iter() {
        http_server.add_middleware(build_repo_endpoint(repo));
    }

    http_server.start(app.app_states.clone(), my_logger::LOGGER.clone());
}

/// One MCP endpoint per repository.
///
/// Every handler registered here is constructed with this repository's context
/// and nothing else, which is what makes the confinement structural: a tool
/// served at `/my-ssh` holds no reference to any other repository's root, so
/// there is no `repo` argument to get wrong or to forge.
fn build_repo_endpoint(repo: &Arc<RepoContext>) -> Arc<McpMiddleware> {
    let mut mcp = McpMiddleware::new(
        repo.mcp_path,
        APP_NAME,
        APP_VERSION,
        build_instructions(repo),
    );

    crate::mcp::register_tools(&mut mcp, repo);

    Arc::new(mcp)
}

/// `McpMiddleware::new` takes `&'static str`, and these are built from settings
/// at runtime, so the string is leaked. Bounded: once per configured
/// repository, at startup, never per request.
fn build_instructions(repo: &Arc<RepoContext>) -> &'static str {
    let description = match repo.description.as_ref() {
        Some(description) => format!("{}\n\n", description),
        None => String::new(),
    };

    let instructions = format!(
        "{}Tools here operate on the '{}' repository on a remote development machine. Every path \
         argument to a file tool is relative to the repository root, and one resolving outside it \
         is refused. Note that run_command is different: the binary is checked against an \
         allowlist, but its arguments are not path-checked, so treat it as running trusted build \
         tooling, not as a confined sandbox.\n\n\
         Builds and tests are asynchronous. `run_command` returns a job_id immediately (and the \
         finished result inline if the command was quick). Poll a long build with `get_job_output` \
         using the cursors it returns — each call resumes exactly where the previous one stopped, \
         so no output is missed. `kill_job` stops a build together with every process it started.\n\n\
         Start with `repo_info` to see the branch, how dirty the tree is, and which workspace \
         crates can be built or tested on their own. Use `search` to navigate; it is far cheaper \
         than listing directories.",
        description, repo.name
    );

    Box::leak(instructions.into_boxed_str())
}
