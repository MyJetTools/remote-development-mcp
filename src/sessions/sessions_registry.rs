use ahash::AHashMap;
use parking_lot::Mutex;
use rust_extensions::date_time::DateTimeAsMicroseconds;

use super::SessionInfo;

/// What `initialize` carried about each live session — the half the middleware
/// does not keep.
///
/// It mirrors the middleware's own session set: a row is added when a session
/// appears and removed when it goes, both driven by the middleware's lifecycle
/// hooks, and [`Self::reconcile_against_live`] sweeps any row whose session the
/// middleware no longer lists — the backstop for the connect/disconnect reorder
/// the hooks alone can leave behind. So it needs no cap of its own: it holds no
/// more than the middleware holds, which the idle sweeper bounds. The console
/// reads this to decorate the session list it drives from the middleware; a
/// missing row just means a session shown without its ip or client, never a
/// session hidden.
///
/// `parking_lot` because nothing is awaited under the lock — a hook inserts or
/// removes one entry, the HTTP side looks one up.
pub struct SessionsRegistry {
    sessions: Mutex<AHashMap<String, SessionInfo>>,
}

impl SessionsRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(AHashMap::new()),
        }
    }

    /// A session appeared, or a reused id was adopted afresh. Either way the
    /// newest incarnation is the one to hold, so this overwrites — and its
    /// `created_at` is what a later disconnect checks itself against.
    pub fn connected(&self, session: SessionInfo) {
        self.sessions
            .lock()
            .insert(key_of(&session.endpoint, &session.session_id), session);
    }

    /// A session is gone.
    ///
    /// Removes the row only when it is still the same incarnation — the one that
    /// is leaving. A session id can be reused (lazy creation adopts a
    /// client-supplied id), and the middleware releases its map lock before
    /// calling the hooks, so a stale disconnect for an old incarnation can
    /// arrive after the new one's connect. Matching on `created_at` stops that
    /// disconnect from deleting the live row. Removing a row that is not there
    /// is a no-op.
    pub fn disconnected(
        &self,
        endpoint: &str,
        session_id: &str,
        created_at: DateTimeAsMicroseconds,
    ) {
        let mut sessions = self.sessions.lock();
        let key = key_of(endpoint, session_id);

        let is_same_incarnation = sessions
            .get(&key)
            .map(|existing| existing.created_at.unix_microseconds == created_at.unix_microseconds)
            .unwrap_or(false);

        if is_same_incarnation {
            sessions.remove(&key);
        }
    }

    /// What `initialize` carried for one session, if it is still remembered.
    ///
    /// The console joins this onto the middleware's own session list rather than
    /// rendering it directly: the middleware knows which sessions exist and when
    /// each was last used, this knows who they are. `None` when the row is not
    /// held — the caller renders the session without the decoration rather than
    /// hiding it.
    pub fn get(&self, endpoint: &str, session_id: &str) -> Option<SessionInfo> {
        self.sessions
            .lock()
            .get(&key_of(endpoint, session_id))
            .cloned()
    }

    /// Drops every row whose session the middleware no longer lists as live.
    ///
    /// This is what keeps the registry bounded without a cap, and it closes the
    /// one hole the hooks leave: the middleware inserts a session into its map
    /// *before* it announces the connect, so a concurrent `DELETE` can announce
    /// the disconnect first — a no-op against a row not yet inserted — after
    /// which `connected` inserts a row that no later disconnect will ever match.
    /// Reconciling against the live set reclaims that orphan; the middleware's
    /// own idle sweeper is what ultimately bounds the live set.
    ///
    /// A session that is live but whose connect has not landed yet is safe: its
    /// key is in `live`, so it is kept — it simply carries no decoration until
    /// the connect arrives.
    pub fn reconcile_against_live<'a>(&self, live: impl IntoIterator<Item = (&'a str, &'a str)>) {
        let keep: ahash::AHashSet<String> = live
            .into_iter()
            .map(|(endpoint, session_id)| key_of(endpoint, session_id))
            .collect();

        self.sessions.lock().retain(|key, _| keep.contains(key));
    }

    /// Newest first. Nothing in the running server reads the whole table — the
    /// console drives its list from the middleware and looks rows up one at a
    /// time with [`Self::get`]. Kept for the tests.
    #[cfg(test)]
    pub fn all(&self) -> Vec<SessionInfo> {
        let mut result: Vec<SessionInfo> = self.sessions.lock().values().cloned().collect();

        result.sort_by(|left, right| {
            right
                .created_at
                .unix_microseconds
                .cmp(&left.created_at.unix_microseconds)
        });

        result
    }
}

