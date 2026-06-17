//! Windows backend: `WH_KEYBOARD_LL` + `WH_MOUSE_LL` feeding the pure `Engine`,
//! with channel control woken via `PostThreadMessageW` (per ADR-0001). The
//! eviction watchdog lands with the supervisor wiring.
//!
//! The hot path is each hook callback: it borrows thread-local state, asks the
//! Engine for a verdict, and returns synchronously — no allocation, no lock. Both
//! hooks share one `HookState` (one Engine), so the Debouncer tracks keyboard and
//! mouse timing in the same place.

use crate::core::Engine;
use crate::core::{Device, EventKind, InputEvent, Verdict};
use crate::messages::{Command, Report};
use crate::platform::watchdog::{Health, Watchdog, WatchdogAction};
use crate::platform::{BackendError, HookBackend};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetLastInputInfo, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, LASTINPUTINFO, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PostThreadMessageW, SetTimer,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, HOOKPROC, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WINDOWS_HOOK_ID, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
    WM_MBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
    WM_XBUTTONDOWN, WM_XBUTTONUP,
};

// Mouse-button `KeyId`s — the standard mouse virtual-key codes, so they never
// collide with keyboard vks (which start at 0x08) and the Debouncer can key on them.
const MOUSE_LEFT: u32 = 0x01;
const MOUSE_RIGHT: u32 = 0x02;
const MOUSE_MIDDLE: u32 = 0x04;
const MOUSE_X1: u32 = 0x05;
const MOUSE_X2: u32 = 0x06;

// --- Eviction watchdog (issue #12) ---
//
// Windows evicts a low-level hook whose callback overruns `LowLevelHooksTimeout`,
// silently — the hook just stops firing. We probe liveness by injecting a benign
// sentinel key and checking the keyboard hook still sees it; if it doesn't, we
// reinstall. The probe is *idle-aware* (skipped when the user is already idle) so
// synthetic input never resets the system idle timer and blocks sleep.

/// How often the watchdog probes hook liveness.
const WATCHDOG_INTERVAL_MS: u32 = 5_000;
/// Skip the probe (and its synthetic input) once the user has been idle this long,
/// so the watchdog never keeps an otherwise-idle machine awake.
const WATCHDOG_IDLE_SKIP_MS: u32 = 15_000;
/// The sentinel virtual-key injected by the probe. `0xFF` is reserved/unassigned,
/// so no application reacts to it even if a dead hook lets it reach the foreground.
const WATCHDOG_SENTINEL_VK: u32 = 0xFF;

/// Set by the keyboard hook when it observes the sentinel — i.e. the hook is alive.
/// Read-and-reset by the watchdog tick on the hook thread.
static SENTINEL_SEEN: AtomicBool = AtomicBool::new(false);

/// Per-hook-thread decision state, reached by the C callbacks through a
/// `thread_local`. Owns the Engine so the hot path never crosses a lock.
pub(crate) struct HookState {
    pub engine: Engine,
    pub reports: Sender<Report>,
    /// Production passes injected events straight through; the integration-test
    /// build sets this so `SendInput` can drive the engine.
    pub process_injected: bool,
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

/// The live hook handles, kept so the watchdog can reinstall them in place without
/// disturbing the engine state in `HOOK_STATE`.
struct HookHandles {
    keyboard: HHOOK,
    mouse: HHOOK,
}

thread_local! {
    static HOOK_HANDLES: RefCell<Option<HookHandles>> = const { RefCell::new(None) };
    static WATCHDOG: RefCell<Watchdog> = RefCell::new(Watchdog::new());
    /// Whether a sentinel was injected on the previous active tick (so the next tick
    /// has a result to evaluate). Cleared while idle, to re-prime on return.
    static WATCHDOG_PRIMED: Cell<bool> = const { Cell::new(false) };
}

/// Run one event through the Engine and return its verdict. Shared by both hook
/// callbacks; allocation-free except an off-path `Report` on a mode change.
fn decide(device: Device, key: u32, kind: EventKind, injected: bool, time_ms: u64) -> Verdict {
    HOOK_STATE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return Verdict::Pass;
        };
        if injected && !state.process_injected {
            return Verdict::Pass; // chatter is physical — never an injected event
        }
        let event = InputEvent {
            device,
            key,
            kind,
            timestamp_ms: time_ms,
            injected,
        };
        let outcome = state.engine.on_event(event);
        if let Some(mode) = outcome.mode_change {
            // Off the hot path (only on a panic-chord toggle); best-effort.
            let _ = state.reports.send(Report::ModeChanged(mode));
        }
        if let Some(gap_ms) = outcome.chatter_gap_ms {
            // Off the hot path (only on an actual suppression); best-effort.
            let _ = state.reports.send(Report::Suppressed {
                device,
                key,
                gap_ms,
            });
        }
        outcome.verdict
    })
}

