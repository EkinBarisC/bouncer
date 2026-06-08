//! Detects the panic hotkey chord against the live set of held keys.
//!
//! Pure: fed every input event, it tracks which keys are currently held and emits
//! an **edge-triggered** toggle when the configured chord becomes fully held. It
//! also reports whether the event should be **consumed** (not leaked to the
//! foreground app). See DESIGN.md §6 and CONTEXT.md ("panic hotkey").

use std::collections::{BTreeSet, HashSet};

use crate::core::event::{InputEvent, KeyId};

/// True for Ctrl/Alt/Shift/Win virtual-key codes.
///
/// `0x10..=0x12` are generic Shift/Control/Alt; `0xA0..=0xA5` their left/right
/// variants; `0x5B`/`0x5C` are left/right Win.
fn is_modifier(key: KeyId) -> bool {
    matches!(key, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

/// Why a proposed chord is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordError {
    /// No modifier key (Ctrl/Alt/Shift/Win) in the chord.
    NoModifier,
    /// No non-modifier key in the chord.
    NoNonModifierKey,
}

/// A validated panic chord: the set of keys that must be held simultaneously.
/// Construction enforces "at least one modifier + at least one non-modifier key"
/// so a chord can't be something triggerable by normal use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicChord {
    keys: BTreeSet<KeyId>,
}

impl PanicChord {
    /// Validate and build a chord from its keys. Requires at least one modifier
    /// and at least one non-modifier key.
    pub fn new(keys: &[KeyId]) -> Result<Self, ChordError> {
        let keys: BTreeSet<KeyId> = keys.iter().copied().collect();
        if !keys.iter().any(|&k| is_modifier(k)) {
            return Err(ChordError::NoModifier);
        }
        if !keys.iter().any(|&k| !is_modifier(k)) {
            return Err(ChordError::NoNonModifierKey);
        }
        Ok(Self { keys })
    }

    /// The default chord: Ctrl+Alt+Shift+F12 (universal keys; no Pause/ScrollLock
    /// that some keyboards lack). L/R modifier vk resolution is handled at wiring (#9).
    pub fn default_chord() -> Self {
        // 0x11 Ctrl, 0x12 Alt, 0x10 Shift, 0x7B F12.
        Self::new(&[0x11, 0x12, 0x10, 0x7B]).expect("default chord is valid")
    }

    /// The chord's keys, ascending — for display and round-tripping through config.
    pub fn keys(&self) -> Vec<KeyId> {
        self.keys.iter().copied().collect()
    }
}

/// What the detector reports for one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanicReaction {
    /// The chord just transitioned into "fully held" on this event (rising edge).
    pub toggled: bool,
    /// This event should be consumed (suppressed) rather than passed to apps.
    pub consume: bool,
}

/// Tracks held keys and emits an edge-triggered toggle when the chord completes.
#[derive(Debug)]
pub struct PanicDetector {
    chord: PanicChord,
    held: HashSet<KeyId>,
    /// Whether the chord was fully held after the previous event (edge state).
    was_full: bool,
}

impl PanicDetector {
    pub fn new(chord: PanicChord) -> Self {
        Self {
            chord,
            held: HashSet::new(),
            was_full: false,
        }
    }

    /// Swap in a new chord (a live rebind). Resets the held-key tracking and edge
    /// state so the change can't fire a spurious toggle.
    pub fn set_chord(&mut self, chord: PanicChord) {
        self.chord = chord;
        self.held.clear();
        self.was_full = false;
    }

    /// Feed one event; update the held-key set and report toggle/consume.
    pub fn on_event(&mut self, event: InputEvent) -> PanicReaction {
        match event.kind {
            crate::core::event::EventKind::Down => {
                self.held.insert(event.key);
            }
            crate::core::event::EventKind::Up => {
                self.held.remove(&event.key);
            }
        }

        let full = self.chord.keys.iter().all(|k| self.held.contains(k));
        let toggled = full && !self.was_full; // rising edge only
        self.was_full = full;

        PanicReaction {
            toggled,
            consume: full,
        }
    }
}

impl Default for PanicDetector {
    fn default() -> Self {
        Self::new(PanicChord::default_chord())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_util::{down, up, A, ALT, CTRL, F12, SHIFT};

    fn detector() -> PanicDetector {
        PanicDetector::new(PanicChord::new(&[CTRL, ALT, SHIFT, F12]).unwrap())
    }

    // --- chord validation ---

    #[test]
    fn chord_with_modifier_and_key_is_valid() {
        assert!(PanicChord::new(&[CTRL, ALT, SHIFT, F12]).is_ok());
        assert!(PanicChord::new(&[CTRL, A]).is_ok()); // minimal valid: 1 modifier + 1 key
    }

    #[test]
    fn chord_without_modifier_is_rejected() {
        assert_eq!(PanicChord::new(&[F12]), Err(ChordError::NoModifier));
        assert_eq!(PanicChord::new(&[A]), Err(ChordError::NoModifier));
    }

    #[test]
    fn chord_without_nonmodifier_key_is_rejected() {
        assert_eq!(PanicChord::new(&[CTRL]), Err(ChordError::NoNonModifierKey));
        assert_eq!(
            PanicChord::new(&[CTRL, SHIFT]),
            Err(ChordError::NoNonModifierKey)
        );
    }

    // --- detection ---

    #[test]
    fn full_chord_held_toggles_once() {
        let mut d = detector();
        assert!(!d.on_event(down(CTRL, 0)).toggled);
        assert!(!d.on_event(down(ALT, 1)).toggled);
        assert!(!d.on_event(down(SHIFT, 2)).toggled);
        assert!(d.on_event(down(F12, 3)).toggled); // chord completes
    }

    #[test]
    fn held_chord_does_not_retoggle() {
        let mut d = detector();
        d.on_event(down(CTRL, 0));
        d.on_event(down(ALT, 1));
        d.on_event(down(SHIFT, 2));
        assert!(d.on_event(down(F12, 3)).toggled);
        // F12 auto-repeat while the chord stays held — no new toggle.
        assert!(!d.on_event(down(F12, 4)).toggled);
        assert!(!d.on_event(down(F12, 5)).toggled);
    }

    #[test]
    fn release_and_repress_toggles_again() {
        let mut d = detector();
        d.on_event(down(CTRL, 0));
        d.on_event(down(ALT, 1));
        d.on_event(down(SHIFT, 2));
        assert!(d.on_event(down(F12, 3)).toggled);
        assert!(!d.on_event(up(F12, 4)).toggled); // release breaks the chord
        assert!(d.on_event(down(F12, 5)).toggled); // re-press completes again
    }

    #[test]
    fn partial_chord_never_toggles() {
        let mut d = detector();
        assert!(!d.on_event(down(CTRL, 0)).toggled);
        assert!(!d.on_event(down(SHIFT, 1)).toggled);
        assert!(!d.on_event(down(F12, 2)).toggled); // ALT missing → never full
    }

    #[test]
    fn completing_event_is_consumed_modifiers_are_not() {
        let mut d = detector();
        assert!(!d.on_event(down(CTRL, 0)).consume); // partial → passes through
        assert!(!d.on_event(down(ALT, 1)).consume);
        assert!(!d.on_event(down(SHIFT, 2)).consume);
        assert!(d.on_event(down(F12, 3)).consume); // completing key is consumed
    }
}
