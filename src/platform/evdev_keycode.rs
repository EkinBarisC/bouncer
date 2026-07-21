//! Linux evdev key/button codes -> the platform-neutral [`KeyCode`].
//!
//! Pure and OS-free — the codes are just numbers from the kernel's
//! `input-event-codes.h`, so this compiles and tests on every platform even though
//! only `linux.rs` calls it. (Same arrangement as [`crate::platform::watchdog`].)
//!
//! evdev codes are *positional*: `KEY_Q` is the physical key in the US-QWERTY `Q`
//! position regardless of the active layout. That matches the Windows backend's
//! virtual-key mapping, so a chord bound on one OS means the same physical keys on
//! the other.

use crate::core::{Device, KeyCode, KeyId, MouseButton};

// Modifiers. Left/right variants fold to one logical modifier, per `KeyCode`.
const KEY_LEFTCTRL: u16 = 29;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_RIGHTSHIFT: u16 = 54;
const KEY_LEFTALT: u16 = 56;
const KEY_RIGHTALT: u16 = 100;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTMETA: u16 = 126;

// Digit row: `KEY_1`..`KEY_9` are contiguous and `KEY_0` follows them.
const KEY_1: u16 = 2;
const KEY_9: u16 = 10;
const KEY_0: u16 = 11;

// Letter rows, each contiguous in US-QWERTY order.
const KEY_Q: u16 = 16;
const KEY_A: u16 = 30;
const KEY_Z: u16 = 44;
const TOP_ROW: &[u8] = b"QWERTYUIOP";
const HOME_ROW: &[u8] = b"ASDFGHJKL";
const BOTTOM_ROW: &[u8] = b"ZXCVBNM";

// Function keys: F1..F10 are contiguous, but F11/F12 were bolted on later.
const KEY_F1: u16 = 59;
const KEY_F10: u16 = 68;
const KEY_F11: u16 = 87;
const KEY_F12: u16 = 88;

// Mouse buttons.
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_SIDE: u16 = 0x113;
const BTN_EXTRA: u16 = 0x114;
/// Last code in the mouse-button block (`BTN_TASK`); the whole block belongs to the
/// mouse for threshold purposes, even the buttons with no [`MouseButton`] name.
const BTN_TASK: u16 = 0x117;

/// Translate an `EV_KEY` code into the core's key identity. Chord-relevant keys get
/// a named variant; every other physical key keeps a stable opaque identity, which
/// is all the debouncer needs.
pub fn to_keycode(code: u16) -> KeyId {
    match code {
        KEY_LEFTSHIFT | KEY_RIGHTSHIFT => KeyCode::Shift,
        KEY_LEFTCTRL | KEY_RIGHTCTRL => KeyCode::Control,
        KEY_LEFTALT | KEY_RIGHTALT => KeyCode::Alt,
        KEY_LEFTMETA | KEY_RIGHTMETA => KeyCode::Meta,
        KEY_1..=KEY_9 => KeyCode::Digit((code - KEY_1 + 1) as u8),
        KEY_0 => KeyCode::Digit(0),
        KEY_F1..=KEY_F10 => KeyCode::Function((code - KEY_F1 + 1) as u8),
        KEY_F11 => KeyCode::Function(11),
        KEY_F12 => KeyCode::Function(12),
        BTN_LEFT => KeyCode::Mouse(MouseButton::Left),
        BTN_RIGHT => KeyCode::Mouse(MouseButton::Right),
        BTN_MIDDLE => KeyCode::Mouse(MouseButton::Middle),
        BTN_SIDE => KeyCode::Mouse(MouseButton::X1),
        BTN_EXTRA => KeyCode::Mouse(MouseButton::X2),
        _ => match letter(code) {
            Some(c) => KeyCode::Letter(c),
            None => KeyCode::Other(code as u32),
        },
    }
}

/// Which device class an `EV_KEY` code belongs to — i.e. which threshold applies.
/// Keyed on the raw code rather than the translated [`KeyCode`] so the unnamed
/// mouse buttons (`BTN_FORWARD`, `BTN_BACK`, `BTN_TASK`) are still debounced as
/// mouse input rather than falling into the keyboard bucket via `Other`.
pub fn device_class(code: u16) -> Device {
    if (BTN_LEFT..=BTN_TASK).contains(&code) {
        Device::Mouse
    } else {
        Device::Keyboard
    }
}