/// The `WH_KEYBOARD_LL` callback. Synchronous: returns `LRESULT(1)` to suppress,
/// or chains on to pass.
unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // SAFETY: for `WH_KEYBOARD_LL`, `lparam` is a pointer to a `KBDLLHOOKSTRUCT`.
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    // The watchdog's liveness sentinel: flag that the hook fired and pass it on
    // (never run it through the Engine).
    if info.vkCode == WATCHDOG_SENTINEL_VK {
        SENTINEL_SEEN.store(true, Ordering::Relaxed);
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let kind = match wparam.0 as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => EventKind::Down,
        WM_KEYUP | WM_SYSKEYUP => EventKind::Up,
        _ => return unsafe { CallNextHookEx(None, code, wparam, lparam) },
    };
    let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
    match decide(
        Device::Keyboard,
        info.vkCode,
        kind,
        injected,
        info.time as u64,
    ) {
        Verdict::Suppress => LRESULT(1),
        Verdict::Pass => unsafe { CallNextHookEx(None, code, wparam, lparam) },
    }
}

/// The `WH_MOUSE_LL` callback. Button events go through the Engine (the
/// double-click bug is per-button release-anchored chatter); moves and wheel pass.
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // SAFETY: for `WH_MOUSE_LL`, `lparam` is a pointer to an `MSLLHOOKSTRUCT`.
    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    let Some((kind, button)) = mouse_button(wparam.0 as u32, info.mouseData) else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };
    let injected = info.flags & LLMHF_INJECTED != 0;
    match decide(Device::Mouse, button, kind, injected, info.time as u64) {
        Verdict::Suppress => LRESULT(1),
        Verdict::Pass => unsafe { CallNextHookEx(None, code, wparam, lparam) },
    }
}

/// Map a mouse message to `(kind, button-key)`; `None` for non-button events
/// (move, wheel) which are never chatter.
fn mouse_button(msg: u32, mouse_data: u32) -> Option<(EventKind, u32)> {
    let pair = match msg {
        WM_LBUTTONDOWN => (EventKind::Down, MOUSE_LEFT),
        WM_LBUTTONUP => (EventKind::Up, MOUSE_LEFT),
        WM_RBUTTONDOWN => (EventKind::Down, MOUSE_RIGHT),
        WM_RBUTTONUP => (EventKind::Up, MOUSE_RIGHT),
        WM_MBUTTONDOWN => (EventKind::Down, MOUSE_MIDDLE),
        WM_MBUTTONUP => (EventKind::Up, MOUSE_MIDDLE),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            // The X button (1 or 2) is in the high word of mouseData.
            let button = match (mouse_data >> 16) as u16 {
                1 => MOUSE_X1,
                2 => MOUSE_X2,
                _ => return None,
            };
            let kind = if msg == WM_XBUTTONDOWN {
                EventKind::Down
            } else {
                EventKind::Up
            };
            (kind, button)
        }
        _ => return None,
    };
    Some(pair)
}

/// Store the decision state for the current thread's hooks. Call before installing.
pub(crate) fn set_hook_state(state: HookState) {
    HOOK_STATE.with(|cell| *cell.borrow_mut() = Some(state));
}

/// Clear the current thread's hook state (after unhooking).
pub(crate) fn clear_hook_state() {
    HOOK_STATE.with(|cell| *cell.borrow_mut() = None);
}

