//! System-tray surface: 4-state icon, state-aware menu, live tooltip (issue #9).
//!
//! This is the **pure presentation + Mode-control core** of the tray, with no OS
//! calls (per ADR-0001 the OS rendering is a thin shell; tray rendering itself is
//! manual-only, DESIGN.md §9). A [`TrayModel`] is a projection of the live Mode +
//! diagnostic overlay + session counters + relevant config, rebuilt by the shell
//! from the `Report` stream. It answers what the tray should *show* (icon, menu
//! labels, tooltip) and resolves what a click should *do* ([`TrayEffect`]) — all
//! deterministic and unit-tested here.

use crate::core::Mode;

/// The four distinct visual states of the tray icon (DESIGN.md §"Tray surface").
/// `ActiveDiagnostic` is the only state carrying the recording badge, because
/// diagnostic recording is meaningful only while `Active`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    Active,
    ActiveDiagnostic,
    Paused,
    Panic,
}

/// A user interaction with the tray, mapped by the shell from a click/menu entry.
/// Left-click maps to `OpenSettings`; the menu entries map to the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// The Pause/Resume entry. Flips Active↔Paused, and clears Panic when in Panic.
    TogglePause,
    /// The Diagnostic entry (a no-op while Paused, where it is greyed out).
    ToggleDiagnostic,
    /// Left-click on the icon, or the Settings… entry.
    OpenSettings,
    /// The Quit entry.
    Quit,
}

/// What the shell should do in response to a [`TrayAction`]. Pure intent — the
/// shell carries it out (sends a `Command`, opens a window, shows a dialog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEffect {
    /// Apply this Mode (send `Command::SetMode`; persist `enabled` for Paused).
    SetMode(Mode),
    /// Apply this diagnostic flag (send `Command::SetDiagnostic`).
    SetDiagnostic(bool),
    /// Open the settings window.
    OpenSettings,
    /// Quit immediately — no confirmation configured.
    Quit,
    /// Show the quit-confirmation dialog (`confirm_on_quit` is set).
    ConfirmQuit,
    /// Nothing to do (e.g. Diagnostic clicked while Paused).
    None,
}

/// The outcome of the quit-confirmation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitResolution {
    /// The user confirmed. `new_confirm_on_quit` is `Some(false)` when they ticked
    /// "Don't ask again" (persist it), else `None` (config unchanged).
    Quit { new_confirm_on_quit: Option<bool> },
    /// The user dismissed the dialog.
    Cancel,
}

/// A projection of the live engine/UI state onto what the tray displays. Rebuilt by
/// the shell whenever the underlying state changes (a `Report`, a counter tick, a
/// config edit). Pure: every method is a deterministic function of these fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    pub mode: Mode,
    pub diagnostic: bool,
    pub keyboard_suppressed: u64,
    pub mouse_suppressed: u64,
    pub confirm_on_quit: bool,
    /// The current panic hotkey, shown in the recovery hint while in Panic.
    pub panic_hotkey: String,
}

impl TrayModel {
    /// The icon variant for the current state. `ActiveDiagnostic` only when both
    /// `Active` and recording; Paused/Panic never show the diagnostic badge.
    pub fn icon(&self) -> IconState {
        match self.mode {
            Mode::Active if self.diagnostic => IconState::ActiveDiagnostic,
            Mode::Active => IconState::Active,
            Mode::Paused => IconState::Paused,
            Mode::Panic => IconState::Panic,
        }
    }

