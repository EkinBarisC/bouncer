//! Detects the panic hotkey chord against the live set of held keys.
//!
//! Behavior is added test-first in issue #4 — this is only the type skeleton.

/// Tracks which keys are currently held and emits an edge-triggered toggle when
/// the configured chord becomes fully held.
#[derive(Debug, Default)]
pub struct PanicDetector {
    // Implementation (held-key set, configured chord, edge state) is introduced
    // with the first failing test in #4.
}

impl PanicDetector {
    pub fn new() -> Self {
        Self::default()
    }
}
