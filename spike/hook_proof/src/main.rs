//! THROWAWAY SPIKE — issue #1.
//!
//! Goal: prove on real Windows 11 hardware that we can (a) install a
//! `WH_KEYBOARD_LL` hook with no admin and no driver, and (b) actually *suppress*
//! an event by returning 1 from the callback. It suppresses every **second**
//! physical press of the target key (default `A`) and passes everything else.
//!
//! This is NOT production code and must not be carried into the bouncer crate.
//! Delete after the findings are recorded on issue #1.
//!
//! Run:  cargo run         (from spike/hook_proof)
//! Quit: press Esc, or Ctrl+C the console.
//!
//! What to look for:
//! - Open Notepad (or any text field), give it focus, and tap `A` repeatedly.
//! - Presses 1,3,5… should type `a`; presses 2,4,6… should produce NOTHING
//!   (suppressed) while the console logs "SUPPRESSED press #N".
//! - That missing character is the proof: returning 1 truly swallows the event.

use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostQuitMessage, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Target virtual-key code. 0x41 = 'A'.
const TARGET_VK: u32 = 0x41;
/// 0x1B = Esc — used here only to quit the spike cleanly.
const VK_ESCAPE: u32 = 0x1B;

/// Number of *distinct* physical presses of the target key seen so far.
static PRESS_COUNT: AtomicU64 = AtomicU64::new(0);
/// Whether the target key is currently held (to ignore auto-repeat downs, so one
/// physical press counts once).
static TARGET_DOWN: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn keyboard_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Per the docs: if ncode < HC_ACTION we must not process, just pass it on.
    if ncode == HC_ACTION as i32 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode;
        let msg = wparam as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

        if vk == VK_ESCAPE && is_down {
            println!("Esc pressed — exiting spike.");
            PostQuitMessage(0);
        }

        if vk == TARGET_VK {
            if is_up {
                TARGET_DOWN.store(false, Ordering::Relaxed);
            } else if is_down && !TARGET_DOWN.swap(true, Ordering::Relaxed) {
                // First down of a fresh physical press.
                let n = PRESS_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 2 == 0 {
                    println!("SUPPRESSED press #{n} of target key (returned 1)");
                    return 1; // <-- the whole point: swallow the event.
                } else {
                    println!("passed press #{n} of target key");
                }
            }
        }
    }
    CallNextHookEx(0, ncode, wparam, lparam)
}

fn main() {
    unsafe {
        let hmod = GetModuleHandleW(ptr::null());
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), hmod, 0);
        if hook == 0 {
            eprintln!("FAILED to install WH_KEYBOARD_LL hook.");
            return;
        }

        println!("WH_KEYBOARD_LL installed (no admin, no driver).");
        println!("Focus a text field and tap 'A' repeatedly:");
        println!("  odd presses type 'a', even presses are SUPPRESSED.");
        println!("Press Esc to quit.\n");

        let mut msg: MSG = std::mem::zeroed();
        // Standard message pump; the LL hook is serviced on this thread.
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnhookWindowsHookEx(hook);
        println!("Hook uninstalled. Bye.");
    }
}
