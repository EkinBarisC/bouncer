//! The pure decision engine: composes `Debouncer` + `PanicDetector` + `Mode` into
//! one synchronous `on_event` call (per ADR-0001).

use crate::core::debouncer::{Debouncer, Decision, Thresholds};
use crate::core::event::InputEvent;
use crate::core::mode::Mode;
use crate::core::panic::PanicDetector;
use crate::core::verdict::{Outcome, Verdict};

/// Owns all decision state. Lives on the hook thread; called synchronously from
/// the hook callback. Pure — no OS, no clock, no I/O.
#[derive(Debug)]
pub struct Engine {
    mode: Mode,
    thresholds: Thresholds,
    debouncer: Debouncer,
    panic: PanicDetector,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            // Safe fallback thresholds; matches Config defaults (keyboard 30 ms,
            // mouse 40 ms). The shell overrides these from the loaded config.
            thresholds: Thresholds {
                keyboard_ms: 30,
                mouse_ms: 40,
            },
            debouncer: Debouncer::default(),
            panic: PanicDetector::default(),
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the primary `Mode`. The shell calls this on a `SetMode` command —
    /// Pause/Resume, or clearing Panic.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Set the active chatter thresholds. The shell calls this on a `SetThresholds`
    /// command (and once at startup from the loaded `Config`). Thresholds are clamped
    /// here so the cap holds at this entry point regardless of the caller (DESIGN.md §6).
    pub fn set_thresholds(&mut self, thresholds: Thresholds) {
        self.thresholds = thresholds.clamped();
    }

    /// Swap in a new panic chord (a live rebind from the Settings window).
    pub fn set_panic_chord(&mut self, chord: crate::core::panic::PanicChord) {
        self.panic.set_chord(chord);
    }

