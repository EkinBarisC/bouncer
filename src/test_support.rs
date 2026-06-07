//! Test-only harness behind the `integration-test` feature (Windows only).
//!
//! Keeps the `SendInput`-driven end-to-end test FFI-free: the test describes a
//! script of synthetic key events and asserts on what reached downstream, while
//! all the Win32 plumbing lives here. Never compiled into a production build.
//!
//! ## How "downstream" is observed
//! Two `WH_KEYBOARD_LL` hooks are installed on one thread: an **observer** first,
//! then Bouncer's hook. Low-level hooks fire most-recently-installed first, so
//! Bouncer decides first — a suppressed event stops the chain and the observer
//! never sees it; a passed event reaches the observer, which records it. The
//! observer then **swallows the synthetic (injected) events** so they can't leak
//! into a real foreground app, while letting any real user input pass untouched.

use crate::core::{Engine, Thresholds};
use crate::platform::windows::{clear_hook_state, install_keyboard_hook, HookState};
use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
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

thread_local! {
    static OBSERVER: RefCell<Option<Sender<ObservedKey>>> = const { RefCell::new(None) };
}

/// Downstream observer hook: records the synthetic events Bouncer let through and
/// swallows them; passes real user input on.
unsafe extern "system" fn observer_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // SAFETY: `WH_KEYBOARD_LL` lparam is a `KBDLLHOOKSTRUCT`.
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key = down || matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
    let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;

    if injected && is_key {
        OBSERVER.with(|cell| {
            if let Some(sink) = cell.borrow().as_ref() {
                let _ = sink.send(ObservedKey {
                    vk: info.vkCode as u16,
                    down,
                });
            }
        });
        return LRESULT(1); // swallow our synthetic input — never reaches a real app
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Serializes runs: each installs *global* keyboard hooks and injects into the
/// shared system input queue, so two concurrent runs would cross-contaminate.
/// Held for the whole run regardless of how many test threads the runner uses.
static E2E_LOCK: Mutex<()> = Mutex::new(());

/// Run a live Bouncer keyboard hook in integration-test mode, replay `script` via
/// `SendInput`, and return the key events that reached downstream, in order.
pub fn run_keyboard_e2e(thresholds: Thresholds, script: &[SynthKey]) -> Vec<ObservedKey> {
    // Recover from a poisoned lock (a prior run panicking) — the guard only
    // protects ordering, not shared data.
    let _guard = E2E_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let (obs_tx, obs_rx) = mpsc::channel::<ObservedKey>();
    let (ready_tx, ready_rx) = mpsc::channel::<u32>();
    let script = script.to_vec();

    let hook_thread = thread::spawn(move || {
        let tid = unsafe { GetCurrentThreadId() };

        // Observer installed first → called *after* Bouncer in the chain.
        OBSERVER.with(|cell| *cell.borrow_mut() = Some(obs_tx));
        let hmod = unsafe { GetModuleHandleW(None) }.expect("module handle");
        let observer = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(observer_proc),
                Some(HINSTANCE(hmod.0)),
                0,
            )
        }
        .expect("install observer hook");

        // Bouncer installed second → called first.
        let mut engine = Engine::new();
        engine.set_thresholds(thresholds);
        let (rep_tx, _rep_rx) = mpsc::channel();
        let bouncer = install_keyboard_hook(HookState {
            engine,
            reports: rep_tx,
            process_injected: true,
        })
        .expect("install bouncer hook");

        ready_tx.send(tid).expect("signal ready");

        // Pump until the orchestrator posts WM_QUIT.
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

        unsafe {
            let _ = UnhookWindowsHookEx(bouncer);
            let _ = UnhookWindowsHookEx(observer);
        }
        clear_hook_state();
        OBSERVER.with(|cell| *cell.borrow_mut() = None);
    });

    let tid = ready_rx.recv().expect("hook thread ready");

    // Replay the script with the scripted inter-event gaps.
    for key in &script {
        if key.gap_ms > 0 {
            thread::sleep(Duration::from_millis(key.gap_ms));
        }
        send_key(key.vk, key.down);
    }

    // Let the input queue drain so all hook callbacks finish before teardown.
    thread::sleep(Duration::from_millis(80));
    unsafe {
        let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
    }
    let _ = hook_thread.join();

    obs_rx.try_iter().collect()
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
