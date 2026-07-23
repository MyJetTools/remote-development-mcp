use rust_extensions::date_time::DateTimeAsMicroseconds;

/// Every field here is derived from something the client sent — a header it
/// chose, or a body it wrote — so each is cut to a length that still reads in a
/// table. Without this a single connection could park an arbitrary amount of
/// text in the registry.
const MAX_FIELD_CHARS: usize = 120;

/// What `initialize` carried about one session, kept beside the middleware's own
/// record of it.
///
/// Deliberately only the fields the middleware does not hold. It knows the id,
/// the protocol version, when the session was created and when it was last used,
/// and the console reads those straight off `McpSession` on every poll —
/// duplicating them here would mean two answers to the same question, and the
/// copy would be the stale one. What it can not know is who is on the other end:
/// the ip, the country a proxy reported, and the name from `clientInfo`.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    /// Url the session was opened against. Sessions are per-endpoint, so a
    /// client connected to two endpoints is two sessions — but one session can
    /// reach every project its endpoint exposes.
    pub endpoint: String,
    /// Real client IP, already resolved through `X-Forwarded-For`.
    pub ip: String,
    /// Whatever the proxy in front reported, when it reports anything. Kept
    /// verbatim — it is the label, and an unrecognised value is still the truth
    /// about what arrived.
    pub country: Option<String>,
    /// The same country as iso3, resolved at connect time because that is how
    /// the flag assets are named. `None` when the header parsed as no country,
    /// which is also what stops an unvalidated header reaching a URL.
    pub country_iso3: Option<String>,
    /// `claude-code 0.5.0`, from the `clientInfo` of `initialize`. Absent when
    /// the session was adopted from an id the server never issued — there is no
    /// `initialize` on that path to carry a name.
    pub client: Option<String>,
    /// The middleware's `create` for this session — its identity, not a
    /// timestamp for display. A session id can be reused (lazy creation adopts a
    /// client-supplied id), so this is what tells the incarnation that is
    /// leaving apart from a newer one already holding the same id: a disconnect
    /// only removes the row when this still matches.
    pub created_at: DateTimeAsMicroseconds,
}

/// Cuts a client-supplied value to something a table can hold, never splitting
/// a character.
pub fn clamp_field(text: &str) -> String {
    let text = text.trim();

    if text.chars().count() <= MAX_FIELD_CHARS {
        return text.to_string();
    }

    let clamped: String = text.chars().take(MAX_FIELD_CHARS).collect();

    format!("{}…", clamped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_value_is_left_alone() {
        assert_eq!(clamp_field("claude-code 0.5.0"), "claude-code 0.5.0");
    }

    #[test]
    fn a_client_can_not_park_an_arbitrary_amount_of_text_here() {
        let clamped = clamp_field(&"x".repeat(10_000));

        assert!(clamped.chars().count() <= MAX_FIELD_CHARS + 1);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn cutting_never_splits_a_character() {
        // Multi-byte throughout: a naive byte truncation would panic or produce
        // invalid UTF-8.
        let clamped = clamp_field(&"é".repeat(10_000));

        assert_eq!(clamped.chars().count(), MAX_FIELD_CHARS + 1);
    }
}
