//! The output of a single engine decision.

use crate::core::mode::Mode;

/// What to do with one event. The shell maps `Suppress` to "swallow" (return 1
/// from the hook) and `Pass` to "let through" (return 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Suppress,
}

/// The result of `Engine::on_event`: the verdict for this event plus an optional
/// mode transition (e.g. the panic hotkey toggling `Panic`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub verdict: Verdict,
    pub mode_change: Option<Mode>,
    /// `Some(gap_ms)` when this event was suppressed as *chatter* (the suppressed
    /// down, carrying its measured gap) — the payload for `Report::Suppressed`.
    /// `None` for the discarded paired up and for panic-chord consumes, so one
    /// chatter incident reports exactly once and the hotkey never counts.
    pub chatter_gap_ms: Option<u64>,
}
