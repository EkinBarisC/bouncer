//! End-to-end keyboard suppression: a real `WH_KEYBOARD_LL` hook decides on input
//! synthesized with `SendInput`, and we assert what actually reached downstream.
//!
//! Windows-only and gated behind `--features integration-test`, because it installs
//! a global hook and synthesizes real keystrokes — not something to run on every
//! `cargo test`. Run with: `cargo test --features integration-test`.
#![cfg(all(windows, feature = "integration-test"))]

use bouncer::core::Thresholds;
use bouncer::test_support::{run_keyboard_e2e, ObservedKey, SynthKey};

const A: u16 = 0x41; // 'A'

/// Defaults: keyboard 30 ms, mouse 40 ms.
const THR: Thresholds = Thresholds {
    keyboard_ms: 30,
    mouse_ms: 40,
};

fn down(vk: u16, gap_ms: u64) -> SynthKey {
    SynthKey {
        vk,
        down: true,
        gap_ms,
    }
}
fn up(vk: u16, gap_ms: u64) -> SynthKey {
    SynthKey {
        vk,
        down: false,
        gap_ms,
    }
}

fn count_downs(observed: &[ObservedKey], vk: u16) -> usize {
    observed
        .iter()
        .filter(|k| **k == ObservedKey { vk, down: true })
        .count()
}

// `down, up, down` where the 2nd down is 5 ms after the up → chatter, suppressed
// end-to-end: only the first A-down should reach downstream.
#[test]
fn chatter_down_is_suppressed_downstream() {
    let observed = run_keyboard_e2e(THR, &[down(A, 0), up(A, 0), down(A, 5)]);
    assert_eq!(
        count_downs(&observed, A),
        1,
        "the 5 ms re-press should be suppressed; observed={observed:?}"
    );
}

// Two deliberate presses 200 ms apart → both pass; two A-downs reach downstream.
#[test]
fn well_spaced_presses_both_pass() {
    let observed = run_keyboard_e2e(THR, &[down(A, 0), up(A, 0), down(A, 200), up(A, 0)]);
    assert_eq!(
        count_downs(&observed, A),
        2,
        "both deliberate presses should pass; observed={observed:?}"
    );
}