    /// The Pause/Resume entry label: "Pause" while Active, "Resume" otherwise
    /// (Paused, and Panic — where clicking it also clears Panic).
    pub fn pause_resume_label(&self) -> &'static str {
        match self.mode {
            Mode::Active => "Pause",
            Mode::Paused | Mode::Panic => "Resume",
        }
    }

    /// Whether the Diagnostic entry is clickable. Greyed (false) while Paused.
    pub fn diagnostic_enabled(&self) -> bool {
        self.mode != Mode::Paused
    }

    /// Whether the Diagnostic entry shows as checked.
    pub fn diagnostic_checked(&self) -> bool {
        self.diagnostic
    }

    /// The live status-line tooltip: Mode + session counter, plus a recovery hint
    /// in the non-Active (pass-through) modes.
    pub fn tooltip(&self) -> String {
        let counts = format!(
            "{} kbd · {} mouse suppressed",
            self.keyboard_suppressed, self.mouse_suppressed
        );
        match self.mode {
            Mode::Active if self.diagnostic => {
                format!("Bouncer — Active (diagnostic) · {counts}")
            }
            Mode::Active => format!("Bouncer — Active · {counts}"),
            Mode::Paused => {
                format!("Bouncer — Paused (pass-through) · right-click ▸ Resume to re-enable · {counts}")
            }
            Mode::Panic => {
                format!(
                    "Bouncer — PANIC (pass-through) · press {} to resume",
                    self.panic_hotkey
                )
            }
        }
    }

    /// Resolve a tray interaction to the effect the shell should carry out.
    pub fn apply(&self, action: TrayAction) -> TrayEffect {
        match action {
            // Active pauses; Paused and Panic both resume to Active (clearing Panic).
            TrayAction::TogglePause => match self.mode {
                Mode::Active => TrayEffect::SetMode(Mode::Paused),
                Mode::Paused | Mode::Panic => TrayEffect::SetMode(Mode::Active),
            },
            TrayAction::ToggleDiagnostic => {
                if self.diagnostic_enabled() {
                    TrayEffect::SetDiagnostic(!self.diagnostic)
                } else {
                    TrayEffect::None
                }
            }
            TrayAction::OpenSettings => TrayEffect::OpenSettings,
            TrayAction::Quit => {
                if self.confirm_on_quit {
                    TrayEffect::ConfirmQuit
                } else {
                    TrayEffect::Quit
                }
            }
        }
    }
}

/// The side length, in pixels, of the generated tray icon.
pub const ICON_SIZE: u32 = 32;

/// Generate the raw RGBA pixels for an icon state: the Bouncer shield silhouette
/// filled in the state colour (teal = Active, grey = Paused, red = Panic) with a
/// white pulse, on a transparent background, plus a red "recording" dot in the
/// corner for the diagnostic badge. `ICON_SIZE × ICON_SIZE` RGBA8 for
/// `tray_icon::Icon::from_rgba`. Pure and super-sampled; the shape matches
/// `assets/logo.svg`, so the colour mapping stays unit-testable.
pub fn icon_rgba(state: IconState) -> Vec<u8> {
    let (sr, sg, sb) = match state {
        IconState::Active | IconState::ActiveDiagnostic => (0x00u32, 0xA0, 0x82),
        IconState::Paused => (0x80, 0x80, 0x80),
        IconState::Panic => (0xD2, 0x28, 0x28),
    };

    // Fit the shield (logo design space: bbox centred on 128,128, height 166) into
    // the icon with a ~1 px margin, super-sampling each pixel for smooth edges.
    let shield = shield_polygon();
    let scale = ICON_SIZE as f32 * 30.0 / 32.0 / 166.0;
    let to_design = |c: f32| 128.0 + (c - ICON_SIZE as f32 / 2.0) / scale;

    const SS: u32 = 4;
    let mut buf = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let (mut ar, mut ag, mut ab, mut cov) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let cx = x as f32 + (sx as f32 + 0.5) / SS as f32;
                    let cy = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    // Diagnostic badge: a filled dot near the top-right corner.
                    if state == IconState::ActiveDiagnostic
                        && (cx - 24.0).powi(2) + (cy - 8.0).powi(2) <= 25.0
                    {
                        ar += 0xE6;
                        ag += 0x1E;
                        ab += 0x1E;
                        cov += 1;
                        continue;
                    }
                    let (px, py) = (to_design(cx), to_design(cy));
                    if point_in_polygon(px, py, &shield) {
                        if dist_to_polyline(px, py, PULSE) <= 5.0 {
                            ar += 0xFF;
                            ag += 0xFF;
                            ab += 0xFF;
                        } else {
                            ar += sr;
                            ag += sg;
                            ab += sb;
                        }
                        cov += 1;
                    }
                }
            }
            let total = SS * SS;
            let avg = |sum: u32| sum.checked_div(cov).unwrap_or(0) as u8;
            buf.extend_from_slice(&[avg(ar), avg(ag), avg(ab), (255 * cov / total) as u8]);
        }
    }
    buf
}

/// The white square-wave pulse inside the shield, in the logo's 256-unit space.
const PULSE: &[(f32, f32)] = &[
    (93.0, 147.0),
    (112.0, 147.0),
    (112.0, 106.0),
    (128.0, 106.0),
    (128.0, 147.0),
    (144.0, 147.0),
    (144.0, 118.0),
    (166.0, 118.0),
];

/// The shield outline as a polygon, sampling the logo path's two quadratic curves.
fn shield_polygon() -> Vec<(f32, f32)> {
    let mut p = vec![(128.0, 45.0), (182.0, 64.0), (182.0, 125.0)];
    push_quad(&mut p, (182.0, 125.0), (182.0, 182.0), (128.0, 211.0));
    push_quad(&mut p, (128.0, 211.0), (74.0, 182.0), (74.0, 125.0));
    p.push((74.0, 64.0));
    p
}

