//! Panic-hotkey rebind capture (issue #10).
//!
//! Pure capture state for the Settings "Rebind" gesture: fed the key events the
//! user produces while the capture field has focus, it tracks the combination
//! currently held and offers it as a candidate [`PanicChord`] — which only
//! validates once it has ≥1 modifier + 1 non-modifier key (DESIGN.md §6). The UI
//! shows [`keys`](RebindCapture::keys) live and enables *Accept* only when
//! [`chord`](RebindCapture::chord) is `Ok`. No OS calls; the shell feeds it events.

use std::collections::{BTreeSet, HashSet};

use crate::core::event::EventKind;
use crate::core::{ChordError, KeyId, PanicChord};

/// Accumulates the keys held during a rebind gesture. The candidate chord is the
/// set held at the moment the most recently-pressed key went down ("whatever you
/// were holding when you pressed the last key"), so releasing the chord leaves the
/// captured combination intact for the user to accept.
#[derive(Debug, Default)]
pub struct RebindCapture {
    held: HashSet<KeyId>,
    captured: BTreeSet<KeyId>,
}

impl RebindCapture {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one key event while capturing. A key down extends the held set and
    /// snapshots it as the candidate; a key up just relaxes the held set.
    pub fn on_event(&mut self, key: KeyId, kind: EventKind) {
        let _ = (key, kind);
        todo!("GREEN")
    }

    /// The captured combination, sorted, for live display. Empty before any input.
    pub fn keys(&self) -> Vec<KeyId> {
        todo!("GREEN")
    }

    /// The candidate chord, validated. `Err` until the captured combination has at
    /// least one modifier and one non-modifier key.
    pub fn chord(&self) -> Result<PanicChord, ChordError> {
        todo!("GREEN")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_util::{A, ALT, CTRL, F12, SHIFT};

    fn down(c: &mut RebindCapture, k: KeyId) {
        c.on_event(k, EventKind::Down);
    }
    fn up(c: &mut RebindCapture, k: KeyId) {
        c.on_event(k, EventKind::Up);
    }

    #[test]
    fn captures_the_full_chord_and_survives_release() {
        let mut c = RebindCapture::new();
        down(&mut c, CTRL);
        down(&mut c, ALT);
        down(&mut c, SHIFT);
        down(&mut c, F12);
        // Releasing the keys leaves the captured combination for the user to accept.
        up(&mut c, F12);
        up(&mut c, SHIFT);
        up(&mut c, ALT);
        up(&mut c, CTRL);

        // keys() is sorted by vk: Shift(0x10), Ctrl(0x11), Alt(0x12), F12(0x7B).
        assert_eq!(c.keys(), vec![SHIFT, CTRL, ALT, F12]);
        assert_eq!(
            c.chord().unwrap(),
            PanicChord::new(&[CTRL, ALT, SHIFT, F12]).unwrap()
        );
    }

    #[test]
    fn rejects_a_chord_with_no_modifier() {
        let mut c = RebindCapture::new();
        down(&mut c, F12);
        assert_eq!(c.chord(), Err(ChordError::NoModifier));
    }

    #[test]
    fn rejects_a_chord_with_no_nonmodifier_key() {
        let mut c = RebindCapture::new();
        down(&mut c, CTRL);
        down(&mut c, SHIFT);
        assert_eq!(c.chord(), Err(ChordError::NoNonModifierKey));
    }

    #[test]
    fn a_fresh_press_replaces_an_earlier_stray_key() {
        let mut c = RebindCapture::new();
        // A stray tap, fully released…
        down(&mut c, A);
        up(&mut c, A);
        // …then the real chord: the candidate reflects the last-pressed combination.
        down(&mut c, CTRL);
        down(&mut c, F12);
        assert_eq!(c.chord().unwrap(), PanicChord::new(&[CTRL, F12]).unwrap());
    }

    #[test]
    fn empty_capture_has_no_keys_and_no_chord() {
        let c = RebindCapture::new();
        assert!(c.keys().is_empty());
        assert!(c.chord().is_err());
    }
}
