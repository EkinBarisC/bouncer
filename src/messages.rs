//! Platform-agnostic messages crossing the hook thread <-> UI thread boundary.
//!
//! Per ADR-0001 the two threads communicate *only* through these channels — never
//! a shared lock on the input path. Variants will gain real payloads as the slices
//! that use them land (control in #9/#10, reports in #7/#11).

use crate::core::{Device, KeyId, Mode};

/// UI thread -> hook thread.
#[derive(Debug, Clone)]
pub enum Command {
    SetThresholds {
        keyboard_ms: u8,
        mouse_ms: u8,
    },
    SetMode(Mode),
    SetDiagnostic(bool),
    /// New panic chord. Placeholder `String`; becomes a typed `Chord` in #4/#9.
    RebindPanic(String),
    Shutdown,
}

/// Hook thread -> UI thread.
#[derive(Debug, Clone)]
pub enum Report {
    Suppressed {
        device: Device,
        key: KeyId,
        gap_ms: u64,
    },
    ModeChanged(Mode),
    HookEvicted,
    HookReinstalled,
}
