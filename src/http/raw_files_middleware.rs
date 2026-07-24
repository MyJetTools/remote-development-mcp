use std::sync::Arc;

use my_http_server::{
    HttpContext, HttpFailResult, HttpOkResult, HttpOutput, HttpServerMiddleware, WebContentType,
};

use crate::app::AppContext;

const PREFIX: &str = "/raw/";

/// Serves one project's files under `/raw/{project}/{path}`.
///
/// A middleware rather than a controller action because the path *is* the file
/// path, and the action router matches fixed segments. That shape is the whole
/// point: an html file previewed in the console asks for its own stylesheet
/// with a relative url, and a relative url is resolved against the address the
/// page was loaded from. Served as `/api/files/v1/raw?path=…`, every such
/// request lands back on the api route and the page renders unstyled; served as
/// `/raw/my-ssh/wwwroot/index.html`, `app.css` beside it resolves to
/// `/raw/my-ssh/wwwroot/app.css` and arrives.
///
/// `Content-Type` comes from the extension, so a browser told not to sniff still
/// applies the stylesheet and shows the images.
///
/// Registered after the api and before the static files, which are the
/// catch-all. Authentication, when configured, sits in front of all three.
pub struct RawFilesMiddleware {
    app: Arc<AppContext>,
}

impl RawFilesMiddleware {
    pub fn new(app: Arc<AppContext>) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl HttpServerMiddleware for RawFilesMiddleware {
    async fn handle_request(
        &self,
        ctx: &mut HttpContext,
    ) -> Option<Result<HttpOkResult, HttpFailResult>> {
        let (project, path) = split_request(ctx.request.http_path.as_str())?;

        let repo = match crate::http::find_project(&self.app, &project) {
            Ok(repo) => repo,
            Err(err) => return Some(Err(err)),
        };

        let file = match crate::scripts::read_file_bytes(repo, &path).await {
            Ok(file) => file,
            Err(err) => return Some(Err(crate::http::bad_request(err))),
        };

        let content_type = file
            .content_type
            .map(|content_type| WebContentType::Raw(content_type.to_string()));

        let result = HttpOutput::from_builder()
            .set_content(file.bytes)
            .set_content_type_opt(content_type)
            // No `Content-Security-Policy: sandbox` here on purpose: a preview
            // has to show the page as it actually behaves, and this console
            // serves the repositories of the machine it runs on over the local
            // network — the html is the reader's own working copy. The iframe
            // showing it is unsandboxed for the same reason; if that changes,
            // both have to change together.
            //
            // Without it the browser sniffs a type for anything served without
            // one, which is how a text file gets executed as html.
            .add_header("X-Content-Type-Options", "nosniff")
            // The url is stable but the file behind it is not — it is a working
            // copy being edited while the console is open. A cached response
            // would show the reader an edit-old version of a file they just
            // changed, with no way to tell. `no-store` is the strong one; the
            // rest are for intermediaries that predate it.
            .add_header(
                "Cache-Control",
                "no-store, no-cache, must-revalidate, max-age=0",
            )
            .add_header("Pragma", "no-cache")
            .add_header("Expires", "0")
            .into_ok_result(false);

        Some(result)
    }
}

/// Splits `/raw/{project}/{path}` into its two halves, percent-decoded.
///
/// `None` for anything that is not this route, which is what lets the request
/// fall through to whatever owns it. A project id can hold no `/` — it is
/// validated to letters, digits, `.`, `_` and `-` at startup — so the first
/// separator after the prefix is unambiguously the end of it.
fn split_request(path: &str) -> Option<(String, String)> {
    // Never present here in practice; cheap to be sure, since it would
    // otherwise become part of the file name.
    let path = match path.split_once('?') {
        Some((path, _)) => path,
        None => path,
    };

    let rest = path.strip_prefix(PREFIX)?;
    let (project, file) = rest.split_once('/')?;

    if project.is_empty() || file.is_empty() {
        return None;
    }

    Some((decode(project), decode(file)))
}

/// Percent-decoding, leaving the `/` separators as they are.
///
/// `+` is left alone on purpose: it means a space in a form-encoded *query*,
/// never in a path, and a file legitimately named `a+b.txt` would otherwise
/// become unreachable.
fn decode(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;

    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[at + 1]), hex(bytes[at + 2])) {
                decoded.push(high * 16 + low);
                at += 3;
                continue;
            }
        }

        decoded.push(bytes[at]);
        at += 1;
    }

    // A path is not required to be utf-8 at the byte level, but every one this
    // server can resolve is; lossy keeps a malformed escape from failing the
    // request instead of failing the lookup, which is the clearer error.
    String::from_utf8_lossy(&decoded).to_string()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_project_off_the_file_path() {
        assert_eq!(
            split_request("/raw/my-ssh/src/main.rs"),
            Some(("my-ssh".to_string(), "src/main.rs".to_string()))
        );

        // One level deep is still a file path.
        assert_eq!(
            split_request("/raw/my-ssh/README.md"),
            Some(("my-ssh".to_string(), "README.md".to_string()))
        );
    }

    #[test]
    fn leaves_every_other_route_alone() {
        assert_eq!(split_request("/api/dashboard/v1/state"), None);
        assert_eq!(split_request("/assets/app.css"), None);
        assert_eq!(split_request("/"), None);

        // A project with no file named is not this route either — it would
        // otherwise try to serve the repository root as a file.
        assert_eq!(split_request("/raw/my-ssh"), None);
        assert_eq!(split_request("/raw/my-ssh/"), None);
    }

    #[test]
    fn decodes_what_the_browser_escaped() {
        assert_eq!(
            split_request("/raw/my-ssh/docs/a%20b.md"),
            Some(("my-ssh".to_string(), "docs/a b.md".to_string()))
        );

        // Already-legal characters are untouched, `+` included.
        assert_eq!(decode("a+b.txt"), "a+b.txt");
        assert_eq!(decode("plain.rs"), "plain.rs");

        // A `%` that is not an escape stays a `%`.
        assert_eq!(decode("100%.txt"), "100%.txt");
        assert_eq!(decode("%zz"), "%zz");
    }

    /// Confinement is the repository's own job — checked here because this is
    /// the one route where the path arrives as path rather than as a parameter.
    #[test]
    fn an_escaped_traversal_is_still_only_a_path_at_this_point() {
        // Decoded, then handed to `read_file_bytes`, which resolves it inside
        // the root and refuses this.
        assert_eq!(
            split_request("/raw/my-ssh/%2E%2E/%2E%2E/etc/passwd"),
            Some(("my-ssh".to_string(), "../../etc/passwd".to_string()))
        );
    }
}
