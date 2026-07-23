use std::collections::VecDeque;

use parking_lot::Mutex;

use super::ActivityEvent;

/// How much history the console keeps: a screenful plus enough to scroll back
/// over what just happened.
const MAX_EVENTS: usize = 100;

/// The feed the browser console renders under "History".
///
/// A bounded ring: the server runs for weeks, so an unbounded log would be a
/// slow leak. `parking_lot` because nothing is awaited while it is held —
/// pushing an event is a `push_back` and a possible `pop_front`.
///
/// Deliberately writes nothing to the terminal. The terminal is reserved for
/// panics, and a feed streaming through it would bury the one thing that has to
/// be impossible to miss. Everything here is read over the REST API instead.
pub struct ActivityLog {
    events: Mutex<VecDeque<ActivityEvent>>,
}

impl ActivityLog {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(VecDeque::with_capacity(MAX_EVENTS)),
        }
    }

    pub fn push(&self, event: ActivityEvent) {
        let mut events = self.events.lock();

        if events.len() == MAX_EVENTS {
            events.pop_front();
        }

        events.push_back(event);
    }

    /// Newest first, at most `amount` — the order the console draws.
    pub fn recent(&self, amount: usize) -> Vec<ActivityEvent> {
        self.events
            .lock()
            .iter()
            .rev()
            .take(amount)
            .cloned()
            .collect()
    }
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(subject: &str) -> ActivityEvent {
        ActivityEvent::tool_call("repo".to_string(), subject.to_string(), "{}".to_string())
    }

    #[test]
    fn newest_comes_first() {
        let log = ActivityLog::new();

        log.push(event("first"));
        log.push(event("second"));
        log.push(event("third"));

        let recent = log.recent(2);

        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].subject, "third");
        assert_eq!(recent[1].subject, "second");
    }

    #[test]
    fn the_ring_stays_bounded_and_drops_the_oldest() {
        let log = ActivityLog::new();

        for index in 0..(MAX_EVENTS + 100) {
            log.push(event(&format!("call-{}", index)));
        }

        assert_eq!(log.recent(usize::MAX).len(), MAX_EVENTS);

        let newest = log.recent(1);
        assert_eq!(newest[0].subject, format!("call-{}", MAX_EVENTS + 99));
    }

    #[test]
    fn asking_for_more_than_there_is_returns_what_there_is() {
        let log = ActivityLog::new();

        log.push(event("only"));

        assert_eq!(log.recent(50).len(), 1);
    }
}
