//! Search-as-you-type debounce. Keystrokes are cheap; API calls are not.

/// FR-S2 / NFR-7: long enough that a burst of typing is one request, short
/// enough that results feel immediate once the user stops.
pub const DEFAULT_DEBOUNCE_MS: u64 = 280;

pub struct SearchDebounce {
    interval_ms: u64,
    pending: Option<String>,
    last_input_ms: u64,
    last_fired: Option<String>,
}

impl SearchDebounce {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            pending: None,
            last_input_ms: 0,
            last_fired: None,
        }
    }

    /// Call on every keystroke. Restarts the timer.
    pub fn note_input(&mut self, query: &str, now_ms: u64) {
        self.pending = Some(query.to_owned());
        self.last_input_ms = now_ms;
    }

    /// Call on every tick. Returns the query to search, at most once each. Compares
    /// against the *last* query fired rather than a history, so deleting back and
    /// retyping the same text searches again — the pane is showing something else.
    pub fn should_fire(&mut self, now_ms: u64) -> Option<String> {
        let q = self.pending.as_ref()?;
        if q.trim().is_empty() {
            return None;
        }
        if now_ms.saturating_sub(self.last_input_ms) < self.interval_ms {
            return None;
        }
        if self.last_fired.as_deref() == Some(q.as_str()) {
            return None;
        }
        let q = q.clone();
        self.last_fired = Some(q.clone());
        Some(q)
    }
}

impl Default for SearchDebounce {
    fn default() -> Self {
        Self::new(DEFAULT_DEBOUNCE_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_interval_is_at_least_280ms() {
        // NFR-7 / FR-S2: firing per keystroke risks a rate limit. A `const` block
        // rather than a plain `assert!` — clippy rejects that, and this is stronger:
        // lowering DEFAULT_DEBOUNCE_MS fails to compile rather than failing a test.
        const { assert!(DEFAULT_DEBOUNCE_MS >= 280) }
    }

    #[test]
    fn does_not_fire_before_the_interval_elapses() {
        let mut d = SearchDebounce::new(300);
        d.note_input("bo", 1000);
        assert_eq!(d.should_fire(1200), None, "too soon");
    }

    #[test]
    fn fires_once_the_interval_has_elapsed() {
        let mut d = SearchDebounce::new(300);
        d.note_input("boards", 1000);
        assert_eq!(d.should_fire(1301).as_deref(), Some("boards"));
    }

    #[test]
    fn does_not_fire_twice_for_the_same_query() {
        let mut d = SearchDebounce::new(300);
        d.note_input("boards", 1000);
        assert!(d.should_fire(1301).is_some());
        assert_eq!(d.should_fire(1600), None, "already searched this text");
    }

    #[test]
    fn a_new_keystroke_restarts_the_timer() {
        let mut d = SearchDebounce::new(300);
        d.note_input("bo", 1000);
        d.note_input("boa", 1200); // resets
        assert_eq!(d.should_fire(1301), None, "timer must restart on new input");
        assert_eq!(d.should_fire(1501).as_deref(), Some("boa"));
    }

    #[test]
    fn an_empty_query_never_fires() {
        let mut d = SearchDebounce::new(300);
        d.note_input("", 1000);
        assert_eq!(d.should_fire(2000), None);
    }

    #[test]
    fn whitespace_only_query_never_fires() {
        let mut d = SearchDebounce::new(300);
        d.note_input("   ", 1000);
        assert_eq!(d.should_fire(2000), None);
    }

    #[test]
    fn retyping_a_previous_query_fires_again() {
        // Deleting back to "bo" and retyping "boards" must re-search: the
        // results pane was showing something else by then.
        let mut d = SearchDebounce::new(300);
        d.note_input("boards", 1000);
        assert!(d.should_fire(1301).is_some());
        d.note_input("bo", 1400);
        assert!(d.should_fire(1701).is_some());
        d.note_input("boards", 1800);
        assert_eq!(d.should_fire(2101).as_deref(), Some("boards"));
    }
}
