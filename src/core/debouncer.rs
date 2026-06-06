//! The per-key, release-anchored chatter decision.
//!
//! Pure: given an event (carrying its own timestamp) and the active thresholds,
//! decide whether it is chatter. State is the previous release time per `KeyId`
//! (plus which keys are mid-suppressed-press, so a suppressed down's paired up is
//! also discarded). See DESIGN.md §5 and CONTEXT.md ("release-anchored").

use crate::config::MAX_THRESHOLD_MS;
use crate::core::event::{Device, InputEvent};
use crate::core::verdict::Verdict;

/// The active per-device-class thresholds (milliseconds). A down arriving *less
/// than* the device's threshold after that key's previous up is chatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    pub keyboard_ms: u8,
    pub mouse_ms: u8,
}

impl Thresholds {
    /// The effective threshold for a device, clamped to `MAX_THRESHOLD_MS` so a
    /// bad value can never widen the window into an input black hole.
    fn for_device(self, device: Device) -> u64 {
        let raw = match device {
            Device::Keyboard => self.keyboard_ms,
            Device::Mouse => self.mouse_ms,
        };
        raw.min(MAX_THRESHOLD_MS) as u64
    }
}

/// Decides, per key/button, whether an incoming event is chatter to `Suppress`
/// or legitimate input to `Pass`.
#[derive(Debug, Default)]
pub struct Debouncer {
    // Fields are introduced during the GREEN step (after test review):
    // last release time per key, and the set of keys whose down was suppressed.
}

