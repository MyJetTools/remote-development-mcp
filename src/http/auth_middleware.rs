use my_http_server::{
    async_trait::async_trait, HttpContext, HttpFailResult, HttpOkResult, HttpRequestHeaders,
    HttpServerMiddleware,
};

const AUTHORIZATION_HEADER: &str = "authorization";
const BEARER_PREFIX: &str = "bearer ";

/// Bearer-token gate in front of everything.
///
/// Registered before the MCP middlewares, so it sees every request first:
/// returning `Some(Err(..))` ends the request there, returning `None` lets it
/// fall through to whichever endpoint owns the path. `mcp-server-middleware`
/// has no authentication of its own, so without this the tunnel would be the
/// only thing standing between the internet and a shell on this machine.
pub struct AuthMiddleware {
    expected_token: String,
}

impl AuthMiddleware {
    pub fn new(expected_token: String) -> Self {
        Self { expected_token }
    }

    fn is_authorized(&self, ctx: &HttpContext) -> bool {
        let header = ctx
            .request
            .get_headers()
            .try_get_case_insensitive_as_str(AUTHORIZATION_HEADER);

        let header = match header {
            Ok(Some(header)) => header,
            Ok(None) => return false,
            Err(_) => return false,
        };

        let token = match strip_bearer(header) {
            Some(token) => token,
            None => return false,
        };

        constant_time_eq(token.as_bytes(), self.expected_token.as_bytes())
    }
}

#[async_trait]
impl HttpServerMiddleware for AuthMiddleware {
    async fn handle_request(
        &self,
        ctx: &mut HttpContext,
    ) -> Option<Result<HttpOkResult, HttpFailResult>> {
        if self.is_authorized(ctx) {
            return None;
        }

        // No detail about what was wrong with the token — that only helps
        // someone guessing at it.
        Some(Err(HttpFailResult::as_unauthorized(Some(
            "a valid bearer token is required",
        ))))
    }
}

fn strip_bearer(header: &str) -> Option<&str> {
    let header = header.trim();

    if header.len() < BEARER_PREFIX.len() {
        return None;
    }

    let (prefix, token) = header.split_at(BEARER_PREFIX.len());

    if !prefix.eq_ignore_ascii_case(BEARER_PREFIX) {
        return None;
    }

    Some(token.trim())
}

/// Compares without an early exit, so the time taken does not reveal how much
/// of the token was guessed correctly.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    let mut difference = 0u8;

    for (left_byte, right_byte) in left.iter().zip(right.iter()) {
        difference |= left_byte ^ right_byte;
    }

    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_token_out_of_the_header() {
        assert_eq!(strip_bearer("Bearer secret"), Some("secret"));
        assert_eq!(strip_bearer("bearer secret"), Some("secret"));
        assert_eq!(strip_bearer("BEARER secret"), Some("secret"));
        assert_eq!(strip_bearer("  Bearer   secret  "), Some("secret"));
    }

    #[test]
    fn refuses_a_header_which_is_not_a_bearer_token() {
        assert_eq!(strip_bearer("Basic secret"), None);
        assert_eq!(strip_bearer("secret"), None);
        assert_eq!(strip_bearer(""), None);
        assert_eq!(strip_bearer("Bear"), None);
    }

    #[test]
    fn a_header_carrying_no_token_is_refused_outright() {
        // `trim` shortens these below the prefix, so they never reach the
        // comparison at all.
        assert_eq!(strip_bearer("Bearer "), None);
        assert_eq!(strip_bearer("Bearer   "), None);

        // And an empty token would not match a configured one anyway — though
        // an empty `auth_token` is refused at startup, so it can never be one.
        assert!(!constant_time_eq(b"", b"secret"));
    }

    #[test]
    fn compares_tokens_correctly() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secrft"));
        assert!(!constant_time_eq(b"secret", b"secret-longer"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
