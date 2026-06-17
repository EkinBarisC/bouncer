//! Human-readable panic-hotkey strings ↔ virtual-key codes.
//!
//! [`display`] renders a captured chord as `Ctrl+Alt+Shift+F12`; [`parse`] turns a
//! config string back into a validated [`PanicChord`]. Pure and unit-tested. This is
//! the on-disk representation of a [`PanicChord`]: `Config` parses the stored string
//! once at load and serializes the chord back through [`display`], so the rest of the
//! app works with the typed chord, never a raw string.

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
        match token_to_vk(token) {
            Some(vk) => keys.push(vk),
            None => return Err(HotkeyError::UnknownToken(token.to_string())),
        }
    }
    if keys.is_empty() {
        return Err(HotkeyError::Empty);
    }
    PanicChord::new(&keys).map_err(HotkeyError::Invalid)
}

/// Sort key: modifiers (Ctrl, Alt, Shift, Win) come before ordinary keys.
fn modifier_rank(vk: KeyId) -> u8 {
    match vk {
        0x11 | 0xA2 | 0xA3 => 0, // Ctrl
        0x12 | 0xA4 | 0xA5 => 1, // Alt
        0x10 | 0xA0 | 0xA1 => 2, // Shift
        0x5B | 0x5C => 3,        // Win
        _ => 4,
    }
}

/// A display name for one virtual-key code.
fn key_name(vk: KeyId) -> String {
    match vk {
        0x10 | 0xA0 | 0xA1 => "Shift".to_string(),
        0x11 | 0xA2 | 0xA3 => "Ctrl".to_string(),
        0x12 | 0xA4 | 0xA5 => "Alt".to_string(),
        0x5B | 0x5C => "Win".to_string(),
        0x30..=0x39 => ((b'0' + (vk - 0x30) as u8) as char).to_string(),
        0x41..=0x5A => ((b'A' + (vk - 0x41) as u8) as char).to_string(),
        0x70..=0x7B => format!("F{}", vk - 0x6F),
        other => format!("0x{other:02X}"),
    }
}

/// Map one parsed token (case-insensitive) to a virtual-key code.
fn token_to_vk(token: &str) -> Option<KeyId> {
    let t = token.to_ascii_lowercase();
    let vk = match t.as_str() {
        "ctrl" | "control" => 0x11,
        "alt" => 0x12,
        "shift" => 0x10,
        "win" | "super" | "meta" | "cmd" => 0x5B,
        // F1..=F12
        _ if t.starts_with('f') && t.len() >= 2 => {
            let n: u8 = t[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                0x6F + n as u32
            } else {
                return None;
            }
        }
        // A single letter or digit.
        _ if t.len() == 1 => {
            let c = t.chars().next()?;
            match c {
                'a'..='z' => 0x41 + (c as u32 - 'a' as u32),
                '0'..='9' => 0x30 + (c as u32 - '0' as u32),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(vk)
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