impl Debouncer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide the verdict for one event. `&mut self` because the decision updates
    /// per-key timing state. Pure otherwise — no clock, no OS (the timestamp is
    /// carried on `event`).
    pub fn decide(&mut self, event: InputEvent, thresholds: Thresholds) -> Verdict {
        // RED: implementation deferred until the tests below are reviewed (#3).
        let _ = (event, thresholds);
        unimplemented!("Debouncer::decide — implemented after test review (#3)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{Device, EventKind, InputEvent, KeyId};
    use crate::core::verdict::Verdict;

    /// Default thresholds used across tests: keyboard 30 ms, mouse 40 ms.
    const THR: Thresholds = Thresholds {
        keyboard_ms: 30,
        mouse_ms: 40,
    };

    const A: KeyId = 0x41; // 'A'
    const B: KeyId = 0x42; // 'B'
    const LMB: KeyId = 0x01; // a mouse button id

    fn ev(device: Device, key: KeyId, kind: EventKind, t: u64) -> InputEvent {
        InputEvent {
            device,
            key,
            kind,
            timestamp_ms: t,
            injected: false,
        }
    }

    fn down(key: KeyId, t: u64) -> InputEvent {
        ev(Device::Keyboard, key, EventKind::Down, t)
    }
    fn up(key: KeyId, t: u64) -> InputEvent {
        ev(Device::Keyboard, key, EventKind::Up, t)
    }

    // 1. A down arriving sooner than the threshold after the same key's up is chatter.
    #[test]
    fn down_sooner_than_threshold_after_up_is_suppressed() {
        let mut d = Debouncer::new();
        assert_eq!(d.decide(down(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(up(A, 0), THR), Verdict::Pass);
        // 5 ms after release — far faster than a human could re-press.
        assert_eq!(d.decide(down(A, 5), THR), Verdict::Suppress);
    }

    // 2. A down well after the threshold is legitimate.
    #[test]
    fn down_well_after_threshold_passes() {
        let mut d = Debouncer::new();
        assert_eq!(d.decide(down(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(up(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(down(A, 200), THR), Verdict::Pass);
    }

    // 3. Boundary: a gap exactly equal to the threshold passes (threshold is exclusive).
    #[test]
    fn gap_exactly_threshold_passes() {
        let mut d = Debouncer::new();
        assert_eq!(d.decide(down(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(up(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(down(A, 30), THR), Verdict::Pass); // gap == 30 ms
    }

    // 4. Held-key auto-repeat (repeated downs with no intervening up) is never chatter.
    #[test]
    fn held_key_autorepeat_always_passes() {
        let mut d = Debouncer::new();
        assert_eq!(d.decide(down(A, 0), THR), Verdict::Pass);
        // Auto-repeat fires downs with no up between them, milliseconds apart.
        assert_eq!(d.decide(down(A, 5), THR), Verdict::Pass);
        assert_eq!(d.decide(down(A, 8), THR), Verdict::Pass);
        assert_eq!(d.decide(down(A, 11), THR), Verdict::Pass);
    }

    // 5. Per-key isolation: one key's recent release never affects a different key.
    #[test]
    fn different_keys_close_together_both_pass() {
        let mut d = Debouncer::new();
        assert_eq!(d.decide(down(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(up(A, 1), THR), Verdict::Pass);
        // B pressed 2 ms after A's release — fine, different physical switch.
        assert_eq!(d.decide(down(B, 3), THR), Verdict::Pass);
    }

    // 6. Suppressing a chatter down also discards its paired up (no orphan up).
    #[test]
    fn suppressed_down_also_suppresses_its_paired_up() {
        let mut d = Debouncer::new();
        assert_eq!(d.decide(down(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(up(A, 0), THR), Verdict::Pass);
        assert_eq!(d.decide(down(A, 5), THR), Verdict::Suppress); // chatter down
        assert_eq!(d.decide(up(A, 6), THR), Verdict::Suppress); // its paired up
    }

    // 7. The first-ever down for a key (no prior release recorded) passes.
    #[test]
    fn first_ever_down_passes() {
        let mut d = Debouncer::new();
        assert_eq!(d.decide(down(A, 12345), THR), Verdict::Pass);
    }

    // 8. Keyboard and mouse use their own independent thresholds.
    #[test]
    fn keyboard_and_mouse_use_independent_thresholds() {
        // A 35 ms gap is above the keyboard threshold (30) but below the mouse one (40).
        let mut kb = Debouncer::new();
        assert_eq!(kb.decide(down(A, 0), THR), Verdict::Pass);
        assert_eq!(kb.decide(up(A, 0), THR), Verdict::Pass);
        assert_eq!(kb.decide(down(A, 35), THR), Verdict::Pass); // 35 >= 30 → legit

        let mut mouse = Debouncer::new();
        let m_down = |t| ev(Device::Mouse, LMB, EventKind::Down, t);
        let m_up = |t| ev(Device::Mouse, LMB, EventKind::Up, t);
        assert_eq!(mouse.decide(m_down(0), THR), Verdict::Pass);
        assert_eq!(mouse.decide(m_up(0), THR), Verdict::Pass);
        assert_eq!(mouse.decide(m_down(35), THR), Verdict::Suppress); // 35 < 40 → chatter
    }

    // 9. A threshold above the cap behaves as the cap (clamped to MAX_THRESHOLD_MS = 100).
    #[test]
    fn threshold_above_cap_behaves_as_cap() {
        let huge = Thresholds {
            keyboard_ms: 200, // honored would suppress a 150 ms gap; clamped (100) must not
            mouse_ms: 40,
        };

        // Discriminating case: 150 ms gap. With the 100 ms cap it is outside the
        // window → passes. (If 200 were honored, this would be suppressed.)
        let mut outside = Debouncer::new();
        assert_eq!(outside.decide(down(A, 0), huge), Verdict::Pass);
        assert_eq!(outside.decide(up(A, 0), huge), Verdict::Pass);
        assert_eq!(outside.decide(down(A, 150), huge), Verdict::Pass);

        // Sanity: a 50 ms gap is still inside the clamped 100 ms window → chatter.
        let mut inside = Debouncer::new();
        assert_eq!(inside.decide(down(A, 0), huge), Verdict::Pass);
        assert_eq!(inside.decide(up(A, 0), huge), Verdict::Pass);
        assert_eq!(inside.decide(down(A, 50), huge), Verdict::Suppress);
    }
}
