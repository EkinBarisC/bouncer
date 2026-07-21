//! Bouncer binary entry point: enforce single-instance, load config, spawn the
//! hook thread, and run the tray + Settings UI (issues #9 shell + #10).
//!
//! Headless and Windows-only; on other platforms the binary is a stub so the
//! workspace still builds.

#![cfg_attr(windows, windows_subsystem = "windows")] // no console window for the tray app

#[cfg(windows)]
fn main() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;

    use bouncer::config::Config;
    use bouncer::core::Engine;
    use bouncer::platform::single_instance::{self, SingleInstance};
    use bouncer::platform::windows::WindowsBackend;
    use bouncer::platform::HookBackend;
    use bouncer::ui::app;

    // Single instance: a second launch pokes the first to show its window, then exits.
    let Some(_instance) = SingleInstance::acquire() else {
        single_instance::signal_show();
        return;
    };

    let cfg = Config::config_path()
        .map(|p| Config::load_from_path(&p))
        .unwrap_or_default();

    // Reconcile autostart registration with the saved preference.
    let _ = bouncer::platform::autostart::set_autostart(cfg.autostart);

    // Build the engine from config: the Config projections own the threshold/mode
    // mapping, and the panic chord is already typed (parsed once at config load).
    let mut engine = Engine::new();
    engine.set_thresholds(cfg.thresholds());
    engine.set_mode(cfg.initial_mode());
    engine.set_panic_chord(cfg.panic_hotkey.clone());

    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (rep_tx, rep_rx) = mpsc::channel();

    // Hook thread: owns the engine and runs the OS message loop until Shutdown.
    let backend = thread::spawn(move || {
        if let Err(e) = WindowsBackend::new().run(engine, cmd_rx, rep_tx) {
            eprintln!("bouncer: hook backend exited: {e}");
        }
    });

    // Surface the window when a second instance signals us.
    let show_requested = Arc::new(AtomicBool::new(false));
    let show_flag = Arc::clone(&show_requested);
    single_instance::spawn_show_listener(move || show_flag.store(true, Ordering::Relaxed));

    if let Err(e) = app::run(cfg, cmd_tx, rep_rx, show_requested) {
        eprintln!("bouncer: UI exited: {e}");
    }

    // UI returned (Quit) — the backend received Shutdown; wait for it to unhook.
    let _ = backend.join();
}

/// Linux: headless daemon. Same core, same config file, no tray and no Settings
/// window yet — the debouncer runs in the foreground until it is killed, and
/// settings are edited in `config.toml`. The backend owns the main thread; a small
/// reader thread drains `Report`s so the channel can't grow unbounded.
///
/// Killing the process (Ctrl-C, SIGTERM, or a crash) closes the device fds, which
/// releases the kernel grabs — so there is no shutdown handshake to get wrong and
/// no way to leave the machine without a keyboard.
#[cfg(target_os = "linux")]
fn main() {
    use std::sync::mpsc;
    use std::thread;

    use bouncer::config::Config;
    use bouncer::core::Engine;
    use bouncer::messages::Report;
    use bouncer::platform::linux::LinuxBackend;
    use bouncer::platform::HookBackend;

    let cfg = Config::config_path()
        .map(|p| Config::load_from_path(&p))
        .unwrap_or_default();

    let mut engine = Engine::new();
    engine.set_thresholds(cfg.thresholds());
    engine.set_mode(cfg.initial_mode());
    engine.set_panic_chord(cfg.panic_hotkey.clone());

    let (_cmd_tx, cmd_rx) = mpsc::channel();
    let (rep_tx, rep_rx) = mpsc::channel();

    thread::spawn(move || {
        for report in rep_rx {
            // Suppressions are far too frequent to log one-by-one; the mode toggle
            // is the one thing a headless user needs to see confirmed.
            if let Report::ModeChanged(mode) = report {
                eprintln!("bouncer: {mode:?}");
            }
        }
    });

    eprintln!("bouncer: watching for chatter (Ctrl-C to stop)");
    if let Err(e) = LinuxBackend::new().run(engine, cmd_rx, rep_tx) {
        eprintln!("bouncer: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("bouncer: Windows and Linux only; this platform is a build stub.");
}
