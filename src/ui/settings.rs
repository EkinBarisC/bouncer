//! The Settings-form model: the pure draft/applied state machine behind the egui
//! window (issue #32, point 3).
//!
//! The window *renders* this and dispatches its decisions ([`crate::ui::app`]), but
//! the rules — what counts as an unsaved edit, what Save commits, what Cancel and
//! Reset restore, and how Pause/Resume and the quit dialog mutate committed state
//! without ever looking like a form edit — live here, with no egui, so they are
//! unit-tested through this interface.

use crate::config::Config;
use crate::core::{PanicChord, Thresholds};

/// Two `Config`s: `applied` is what's committed (and drives the live engine), `draft`
/// is the edit buffer the form mutates. They start equal; the form is *dirty* when
/// they diverge.
#[derive(Debug)]
pub struct SettingsForm {
    applied: Config,
    draft: Config,
}

/// What a [`SettingsForm::commit`] changed, so the shell can apply the side effects.
/// `thresholds` is always pushed (cheap, idempotent); `autostart` and `panic_chord`
/// are `Some` only when that setting actually changed, so a Save doesn't re-toggle
/// the registry key or re-send a chord that didn't move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsCommit {
    pub thresholds: Thresholds,
    pub autostart: Option<bool>,
    pub panic_chord: Option<PanicChord>,
}

impl SettingsForm {
    /// Start a form from the committed settings; the draft begins as a copy.
    pub fn new(applied: Config) -> Self {
        let draft = applied.clone();
        Self { applied, draft }
    }

    /// The committed settings (drive the live engine + persisted to disk).
    pub fn applied(&self) -> &Config {
        &self.applied
    }

    /// The edit buffer, for display.
    pub fn draft(&self) -> &Config {
        &self.draft
    }

    /// The edit buffer, mutably — the egui widgets bind to these fields.
    pub fn draft_mut(&mut self) -> &mut Config {
        &mut self.draft
    }

    /// Whether the draft has edits not yet committed.
    pub fn is_dirty(&self) -> bool {
        self.draft != self.applied
    }

    /// Commit the draft to `applied`, returning what changed so the shell can apply
    /// the live/OS side effects.
    pub fn commit(&mut self) -> SettingsCommit {
        let autostart =
            (self.draft.autostart != self.applied.autostart).then_some(self.draft.autostart);
        let panic_chord = (self.draft.panic_hotkey != self.applied.panic_hotkey)
            .then(|| self.draft.panic_hotkey.clone());
        self.applied = self.draft.clone();
        SettingsCommit {
            thresholds: self.applied.thresholds(),
            autostart,
            panic_chord,
        }
    }

    /// Discard uncommitted edits.
    pub fn cancel(&mut self) {
        self.draft = self.applied.clone();
    }

    /// Load factory defaults into the draft, preserving the live pause state
    /// (`enabled`), which Pause/Resume owns — not the form. The user still Saves.
    pub fn reset_to_defaults(&mut self) {
        self.draft = Config {
            enabled: self.applied.enabled,
            ..Config::default()
        };
    }

    /// Set `enabled` on *both* applied and draft (Pause/Resume owns this, so toggling
    /// pause never shows up as an unsaved form edit). The shell still persists.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.applied.enabled = enabled;
        self.draft.enabled = enabled;
    }

    /// Set `confirm_on_quit` on both applied and draft (the quit dialog's "Don't ask
    /// again" commits directly, outside the form's Save). The shell still persists.
    pub fn set_confirm_on_quit(&mut self, value: bool) {
        self.applied.confirm_on_quit = value;
        self.draft.confirm_on_quit = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_clean_with_draft_equal_to_applied() {
        let form = SettingsForm::new(Config::default());
        assert!(!form.is_dirty());
        assert_eq!(form.draft(), form.applied());
    }

    #[test]
    fn editing_the_draft_makes_it_dirty() {
        let mut form = SettingsForm::new(Config::default());
        form.draft_mut().keyboard_threshold_ms = 7;
        assert!(form.is_dirty());
    }

    #[test]
    fn cancel_reverts_the_draft() {
        let mut form = SettingsForm::new(Config::default());
        form.draft_mut().keyboard_threshold_ms = 7;
        form.cancel();
        assert!(!form.is_dirty());
        assert_eq!(form.draft().keyboard_threshold_ms, 30);
    }

    #[test]
    fn commit_promotes_the_draft_and_reports_changed_thresholds() {
        let mut form = SettingsForm::new(Config::default());
        form.draft_mut().keyboard_threshold_ms = 12;
        form.draft_mut().debounce_mouse = false; // disabled class → 0 ms
        let commit = form.commit();

        assert!(!form.is_dirty(), "commit clears the dirty state");
        assert_eq!(form.applied().keyboard_threshold_ms, 12);
        assert_eq!(
            commit.thresholds,
            Thresholds {
                keyboard_ms: 12,
                mouse_ms: 0
            }
        );
    }

    #[test]
    fn commit_flags_autostart_and_hotkey_only_when_they_change() {
        let mut form = SettingsForm::new(Config::default());
        // Only a threshold edit: autostart/hotkey unchanged → both None.
        form.draft_mut().keyboard_threshold_ms = 5;
        let commit = form.commit();
        assert_eq!(commit.autostart, None);
        assert_eq!(commit.panic_chord, None);

        // Now flip autostart and the chord.
        let chord = crate::core::hotkey::parse("Ctrl+Alt+Q").unwrap();
        form.draft_mut().autostart = !form.applied().autostart;
        form.draft_mut().panic_hotkey = chord.clone();
        let commit = form.commit();
        assert_eq!(commit.autostart, Some(false));
        assert_eq!(commit.panic_chord, Some(chord));
    }

    #[test]
    fn reset_to_defaults_keeps_the_live_pause_state() {
        let applied = Config {
            enabled: false, // user is Paused
            keyboard_threshold_ms: 99,
            ..Config::default()
        };
        let mut form = SettingsForm::new(applied);

        form.reset_to_defaults();
        // Threshold returns to default, but enabled (pause) is preserved.
        assert_eq!(form.draft().keyboard_threshold_ms, 30);
        assert!(!form.draft().enabled);
    }

    #[test]
    fn set_enabled_stays_in_step_so_it_is_never_a_form_edit() {
        let mut form = SettingsForm::new(Config::default());
        form.set_enabled(false);
        assert!(!form.is_dirty(), "pausing is not an unsaved edit");
        assert!(!form.applied().enabled);
        assert!(!form.draft().enabled);
    }

    #[test]
    fn set_confirm_on_quit_stays_in_step() {
        let mut form = SettingsForm::new(Config::default());
        form.set_confirm_on_quit(false);
        assert!(!form.is_dirty());
        assert!(!form.applied().confirm_on_quit);
        assert!(!form.draft().confirm_on_quit);
    }
}
