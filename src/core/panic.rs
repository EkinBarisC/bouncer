//! Detects the panic hotkey chord against the live set of held keys.
//!
//! Pure: fed every input event, it tracks which keys are currently held and emits
//! an **edge-triggered** toggle when the configured chord becomes fully held. It
//! also reports whether the event should be **consumed** (not leaked to the
//! foreground app). See DESIGN.md §6 and CONTEXT.md ("panic hotkey").

use crate::core::event::InputEvent;

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
    // Real field (the key set) is introduced during the GREEN step.
}

impl PanicChord {
    /// Validate and build a chord from its keys.
    pub fn new(keys: &[crate::core::event::KeyId]) -> Result<Self, ChordError> {
        let _ = keys;
        unimplemented!("PanicChord::new — implemented after test review (#4)")
    }

    /// The default chord: Ctrl+Alt+Shift+F12 (universal keys; no Pause/ScrollLock
    /// that some keyboards lack). L/R modifier vk resolution is handled at wiring (#9).
    pub fn default_chord() -> Self {
        unimplemented!("PanicChord::default_chord — implemented after test review (#4)")
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
    // Implementation (held-key set, configured chord, edge state) is introduced
    // during the GREEN step.
}

impl PanicDetector {
    pub fn new(chord: PanicChord) -> Self {
        let _ = chord;
        unimplemented!("PanicDetector::new — implemented after test review (#4)")
    }

    /// Feed one event; update the held-key set and report toggle/consume.
    pub fn on_event(&mut self, event: InputEvent) -> PanicReaction {
        let _ = event;
        unimplemented!("PanicDetector::on_event — implemented after test review (#4)")
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
    use crate::core::event::{Device, EventKind, InputEvent, KeyId};

    const CTRL: KeyId = 0x11;
    const ALT: KeyId = 0x12;
    const SHIFT: KeyId = 0x10;
    const F12: KeyId = 0x7B;
    const A: KeyId = 0x41;

    fn down(key: KeyId, t: u64) -> InputEvent {
        InputEvent {
            device: Device::Keyboard,
            key,
            kind: EventKind::Down,
            timestamp_ms: t,
            injected: false,
        }
    }
    fn up(key: KeyId, t: u64) -> InputEvent {
        InputEvent {
            device: Device::Keyboard,
            key,
            kind: EventKind::Up,
            timestamp_ms: t,
            injected: false,
        }
    }

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