    /// The single synchronous decision for one input event.
    ///
    /// The `PanicDetector` observes *every* event (it must track held keys to see
    /// the chord) regardless of Mode, and toggles `Panic` on the chord's rising
    /// edge. The chord's own keys are always consumed so the hotkey never leaks to
    /// the foreground app. Otherwise the verdict is Mode-gated: while `Active` it
    /// delegates to the `Debouncer`; while `Paused`/`Panic` every event passes
    /// (the fail-open, always-recoverable invariant).
    pub fn on_event(&mut self, event: InputEvent) -> Outcome {
        let reaction = self.panic.on_event(event);

        let mode_change = if reaction.toggled {
            let next = if self.mode == Mode::Panic {
                Mode::Active
            } else {
                Mode::Panic
            };
            self.mode = next;
            Some(next)
        } else {
            None
        };

        let (verdict, chatter_gap_ms) = if reaction.consume {
            (Verdict::Suppress, None)
        } else {
            match self.mode {
                Mode::Active => match self.debouncer.decide(event, self.thresholds) {
                    Decision::Pass => (Verdict::Pass, None),
                    Decision::SuppressChatter { gap_ms } => (Verdict::Suppress, Some(gap_ms)),
                    Decision::SuppressPairedUp => (Verdict::Suppress, None),
                },
                Mode::Paused | Mode::Panic => (Verdict::Pass, None),
            }
        };

        Outcome {
            verdict,
            mode_change,
            chatter_gap_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{Device, EventKind, InputEvent, KeyCode};
    use crate::core::mode::Mode;
    use crate::core::test_util::{down, up, A, ALT, CTRL, F12, SHIFT};
    use crate::core::verdict::Verdict;
    use proptest::prelude::*;

    /// Press the full default chord (Ctrl+Alt+Shift+F12); return the outcome of
    /// the completing (F12) event, which is where the toggle fires.
    fn press_panic_chord(e: &mut Engine, t0: u64) -> Outcome {
        e.on_event(down(CTRL, t0));
        e.on_event(down(ALT, t0 + 1));
        e.on_event(down(SHIFT, t0 + 2));
        e.on_event(down(F12, t0 + 3))
    }

    // 1. While Active, the engine delegates to the Debouncer — chatter is suppressed.
    #[test]
    fn active_delegates_to_debouncer_suppress() {
        let mut e = Engine::new(); // default Mode is Active
        assert_eq!(e.on_event(down(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(up(A, 0)).verdict, Verdict::Pass);
        // 5 ms after release; default keyboard threshold is 30 ms → chatter.
        assert_eq!(e.on_event(down(A, 5)).verdict, Verdict::Suppress);
    }

    // 2. While Active, legitimately-spaced input passes (delegation, pass side).
    #[test]
    fn active_delegates_to_debouncer_pass() {
        let mut e = Engine::new();
        assert_eq!(e.on_event(down(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(up(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(down(A, 200)).verdict, Verdict::Pass);
    }

    // 3. While Paused, every event passes — even one the Debouncer would call chatter.
    #[test]
    fn paused_passes_what_active_would_suppress() {
        let mut e = Engine::new();
        e.set_mode(Mode::Paused);
        assert_eq!(e.on_event(down(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(up(A, 0)).verdict, Verdict::Pass);
        // Identical 5 ms re-press that test 1 suppressed — here it passes.
        let out = e.on_event(down(A, 5));
        assert_eq!(out.verdict, Verdict::Pass);
        assert_eq!(out.mode_change, None); // ordinary events never change Mode
    }

    // 4. While Panic, every event passes.
    #[test]
    fn panic_passes_what_active_would_suppress() {
        let mut e = Engine::new();
        e.set_mode(Mode::Panic);
        assert_eq!(e.on_event(down(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(up(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(down(A, 5)).verdict, Verdict::Pass);
    }

    // 5. The panic chord while Active toggles into Panic; the completing key is
    //    consumed so the chord never leaks to the foreground app (story #12).
    #[test]
    fn panic_chord_while_active_enters_panic() {
        let mut e = Engine::new();
        let out = press_panic_chord(&mut e, 0);
        assert_eq!(out.mode_change, Some(Mode::Panic));
        assert_eq!(out.verdict, Verdict::Suppress);
    }

    // 6. The panic chord while Panic toggles back to Active.
    #[test]
    fn panic_chord_while_panic_returns_to_active() {
        let mut e = Engine::new();
        e.set_mode(Mode::Panic);
        let out = press_panic_chord(&mut e, 0);
        assert_eq!(out.mode_change, Some(Mode::Active));
    }

    // 7. The toggle actually changes state: after entering Panic via the chord,
    //    later chatter passes (proves mode_change isn't merely cosmetic).
    #[test]
    fn chord_toggle_actually_enters_panic_state() {
        let mut e = Engine::new();
        press_panic_chord(&mut e, 0);
        // Release the chord so subsequent events aren't consumed as chord keys.
        e.on_event(up(F12, 10));
        e.on_event(up(SHIFT, 11));
        e.on_event(up(ALT, 12));
        e.on_event(up(CTRL, 13));
        // Now in Panic: a re-press that Active would suppress instead passes.
        assert_eq!(e.on_event(down(A, 100)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(up(A, 100)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(down(A, 103)).verdict, Verdict::Pass);
    }

    // 8. A rebound chord toggles Panic; the old chord no longer does.
    #[test]
    fn rebound_chord_toggles_panic() {
        use crate::core::panic::PanicChord;
        let mut e = Engine::new();
        e.set_panic_chord(PanicChord::new(&[CTRL, A]).unwrap());

        // The old default chord (Ctrl+Alt+Shift+F12) no longer toggles.
        assert_eq!(press_panic_chord(&mut e, 0).mode_change, None);

        // The new chord (Ctrl+A) does.
        e.on_event(down(CTRL, 10));
        let out = e.on_event(down(A, 11));
        assert_eq!(out.mode_change, Some(Mode::Panic));
        assert_eq!(out.verdict, Verdict::Suppress); // completing key consumed
    }

    // 9. A chatter suppression surfaces its measured gap (the Report::Suppressed
    //    payload, #27); the paired up and a chord consume do not.
    #[test]
    fn chatter_gap_is_surfaced_but_paired_up_and_chord_consume_are_not() {
        let mut e = Engine::new();
        assert_eq!(e.on_event(down(A, 0)).chatter_gap_ms, None);
        assert_eq!(e.on_event(up(A, 0)).chatter_gap_ms, None);
        assert_eq!(e.on_event(down(A, 5)).chatter_gap_ms, Some(5)); // chatter
        assert_eq!(e.on_event(up(A, 6)).chatter_gap_ms, None); // its paired up

        // The panic chord's completing key is suppressed but is not chatter.
        let out = press_panic_chord(&mut e, 1000);
        assert_eq!(out.verdict, Verdict::Suppress);
        assert_eq!(out.chatter_gap_ms, None);
    }

    // 10. set_thresholds clamps at the entry point: an out-of-range live SetThresholds
    //     can't widen the suppression window past the cap (DESIGN.md §6).
    #[test]
    fn set_thresholds_clamps_to_the_cap() {
        let mut e = Engine::new();
        e.set_thresholds(Thresholds {
            keyboard_ms: 250, // would suppress a 150 ms gap if honored…
            mouse_ms: 40,
        });
        // …but clamped to 100 ms, so a 150 ms re-press is legitimate input.
        assert_eq!(e.on_event(down(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(up(A, 0)).verdict, Verdict::Pass);
        assert_eq!(e.on_event(down(A, 150)).verdict, Verdict::Pass);
    }

    // --- Fail-open property tests (the safety invariant) ---

    /// A fully arbitrary `KeyCode`, spanning every variant (so a chord may form).
    fn arb_keycode() -> impl Strategy<Value = KeyCode> {
        prop_oneof![
            Just(KeyCode::Shift),
            Just(KeyCode::Control),
            Just(KeyCode::Alt),
            Just(KeyCode::Meta),
            (1u8..=12).prop_map(KeyCode::Function),
            (0x41u32..=0x5A).prop_map(|n| KeyCode::Letter(n as u8 as char)),
            (0u8..=9).prop_map(KeyCode::Digit),
            any::<u32>().prop_map(KeyCode::Other),
        ]
    }

    /// A fully arbitrary event, including modifier keys (so the chord may form).
    fn arb_any_event() -> impl Strategy<Value = InputEvent> {
        (
            prop_oneof![Just(Device::Keyboard), Just(Device::Mouse)],
            arb_keycode(),
            prop_oneof![Just(EventKind::Down), Just(EventKind::Up)],
            any::<u64>(),
            any::<bool>(),
        )
            .prop_map(|(device, key, kind, timestamp_ms, injected)| InputEvent {
                device,
                key,
                kind,
                timestamp_ms,
                injected,
            })
    }

    /// An arbitrary event whose key is restricted to the letters A–Z — all
    /// non-modifier, so the panic chord can never form. Isolates Mode gating.
    fn arb_nonchord_event() -> impl Strategy<Value = InputEvent> {
        (
            prop_oneof![Just(Device::Keyboard), Just(Device::Mouse)],
            (0x41u32..=0x5A).prop_map(|n| KeyCode::Letter(n as u8 as char)),
            prop_oneof![Just(EventKind::Down), Just(EventKind::Up)],
            any::<u64>(),
            any::<bool>(),
        )
            .prop_map(|(device, key, kind, timestamp_ms, injected)| InputEvent {
                device,
                key,
                kind,
                timestamp_ms,
                injected,
            })
    }

    proptest! {
        // Fail-open, part 1: `on_event` never panics over arbitrary event
        // sequences. The hook FFI boundary is `panic = "abort"`, so a panic here
        // would crash the process and lock the user out — the inviolable invariant.
        #[test]
        fn on_event_never_panics(events in prop::collection::vec(arb_any_event(), 0..256)) {
            let mut e = Engine::new();
            for ev in events {
                let _ = e.on_event(ev);
            }
        }

        // Fail-open, part 2: in a pass-through Mode (Paused or Panic), every event
        // passes — the user can always recover their input, whatever the sequence.
        #[test]
        fn passthrough_modes_never_suppress(
            events in prop::collection::vec(arb_nonchord_event(), 0..256),
            paused in any::<bool>(),
        ) {
            let mut e = Engine::new();
            e.set_mode(if paused { Mode::Paused } else { Mode::Panic });
            for ev in events {
                prop_assert_eq!(e.on_event(ev).verdict, Verdict::Pass);
            }
        }
    }
}
