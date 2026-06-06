//! UI-side observability state, built from the `Report` stream (never in the
//! Engine, per ADR-0001). All in-memory: the counter resets every boot and nothing
//! keystroke-related touches disk.
//!
//! Behavior is added test-first in issue #11 — this is only the type skeleton.

/// Session suppressed counters + the diagnostic ring buffer + the gap histogram.
#[derive(Debug, Default)]
pub struct Stats {
    // Implementation (per-device counters, bounded ring buffer, histogram buckets,
    // diagnostic expiry) is introduced with the first failing test in #11.
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }
}
