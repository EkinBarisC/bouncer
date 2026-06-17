//! The running application: tray icon + egui Settings window, wired to the hook
//! thread over the `Command`/`Report` channels (issues #9 shell + #10).
//!
//! This is the imperative shell — OS tray rendering and the egui window are
//! verified manually (DESIGN.md §9). The decision logic it leans on is the tested
//! pure core: [`TrayModel`] (icon/menu/tooltip + click→effect) and
//! [`RebindCapture`] (chord validation).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::config::Config;
use crate::core::event::EventKind;
use crate::core::hotkey;
use crate::core::{KeyId, Mode};
use crate::messages::{Command, Report};
use crate::platform::windows::post_wake;
use crate::stats::{Stats, HISTOGRAM_BUCKETS, HISTOGRAM_BUCKET_MS};
use crate::ui::rebind::RebindCapture;
use crate::ui::settings::SettingsForm;
use crate::ui::tray::{
    icon_rgba, resolve_quit_dialog, IconState, QuitResolution, TrayAction, TrayEffect, TrayModel,
    ICON_SIZE,
};

/// Launch the tray + settings UI on the calling (main) thread. Blocks until quit.
pub fn run(
    cfg: Config,
    cmd_tx: Sender<Command>,
    reports: Receiver<Report>,
    show_requested: Arc<AtomicBool>,
) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 520.0])
            .with_visible(false), // tray-resident: start hidden
        ..Default::default()
    };
    eframe::run_native(
        "Bouncer",
        options,
        Box::new(move |cc| {
            let app = BouncerApp::new(cc, cfg, cmd_tx, reports, show_requested)
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

/// Handles to the live tray menu entries, so their labels/enabled/checked state can
/// be updated as the Mode changes.
struct MenuItems {
    pause_resume: MenuItem,
    diagnostic: CheckMenuItem,
    settings: MenuItem,
    quit: MenuItem,
}

struct BouncerApp {
    /// The Settings-form model: committed (`applied`) vs draft, dirty tracking, and
    /// the commit/cancel/reset rules. See [`SettingsForm`].
    form: SettingsForm,
    cfg_path: Option<PathBuf>,
    cmd_tx: Sender<Command>,
    reports: Receiver<Report>,
    backend_thread_id: Option<u32>,

    mode: Mode,
    diagnostic: bool,
    /// UI-side observability: session counters + diagnostic ring buffer + histogram,
    /// folded from the `Report` stream (never the Engine, per ADR-0001).
    stats: Stats,

    tray: TrayIcon,
    items: MenuItems,
    last_icon: Option<IconState>,

    show_requested: Arc<AtomicBool>,
    quitting: bool,
    confirm_quit_open: bool,
    dont_ask_again: bool,

    rebinding: bool,
    rebind_candidate: RebindCapture,
}

impl BouncerApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        cmd_tx: Sender<Command>,
        reports: Receiver<Report>,
        show_requested: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        // Heartbeat: the window starts hidden, so drive repaints from a side thread
        // to guarantee `logic` keeps running (and polling the tray) while hidden.
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(100));
            ctx.request_repaint();
        });

        let mode = cfg.initial_mode();
        let model = TrayModel {
            mode,
            diagnostic: false,
            keyboard_suppressed: 0,
            mouse_suppressed: 0,
            confirm_on_quit: cfg.confirm_on_quit,
            panic_hotkey: hotkey::display(&cfg.panic_hotkey.keys()),
        };

        let menu = Menu::new();
        let pause_resume = MenuItem::new(model.pause_resume_label(), true, None);
        let diagnostic =
            CheckMenuItem::new("Diagnostic mode", model.diagnostic_enabled(), false, None);
        let settings = MenuItem::new("Settings…", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let sep = || PredefinedMenuItem::separator();
        menu.append(&pause_resume).map_err(|e| e.to_string())?;
        menu.append(&sep()).map_err(|e| e.to_string())?;
        menu.append(&diagnostic).map_err(|e| e.to_string())?;
        menu.append(&settings).map_err(|e| e.to_string())?;
        menu.append(&sep()).map_err(|e| e.to_string())?;
        menu.append(&quit).map_err(|e| e.to_string())?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(model.tooltip())
            .with_icon(make_icon(model.icon())?)
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Self {
            cfg_path: Config::config_path(),
            form: SettingsForm::new(cfg),
            cmd_tx,
            reports,
            backend_thread_id: None,
            mode,
            diagnostic: false,
            stats: Stats::new(),
            last_icon: Some(model.icon()),
            tray,
            items: MenuItems {
                pause_resume,
                diagnostic,
                settings,
                quit,
            },
            show_requested,
            quitting: false,
            confirm_quit_open: false,
            dont_ask_again: false,
            rebinding: false,
            rebind_candidate: RebindCapture::new(),
        })
    }

    fn tray_model(&self) -> TrayModel {
        TrayModel {
            mode: self.mode,
            diagnostic: self.diagnostic,
            keyboard_suppressed: self.stats.keyboard_suppressed(),
            mouse_suppressed: self.stats.mouse_suppressed(),
            confirm_on_quit: self.form.applied().confirm_on_quit,
            panic_hotkey: hotkey::display(&self.form.applied().panic_hotkey.keys()),
        }
    }

    /// Send a Command to the hook thread and wake its message loop to drain it.
    fn send(&self, cmd: Command) {
        let _ = self.cmd_tx.send(cmd);
        if let Some(tid) = self.backend_thread_id {
            let _ = post_wake(tid);
        }
    }

    fn save_config(&self) {
        if let Some(path) = &self.cfg_path {
            let _ = self.form.applied().save_to_path(path);
        }
    }

    /// Commit the draft via the form, then carry out the side effects it reports:
    /// push thresholds, (un)register autostart if it changed, re-arm the panic chord
    /// if it changed, and persist.
    fn save_settings(&mut self) {
        let commit = self.form.commit();
        self.send(Command::SetThresholds(commit.thresholds));
        if let Some(on) = commit.autostart {
            let _ = crate::platform::autostart::set_autostart(on);
        }
        if let Some(chord) = commit.panic_chord {
            self.send(Command::RebindPanic(chord));
        }
        self.save_config();
    }

    /// Discard uncommitted edits.
    fn cancel_settings(&mut self) {
        self.form.cancel();
        self.rebinding = false;
    }

    /// Load factory defaults into the draft (the form preserves the live pause state).
    /// The user still reviews and Saves to apply.
    fn reset_to_defaults(&mut self) {
        self.form.reset_to_defaults();
        self.rebinding = false;
    }

    fn refresh_tray(&mut self) {
        let model = self.tray_model();
        let icon = model.icon();
        if self.last_icon != Some(icon) {
            if let Ok(i) = make_icon(icon) {
                let _ = self.tray.set_icon(Some(i));
            }
            self.last_icon = Some(icon);
        }
        let _ = self.tray.set_tooltip(Some(model.tooltip()));
        self.items.pause_resume.set_text(model.pause_resume_label());
        self.items
            .diagnostic
            .set_enabled(model.diagnostic_enabled());
        self.items
            .diagnostic
            .set_checked(model.diagnostic_checked());
    }

    /// Carry out the effect of a tray/menu interaction.
    fn execute(&mut self, effect: TrayEffect, ctx: &egui::Context) {
        match effect {
            TrayEffect::SetMode(m) => {
                self.mode = m;
                // Paused persists as `enabled = false`; Panic is never persisted.
                // The form keeps applied+draft in step so pausing isn't a form edit.
                if m != Mode::Panic {
                    self.form.set_enabled(m == Mode::Active);
                    self.save_config();
                }
                self.send(Command::SetMode(m));
            }
            TrayEffect::SetDiagnostic(on) => {
                self.diagnostic = on;
                // Drive the ring buffer's recording window (auto-expires after 1 h).
                if on {
                    self.stats.start_recording(Instant::now());
                } else {
                    self.stats.stop_recording();
                }
                self.send(Command::SetDiagnostic(on));
            }
            TrayEffect::OpenSettings => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            TrayEffect::ConfirmQuit => {
                self.confirm_quit_open = true;
                self.dont_ask_again = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            TrayEffect::Quit => self.do_quit(ctx),
            TrayEffect::None => {}
        }
    }

    fn do_quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        self.send(Command::Shutdown);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Drain hook-thread reports into the live UI state.
    fn drain_reports(&mut self) {
        let now = Instant::now();
        while let Ok(report) = self.reports.try_recv() {
            match report {
                Report::BackendReady { thread_id } => self.backend_thread_id = Some(thread_id),
                Report::ModeChanged(m) => self.mode = m,
                Report::HookEvicted | Report::HookReinstalled => {}
                // Counters + diagnostic ring buffer + histogram all live in Stats.
                Report::Suppressed { .. } => self.stats.record(&report, now),
            }
        }
    }

    /// If diagnostic recording reached its 1-hour TTL, switch it off: clear the
    /// buffer (DESIGN.md §7 — cleared on timeout), drop the badge, and tell the
    /// engine. Counters are untouched.
    fn expire_diagnostic(&mut self) {
        if self.diagnostic && !self.stats.is_recording(Instant::now()) {
            self.diagnostic = false;
            self.stats.clear();
            self.send(Command::SetDiagnostic(false));
        }
    }

    /// Drain global tray + menu events and act on them.
    fn drain_tray_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let action = if ev.id == *self.items.pause_resume.id() {
                Some(TrayAction::TogglePause)
            } else if ev.id == *self.items.diagnostic.id() {
                Some(TrayAction::ToggleDiagnostic)
            } else if ev.id == *self.items.settings.id() {
                Some(TrayAction::OpenSettings)
            } else if ev.id == *self.items.quit.id() {
                Some(TrayAction::Quit)
            } else {
                None
            };
            if let Some(a) = action {
                let effect = self.tray_model().apply(a);
                self.execute(effect, ctx);
            }
        }
        // Left-click the icon opens Settings (right-click shows the menu natively).
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                let effect = self.tray_model().apply(TrayAction::OpenSettings);
                self.execute(effect, ctx);
            }
        }
    }

    /// Feed egui key input into the rebind capture while the gesture is active.
    fn capture_rebind(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if let Some(vk) = key_to_vk(*key) {
                        // Rebuild the candidate from the modifiers held + this key.
                        let mut cap = RebindCapture::new();
                        if modifiers.ctrl || modifiers.command {
                            cap.on_event(0x11, EventKind::Down);
                        }
                        if modifiers.alt {
                            cap.on_event(0x12, EventKind::Down);
                        }
                        if modifiers.shift {
                            cap.on_event(0x10, EventKind::Down);
                        }
                        cap.on_event(vk, EventKind::Down);
                        self.rebind_candidate = cap;
                    }
                }
            }
        });
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.heading("Bouncer");
        ui.separator();
        self.draw_status(ui, &ctx);
        ui.separator();
        self.draw_tuning(ui);
        ui.separator();
        self.draw_diagnostics(ui, &ctx);
        ui.separator();
        self.draw_settings_group(ui);
        ui.separator();
        self.draw_actions(ui);

        if self.confirm_quit_open {
            self.draw_quit_dialog(&ctx);
        }
    }

    fn draw_status(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let (color, label) = match self.mode {
                Mode::Active => (egui::Color32::from_rgb(0, 0xA0, 0x82), "Active"),
                Mode::Paused => (egui::Color32::GRAY, "Paused"),
                Mode::Panic => (egui::Color32::from_rgb(0xD2, 0x28, 0x28), "Panic"),
            };
            ui.colored_label(color, "⏺");
            ui.label(format!("Status: {label}"));
        });
        ui.label(format!(
            "Suppressed this session — {} keyboard · {} mouse",
            self.stats.keyboard_suppressed(),
            self.stats.mouse_suppressed()
        ));
        let btn = self.tray_model().pause_resume_label();
        if ui.button(btn).clicked() {
            let effect = self.tray_model().apply(TrayAction::TogglePause);
            self.execute(effect, ctx);
        }
    }

    fn draw_tuning(&mut self, ui: &mut egui::Ui) {
        ui.label("Tuning");
        let draft = self.form.draft_mut();
        ui.checkbox(&mut draft.debounce_keyboard, "Debounce keyboard");
        ui.add(egui::Slider::new(&mut draft.keyboard_threshold_ms, 0..=100).text("ms keyboard"));
        ui.checkbox(&mut draft.debounce_mouse, "Debounce mouse");
        ui.add(egui::Slider::new(&mut draft.mouse_threshold_ms, 0..=100).text("ms mouse"));
    }

    fn draw_diagnostics(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("Diagnostics");
        let enabled = self.tray_model().diagnostic_enabled();
        ui.add_enabled_ui(enabled, |ui| {
            let mut on = self.diagnostic;
            if ui.checkbox(&mut on, "Diagnostic mode").changed() {
                let effect = self.tray_model().apply(TrayAction::ToggleDiagnostic);
                self.execute(effect, ctx);
            }
        });

        // Recording indicator + auto-expiry countdown.
        if let Some(left) = self.stats.time_remaining(Instant::now()) {
            let mins = left.as_secs() / 60;
            let secs = left.as_secs() % 60;
            ui.colored_label(
                egui::Color32::from_rgb(0xD2, 0x28, 0x28),
                format!("● Recording — auto-stops in {mins}:{secs:02}"),
            );
        } else if !enabled {
            ui.weak("Diagnostic mode is available only while Active.");
        }

        // The suppression-gap histogram, with a line marking the keyboard threshold.
        let samples = self.stats.samples().len();
        if samples == 0 {
            ui.weak("No suppressions recorded yet — gaps appear here while recording.");
        } else {
            self.draw_histogram(ui);
            ui.horizontal(|ui| {
                ui.label(format!("{samples} samples"));
                if ui.button("Clear").clicked() {
                    self.stats.clear();
                }
            });
        }
    }

    /// Paint the gap histogram: one bar per [`HISTOGRAM_BUCKETS`] bucket, plus a
    /// vertical line at the current keyboard threshold so the user can see how their
    /// setting sits against the real suppressed-gap distribution.
    fn draw_histogram(&self, ui: &mut egui::Ui) {
        let bins = self.stats.histogram();
        let peak = bins.iter().copied().max().unwrap_or(0).max(1) as f32;

        let span = (HISTOGRAM_BUCKETS as u64 * HISTOGRAM_BUCKET_MS) as f32; // 100 ms
        let desired = egui::vec2(ui.available_width().min(380.0), 70.0);
        let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let bar_w = rect.width() / HISTOGRAM_BUCKETS as f32;

        for (i, &count) in bins.iter().enumerate() {
            let h = (count as f32 / peak) * rect.height();
            let x0 = rect.left() + i as f32 * bar_w;
            let bar = egui::Rect::from_min_max(
                egui::pos2(x0 + 1.0, rect.bottom() - h),
                egui::pos2(x0 + bar_w - 1.0, rect.bottom()),
            );
            painter.rect_filled(bar, 0.0, egui::Color32::from_rgb(0x00, 0xA0, 0x82));
        }

        // Threshold marker (keyboard): a vertical line at threshold/span across.
        let threshold = self.form.applied().keyboard_threshold_ms as f32;
        let tx = rect.left() + (threshold / span).clamp(0.0, 1.0) * rect.width();
        painter.line_segment(
            [egui::pos2(tx, rect.top()), egui::pos2(tx, rect.bottom())],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(0xD2, 0x28, 0x28)),
        );
        painter.text(
            egui::pos2(tx + 3.0, rect.top()),
            egui::Align2::LEFT_TOP,
            format!("{}ms", self.form.applied().keyboard_threshold_ms),
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(0xD2, 0x28, 0x28),
        );
    }

    fn draw_settings_group(&mut self, ui: &mut egui::Ui) {
        ui.label("Settings");
        ui.checkbox(&mut self.form.draft_mut().autostart, "Start on login");

        ui.horizontal(|ui| {
            ui.label("Panic hotkey:");
            ui.monospace(hotkey::display(&self.form.draft().panic_hotkey.keys()));
        });
        if !self.rebinding {
            if ui.button("Rebind…").clicked() {
                self.rebinding = true;
                self.rebind_candidate = RebindCapture::new();
            }
        } else {
            ui.label("Press the new chord (≥1 modifier + 1 key)…");
            let keys = self.rebind_candidate.keys();
            ui.monospace(if keys.is_empty() {
                "—".to_string()
            } else {
                hotkey::display(&keys)
            });
            ui.horizontal(|ui| {
                let candidate = self.rebind_candidate.chord();
                // Accept only stages the chord into the draft; Save applies it.
                if ui
                    .add_enabled(candidate.is_ok(), egui::Button::new("Accept"))
                    .clicked()
                {
                    if let Ok(chord) = candidate {
                        self.form.draft_mut().panic_hotkey = chord;
                    }
                    self.rebinding = false;
                }
                if ui.button("Cancel").clicked() {
                    self.rebinding = false;
                }
            });
        }

        ui.checkbox(
            &mut self.form.draft_mut().confirm_on_quit,
            "Confirm before quitting",
        );
    }

    /// The Save / Cancel / Reset row that commits or discards the draft.
    fn draw_actions(&mut self, ui: &mut egui::Ui) {
        let dirty = self.form.is_dirty();
        ui.horizontal(|ui| {
            if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
                self.save_settings();
            }
            if ui.add_enabled(dirty, egui::Button::new("Cancel")).clicked() {
                self.cancel_settings();
            }
            if ui.button("Reset to defaults").clicked() {
                self.reset_to_defaults();
            }
        });
        if dirty {
            ui.colored_label(egui::Color32::from_rgb(0xC0, 0x80, 0x00), "Unsaved changes");
        }
    }

    fn draw_quit_dialog(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Quit Bouncer?")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Quit Bouncer? Chatter protection will stop.");
                ui.checkbox(&mut self.dont_ask_again, "Don't ask again");
                ui.horizontal(|ui| {
                    if ui.button("Quit").clicked() {
                        if let QuitResolution::Quit {
                            new_confirm_on_quit,
                        } = resolve_quit_dialog(true, self.dont_ask_again)
                        {
                            if let Some(v) = new_confirm_on_quit {
                                self.form.set_confirm_on_quit(v);
                                self.save_config();
                            }
                            self.confirm_quit_open = false;
                            self.do_quit(ctx);
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_quit_open = false;
                    }
                });
            });
        if !open {
            self.confirm_quit_open = false;
        }
    }
}

