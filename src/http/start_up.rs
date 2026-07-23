use std::{net::SocketAddr, sync::Arc};

use mcp_server_middleware::McpMiddleware;
use my_http_server::{MyHttpServer, StaticFilesMiddleware};

use crate::{
    app::{AppContext, APP_NAME, APP_VERSION},
    repo::{Endpoint, RepoContext},
    sessions::{SessionObserver, SessionsRegistry},
};

use super::{AuthMiddleware, IndexRewriteMiddleware};

pub async fn start(app: &Arc<AppContext>) {
    let addr: SocketAddr = app
        .bind_addr
        .parse()
        .unwrap_or_else(|_| panic!("bind_addr '{}' is not a valid address", app.bind_addr));

    let mut http_server = MyHttpServer::new(addr);

    // Registered first when there is a token, so nothing below it ever sees an
    // unauthenticated request. With no token the server authenticates nothing
    // and trusts what reaches it — which is the intended setup behind a reverse
    // proxy that terminates authentication itself.
    if let Some(auth_token) = app.auth_token.as_ref() {
        http_server.add_middleware(Arc::new(AuthMiddleware::new(auth_token.clone())));
    }

    for endpoint in app.endpoints.iter() {
        http_server.add_middleware(build_endpoint(endpoint, &app.sessions));
    }

    // The REST surface the browser console reads.
    http_server.add_middleware(Arc::new(super::build_controllers(app)));

    http_server.add_middleware(Arc::new(IndexRewriteMiddleware::new()));

    // Last, and deliberately: it is the only catch-all here, so anything
    // registered after it would never be reached. `index.html` is served for
    // unknown paths because the console is a single-page app — a deep link has
    // to reach the router in the browser rather than 404 on the server.
    http_server.add_middleware(Arc::new(
        StaticFilesMiddleware::new()
            .add_index_file("index.html")
            .set_not_found_file("index.html".to_string()),
    ));

    http_server.start(app.app_states.clone(), my_logger::LOGGER.clone());
}

/// One MCP endpoint per configured url, serving the projects that url exposes.
///
/// The tool set is registered once here no matter how many projects are behind
/// it — which is the point: a client pays for these schemas once per connection,
/// not once per repository. Which projects a call may name is fixed by the
/// `Endpoint` the handlers hold, so a project this url does not list can not be
/// reached through it even by passing its id.
fn build_endpoint(
    endpoint: &Arc<Endpoint>,
    sessions: &Arc<SessionsRegistry>,
) -> Arc<McpMiddleware> {
    let mut mcp = McpMiddleware::new(
        endpoint.url,
        APP_NAME,
        APP_VERSION,
        build_instructions(endpoint),
    );

    crate::mcp::register_tools(&mut mcp, endpoint);

    // The middleware owns the truth about which sessions exist — it is what
    // creates them and what sweeps them — so the console reads its events
    // rather than inferring anything from request traffic.
    mcp.register_connection_info(Arc::new(SessionObserver::new(
        endpoint.url.to_string(),
        sessions.clone(),
    )));

    let mcp = Arc::new(mcp);

    // Handed back to the endpoint so the console can pull live sessions from it.
    // The observer above still records what only `initialize` carries — ip,
    // country, client name — which the middleware's own snapshot does not hold.
    endpoint.set_middleware(mcp.clone());

    mcp
}

/// `McpMiddleware::new` takes `&'static str`, and these are built from settings
/// at runtime, so the string is leaked. Bounded: once per configured endpoint,
/// at startup, never per request.
///
/// The project list is generated rather than configured, so it can not drift
/// from what the endpoint actually serves. It is also the only place a client
/// learns which ids exist — cheaper than a discovery tool, which would cost a
/// round trip at the start of every conversation.
fn build_instructions(endpoint: &Arc<Endpoint>) -> &'static str {
    let mut instructions = String::new();

    if let Some(description) = endpoint.description.as_ref() {
        instructions.push_str(description);
        instructions.push_str("\n\n");
    }

    instructions.push_str(
        "Tools here operate on repositories checked out on a remote development machine.\n\n",
    );

    if endpoint.is_single_project() {
        let only = &endpoint.projects()[0];

        instructions.push_str(&format!(
            "This endpoint serves one project, '{}'{}. The `project` argument can be left out.\n\n",
            only.name,
            describe(only)
        ));
    } else {
        instructions.push_str(
            "Every tool takes a `project` argument naming which repository to work in. Available:\n",
        );

        for project in endpoint.projects() {
            instructions.push_str(&format!("- {}{}\n", project.name, describe(project)));
        }

        instructions.push_str(
            "\nPass it on every call — there is no default, and a call without it is refused \
             rather than guessed at.\n\n",
        );
    }

    instructions.push_str(
        "Every path argument to a file tool is relative to that project's root, and one resolving \
         outside it is refused. Note that run_command is different: the binary is checked against \
         an allowlist, but its arguments are not path-checked, so treat it as running trusted \
         build tooling, not as a confined sandbox.\n\n\
         Builds and tests are asynchronous. `run_command` returns a job_id immediately (and the \
         finished result inline if the command was quick). A job id carries its project \
         ('my-ssh:job-000001'), so `get_job_output`, `list_jobs` and `kill_job` need no `project` \
         of their own — a build started in one project can be polled after the work has moved \
         back to another. Poll a long build with `get_job_output` using the cursors it returns — \
         each call resumes exactly where the previous one stopped, so no output is missed. \
         `kill_job` stops a build together with every process it started.\n\n\
         Start with `repo_info` to see the branch, how dirty the tree is, and which workspace \
         crates can be built or tested on their own. Use `search` to navigate; it is far cheaper \
         than listing directories.",
    );

    Box::leak(instructions.into_boxed_str())
}

fn describe(project: &Arc<RepoContext>) -> String {
    match project.description.as_ref() {
        Some(description) => format!(" — {}", description.trim()),
        None => String::new(),
    }
}