/// The letter on the key at this position, uppercase, or `None` if it isn't a
/// letter key.
fn letter(code: u16) -> Option<char> {
    let row = |start: u16, letters: &[u8]| {
        letters
            .get((code.checked_sub(start)?) as usize)
            .map(|&b| b as char)
    };
    row(KEY_Q, TOP_ROW)
        .or_else(|| row(KEY_A, HOME_ROW))
        .or_else(|| row(KEY_Z, BOTTOM_ROW))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_fold_left_and_right_together() {
        assert_eq!(to_keycode(KEY_LEFTSHIFT), KeyCode::Shift);
        assert_eq!(to_keycode(KEY_RIGHTSHIFT), KeyCode::Shift);
        assert_eq!(to_keycode(KEY_LEFTCTRL), KeyCode::Control);
        assert_eq!(to_keycode(KEY_RIGHTCTRL), KeyCode::Control);
        assert_eq!(to_keycode(KEY_LEFTALT), KeyCode::Alt);
        assert_eq!(to_keycode(KEY_RIGHTALT), KeyCode::Alt);
        assert_eq!(to_keycode(KEY_LEFTMETA), KeyCode::Meta);
        assert_eq!(to_keycode(KEY_RIGHTMETA), KeyCode::Meta);
    }

    #[test]
    fn digits_map_by_face_value_with_zero_after_nine() {
        assert_eq!(to_keycode(KEY_1), KeyCode::Digit(1));
        assert_eq!(to_keycode(KEY_9), KeyCode::Digit(9));
        assert_eq!(to_keycode(KEY_0), KeyCode::Digit(0));
    }

    #[test]
    fn function_keys_cover_f1_to_f12_including_the_split_f11_f12() {
        assert_eq!(to_keycode(KEY_F1), KeyCode::Function(1));
        assert_eq!(to_keycode(KEY_F10), KeyCode::Function(10));
        assert_eq!(to_keycode(KEY_F11), KeyCode::Function(11));
        assert_eq!(to_keycode(KEY_F12), KeyCode::Function(12));
    }

    /// Every letter key maps to its US-QWERTY face, and to a distinct one.
    #[test]
    fn all_twenty_six_letters_map_to_distinct_uppercase_faces() {
        let mut seen = Vec::new();
        for (start, row) in [(KEY_Q, TOP_ROW), (KEY_A, HOME_ROW), (KEY_Z, BOTTOM_ROW)] {
            for (i, &b) in row.iter().enumerate() {
                let key = to_keycode(start + i as u16);
                assert_eq!(key, KeyCode::Letter(b as char), "code {}", start + i as u16);
                seen.push(b);
            }
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 26, "expected 26 distinct letters");
    }

    /// The keys bracketing each letter row are *not* letters — proves the row
    /// windows don't run off their ends (`KEY_ENTER`/`KEY_LEFTCTRL` after the top
    /// row, `KEY_LEFTSHIFT`/`KEY_BACKSLASH` after the home row).
    #[test]
    fn keys_adjacent_to_the_letter_rows_are_not_letters() {
        for code in [15u16, 26, 27, 28, 39, 40, 41, 51, 52, 53] {
            assert!(
                !matches!(to_keycode(code), KeyCode::Letter(_)),
                "code {code} should not be a letter"
            );
        }
    }

    #[test]
    fn mouse_buttons_map_to_named_variants() {
        assert_eq!(to_keycode(BTN_LEFT), KeyCode::Mouse(MouseButton::Left));
        assert_eq!(to_keycode(BTN_RIGHT), KeyCode::Mouse(MouseButton::Right));
        assert_eq!(to_keycode(BTN_MIDDLE), KeyCode::Mouse(MouseButton::Middle));
        assert_eq!(to_keycode(BTN_SIDE), KeyCode::Mouse(MouseButton::X1));
        assert_eq!(to_keycode(BTN_EXTRA), KeyCode::Mouse(MouseButton::X2));
    }

    /// An unnamed key still gets a stable, collision-free identity: `Other` carries
    /// the raw code, so two different keys never share one debounce slot.
    #[test]
    fn unnamed_keys_keep_a_distinct_opaque_identity() {
        assert_eq!(to_keycode(1), KeyCode::Other(1)); // KEY_ESC
        assert_eq!(to_keycode(57), KeyCode::Other(57)); // KEY_SPACE
        assert_ne!(to_keycode(1), to_keycode(57));
    }

    #[test]
    fn the_whole_mouse_button_block_is_classified_as_mouse() {
        for code in BTN_LEFT..=BTN_TASK {
            assert_eq!(device_class(code), Device::Mouse, "code {code:#x}");
        }
    }

    #[test]
    fn keyboard_keys_are_classified_as_keyboard() {
        for code in [KEY_A, KEY_F12, KEY_LEFTSHIFT, 1, 57] {
            assert_eq!(device_class(code), Device::Keyboard, "code {code}");
        }
        // Just past the mouse block (BTN_JOYSTICK) is not a mouse button.
        assert_eq!(device_class(BTN_TASK + 1), Device::Keyboard);
    }
}
