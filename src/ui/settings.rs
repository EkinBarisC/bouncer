//! egui settings window: Status / Tuning / Diagnostics / Settings groups (issue #10).
//!
//! The window and its four groups are implemented in [`crate::ui::app`] (the
//! `BouncerApp` `draw_*` methods), which also owns the tray and the channel wiring;
//! keeping it in one place avoids threading the shared app state through a separate
//! module. This file is retained as the documented home of the settings surface.
