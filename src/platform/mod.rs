//! The imperative shell: OS-specific input hooks behind one trait.
//!
//! Per ADR-0001 a backend `run`s the OS message loop on the calling (hook) thread,
//! owning the `Engine` and talking to the UI only over channels. The blocking
//! method shape is deliberate — the verdict is computed synchronously inside the
//! hook callback so the input path is never stalled.

use crate::core::Engine;
use crate::messages::{Command, Report};
use std::sync::mpsc::{Receiver, Sender};

/// Errors a backend can surface to the supervisor. A real error type replaces this
/// placeholder when the Windows backend lands (#7).
pub type BackendError = String;

/// An OS input-hook backend. One implementation per platform; only `windows` for v1.
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
pub mod windows;