fn push_quad(out: &mut Vec<(f32, f32)>, p0: (f32, f32), c: (f32, f32), p1: (f32, f32)) {
    const STEPS: u32 = 16;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        out.push((
            u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0,
            u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1,
        ));
    }
}

/// Even-odd ray-cast point-in-polygon (the polygon is implicitly closed).
fn point_in_polygon(x: f32, y: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for (i, &(xi, yi)) in poly.iter().enumerate() {
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) {
            let xint = xi + (y - yi) / (yj - yi) * (xj - xi);
            if x < xint {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn dist_to_polyline(x: f32, y: f32, pts: &[(f32, f32)]) -> f32 {
    pts.windows(2)
        .map(|w| dist_to_segment(x, y, w[0], w[1]))
        .fold(f32::MAX, f32::min)
}

fn dist_to_segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// Resolve the quit-confirmation dialog. Confirming with "Don't ask again" ticked
/// persists `confirm_on_quit = false`; cancelling leaves everything untouched.
pub fn resolve_quit_dialog(confirmed: bool, dont_ask_again: bool) -> QuitResolution {
    if !confirmed {
        return QuitResolution::Cancel;
    }
    QuitResolution::Quit {
        new_confirm_on_quit: if dont_ask_again { Some(false) } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A default Active model with zeroed counters.
    fn model(mode: Mode) -> TrayModel {
        TrayModel {
            mode,
            diagnostic: false,
            keyboard_suppressed: 0,
            mouse_suppressed: 0,
            confirm_on_quit: true,
            panic_hotkey: "Ctrl+Alt+Shift+F12".to_string(),
        }
    }

    // --- AC: 4 distinct visual states mapped to Mode (+ diagnostic badge) ---

    #[test]
    fn icon_maps_each_mode_to_a_distinct_state() {
        assert_eq!(model(Mode::Active).icon(), IconState::Active);
        assert_eq!(model(Mode::Paused).icon(), IconState::Paused);
        assert_eq!(model(Mode::Panic).icon(), IconState::Panic);
    }

    #[test]
    fn diagnostic_badge_shows_only_while_active() {
        let mut m = model(Mode::Active);
        m.diagnostic = true;
        assert_eq!(m.icon(), IconState::ActiveDiagnostic);

        // The badge is suppressed in pass-through modes (diagnostic is meaningful
        // only while Active), so the icon stays the plain Paused/Panic state.
        m.mode = Mode::Paused;
        assert_eq!(m.icon(), IconState::Paused);
        m.mode = Mode::Panic;
        assert_eq!(m.icon(), IconState::Panic);
    }

    // --- AC: right-click menu (Pause/Resume label flips; Diagnostic greyed Paused) ---

    #[test]
    fn pause_resume_label_flips_with_mode() {
        assert_eq!(model(Mode::Active).pause_resume_label(), "Pause");
        assert_eq!(model(Mode::Paused).pause_resume_label(), "Resume");
        // In Panic the entry resumes (clears Panic), so it reads "Resume".
        assert_eq!(model(Mode::Panic).pause_resume_label(), "Resume");
    }

    #[test]
    fn diagnostic_entry_greyed_only_while_paused() {
        assert!(model(Mode::Active).diagnostic_enabled());
        assert!(model(Mode::Panic).diagnostic_enabled());
        assert!(!model(Mode::Paused).diagnostic_enabled());
    }

    #[test]
    fn diagnostic_checked_reflects_the_flag() {
        let mut m = model(Mode::Active);
        assert!(!m.diagnostic_checked());
        m.diagnostic = true;
        assert!(m.diagnostic_checked());
    }

    #[test]
    fn pause_from_active_pauses() {
        assert_eq!(
            model(Mode::Active).apply(TrayAction::TogglePause),
            TrayEffect::SetMode(Mode::Paused)
        );
    }

    #[test]
    fn resume_from_paused_reactivates() {
        assert_eq!(
            model(Mode::Paused).apply(TrayAction::TogglePause),
            TrayEffect::SetMode(Mode::Active)
        );
    }

    #[test]
    fn pause_resume_clears_panic() {
        assert_eq!(
            model(Mode::Panic).apply(TrayAction::TogglePause),
            TrayEffect::SetMode(Mode::Active)
        );
    }

    #[test]
    fn diagnostic_toggles_when_active() {
        assert_eq!(
            model(Mode::Active).apply(TrayAction::ToggleDiagnostic),
            TrayEffect::SetDiagnostic(true)
        );
        let mut on = model(Mode::Active);
        on.diagnostic = true;
        assert_eq!(
            on.apply(TrayAction::ToggleDiagnostic),
            TrayEffect::SetDiagnostic(false)
        );
    }

    #[test]
    fn diagnostic_toggle_is_inert_while_paused() {
        assert_eq!(
            model(Mode::Paused).apply(TrayAction::ToggleDiagnostic),
            TrayEffect::None
        );
    }

    // --- AC: left-click opens the Settings window ---

    #[test]
    fn open_settings_action_opens_settings() {
        assert_eq!(
            model(Mode::Active).apply(TrayAction::OpenSettings),
            TrayEffect::OpenSettings
        );
    }

    // --- AC: tooltip is a live status line (Mode + counter; recovery hint off-Active) ---

    #[test]
    fn tooltip_shows_mode_and_session_counts_while_active() {
        let mut m = model(Mode::Active);
        m.keyboard_suppressed = 7;
        m.mouse_suppressed = 3;
        let tip = m.tooltip();
        assert!(tip.contains("Active"), "tooltip names the Mode: {tip}");
        assert!(
            tip.contains('7') && tip.contains('3'),
            "shows counts: {tip}"
        );
    }

    #[test]
    fn tooltip_marks_diagnostic_while_recording() {
        let mut m = model(Mode::Active);
        m.diagnostic = true;
        assert!(m.tooltip().to_lowercase().contains("diagnostic"));
    }

    #[test]
    fn tooltip_paused_explains_how_to_recover() {
        let tip = model(Mode::Paused).tooltip();
        assert!(tip.contains("Paused"), "{tip}");
        assert!(tip.contains("Resume"), "recovery hint present: {tip}");
    }

    #[test]
    fn tooltip_panic_shows_the_hotkey_to_resume() {
        let tip = model(Mode::Panic).tooltip();
        assert!(tip.contains("PANIC"), "{tip}");
        assert!(
            tip.contains("Ctrl+Alt+Shift+F12"),
            "names the hotkey: {tip}"
        );
        assert!(tip.to_lowercase().contains("resume"), "{tip}");
    }

    // --- AC: quit shows a confirmation with a "Don't ask again" checkbox ---

    #[test]
    fn quit_asks_for_confirmation_when_configured() {
        let m = model(Mode::Active); // confirm_on_quit defaults true
        assert_eq!(m.apply(TrayAction::Quit), TrayEffect::ConfirmQuit);
    }

    #[test]
    fn quit_is_immediate_once_confirmation_disabled() {
        let mut m = model(Mode::Active);
        m.confirm_on_quit = false;
        assert_eq!(m.apply(TrayAction::Quit), TrayEffect::Quit);
    }

    #[test]
    fn confirming_quit_without_dont_ask_leaves_config_untouched() {
        assert_eq!(
            resolve_quit_dialog(true, false),
            QuitResolution::Quit {
                new_confirm_on_quit: None
            }
        );
    }

    #[test]
    fn confirming_quit_with_dont_ask_persists_the_preference() {
        assert_eq!(
            resolve_quit_dialog(true, true),
            QuitResolution::Quit {
                new_confirm_on_quit: Some(false)
            }
        );
    }

    #[test]
    fn cancelling_quit_does_nothing() {
        assert_eq!(resolve_quit_dialog(false, true), QuitResolution::Cancel);
        assert_eq!(resolve_quit_dialog(false, false), QuitResolution::Cancel);
    }

    // --- icon pixels ---

    #[test]
    fn icon_rgba_has_the_expected_size_and_distinct_colors() {
        let n = (ICON_SIZE * ICON_SIZE * 4) as usize;
        for s in [
            IconState::Active,
            IconState::ActiveDiagnostic,
            IconState::Paused,
            IconState::Panic,
        ] {
            assert_eq!(icon_rgba(s).len(), n, "{s:?} is ICON_SIZE² RGBA8");
        }
        // A pixel on the lower shield body (below the pulse) carries the state colour.
        let body = ((24 * ICON_SIZE + ICON_SIZE / 2) * 4) as usize;
        let rgb = |s| icon_rgba(s)[body..body + 3].to_vec();
        assert_ne!(rgb(IconState::Active), rgb(IconState::Paused));
        assert_ne!(rgb(IconState::Active), rgb(IconState::Panic));
        assert_ne!(rgb(IconState::Paused), rgb(IconState::Panic));
    }
}
