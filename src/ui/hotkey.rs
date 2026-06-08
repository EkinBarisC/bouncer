//! Human-readable panic-hotkey strings ↔ virtual-key codes (issue #10).
//!
//! [`display`] renders a captured chord as `Ctrl+Alt+Shift+F12`; [`parse`] turns a
//! config string back into a validated [`PanicChord`]. Pure and unit-tested, so the
//! persisted `panic_hotkey` is authoritative: the shell parses it at startup to seed
//! the engine, and on Save to apply a rebind or a reset-to-default.

use crate::core::{ChordError, KeyId, PanicChord};

/// Why a hotkey string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// No tokens at all.
    Empty,
    /// A token that isn't a known modifier/letter/digit/function key.
    UnknownToken(String),
    /// Tokens parsed, but the combination isn't a valid chord (e.g. no modifier).
    Invalid(ChordError),
}

/// Render keys as a stable `Mod+Mod+Key` label, modifiers first (Ctrl, Alt, Shift,
/// Win), then the remaining keys ascending.
pub fn display(keys: &[KeyId]) -> String {
    let _ = keys;
    todo!("GREEN")
}

/// Parse a hotkey string (e.g. `"Ctrl+Alt+Shift+F12"`) into a validated chord.
/// Case-insensitive; surrounding whitespace per token is ignored.
pub fn parse(s: &str) -> Result<PanicChord, HotkeyError> {
    let _ = s;
    todo!("GREEN")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: KeyId = 0x10;
    const CTRL: KeyId = 0x11;
    const ALT: KeyId = 0x12;
    const F12: KeyId = 0x7B;
    const A: KeyId = 0x41;

    #[test]
    fn parses_the_default_chord() {
        assert_eq!(
            parse("Ctrl+Alt+Shift+F12").unwrap(),
            PanicChord::new(&[CTRL, ALT, SHIFT, F12]).unwrap()
        );
    }

    #[test]
    fn parsing_is_case_insensitive_and_trims_tokens() {
        assert_eq!(
            parse("  ctrl + a ").unwrap(),
            PanicChord::new(&[CTRL, A]).unwrap()
        );
    }

    #[test]
    fn rejects_an_unknown_token() {
        assert_eq!(
            parse("Ctrl+Banana"),
            Err(HotkeyError::UnknownToken("Banana".to_string()))
        );
    }

    #[test]
    fn rejects_a_chord_with_no_modifier() {
        assert_eq!(
            parse("F12"),
            Err(HotkeyError::Invalid(ChordError::NoModifier))
        );
    }

    #[test]
    fn rejects_empty_input() {
        assert_eq!(parse(""), Err(HotkeyError::Empty));
        assert_eq!(parse("   "), Err(HotkeyError::Empty));
    }

    #[test]
    fn display_orders_modifiers_first() {
        assert_eq!(display(&[F12, SHIFT, ALT, CTRL]), "Ctrl+Alt+Shift+F12");
    }

    #[test]
    fn parse_and_display_round_trip() {
        let s = "Ctrl+Shift+A";
        let chord = parse(s).unwrap();
        assert_eq!(display(&chord.keys()), s);
        assert_eq!(parse(&display(&chord.keys())).unwrap(), chord);
    }
}
