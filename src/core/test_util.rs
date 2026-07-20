//! Shared constructors and constants for the core unit tests.
//!
//! Compiled only under `cfg(test)` and `pub(crate)` so every core test module can
//! reuse one set of event builders instead of re-declaring them. Keeps the test
//! modules focused on behavior, not boilerplate.

use crate::core::debouncer::Thresholds;
use crate::core::event::{Device, EventKind, InputEvent, KeyCode, KeyId, MouseButton};

// KeyCodes used across the core tests.
pub(crate) const A: KeyId = KeyCode::Letter('A');
pub(crate) const B: KeyId = KeyCode::Letter('B');
pub(crate) const CTRL: KeyId = KeyCode::Control;
pub(crate) const ALT: KeyId = KeyCode::Alt;
pub(crate) const SHIFT: KeyId = KeyCode::Shift;
pub(crate) const F12: KeyId = KeyCode::Function(12);
pub(crate) const LMB: KeyId = KeyCode::Mouse(MouseButton::Left);

/// Default thresholds: keyboard 30 ms, mouse 40 ms (matches `Config::default`).
pub(crate) const THR: Thresholds = Thresholds {
    keyboard_ms: 30,
    mouse_ms: 40,
};

/// Build an event for any device/kind at time `t` (ms); never injected.
pub(crate) fn ev(device: Device, key: KeyId, kind: EventKind, t: u64) -> InputEvent {
    InputEvent {
        device,
        key,
        kind,
        timestamp_ms: t,
        injected: false,
    }
}

/// Keyboard key-down at `t`.
pub(crate) fn down(key: KeyId, t: u64) -> InputEvent {
    ev(Device::Keyboard, key, EventKind::Down, t)
}

/// Keyboard key-up at `t`.
pub(crate) fn up(key: KeyId, t: u64) -> InputEvent {
    ev(Device::Keyboard, key, EventKind::Up, t)
}

/// Mouse button-down at `t`.
pub(crate) fn mouse_down(key: KeyId, t: u64) -> InputEvent {
    ev(Device::Mouse, key, EventKind::Down, t)
}

/// Mouse button-up at `t`.
pub(crate) fn mouse_up(key: KeyId, t: u64) -> InputEvent {
    ev(Device::Mouse, key, EventKind::Up, t)
}