impl eframe::App for BouncerApp {
    /// Per-frame non-drawing work — also runs while the window is hidden, so the
    /// tray keeps responding when only the icon is visible.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_reports();
        self.expire_diagnostic();
        self.drain_tray_events(ctx);
        if self.rebinding {
            self.capture_rebind(ctx);
        }
        self.refresh_tray();

        if self.show_requested.swap(false, Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Closing the window hides it (tray keeps running); real exit is via Quit.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // Keep polling the tray channels even while the window is hidden/idle.
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

fn make_icon(state: IconState) -> Result<Icon, String> {
    Icon::from_rgba(icon_rgba(state), ICON_SIZE, ICON_SIZE).map_err(|e| e.to_string())
}

/// Map an egui key to its Windows virtual-key code, for the rebind capture. Covers
/// the keys a panic chord realistically uses (letters, digits, function keys).
fn key_to_vk(key: egui::Key) -> Option<KeyId> {
    use egui::Key::*;
    let vk = match key {
        A => 0x41,
        B => 0x42,
        C => 0x43,
        D => 0x44,
        E => 0x45,
        F => 0x46,
        G => 0x47,
        H => 0x48,
        I => 0x49,
        J => 0x4A,
        K => 0x4B,
        L => 0x4C,
        M => 0x4D,
        N => 0x4E,
        O => 0x4F,
        P => 0x50,
        Q => 0x51,
        R => 0x52,
        S => 0x53,
        T => 0x54,
        U => 0x55,
        V => 0x56,
        W => 0x57,
        X => 0x58,
        Y => 0x59,
        Z => 0x5A,
        Num0 => 0x30,
        Num1 => 0x31,
        Num2 => 0x32,
        Num3 => 0x33,
        Num4 => 0x34,
        Num5 => 0x35,
        Num6 => 0x36,
        Num7 => 0x37,
        Num8 => 0x38,
        Num9 => 0x39,
        F1 => 0x70,
        F2 => 0x71,
        F3 => 0x72,
        F4 => 0x73,
        F5 => 0x74,
        F6 => 0x75,
        F7 => 0x76,
        F8 => 0x77,
        F9 => 0x78,
        F10 => 0x79,
        F11 => 0x7A,
        F12 => 0x7B,
        _ => return None,
    };
    Some(vk)
}
