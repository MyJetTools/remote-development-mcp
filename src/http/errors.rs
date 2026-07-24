use std::sync::Arc;

use my_http_server::{HttpFailResult, HttpOutput};

use crate::{app::AppContext, repo::RepoContext};

/// A plain 400 carrying the script's own message.
///
/// Built by hand rather than through `as_validation_error`, which prefixes
/// "Validation error: " — "'src' is a directory, not a file" is not a
/// validation error, and reading it with that in front is worse than reading it
/// alone. Content first, status second: `set_content_as_text` on an empty
/// builder starts the response at 200.
pub fn bad_request(message: impl Into<String>) -> HttpFailResult {
    HttpOutput::from_builder()
        .set_content_as_text(message)
        .set_status_code(400)
        .into_http_fail_result(false, false)
}

/// Turns the project id a request carries into the project itself.
///
/// Every action taking a `repo` needs this same lookup and the same 404, so it
/// lives here rather than being re-typed per action — and, more to the point,
/// so an action can not accidentally answer with a different status for the
/// same "no such project".
pub fn find_project<'s>(
    app: &'s Arc<AppContext>,
    repo: &str,
) -> Result<&'s Arc<RepoContext>, HttpFailResult> {
    let found = app.projects.iter().find(|project| project.name == repo);

    match found {
        Some(project) => Ok(project),
        None => Err(HttpFailResult::as_not_found(
            format!("No project named '{}'", repo),
            false,
        )),
    }
}
