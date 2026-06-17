//! Test-only harness behind the `integration-test` feature (Windows only).
//!
//! Keeps the `SendInput`-driven end-to-end tests FFI-free: a test describes a
//! script of synthetic events and asserts on what reached downstream, while all
//! the Win32 plumbing lives here. Never compiled into a production build.
//!
//! ## How "downstream" is observed
//! For each device, two low-level hooks are installed on one thread: an
//! **observer** first, then Bouncer's hook. LL hooks fire most-recently-installed
//! first, so Bouncer decides first — a suppressed event stops the chain and the
//! observer never sees it; a passed event reaches the observer, which records it.
//! The observer then **swallows the synthetic (injected) events** so they can't
//! leak into a real foreground app, while letting real user input pass untouched.

use crate::core::{Engine, EventKind, Thresholds};
use crate::platform::windows::{
    clear_hook_state, install_keyboard_hook, install_low_level_hook, install_mouse_hook,
    set_hook_state, HookState,
};
use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN,
    MOUSEEVENTF_XUP, MOUSEINPUT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

/// One scripted synthetic key event to inject (flagged injected; the backend runs
/// in integration-test mode so it processes these rather than passing them through).
#[derive(Debug, Clone, Copy)]
pub struct SynthKey {
    /// Virtual-key code.
    pub vk: u16,
    /// `true` = key-down, `false` = key-up.
    pub down: bool,
    /// Delay before injecting this event, measured from the previous one (ms).
    pub gap_ms: u64,
}

/// A key event seen downstream — i.e. one Bouncer did **not** suppress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedKey {
    pub vk: u16,
    pub down: bool,
}

/// One scripted synthetic mouse-button event to inject. `button` is a mouse vk
/// (1 = left, 2 = right, 4 = middle, 5/6 = X1/X2).
#[derive(Debug, Clone, Copy)]
pub struct MouseClick {
    pub button: u16,
    /// `true` = button-down, `false` = button-up.
    pub down: bool,
    /// Delay before injecting this event, measured from the previous one (ms).
    pub gap_ms: u64,
}

/// A mouse-button event seen downstream — i.e. one Bouncer did **not** suppress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedClick {
    pub button: u16,
    pub down: bool,
}

thread_local! {
    static OBSERVER_KBD: RefCell<Option<Sender<ObservedKey>>> = const { RefCell::new(None) };
    static OBSERVER_MOUSE: RefCell<Option<Sender<ObservedClick>>> = const { RefCell::new(None) };
}

