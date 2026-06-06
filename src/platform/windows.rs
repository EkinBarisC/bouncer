//! Windows backend: `WH_KEYBOARD_LL` + `WH_MOUSE_LL`, channel control via
//! `PostThreadMessageW`, and the eviction watchdog.
//!
//! Behavior is added in issues #7 (keyboard) and #8 (mouse). This is only the
//! skeleton; the `windows` crate dependency is introduced with #7.

use crate::core::Engine;
use crate::messages::{Command, Report};
use crate::platform::{BackendError, HookBackend};
use std::sync::mpsc::{Receiver, Sender};

/// The Windows low-level-hook backend.
#[derive(Default)]
pub struct WindowsBackend;

impl WindowsBackend {
    pub fn new() -> Self {
        WindowsBackend
    }
}

impl HookBackend for WindowsBackend {
    fn run(
        self,
        _engine: Engine,
        _commands: Receiver<Command>,
        _reports: Sender<Report>,
    ) -> Result<(), BackendError> {
        todo!("install WH_*_LL hooks and run the message loop — issue #7")
    }
}
