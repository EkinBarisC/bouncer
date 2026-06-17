//! Hook-eviction watchdog **policy** (issue #12, DESIGN.md §6.3).
//!
//! Windows silently evicts a low-level hook whose callback overruns
//! `LowLevelHooksTimeout` — the hook just stops firing, with no notification. A
//! background liveness probe (in the shell) periodically reports whether the hook
//! still runs; this module is the pure decision core that turns that stream of
//! [`Health`] observations into actions (reinstall) and the user-visible
//! [`Report::HookEvicted`] / [`Report::HookReinstalled`] transitions.
//!
//! Pure and unit-tested: the OS probe + the actual `SetWindowsHookExW` reinstall
//! live in `windows.rs` (manual-verify), but *when* to reinstall and *what* to
//! report is decided here so the state machine is provable.

use crate::messages::Report;

/// The result of one liveness probe of the installed hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// The probe confirmed the hook still fires.
    Alive,
    /// The probe expired without the hook firing — assume evicted.
    Dead,
}

/// What the shell should do after an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    /// Hook looks healthy; do nothing.
    Nothing,
    /// Hook is down; (re)install it.
    Reinstall,
}

/// One step's decision: an action plus an optional state-transition report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchdogStep {
    pub action: WatchdogAction,
    pub report: Option<Report>,
}

/// Tracks whether the hooks are believed alive and emits edge-triggered reports.
/// Starts in the `alive` state (a hook was just installed when it's constructed).
#[derive(Debug)]
pub struct Watchdog {
    alive: bool,
}

impl Default for Watchdog {
    fn default() -> Self {
        Self { alive: true }
    }
}

impl Watchdog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the hooks are currently believed to be alive.
    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// Fold one liveness observation into the state machine.
    ///
    /// - `Dead` always asks for a reinstall (retried each probe until it recovers),
    ///   but only the *first* `Dead` after being alive reports [`Report::HookEvicted`].
    /// - `Alive` after a dead spell reports [`Report::HookReinstalled`] exactly once;
    ///   a steady-alive probe reports nothing.
    pub fn observe(&mut self, health: Health) -> WatchdogStep {
        match health {
            Health::Alive => {
                let report = if !self.alive {
                    self.alive = true;
                    Some(Report::HookReinstalled)
                } else {
                    None
                };
                WatchdogStep {
                    action: WatchdogAction::Nothing,
                    report,
                }
            }
            Health::Dead => {
                let report = if self.alive {
                    self.alive = false;
                    Some(Report::HookEvicted)
                } else {
                    None
                };
                WatchdogStep {
                    action: WatchdogAction::Reinstall,
                    report,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_alive_and_a_healthy_probe_is_a_no_op() {
        let mut w = Watchdog::new();
        assert!(w.is_alive());
        assert_eq!(
            w.observe(Health::Alive),
            WatchdogStep {
                action: WatchdogAction::Nothing,
                report: None
            }
        );
    }

    #[test]
    fn first_dead_probe_reports_evicted_and_asks_for_reinstall() {
        let mut w = Watchdog::new();
        let step = w.observe(Health::Dead);
        assert_eq!(step.action, WatchdogAction::Reinstall);
        assert_eq!(step.report, Some(Report::HookEvicted));
        assert!(!w.is_alive());
    }

    #[test]
    fn repeated_dead_probes_keep_reinstalling_without_duplicate_reports() {
        let mut w = Watchdog::new();
        w.observe(Health::Dead); // evicted reported once
        let step = w.observe(Health::Dead);
        assert_eq!(step.action, WatchdogAction::Reinstall);
        assert_eq!(
            step.report, None,
            "no duplicate HookEvicted while still dead"
        );
    }

    #[test]
    fn recovery_reports_reinstalled_exactly_once() {
        let mut w = Watchdog::new();
        w.observe(Health::Dead);
        let recovered = w.observe(Health::Alive);
        assert_eq!(recovered.action, WatchdogAction::Nothing);
        assert_eq!(recovered.report, Some(Report::HookReinstalled));
        assert!(w.is_alive());
        // A subsequent healthy probe is silent again.
        assert_eq!(w.observe(Health::Alive).report, None);
    }

    #[test]
    fn full_evict_then_recover_cycle() {
        let mut w = Watchdog::new();
        // Healthy for a while…
        assert_eq!(w.observe(Health::Alive).report, None);
        // …evicted…
        assert_eq!(w.observe(Health::Dead).report, Some(Report::HookEvicted));
        // …still down, retrying…
        assert_eq!(w.observe(Health::Dead).action, WatchdogAction::Reinstall);
        // …back up.
        assert_eq!(
            w.observe(Health::Alive).report,
            Some(Report::HookReinstalled)
        );
    }
}
