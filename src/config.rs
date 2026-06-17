//! User settings — the single on-disk artifact.
//!
//! This is only the type + safe defaults. TOML (de)serialization, defensive load,
//! and clamping are added test-first in issue #6.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The hard cap applied to both thresholds, so a hand-edited config can't turn
/// Bouncer into an input black hole (DESIGN.md §6, §8).
pub const MAX_THRESHOLD_MS: u8 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
        let raw: RawConfig = toml::from_str(toml).unwrap_or_default();
        let d = Config::default();
        Config {
            keyboard_threshold_ms: raw
                .keyboard_threshold_ms
                .unwrap_or(d.keyboard_threshold_ms)
                .min(MAX_THRESHOLD_MS),
            mouse_threshold_ms: raw
                .mouse_threshold_ms
                .unwrap_or(d.mouse_threshold_ms)
                .min(MAX_THRESHOLD_MS),
            enabled: raw.enabled.unwrap_or(d.enabled),
            debounce_keyboard: raw.debounce_keyboard.unwrap_or(d.debounce_keyboard),
            debounce_mouse: raw.debounce_mouse.unwrap_or(d.debounce_mouse),
            autostart: raw.autostart.unwrap_or(d.autostart),
            panic_hotkey: raw.panic_hotkey.unwrap_or(d.panic_hotkey),
            confirm_on_quit: raw.confirm_on_quit.unwrap_or(d.confirm_on_quit),
        }
    }

    /// Serialize to a TOML string suitable for writing to disk.
    pub fn to_toml_string(&self) -> String {
        toml::to_string(self).expect("Config is always serializable")
    }

    /// The canonical on-disk location, `…\Bouncer\config.toml` (via `directories`,
    /// so macOS/Linux paths come free later). `None` only if no home dir resolves.
    pub fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "Bouncer")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    /// Load from a specific file. **Defensive** like [`load_from_str`](Self::load_from_str):
    /// a missing or unreadable file yields defaults, and a corrupt one is parsed
    /// leniently. Never fails.
    pub fn load_from_path(path: &Path) -> Config {
        match std::fs::read_to_string(path) {
            Ok(contents) => Config::load_from_str(&contents),
            Err(_) => Config::default(),
        }
    }

    /// Persist to a specific file, creating parent directories as needed.
    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml_string())
    }
}

/// On-disk shape: every field optional so a partial or unknown-key-laden file
/// parses cleanly, with missing fields backfilled from defaults in `load_from_str`.
/// `serde` ignores unknown keys by default, satisfying forward-compatibility.
#[derive(Default, Deserialize)]
struct RawConfig {
    keyboard_threshold_ms: Option<u8>,
    mouse_threshold_ms: Option<u8>,
    enabled: Option<bool>,
    debounce_keyboard: Option<bool>,
    debounce_mouse: Option<bool>,
    autostart: Option<bool>,
    panic_hotkey: Option<String>,
    confirm_on_quit: Option<bool>,
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

    // --- defensive edge cases (#12): wrong types / out-of-range never crash ---

    // 6a. A wrong-typed field (string where a number is expected) falls back to
    //     defaults rather than panicking.
    #[test]
    fn wrong_typed_field_falls_back_to_defaults() {
        let c = Config::load_from_str(r#"keyboard_threshold_ms = "not a number""#);
        assert_eq!(c, Config::default());
    }

    // 6b. A float for an integer field, and a negative for an unsigned field, both
    //     load defensively (no crash).
    #[test]
    fn float_and_negative_numbers_do_not_crash() {
        assert_eq!(
            Config::load_from_str("mouse_threshold_ms = 3.5"),
            Config::default()
        );
        assert_eq!(
            Config::load_from_str("keyboard_threshold_ms = -10"),
            Config::default()
        );
    }

    // 6c. A wrong-typed boolean field is tolerated too.
    #[test]
    fn wrong_typed_bool_field_does_not_crash() {
        assert_eq!(
            Config::load_from_str(r#"enabled = "yes""#),
            Config::default()
        );
    }

    // --- file lifecycle (#10) ---

    /// A unique scratch path under the OS temp dir, removed when the guard drops.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "bouncer-test-{}-{}-{}",
                std::process::id(),
                tag,
                N.fetch_add(1, Ordering::Relaxed),
            ));
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // 7. A missing file loads as defaults (never errors).
    #[test]
    fn load_from_missing_path_yields_defaults() {
        let dir = TempDir::new("missing");
        let path = dir.path().join("does-not-exist.toml");
        assert_eq!(Config::load_from_path(&path), Config::default());
    }

    // 8. Save then load round-trips a non-default config, creating parent dirs.
    #[test]
    fn save_then_load_round_trips() {
        let dir = TempDir::new("roundtrip");
        let path = dir.path().join("nested").join("config.toml");
        let cfg = Config {
            keyboard_threshold_ms: 17,
            confirm_on_quit: false,
            panic_hotkey: "Ctrl+Shift+Pause".to_string(),
            ..Config::default()
        };

        cfg.save_to_path(&path)
            .expect("save creates dirs and writes");
        assert_eq!(Config::load_from_path(&path), cfg);
    }

    // 9. A corrupt file on disk still loads defensively (defaults), never panics.
    #[test]
    fn load_from_corrupt_path_yields_defaults() {
        let dir = TempDir::new("corrupt");
        let path = dir.path().join("config.toml");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(&path, "@@@ not toml @@@").unwrap();
        assert_eq!(Config::load_from_path(&path), Config::default());
    }
}