/// Install a low-level hook of `id` with `proc` on the **current** thread. The
/// returned handle must be unhooked on the same thread. Shared by the production
/// backend and the integration-test observer hooks.
pub(crate) fn install_low_level_hook(
    id: WINDOWS_HOOK_ID,
    proc: HOOKPROC,
) -> Result<HHOOK, BackendError> {
    let hmod = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    unsafe { SetWindowsHookExW(id, proc, Some(HINSTANCE(hmod.0)), 0) }.map_err(|e| e.to_string())
}

/// Install the Bouncer keyboard hook (assumes [`set_hook_state`] was called).
pub(crate) fn install_keyboard_hook() -> Result<HHOOK, BackendError> {
    install_low_level_hook(WH_KEYBOARD_LL, Some(keyboard_hook_proc))
}

/// Install the Bouncer mouse hook (assumes [`set_hook_state`] was called).
pub(crate) fn install_mouse_hook() -> Result<HHOOK, BackendError> {
    install_low_level_hook(WH_MOUSE_LL, Some(mouse_hook_proc))
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
        // Announce our thread id so the UI can wake this loop (`post_wake`) after
        // sending a Command — best-effort, off the hot path.
        let thread_id = unsafe { GetCurrentThreadId() };
        let _ = reports.send(Report::BackendReady { thread_id });

        // The watchdog reports eviction/reinstall on its own; keep a sender for it.
        let watch_reports = reports.clone();

        set_hook_state(HookState {
            engine,
            reports,
            process_injected: false,
        });
        let keyboard = install_keyboard_hook()?;
        let mouse = install_mouse_hook()?;
        HOOK_HANDLES.with(|h| *h.borrow_mut() = Some(HookHandles { keyboard, mouse }));

        // A periodic thread timer drives the eviction watchdog (WM_TIMER below).
        let timer = unsafe { SetTimer(None, 0, WATCHDOG_INTERVAL_MS, None) };

        let result = pump_messages(&commands, &watch_reports);

        if timer != 0 {
            unsafe {
                let _ = KillTimer(None, timer);
            }
        }
        // Unhook whatever handles are current (the watchdog may have reinstalled them).
        if let Some(h) = HOOK_HANDLES.with(|h| h.borrow_mut().take()) {
            unsafe {
                let _ = UnhookWindowsHookEx(h.mouse);
                let _ = UnhookWindowsHookEx(h.keyboard);
            }
        }
        clear_hook_state();
        result
    }
}

