//! The input event the core reasons about — platform-agnostic.

/// A physical mouse button, named independently of any OS button numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// A platform-neutral identity for a key or mouse button.
///
/// Each OS backend maps its native key/button codes into this enum, so the core
/// (debounce timing, panic-chord matching, hotkey parse/display) never depends on
/// any OS keycode convention. Named variants exist for the keys a panic chord can
/// use — modifiers, letters, digits, function keys; every other physical key is
/// carried opaquely as [`KeyCode::Other`], since the debouncer only needs a stable
/// per-key identity, not a name.
///
/// Left/right modifier variants fold to a single logical modifier (e.g. both shifts
/// are [`KeyCode::Shift`]) so a chord bound to "Shift" matches either physical key.
///
/// The variant order defines the sort order used when displaying a captured chord;
/// the human-readable rendering itself lives in [`crate::core::hotkey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCode {
    Shift,
    Control,
    Alt,
    /// The Windows / Super / Command key.
    Meta,
    /// A function key `Fn` (`F1`..).
    Function(u8),
    /// A letter key, canonicalised to uppercase ASCII (`'A'..='Z'`).
    Letter(char),
    /// A digit key (`0..=9`).
    Digit(u8),
    /// A mouse button.
    Mouse(MouseButton),
    /// Any other physical key: an opaque, per-backend-stable identity with no human
    /// name. Only ever used for debounce timing, never in a panic chord.
    Other(u32),
}

/// A key or mouse-button identity. Formerly a raw Windows virtual-key `u32`; now the
/// platform-neutral [`KeyCode`]. The `KeyId` name is kept as an alias because the
/// core and shell refer to this identity as a `KeyId` throughout.
pub type KeyId = KeyCode;

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
