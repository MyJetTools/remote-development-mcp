use std::sync::Arc;

use mcp_server_middleware::{McpConnectionInfo, McpInputData, McpInputPayload, McpSession};
use my_http_server::{HttpContext, HttpRequestHeaders};
use rust_extensions::date_time::DateTimeAsMicroseconds;

use super::{clamp_field, SessionInfo, SessionsRegistry};

/// Headers a proxy may use to report the client's country, in the order they
/// are tried. `CF-IPCountry` is Cloudflare's; the other two are what most
/// hand-rolled proxies settle on.
///
/// Nothing here is trusted for anything — it is shown, not acted on.
const COUNTRY_HEADERS: [&str; 3] = ["cf-ipcountry", "x-country-code", "x-country"];

/// Reports the sessions of one endpoint into the shared registry.
///
/// One per endpoint, because the middleware issues session ids per endpoint and
/// the console shows which url a client is working through.
pub struct SessionObserver {
    endpoint: String,
    registry: Arc<SessionsRegistry>,
}

impl SessionObserver {
    pub fn new(endpoint: String, registry: Arc<SessionsRegistry>) -> Self {
        Self { endpoint, registry }
    }
}

#[async_trait::async_trait]
impl McpConnectionInfo for SessionObserver {
    async fn on_connected(&self, session: &McpSession, ctx: &mut HttpContext) {
        let ip = ctx.request.get_ip().get_real_ip_as_string();

        let country = read_country(ctx);

        // The middleware has already buffered the body, so reading it again to
        // name the client costs nothing. Absent on the lazily-adopted path:
        // there is no `initialize` there to carry a name.
        let client = match ctx.request.get_body().await {
            Ok(body) => read_client(body.as_slice()),
            Err(_) => None,
        };

        // Resolved once here rather than on every poll: the console asks for the
        // dashboard several times a minute, and `as_iso3_str` walks the whole
        // country table on each call.
        let country_iso3 = country.as_deref().and_then(to_iso3);

        self.registry.connected(SessionInfo {
            session_id: clamp_field(&session.id),
            endpoint: self.endpoint.clone(),
            ip: clamp_field(&ip),
            country,
            country_iso3,
            client,
            connected_at: DateTimeAsMicroseconds::now(),
        });
    }

    async fn on_disconnected(&self, session: &McpSession) {
        self.registry
            .disconnected(&self.endpoint, &clamp_field(&session.id));
    }
}

/// The reported code as iso3, which is how the flag assets are named.
///
/// `CountryCode::parse` takes iso2 or iso3, so a proxy sending either is
/// handled — and anything it does not recognise comes back `None` rather than
/// being passed through, since the value is about to become part of a URL. The
/// header is client-adjacent and unvalidated; this is what keeps it from
/// choosing what the console fetches.
fn to_iso3(country: &str) -> Option<String> {
    let country = country.trim().to_uppercase();

    match rust_common::country_code::CountryCode::parse(&country) {
        Ok(country) => Some(country.as_iso3_str().to_string()),
        Err(_) => None,
    }
}

fn read_country(ctx: &HttpContext) -> Option<String> {
    for header in COUNTRY_HEADERS {
        let value = ctx
            .request
            .get_headers()
            .try_get_case_insensitive_as_str(header)
            .ok()
            .flatten();

        if let Some(value) = value {
            let value = clamp_field(value);

            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    None
}

/// `claude-code 0.5.0` out of the `initialize` body, when it named itself.
fn read_client(body: &[u8]) -> Option<String> {
    let payload = McpInputPayload::try_parse(body).ok()?;

    let contract = match payload.data {
        McpInputData::Initialize(contract) => contract,
        _ => return None,
    };

    let info = contract.client_info?;

    let name = info.name?;

    let rendered = match info.version {
        Some(version) => format!("{} {}", name, version),
        None => name,
    };

    Some(clamp_field(&rendered))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_iso2_country_becomes_the_iso3_the_flag_is_named_by() {
        assert_eq!(to_iso3("DE").unwrap(), "DEU");
        assert_eq!(to_iso3("US").unwrap(), "USA");
        assert_eq!(to_iso3("AF").unwrap(), "AFG");

        // Cloudflare sends uppercase, but nothing guarantees the next proxy does.
        assert_eq!(to_iso3("de").unwrap(), "DEU");
        assert_eq!(to_iso3(" de ").unwrap(), "DEU");

        // Already iso3 is passed through rather than rejected.
        assert_eq!(to_iso3("DEU").unwrap(), "DEU");
    }

    #[test]
    fn an_unrecognised_country_yields_no_flag_rather_than_a_url() {
        // The header is unvalidated and this value ends up in a URL the console
        // fetches, so anything that is not a country has to stop here.
        assert!(to_iso3("ZZ").is_none());
        assert!(to_iso3("XX").is_none());
        assert!(to_iso3("").is_none());
        assert!(to_iso3("../../etc/passwd").is_none());
        assert!(to_iso3("T1").is_none());
    }

    fn initialize_body(client_info: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","method":"initialize","id":1,"params":{{"protocolVersion":"2025-06-18","capabilities":{{}}{}}}}}"#,
            client_info
        )
    }

    #[test]
    fn a_client_which_names_itself_is_rendered_with_its_version() {
        let body = initialize_body(r#","clientInfo":{"name":"claude-code","version":"0.5.0"}"#);

        assert_eq!(
            read_client(body.as_bytes()),
            Some("claude-code 0.5.0".to_string())
        );
    }

    #[test]
    fn a_client_without_a_version_is_still_named() {
        let body = initialize_body(r#","clientInfo":{"name":"openai-mcp"}"#);

        assert_eq!(read_client(body.as_bytes()), Some("openai-mcp".to_string()));
    }

    #[test]
    fn an_initialize_with_no_client_info_names_nobody() {
        assert_eq!(read_client(initialize_body("").as_bytes()), None);
    }

    #[test]
    fn a_body_which_is_not_an_initialize_names_nobody() {
        // The lazily-adopted path: a session appears on an ordinary call, and
        // there is no name to be had.
        let body = r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#;

        assert_eq!(read_client(body.as_bytes()), None);
    }

    #[test]
    fn rubbish_in_the_body_is_not_a_panic() {
        assert_eq!(read_client(b"not json at all"), None);
        assert_eq!(read_client(b""), None);
    }

    #[test]
    fn a_client_name_can_not_park_an_arbitrary_amount_of_text() {
        let long = "n".repeat(10_000);
        let body = initialize_body(&format!(r#","clientInfo":{{"name":"{}"}}"#, long));

        let rendered = read_client(body.as_bytes()).unwrap();

        assert!(rendered.ends_with('…'));
        assert!(rendered.chars().count() < 200);
    }
}