/// Run the OS message loop until `WM_QUIT` or a `Command::Shutdown`. The loop is
/// woken for commands by `post_wake` (`PostThreadMessageW`); input events are
/// delivered to the hook callbacks by the OS while we block in `GetMessageW`, and a
/// `WM_TIMER` drives the eviction watchdog.
fn pump_messages(
    commands: &Receiver<Command>,
    reports: &Sender<Report>,
) -> Result<(), BackendError> {
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

        if msg.message == WM_TIMER {
            watchdog_tick(reports);
        }

        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// One eviction-watchdog probe. Skipped while the user is idle (so the synthetic
/// sentinel never blocks system sleep); otherwise it evaluates the previous tick's
/// sentinel, reinstalls the hooks if the keyboard hook stopped firing, and injects
/// a fresh sentinel for the next tick.
fn watchdog_tick(reports: &Sender<Report>) {
    if user_idle() {
        WATCHDOG_PRIMED.with(|p| p.set(false)); // re-prime on return, don't false-alarm
        return;
    }

    // Only judge liveness once a sentinel from a prior active tick could be observed.
    if WATCHDOG_PRIMED.with(|p| p.get()) {
        let health = if SENTINEL_SEEN.swap(false, Ordering::Relaxed) {
            Health::Alive
        } else {
            Health::Dead
        };
        let step = WATCHDOG.with(|w| w.borrow_mut().observe(health));
        if let Some(report) = step.report {
            let _ = reports.send(report);
        }
        if step.action == WatchdogAction::Reinstall {
            reinstall_hooks();
        }
    }

    inject_sentinel();
    WATCHDOG_PRIMED.with(|p| p.set(true));
}

/// Reinstall both hooks in place (the engine state in `HOOK_STATE` is untouched, so
/// debounce timing survives). Best-effort: a failed install leaves the dead handle,
/// and the next probe retries.
fn reinstall_hooks() {
    HOOK_HANDLES.with(|h| {
        let mut slot = h.borrow_mut();
        if let Some(handles) = slot.as_mut() {
            unsafe {
                let _ = UnhookWindowsHookEx(handles.keyboard);
                let _ = UnhookWindowsHookEx(handles.mouse);
            }
            if let Ok(kb) = install_keyboard_hook() {
                handles.keyboard = kb;
            }
            if let Ok(ms) = install_mouse_hook() {
                handles.mouse = ms;
            }
        }
    });
}

/// Inject the benign sentinel key (down+up) so a live keyboard hook flags itself.
fn inject_sentinel() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(WATCHDOG_SENTINEL_VK as u16),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Whether the user has been idle longer than [`WATCHDOG_IDLE_SKIP_MS`]. Failing to
/// read idle time falls back to "not idle" (probe anyway — safety over battery).
fn user_idle() -> bool {
    let mut lii = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if unsafe { GetLastInputInfo(&mut lii) }.as_bool() {
        let now = unsafe { GetTickCount() };
        now.wrapping_sub(lii.dwTime) > WATCHDOG_IDLE_SKIP_MS
    } else {
        false
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
            Command::SetThresholds(thresholds) => {
                state.engine.set_thresholds(thresholds);
                false
            }
            Command::SetMode(mode) => {
                state.engine.set_mode(mode);
                false
            }
            Command::SetDiagnostic(_) => false, // UI-side stats overlay (#11)
            Command::RebindPanic(chord) => {
                state.engine.set_panic_chord(chord);
                false
            }
            Command::Shutdown => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const A: u32 = 0x41;

    /// Feed `decide` directly (it's pure given the thread-local state): a chatter
    /// re-press must emit `Report::Suppressed` so the UI counters tick (#27).
    #[test]
    fn chatter_suppression_sends_a_suppressed_report() {
        let (tx, rx) = mpsc::channel();
        set_hook_state(HookState {
            engine: Engine::new(), // default keyboard threshold 30 ms
            reports: tx,
            process_injected: false,
        });

        // Legit press + release, then a 5 ms re-press: chatter, suppressed.
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Down, false, 0),
            Verdict::Pass
        );
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Up, false, 0),
            Verdict::Pass
        );
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Down, false, 5),
            Verdict::Suppress
        );
        // The discarded paired up is also suppressed, but it's the same chatter
        // incident — it must not tick the counter a second time.
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Up, false, 6),
            Verdict::Suppress
        );
        clear_hook_state();

        let reports: Vec<Report> = rx.try_iter().collect();
        match reports.as_slice() {
            [Report::Suppressed {
                device: Device::Keyboard,
                key: A,
                gap_ms: 5,
            }] => {}
            other => panic!("expected exactly one Suppressed{{A, gap 5}} report, got {other:?}"),
        }
    }

    // --- fail-open audit (#12): the wired hot path passes on any doubt ---

    /// With no hook state installed, the callback path must fail open (pass), never
    /// suppress — a half-initialised or torn-down backend can't swallow input.
    #[test]
    fn decide_fails_open_when_no_hook_state() {
        clear_hook_state();
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Down, false, 0),
            Verdict::Pass
        );
        assert_eq!(
            decide(Device::Mouse, A, EventKind::Down, false, 0),
            Verdict::Pass
        );
    }

    /// In production (`process_injected = false`) an injected event always passes —
    /// even one timed as chatter — because chatter is physical (DESIGN.md D12).
    #[test]
    fn injected_events_pass_through_in_production() {
        let (tx, _rx) = mpsc::channel();
        set_hook_state(HookState {
            engine: Engine::new(),
            reports: tx,
            process_injected: false,
        });

        // A chatter-timed sequence, but flagged injected → never suppressed.
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Down, true, 0),
            Verdict::Pass
        );
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Up, true, 0),
            Verdict::Pass
        );
        assert_eq!(
            decide(Device::Keyboard, A, EventKind::Down, true, 5),
            Verdict::Pass
        );
        clear_hook_state();
    }
}
