//! Bouncer — a headless input debouncer for Windows.
//!
//! The crate is split into a **pure core** (all decision logic, no OS) and a thin
//! **imperative shell** (`platform`, `ui`) per [ADR-0001]. See `DESIGN.md` for the
//! spec and `UBIQUITOUS_LANGUAGE.md` for the glossary.
//!
//! [ADR-0001]: ../docs/adr/0001-pure-engine-on-hook-thread-channel-control.md

pub mod config;
pub mod core;
pub mod messages;
pub mod platform;
pub mod stats;
pub mod ui;

/// Test-only harness for the SendInput-driven keyboard integration test (#7).
/// Compiled only under `--features integration-test` on Windows; never shipped.
#[cfg(all(windows, feature = "integration-test"))]
pub mod test_support;
