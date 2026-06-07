//! Windows backend: `WH_KEYBOARD_LL` feeding the pure `Engine`, with channel
//! control woken via `PostThreadMessageW` (per ADR-0001). Mouse (`WH_MOUSE_LL`)
//! and the eviction watchdog land in later slices (#8, and the supervisor wiring).
//!
//! The hot path is the hook callback: it borrows thread-local state, asks the
//! Engine for a verdict, and returns synchronously — no allocation, no lock.

use crate::core::Engine;
use crate::core::{Device, EventKind, InputEvent, Thresholds, Verdict};
use crate::messages::{Command, Report};
use crate::platform::{BackendError, HookBackend};
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Per-hook-thread decision state, reached by the C callback through a
/// `thread_local`. Owns the Engine so the hot path never crosses a lock.
pub(crate) struct HookState {
    pub engine: Engine,
    pub reports: Sender<Report>,
    /// Production passes `LLKHF_INJECTED` events straight through; the
    /// integration-test build sets this so `SendInput` can drive the engine.
    pub process_injected: bool,
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

/// The `WH_KEYBOARD_LL` callback. Synchronous and allocation-free on the common
/// path: returns `LRESULT(1)` to suppress, or chains on to pass.
unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        // Not an action we may process; the docs require chaining on.
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    // SAFETY: for `WH_KEYBOARD_LL`, `lparam` is a pointer to a `KBDLLHOOKSTRUCT`.
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let kind = match wparam.0 as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(EventKind::Down),
        WM_KEYUP | WM_SYSKEYUP => Some(EventKind::Up),
        _ => None,
    };

    let verdict = HOOK_STATE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return Verdict::Pass;
        };
        let Some(kind) = kind else {
            return Verdict::Pass;
        };

        let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
        if injected && !state.process_injected {
            return Verdict::Pass; // chatter is physical — never an injected event
        }

        let event = InputEvent {
            device: Device::Keyboard,
            key: info.vkCode,
            kind,
            timestamp_ms: info.time as u64,
            injected,
        };
        let outcome = state.engine.on_event(event);
        if let Some(mode) = outcome.mode_change {
            // Off the hot path (only on a panic-chord toggle); best-effort.
            let _ = state.reports.send(Report::ModeChanged(mode));
        }
        outcome.verdict
    });

    match verdict {
        Verdict::Suppress => LRESULT(1),
        Verdict::Pass => unsafe { CallNextHookEx(None, code, wparam, lparam) },
    }
}

/// Install `WH_KEYBOARD_LL` on the **current** thread and store its `state`. The
/// returned handle must be unhooked on the same thread (see [`WindowsBackend::run`]
/// / the test harness). Shared by the production backend and the integration test.
pub(crate) fn install_keyboard_hook(state: HookState) -> Result<HHOOK, BackendError> {
    HOOK_STATE.with(|cell| *cell.borrow_mut() = Some(state));
    let hmod = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            Some(HINSTANCE(hmod.0)),
            0,
        )
    }
    .map_err(|e| e.to_string())
}

/// Clear the current thread's hook state (after unhooking).
pub(crate) fn clear_hook_state() {
    HOOK_STATE.with(|cell| *cell.borrow_mut() = None);
}

/// Wake a backend message loop so it drains pending `Command`s. A `Command` sender
/// calls this with the backend thread id right after sending (the supervisor wiring
/// that owns that id lands with the UI slice).
pub fn post_wake(thread_id: u32) -> Result<(), BackendError> {
    unsafe { PostThreadMessageW(thread_id, WM_APP, WPARAM(0), LPARAM(0)) }
        .map_err(|e| e.to_string())
}

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
        engine: Engine,
        commands: Receiver<Command>,
        reports: Sender<Report>,
    ) -> Result<(), BackendError> {
        let hook = install_keyboard_hook(HookState {
            engine,
            reports,
            process_injected: false,
        })?;

        let result = pump_messages(&commands);

        unsafe {
            let _ = UnhookWindowsHookEx(hook);
        }
        clear_hook_state();
        result
    }
}

/// Run the OS message loop until `WM_QUIT` or a `Command::Shutdown`. The loop is
/// woken for commands by `post_wake` (`PostThreadMessageW`); keyboard events are
/// delivered to the hook callback by the OS while we block in `GetMessageW`.
fn pump_messages(commands: &Receiver<Command>) -> Result<(), BackendError> {
    loop {
        let mut msg = MSG::default();
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 == -1 {
            return Err("GetMessageW failed".to_string());
        }
        if got.0 == 0 {
            return Ok(()); // WM_QUIT
        }

        while let Ok(cmd) = commands.try_recv() {
            if apply_command(cmd) {
                return Ok(()); // Shutdown
            }
        }

        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Apply one `Command` to the live Engine. Returns `true` for `Shutdown`.
fn apply_command(cmd: Command) -> bool {
    HOOK_STATE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return matches!(cmd, Command::Shutdown);
        };
        match cmd {
            Command::SetThresholds {
                keyboard_ms,
                mouse_ms,
            } => {
                state.engine.set_thresholds(Thresholds {
                    keyboard_ms,
                    mouse_ms,
                });
                false
            }
            Command::SetMode(mode) => {
                state.engine.set_mode(mode);
                false
            }
            Command::SetDiagnostic(_) => false, // UI-side stats overlay (#11)
            Command::RebindPanic(_) => false,   // typed chord rebind (#9)
            Command::Shutdown => true,
        }
    })
}