/// Keyboard observer: records the synthetic keys Bouncer let through and swallows
/// them; passes real user input on.
unsafe extern "system" fn observer_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // SAFETY: `WH_KEYBOARD_LL` lparam is a `KBDLLHOOKSTRUCT`.
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key = down || matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
    let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;

    if injected && is_key {
        OBSERVER_KBD.with(|cell| {
            if let Some(sink) = cell.borrow().as_ref() {
                let _ = sink.send(ObservedKey {
                    vk: info.vkCode as u16,
                    down,
                });
            }
        });
        return LRESULT(1); // swallow our synthetic input
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Mouse observer: records the synthetic button events Bouncer let through and
/// swallows them; passes real user input on.
unsafe extern "system" fn observer_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // SAFETY: `WH_MOUSE_LL` lparam is an `MSLLHOOKSTRUCT`.
    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    let injected = info.flags & LLMHF_INJECTED != 0;

    if injected {
        // Reuse the production `WM_*BUTTON*` mapping (one source of truth), adapting
        // its `(EventKind, vk)` to the observer's `(button: u16, down: bool)`.
        if let Some((kind, vk)) =
            crate::platform::windows::mouse_button(wparam.0 as u32, info.mouseData)
        {
            OBSERVER_MOUSE.with(|cell| {
                if let Some(sink) = cell.borrow().as_ref() {
                    let _ = sink.send(ObservedClick {
                        button: vk as u16,
                        down: kind == EventKind::Down,
                    });
                }
            });
            return LRESULT(1); // swallow our synthetic input
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Serializes runs: each installs *global* hooks and injects into the shared
/// system input queue, so two concurrent runs would cross-contaminate. Held for
/// the whole run regardless of how many test threads the runner uses.
static E2E_LOCK: Mutex<()> = Mutex::new(());

/// Run a live Bouncer keyboard hook in integration-test mode, replay `script` via
/// `SendInput`, and return the key events that reached downstream, in order.
pub fn run_keyboard_e2e(thresholds: Thresholds, script: &[SynthKey]) -> Vec<ObservedKey> {
    let script = script.to_vec();
    drive_e2e(
        move |sink| {
            OBSERVER_KBD.with(|cell| *cell.borrow_mut() = Some(sink));
            let observer = install_low_level_hook(WH_KEYBOARD_LL, Some(observer_keyboard_proc))
                .expect("install keyboard observer");
            set_bouncer_state(thresholds);
            let bouncer = install_keyboard_hook().expect("install bouncer keyboard hook");
            vec![observer, bouncer]
        },
        move || {
            for key in &script {
                gap(key.gap_ms);
                send_key(key.vk, key.down);
            }
        },
    )
}

/// Run a live Bouncer mouse hook in integration-test mode, replay `script` via
/// `SendInput`, and return the button events that reached downstream, in order.
pub fn run_mouse_e2e(thresholds: Thresholds, script: &[MouseClick]) -> Vec<ObservedClick> {
    let script = script.to_vec();
    drive_e2e(
        move |sink| {
            OBSERVER_MOUSE.with(|cell| *cell.borrow_mut() = Some(sink));
            let observer = install_low_level_hook(WH_MOUSE_LL, Some(observer_mouse_proc))
                .expect("install mouse observer");
            set_bouncer_state(thresholds);
            let bouncer = install_mouse_hook().expect("install bouncer mouse hook");
            vec![observer, bouncer]
        },
        move || {
            for click in &script {
                gap(click.gap_ms);
                send_mouse(click.button, click.down);
            }
        },
    )
}

/// Shared orchestration: on a dedicated thread, `setup` installs the observer +
/// Bouncer hooks (returning the handles to unhook) and wires the observation
/// `sink`; the caller thread then runs `replay` (the `SendInput` script). Returns
/// everything the observer recorded.
fn drive_e2e<Obs, S>(setup: S, replay: impl FnOnce()) -> Vec<Obs>
where
    Obs: Send + 'static,
    S: FnOnce(Sender<Obs>) -> Vec<HHOOK> + Send + 'static,
{
    // Recover from a poisoned lock (a prior run panicking) — it only orders runs.
    let _guard = E2E_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let (obs_tx, obs_rx) = mpsc::channel::<Obs>();
    let (ready_tx, ready_rx) = mpsc::channel::<u32>();

    let hook_thread = thread::spawn(move || {
        let tid = unsafe { GetCurrentThreadId() };
        let hooks = setup(obs_tx);
        ready_tx.send(tid).expect("signal ready");

        pump_until_quit();

        for hook in hooks {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
        teardown_thread_locals();
    });

    let tid = ready_rx.recv().expect("hook thread ready");
    replay();

    // Let the input queue drain so all hook callbacks finish before teardown.
    thread::sleep(Duration::from_millis(80));
    unsafe {
        let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
    }
    let _ = hook_thread.join();

    obs_rx.try_iter().collect()
}

/// Build an Engine at `thresholds` and store it as the current thread's hook state.
fn set_bouncer_state(thresholds: Thresholds) {
    let mut engine = Engine::new();
    engine.set_thresholds(thresholds);
    let (reports, _rx) = mpsc::channel();
    set_hook_state(HookState {
        engine,
        reports,
        process_injected: true,
    });
}

/// Pump the OS message loop until the orchestrator posts `WM_QUIT`.
fn pump_until_quit() {
    loop {
        let mut msg = MSG::default();
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn teardown_thread_locals() {
    clear_hook_state();
    OBSERVER_KBD.with(|cell| *cell.borrow_mut() = None);
    OBSERVER_MOUSE.with(|cell| *cell.borrow_mut() = None);
}

/// Sleep the scripted inter-event gap (skipped when zero).
fn gap(gap_ms: u64) {
    if gap_ms > 0 {
        thread::sleep(Duration::from_millis(gap_ms));
    }
}

/// Inject one synthetic key event via `SendInput` (flagged injected by the OS).
fn send_key(vk: u16, down: bool) {
    let dw_flags = if down {
        KEYBD_EVENT_FLAGS(0)
    } else {
        KEYEVENTF_KEYUP
    };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: dw_flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Inject one synthetic mouse-button event via `SendInput`.
fn send_mouse(button: u16, down: bool) {
    let (dw_flags, mouse_data) = match (button, down) {
        (1, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (1, false) => (MOUSEEVENTF_LEFTUP, 0),
        (2, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (2, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (4, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (4, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        // X buttons: mouseData carries XBUTTON1 (1) / XBUTTON2 (2).
        (5 | 6, true) => (MOUSEEVENTF_XDOWN, (button - 4) as u32),
        (5 | 6, false) => (MOUSEEVENTF_XUP, (button - 4) as u32),
        _ => return,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: mouse_data,
                dwFlags: dw_flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}
