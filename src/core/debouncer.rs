//! The per-key, release-anchored chatter decision.
//!
//! Pure: given an event (carrying its own timestamp) and the active thresholds,
//! decide whether it is chatter. State is the previous release time per `KeyId`
//! (plus which keys are mid-suppressed-press, so a suppressed down's paired up is
//! also discarded). See DESIGN.md §5 and CONTEXT.md ("release-anchored").

use std::collections::{HashMap, HashSet};

use crate::config::MAX_THRESHOLD_MS;
use crate::core::event::{Device, EventKind, InputEvent, KeyId};
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
    /// Timestamp of the last release (up) seen for each key. The chatter window is
    /// measured from here (release-anchored).
    last_up: HashMap<KeyId, u64>,
    /// Keys whose down was suppressed and whose paired up must also be discarded,
    /// so no orphan up reaches downstream applications.
    suppressing: HashSet<KeyId>,
}

impl Debouncer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide the verdict for one event. `&mut self` because the decision updates
    /// per-key timing state. Pure otherwise — no clock, no OS (the timestamp is
    /// carried on `event`).
    pub fn decide(&mut self, event: InputEvent, thresholds: Thresholds) -> Verdict {
        match event.kind {
            EventKind::Up => {
                // If this up pairs with a suppressed down, discard it too.
                if self.suppressing.remove(&event.key) {
                    return Verdict::Suppress;
                }
                self.last_up.insert(event.key, event.timestamp_ms);
                Verdict::Pass
            }
            EventKind::Down => {
                let threshold = thresholds.for_device(event.device);
                if let Some(&last_up) = self.last_up.get(&event.key) {
                    let gap = event.timestamp_ms.saturating_sub(last_up);
                    if gap < threshold {
                        // Chatter: re-press faster than humanly possible. Suppress
                        // this down and mark the key so its paired up is dropped.
                        self.suppressing.insert(event.key);
                        return Verdict::Suppress;
                    }
                }
                Verdict::Pass
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_util::{down, mouse_down, mouse_up, up, A, B, LMB, THR};
    use crate::core::verdict::Verdict;

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
        assert_eq!(mouse.decide(mouse_down(LMB, 0), THR), Verdict::Pass);
        assert_eq!(mouse.decide(mouse_up(LMB, 0), THR), Verdict::Pass);
        assert_eq!(mouse.decide(mouse_down(LMB, 35), THR), Verdict::Suppress); // 35 < 40 → chatter
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
