use my_http_server::{HttpContext, HttpFailResult, HttpOkResult, HttpPath, HttpServerMiddleware};

/// Rewrites a request for the site root into a request for `index.html`.
///
/// The static-files middleware picks a `Content-Type` from the *request* path's
/// extension, so serving the console at `/` leaves the header off entirely and
/// the browser is left to sniff — which breaks outright behind a proxy that
/// sends `X-Content-Type-Options: nosniff`. Naming the file makes the extension
/// visible, and `text/html` comes back with it.
///
/// Registered before the static middleware and after everything else: it
/// touches only the root, and it never answers a request itself.
pub struct IndexRewriteMiddleware;

impl IndexRewriteMiddleware {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl HttpServerMiddleware for IndexRewriteMiddleware {
    async fn handle_request(
        &self,
        ctx: &mut HttpContext,
    ) -> Option<Result<HttpOkResult, HttpFailResult>> {
        if is_site_root(ctx.request.http_path.as_str()) {
            ctx.request.http_path = HttpPath::from_str("/index.html");
        }

        // Never answers — the static middleware behind it does.
        None
    }
}

fn is_site_root(path: &str) -> bool {
    path.is_empty() || path == "/"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_root_is_rewritten() {
        assert!(is_site_root("/"));
        assert!(is_site_root(""));
    }

    #[test]
    fn anything_with_a_path_is_left_alone() {
        // A deep link is already handled by the not-found file, which does set
        // the content type — rewriting it here would only hide that.
        assert!(!is_site_root("/index.html"));
        assert!(!is_site_root("/assets/app.css"));
        assert!(!is_site_root("/api/dashboard/v1/state"));
    }
}
