//! Human-readable panic-hotkey strings ↔ [`KeyCode`]s.
//!
//! [`display`] renders a captured chord as `Ctrl+Alt+Shift+F12`; [`parse`] turns a
//! config string back into a validated [`PanicChord`]. Pure and unit-tested. This is
//! the on-disk representation of a [`PanicChord`]: `Config` parses the stored string
//! once at load and serializes the chord back through [`display`], so the rest of the
//! app works with the typed chord, never a raw string.

use crate::core::event::KeyCode;
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
    let mut sorted: Vec<KeyId> = keys.to_vec();
    sorted.sort_by_key(|&k| (modifier_rank(k), k));
    sorted
        .into_iter()
        .map(key_name)
        .collect::<Vec<_>>()
        .join("+")
}

/// Parse a hotkey string (e.g. `"Ctrl+Alt+Shift+F12"`) into a validated chord.
/// Case-insensitive; surrounding whitespace per token is ignored.
pub fn parse(s: &str) -> Result<PanicChord, HotkeyError> {
    let mut keys = Vec::new();
    for token in s.split('+').map(str::trim).filter(|t| !t.is_empty()) {
        match token_to_keycode(token) {
            Some(key) => keys.push(key),
            None => return Err(HotkeyError::UnknownToken(token.to_string())),
        }
    }
    if keys.is_empty() {
        return Err(HotkeyError::Empty);
    }
    PanicChord::new(&keys).map_err(HotkeyError::Invalid)
}

/// Sort key: modifiers (Ctrl, Alt, Shift, Win) come before ordinary keys.
fn modifier_rank(key: KeyId) -> u8 {
    match key {
        KeyCode::Control => 0,
        KeyCode::Alt => 1,
        KeyCode::Shift => 2,
        KeyCode::Meta => 3,
        _ => 4,
    }
}

/// A display name for one [`KeyCode`].
fn key_name(key: KeyId) -> String {
    match key {
        KeyCode::Shift => "Shift".to_string(),
        KeyCode::Control => "Ctrl".to_string(),
        KeyCode::Alt => "Alt".to_string(),
        KeyCode::Meta => "Win".to_string(),
        KeyCode::Function(n) => format!("F{n}"),
        KeyCode::Letter(c) => c.to_string(),
        KeyCode::Digit(d) => d.to_string(),
        KeyCode::Mouse(b) => format!("{b:?}"),
        KeyCode::Other(code) => format!("0x{code:02X}"),
    }
}

/// Map one parsed token (case-insensitive) to a [`KeyCode`].
fn token_to_keycode(token: &str) -> Option<KeyId> {
    let t = token.to_ascii_lowercase();
    let key = match t.as_str() {
        "ctrl" | "control" => KeyCode::Control,
        "alt" => KeyCode::Alt,
        "shift" => KeyCode::Shift,
        "win" | "super" | "meta" | "cmd" => KeyCode::Meta,
        // F1..=F12
        _ if t.starts_with('f') && t.len() >= 2 => {
            let n: u8 = t[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                KeyCode::Function(n)
            } else {
                return None;
            }
        }
        // A single letter or digit.
        _ if t.len() == 1 => {
            let c = t.chars().next()?;
            match c {
                'a'..='z' => KeyCode::Letter(c.to_ascii_uppercase()),
                '0'..='9' => KeyCode::Digit(c as u8 - b'0'),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: KeyId = KeyCode::Shift;
    const CTRL: KeyId = KeyCode::Control;
    const ALT: KeyId = KeyCode::Alt;
    const F12: KeyId = KeyCode::Function(12);
    const A: KeyId = KeyCode::Letter('A');

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
