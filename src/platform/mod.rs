//! The imperative shell: OS-specific input hooks behind one trait.
//!
//! Per ADR-0001 a backend `run`s the OS message loop on the calling (hook) thread,
//! owning the `Engine` and talking to the UI only over channels. The blocking
//! method shape is deliberate — the verdict is computed synchronously inside the
//! hook callback so the input path is never stalled.

use crate::core::Engine;
use crate::messages::{Command, Report};
use std::sync::mpsc::{Receiver, Sender};

/// Errors a backend can surface to the supervisor. A plain string for now — there is
/// one backend and failures are terminal, so a structured type would buy nothing yet.
pub type BackendError = String;

/// An OS input-hook backend. One implementation per platform: `windows` (low-level
/// hooks) and `linux` (evdev grab + uinput replay).
pub trait HookBackend {
    /// Install hooks and run the OS message loop on the calling thread until a
    /// `Command::Shutdown` is received. Applies `Command`s to `engine` between
    /// events and emits `Report`s back to the UI.
    fn run(
        self,
        engine: Engine,
        commands: Receiver<Command>,
        reports: Sender<Report>,
    ) -> Result<(), BackendError>;
}

#[cfg(windows)]
pub mod autostart;
/// Pure evdev-code -> `KeyCode` translation. OS-free (the codes are just numbers),
/// so it compiles and tests everywhere; only `linux.rs` calls it.
pub mod evdev_keycode;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod single_instance;
/// Pure hook-eviction watchdog policy — OS-free, so it compiles and tests on every
/// platform; the probe + reinstall that drive it live in `windows.rs`.
pub mod watchdog;
#[cfg(windows)]
pub mod windows;
