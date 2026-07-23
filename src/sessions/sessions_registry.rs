use ahash::AHashMap;
use parking_lot::Mutex;

use super::SessionInfo;

/// Ceiling on how many live sessions are tracked.
///
/// The middleware sweeps idle sessions on its own, so in normal use the map
/// stays tiny. The cap is here for the abnormal case: a session id may be
/// chosen by the client, so anything that can reach an endpoint can mint new
/// ones faster than the sweeper retires them.
const MAX_SESSIONS: usize = 200;

/// The live MCP sessions, as the browser console shows them.
///
/// Shared by every endpoint's observer and the REST layer. `parking_lot`
/// because nothing is awaited under the lock — a hook only inserts or removes
/// one entry, and the HTTP side only clones the map out.
pub struct SessionsRegistry {
    sessions: Mutex<AHashMap<String, SessionInfo>>,
}

impl SessionsRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(AHashMap::new()),
        }
    }

    /// A session appeared.
    ///
    /// Keyed by endpoint as well as by id, because the middleware issues ids per
    /// endpoint: two of them can hand out the same id without either
    /// knowing, and a shared key would let one overwrite the other.
    pub fn connected(&self, session: SessionInfo) {
        let mut sessions = self.sessions.lock();

        sessions.insert(key_of(&session.endpoint, &session.session_id), session);

        prune(&mut sessions);
    }

    /// A session is gone. Removing one that is not there is not an error: the
    /// cap below may already have dropped it.
    pub fn disconnected(&self, endpoint: &str, session_id: &str) {
        self.sessions.lock().remove(&key_of(endpoint, session_id));
    }

    /// What `initialize` carried for one session, if it is still remembered.
    ///
    /// The console joins this onto the middleware's own session list rather than
    /// rendering it directly: the middleware knows which sessions exist and when
    /// each was last used, this knows who they are. Returns `None` when the cap
    /// below has already dropped the row, which is why the caller must render a
    /// session without it rather than hide the session.
    pub fn get(&self, endpoint: &str, session_id: &str) -> Option<SessionInfo> {
        self.sessions
            .lock()
            .get(&key_of(endpoint, session_id))
            .cloned()
    }

    /// Newest first.
    ///
    /// Nothing in the server reads the whole table any more — the console drives
    /// its list from the middleware and looks rows up one at a time with
    /// [`Self::get`]. Kept for the tests, which is where the cap below is worth
    /// observing.
    #[cfg(test)]
    pub fn all(&self) -> Vec<SessionInfo> {
        let mut result: Vec<SessionInfo> = self.sessions.lock().values().cloned().collect();

        result.sort_by(|left, right| {
            right
                .connected_at
                .unix_microseconds
                .cmp(&left.connected_at.unix_microseconds)
        });

        result
    }
}

impl Default for SessionsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn key_of(endpoint: &str, session_id: &str) -> String {
    format!("{}\u{0}{}", endpoint, session_id)
}

/// Drops the oldest sessions past the cap. Oldest rather than newest: a flood
/// of forged ids must not push out the connection someone is actually working
/// through, which by then is the older one.
fn prune(sessions: &mut AHashMap<String, SessionInfo>) {
    if sessions.len() <= MAX_SESSIONS {
        return;
    }

    let mut by_age: Vec<(String, i64)> = sessions
        .iter()
        .map(|(key, session)| (key.clone(), session.connected_at.unix_microseconds))
        .collect();

    by_age.sort_by_key(|(_, when)| *when);

    let to_remove = sessions.len() - MAX_SESSIONS;

    for (key, _) in by_age.iter().take(to_remove) {
        sessions.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rust_extensions::date_time::DateTimeAsMicroseconds;

    fn session(endpoint: &str, id: &str) -> SessionInfo {
        SessionInfo {
            session_id: id.to_string(),
            endpoint: endpoint.to_string(),
            ip: "10.0.0.1".to_string(),
            country: Some("DE".to_string()),
            country_iso3: Some("DEU".to_string()),
            client: Some("claude-code".to_string()),
            connected_at: DateTimeAsMicroseconds::now(),
        }
    }

    #[test]
    fn a_session_appears_and_then_goes() {
        let registry = SessionsRegistry::new();

        registry.connected(session("demo", "s1"));
        assert_eq!(registry.all().len(), 1);

        registry.disconnected("demo", "s1");
        assert!(registry.all().is_empty());
    }

    #[test]
    fn the_same_id_on_two_endpoints_is_two_sessions() {
        let registry = SessionsRegistry::new();

        registry.connected(session("demo", "same-id"));
        registry.connected(session("other", "same-id"));

        assert_eq!(registry.all().len(), 2);

        // Closing one must not close the other.
        registry.disconnected("demo", "same-id");

        let left = registry.all();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].endpoint, "other");
    }

    #[test]
    fn closing_a_session_that_is_not_there_is_harmless() {
        let registry = SessionsRegistry::new();

        registry.disconnected("demo", "never-seen");

        assert!(registry.all().is_empty());
    }

    #[test]
    fn the_registry_stays_bounded_under_a_flood_of_ids() {
        let registry = SessionsRegistry::new();

        for index in 0..(MAX_SESSIONS + 50) {
            registry.connected(session("demo", &format!("s{}", index)));
        }

        assert_eq!(registry.all().len(), MAX_SESSIONS);
    }

    #[test]
    fn newest_is_first() {
        let registry = SessionsRegistry::new();

        let mut older = session("demo", "older");
        older.connected_at.unix_microseconds -= 60_000_000;

        registry.connected(older);
        registry.connected(session("demo", "newer"));

        assert_eq!(registry.all()[0].session_id, "newer");
    }
}
