//! The input event the core reasons about — platform-agnostic.

/// Identifies a key or mouse button. On Windows this is the virtual-key code for
/// keyboard keys and a distinct id per mouse button; the core treats it as an
/// opaque per-key identity (timing state is tracked independently per `KeyId`).
pub type KeyId = u32;

/// Which device class an event came from. Selects which threshold applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Device {
    Keyboard,
    Mouse,
}

/// Press vs release. The debounce rule is *release-anchored*: chatter is measured
/// from a key's previous `Up`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    Down,
    Up,
}

/// A single input event, carrying its own timestamp so the core never reads a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    pub device: Device,
    pub key: KeyId,
    pub kind: EventKind,
    /// Milliseconds from an arbitrary monotonic origin (supplied by the shell).
    pub timestamp_ms: u64,
    /// True if the OS flagged the event as synthetic (`LLKHF_INJECTED`). The
    /// production shell passes injected events straight through; the integration
    /// test build processes them so `SendInput` can drive end-to-end tests.
    pub injected: bool,
}
