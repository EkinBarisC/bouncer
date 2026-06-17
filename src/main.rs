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

#[cfg(not(windows))]
fn main() {
    eprintln!("bouncer: Windows-only; this platform is a build stub.");
}