impl Default for SessionsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// The session id alone is not a key: the middleware issues ids per endpoint, so
/// two endpoints can hand out the same one. The NUL separator can not occur in a
/// url, so no `(endpoint, id)` pair can collide with another.
fn key_of(endpoint: &str, session_id: &str) -> String {
    format!("{}\u{0}{}", endpoint, session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_at(endpoint: &str, id: &str, created_micros: i64) -> SessionInfo {
        SessionInfo {
            session_id: id.to_string(),
            endpoint: endpoint.to_string(),
            ip: "10.0.0.1".to_string(),
            country: Some("DE".to_string()),
            country_iso3: Some("DEU".to_string()),
            client: Some("claude-code".to_string()),
            created_at: DateTimeAsMicroseconds::new(created_micros),
        }
    }

    fn session(endpoint: &str, id: &str) -> SessionInfo {
        session_at(
            endpoint,
            id,
            DateTimeAsMicroseconds::now().unix_microseconds,
        )
    }

    #[test]
    fn a_session_appears_and_then_goes() {
        let registry = SessionsRegistry::new();

        let appeared = session("demo", "s1");
        let created_at = appeared.created_at;
        registry.connected(appeared);
        assert_eq!(registry.all().len(), 1);

        registry.disconnected("demo", "s1", created_at);
        assert!(registry.all().is_empty());
    }

    #[test]
    fn the_same_id_on_two_endpoints_is_two_sessions() {
        let registry = SessionsRegistry::new();

        let on_demo = session("demo", "same-id");
        let demo_created = on_demo.created_at;
        registry.connected(on_demo);
        registry.connected(session("other", "same-id"));

        assert_eq!(registry.all().len(), 2);

        // Closing one must not close the other.
        registry.disconnected("demo", "same-id", demo_created);

        let left = registry.all();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].endpoint, "other");
    }

    #[test]
    fn a_stale_disconnect_does_not_delete_a_reused_ids_new_incarnation() {
        let registry = SessionsRegistry::new();

        // The id is adopted, dropped, and adopted again under the same id — a
        // different incarnation each time, told apart by `created_at`.
        let first = session_at("demo", "reused", 1_000);
        let first_created = first.created_at;
        registry.connected(first);

        // The new incarnation replaces the row.
        registry.connected(session_at("demo", "reused", 2_000));

        // The old incarnation's disconnect arrives late — it must not touch the
        // live row.
        registry.disconnected("demo", "reused", first_created);

        let left = registry.all();
        assert_eq!(left.len(), 1, "the live incarnation was wrongly removed");
        assert_eq!(left[0].created_at.unix_microseconds, 2_000);

        // The new incarnation's own disconnect does remove it.
        registry.disconnected("demo", "reused", DateTimeAsMicroseconds::new(2_000));
        assert!(registry.all().is_empty());
    }

    #[test]
    fn closing_a_session_that_is_not_there_is_harmless() {
        let registry = SessionsRegistry::new();

        registry.disconnected("demo", "never-seen", DateTimeAsMicroseconds::new(1));

        assert!(registry.all().is_empty());
    }

    #[test]
    fn a_flood_of_distinct_ids_never_evicts_an_established_session() {
        let registry = SessionsRegistry::new();

        // The established session, oldest of all.
        registry.connected(session_at("demo", "established", 1));

        // A burst of fresh ids, none of which is ever disconnected.
        for index in 0..1_000 {
            registry.connected(session_at(
                "demo",
                &format!("flood-{}", index),
                1_000 + index,
            ));
        }

        // Nothing dropped it: every id is its own row, and the burst leaves the
        // established one untouched.
        assert!(
            registry.all().iter().any(|s| s.session_id == "established"),
            "the established session was evicted by the flood"
        );
        assert_eq!(registry.all().len(), 1_001);
    }

    #[test]
    fn reconcile_reclaims_an_orphan_the_middleware_no_longer_lists() {
        let registry = SessionsRegistry::new();

        // A row the hooks left behind: connected fired, its matching disconnect
        // can no longer arrive (or already did as a no-op before the insert).
        registry.connected(session_at("demo", "orphan", 1_000));
        // A genuinely live one.
        registry.connected(session_at("demo", "live", 2_000));

        // The middleware lists only the live session.
        registry.reconcile_against_live([("demo", "live")]);

        let left = registry.all();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].session_id, "live");
    }

    #[test]
    fn reconcile_keeps_a_live_session_even_before_its_connect_lands() {
        let registry = SessionsRegistry::new();

        registry.connected(session_at("demo", "decorated", 1_000));

        // Two live sessions — one not yet decorated (no row). Reconcile must not
        // invent a rule that drops the decorated one just because the other has
        // no row: it keeps every key the middleware lists.
        registry.reconcile_against_live([("demo", "decorated"), ("demo", "pending")]);

        assert_eq!(registry.all().len(), 1);
        assert_eq!(registry.all()[0].session_id, "decorated");
    }

    #[test]
    fn newest_is_first() {
        let registry = SessionsRegistry::new();

        registry.connected(session_at("demo", "older", 1_000));
        registry.connected(session_at("demo", "newer", 2_000));

        assert_eq!(registry.all()[0].session_id, "newer");
    }
}
