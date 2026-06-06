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

impl Config {
    /// Parse settings from a TOML string. **Defensive**: never fails — invalid,
    /// partial, or empty input falls back to defaults for the affected fields,
    /// unknown keys are ignored, and thresholds are clamped to `MAX_THRESHOLD_MS`.
    pub fn load_from_str(toml: &str) -> Config {
        let _ = toml;
        unimplemented!("Config::load_from_str — implemented after test review (#6)")
    }

    /// Serialize to a TOML string suitable for writing to disk.
    pub fn to_toml_string(&self) -> String {
        unimplemented!("Config::to_toml_string — implemented after test review (#6)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Serialize then parse yields an equal config.
    #[test]
    fn round_trips_through_toml() {
        let original = Config::default();
        let parsed = Config::load_from_str(&original.to_toml_string());
        assert_eq!(parsed, original);
    }

    // 2. Empty input → all defaults.
    #[test]
    fn empty_input_yields_defaults() {
        assert_eq!(Config::load_from_str(""), Config::default());
    }

    // 3. Unparseable input → defaults (never panics).
    #[test]
    fn garbage_input_yields_defaults() {
        assert_eq!(
            Config::load_from_str("@@@ this is not valid toml @@@"),
            Config::default()
        );
    }

    // 4. A partial file sets the present fields and defaults the rest.
    #[test]
    fn partial_input_fills_missing_with_defaults() {
        let c = Config::load_from_str("keyboard_threshold_ms = 12");
        assert_eq!(c.keyboard_threshold_ms, 12);
        assert_eq!(c.mouse_threshold_ms, Config::default().mouse_threshold_ms);
        assert_eq!(c.confirm_on_quit, Config::default().confirm_on_quit);
    }

    // 5. Out-of-range thresholds are clamped on load.
    #[test]
    fn thresholds_are_clamped_on_load() {
        let c = Config::load_from_str("keyboard_threshold_ms = 250\nmouse_threshold_ms = 200");
        assert_eq!(c.keyboard_threshold_ms, MAX_THRESHOLD_MS);
        assert_eq!(c.mouse_threshold_ms, MAX_THRESHOLD_MS);
    }

    // 6. Unknown keys are ignored rather than causing a failure.
    #[test]
    fn unknown_keys_are_ignored() {
        let c = Config::load_from_str("keyboard_threshold_ms = 25\nbogus_field = \"ignored\"");
        assert_eq!(c.keyboard_threshold_ms, 25);
    }
}
