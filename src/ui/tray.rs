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
        todo!("GREEN")
    }

    /// The Pause/Resume entry label: "Pause" while Active, "Resume" otherwise
    /// (Paused, and Panic — where clicking it also clears Panic).
    pub fn pause_resume_label(&self) -> &'static str {
        todo!("GREEN")
    }

    /// Whether the Diagnostic entry is clickable. Greyed (false) while Paused.
    pub fn diagnostic_enabled(&self) -> bool {
        todo!("GREEN")
    }

    /// Whether the Diagnostic entry shows as checked.
    pub fn diagnostic_checked(&self) -> bool {
        todo!("GREEN")
    }

    /// The live status-line tooltip: Mode + session counter, plus a recovery hint
    /// in the non-Active (pass-through) modes.
    pub fn tooltip(&self) -> String {
        todo!("GREEN")
    }

    /// Resolve a tray interaction to the effect the shell should carry out.
    pub fn apply(&self, action: TrayAction) -> TrayEffect {
        let _ = action;
        todo!("GREEN")
    }
}

/// Resolve the quit-confirmation dialog. Confirming with "Don't ask again" ticked
/// persists `confirm_on_quit = false`; cancelling leaves everything untouched.
pub fn resolve_quit_dialog(confirmed: bool, dont_ask_again: bool) -> QuitResolution {
    let _ = (confirmed, dont_ask_again);
    todo!("GREEN")
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
}
