//! Platform-agnostic messages crossing the hook thread <-> UI thread boundary.
//!
//! Per ADR-0001 the two threads communicate *only* through these channels — never
//! a shared lock on the input path. Variants will gain real payloads as the slices
//! that use them land (control in #9/#10, reports in #7/#11).

use crate::core::{Device, KeyId, Mode, PanicChord};

/// UI thread -> hook thread.
#[derive(Debug, Clone)]
pub enum Command {
    SetThresholds {
        keyboard_ms: u8,
        mouse_ms: u8,
    },
    SetMode(Mode),
    SetDiagnostic(bool),
    /// New panic chord, captured + validated by the Settings rebind UI.
    RebindPanic(PanicChord),
    Shutdown,
}

/// Hook thread -> UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// Sent once when the backend's message loop is up. Carries the hook thread id
    /// so the UI can `post_wake` it after sending a `Command` (low-level hooks don't
    /// post queue messages, so the loop must be woken to drain commands promptly).
    BackendReady {
        thread_id: u32,
    },
    Suppressed {
        device: Device,
        key: KeyId,
        gap_ms: u64,
    },
    ModeChanged(Mode),
    HookEvicted,
    HookReinstalled,
}
