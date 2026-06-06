//! The primary state of the engine. Exactly one `Mode` is active at a time.
//!
//! `Paused` is persisted across reboots; `Panic` is never persisted (Bouncer
//! always boots out of Panic). The orthogonal diagnostic overlay lives elsewhere
//! (it is meaningful only while `Active`). See `CONTEXT.md`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Normal operation: chatter is suppressed.
    #[default]
    Active,
    /// Deliberate user pause: every event passes. Persisted.
    Paused,
    /// Emergency pass-through from the panic hotkey: every event passes. Not persisted.
    Panic,
}
