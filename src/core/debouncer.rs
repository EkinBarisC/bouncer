//! The per-key, release-anchored chatter decision.
//!
//! Behavior is added test-first in issue #3 — this is only the type skeleton.

/// Tracks, per `KeyId`, the timestamp of the last release, and decides whether an
/// incoming down is chatter (arrived sooner than the device threshold after that
/// key's previous up).
#[derive(Debug, Default)]
pub struct Debouncer {
    // Implementation (e.g. `last_up: HashMap<KeyId, u64>`) is introduced with the
    // first failing test in #3.
}

impl Debouncer {
    pub fn new() -> Self {
        Self::default()
    }
}
