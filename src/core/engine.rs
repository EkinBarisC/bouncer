//! The pure decision engine: composes `Debouncer` + `PanicDetector` + `Mode` into
//! one synchronous `on_event` call (per ADR-0001).
//!
//! Behavior is added test-first in issue #5 — this is only the type skeleton.

use crate::core::{debouncer::Debouncer, mode::Mode, panic::PanicDetector};

/// Owns all decision state. Lives on the hook thread; called synchronously from
/// the hook callback. Pure — no OS, no clock, no I/O.
#[derive(Debug, Default)]
pub struct Engine {
    mode: Mode,
    debouncer: Debouncer,
    panic: PanicDetector,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }
}
