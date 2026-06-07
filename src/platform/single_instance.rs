//! Single-instance enforcement + "surface the existing window" (issue #10).
//!
//! A named mutex makes the second launch detectable; a named auto-reset event lets
//! that second launch poke the first instance to show its Settings window before it
//! exits. Both are session-local (`Local\…`). OS glue, verified manually.

use std::thread;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE,
};

const MUTEX_NAME: &str = r"Local\BouncerSingletonMutex";
const SHOW_EVENT_NAME: &str = r"Local\BouncerShowWindowEvent";

/// A null-terminated UTF-16 buffer for a Win32 `PCWSTR` name.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A raw handle moved across the thread boundary into the listener. The OS handle
/// is process-global and thread-safe to wait on, but `HANDLE` isn't `Send`.
struct SendHandle(HANDLE);
// SAFETY: a kernel object handle is valid process-wide and safe to use from another
// thread; we only ever wait on / close it.
unsafe impl Send for SendHandle {}

/// Holds the singleton mutex for the process lifetime; released on drop.
pub struct SingleInstance {
    mutex: Option<HANDLE>,
}

impl SingleInstance {
    /// Try to become the single instance. `Some` if we are the first (or on an
    /// unexpected error — fail-open, let the app run); `None` if another instance
    /// already holds the mutex.
    pub fn acquire() -> Option<SingleInstance> {
        let name = wide(MUTEX_NAME);
        match unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) } {
            Ok(handle) => {
                if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    None
                } else {
                    Some(SingleInstance {
                        mutex: Some(handle),
                    })
                }
            }
            // Couldn't create the mutex at all — don't block the user from running.
            Err(_) => Some(SingleInstance { mutex: None }),
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if let Some(handle) = self.mutex.take() {
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }
}

/// Create the show-window event and spawn a listener that calls `on_show` each time
/// the event is signalled (by a second launch). The first instance calls this once.
pub fn spawn_show_listener<F: Fn() + Send + 'static>(on_show: F) {
    let name = wide(SHOW_EVENT_NAME);
    let event = match unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) } {
        Ok(h) => SendHandle(h),
        Err(_) => return, // no listener; single-instance still enforced by the mutex
    };
    thread::spawn(move || {
        // Capture the whole `SendHandle` (which is `Send`), not just its inner
        // `HANDLE` field — Rust 2021 would otherwise capture the field directly.
        let event = event;
        loop {
            let wait = unsafe { WaitForSingleObject(event.0, INFINITE) };
            if wait == WAIT_OBJECT_0 {
                on_show();
            } else {
                break; // the event went away; stop listening
            }
        }
    });
}

/// Signal the running instance to surface its window. Called by a second launch
/// just before it exits.
pub fn signal_show() {
    let name = wide(SHOW_EVENT_NAME);
    // Opens the existing event (or creates it); either way SetEvent wakes the
    // listener if one is waiting.
    if let Ok(event) = unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) } {
        unsafe {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
}
