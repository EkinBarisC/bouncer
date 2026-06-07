//! End-to-end mouse double-click-bug suppression: a real `WH_MOUSE_LL` hook
//! decides on input synthesized with `SendInput`, asserting what reached
//! downstream.
//!
//! Windows-only and gated behind `--features integration-test` (installs a global
//! hook and synthesizes real clicks). Run with: `cargo test --features integration-test`.
#![cfg(all(windows, feature = "integration-test"))]

use bouncer::core::Thresholds;
use bouncer::test_support::{run_mouse_e2e, MouseClick, ObservedClick};

const LMB: u16 = 0x01; // left mouse button

/// Defaults: keyboard 30 ms, mouse 40 ms.
const THR: Thresholds = Thresholds {
    keyboard_ms: 30,
    mouse_ms: 40,
};

fn down(button: u16, gap_ms: u64) -> MouseClick {
    MouseClick {
        button,
        down: true,
        gap_ms,
    }
}
fn up(button: u16, gap_ms: u64) -> MouseClick {
    MouseClick {
        button,
        down: false,
        gap_ms,
    }
}

fn count_downs(observed: &[ObservedClick], button: u16) -> usize {
    observed
        .iter()
        .filter(|c| **c == ObservedClick { button, down: true })
        .count()
}

// The failing-mouse double-click bug: a 2nd click 8 ms after the 1st release is
// chatter (below the 40 ms mouse threshold) → the 2nd click is suppressed.
#[test]
fn double_click_chatter_is_suppressed() {
    let observed = run_mouse_e2e(THR, &[down(LMB, 0), up(LMB, 0), down(LMB, 8), up(LMB, 0)]);
    assert_eq!(
        count_downs(&observed, LMB),
        1,
        "the 8 ms re-click should be suppressed; observed={observed:?}"
    );
}

// A deliberate double-click 250 ms apart → both clicks pass.
#[test]
fn well_spaced_double_click_both_pass() {
    let observed = run_mouse_e2e(THR, &[down(LMB, 0), up(LMB, 0), down(LMB, 250), up(LMB, 0)]);
    assert_eq!(
        count_downs(&observed, LMB),
        2,
        "both deliberate clicks should pass; observed={observed:?}"
    );
}
