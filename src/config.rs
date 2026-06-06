//! User settings — the single on-disk artifact.
//!
//! This is only the type + safe defaults. TOML (de)serialization, defensive load,
//! and clamping are added test-first in issue #6.

/// The hard cap applied to both thresholds, so a hand-edited config can't turn
/// Bouncer into an input black hole (DESIGN.md §6, §8).
pub const MAX_THRESHOLD_MS: u8 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub keyboard_threshold_ms: u8,
    pub mouse_threshold_ms: u8,
    /// `false` == Paused (persisted pass-through).
    pub enabled: bool,
    pub debounce_keyboard: bool,
    pub debounce_mouse: bool,
    pub autostart: bool,
    /// Placeholder representation; becomes a typed chord in #4/#9.
    pub panic_hotkey: String,
    pub confirm_on_quit: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keyboard_threshold_ms: 30,
            mouse_threshold_ms: 40,
            enabled: true,
            debounce_keyboard: true,
            debounce_mouse: true,
            autostart: true,
            panic_hotkey: "Ctrl+Alt+Shift+F12".to_string(),
            confirm_on_quit: true,
        }
    }
}
