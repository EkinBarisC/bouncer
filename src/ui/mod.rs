//! The tray + egui settings window.
//!
//! Skeleton only. The tray surface lands in #9 and the settings window in #10; the
//! egui dependency is introduced then.

#[cfg(windows)]
pub mod app;
pub mod hotkey;
pub mod rebind;
pub mod settings;
pub mod tray;
