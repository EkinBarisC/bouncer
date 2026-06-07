//! Test-only harness behind the `integration-test` feature (Windows only).
//!
//! Keeps the `SendInput`-driven end-to-end test FFI-free: the test describes a
//! script of synthetic key events and asserts on what reached downstream, while
//! all the Win32 plumbing (installing `WH_KEYBOARD_LL`, a downstream observer
//! hook, the message loop, `SendInput` replay) lives here. Never compiled into a
//! production build.
//!
//! Implemented to GREEN in issue #7.

use crate::core::Thresholds;

/// One scripted synthetic key event to inject (flagged injected; the backend runs
/// in integration-test mode so it processes these rather than passing them through).
#[derive(Debug, Clone, Copy)]
pub struct SynthKey {
    /// Virtual-key code.
    pub vk: u16,
    /// `true` = key-down, `false` = key-up.
    pub down: bool,
    /// Delay before injecting this event, measured from the previous one (ms).
    pub gap_ms: u64,
}

/// A key event seen downstream — i.e. one Bouncer did **not** suppress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedKey {
    pub vk: u16,
    pub down: bool,
}

/// Run a live Bouncer keyboard hook in integration-test mode, replay `script` via
/// `SendInput`, and return the key events that reached downstream, in order.
///
/// Orchestration only — the test stays free of Win32 details.
pub fn run_keyboard_e2e(thresholds: Thresholds, script: &[SynthKey]) -> Vec<ObservedKey> {
    let _ = (thresholds, script);
    todo!("install hook + downstream observer, SendInput-replay, capture — issue #7 GREEN")
}
