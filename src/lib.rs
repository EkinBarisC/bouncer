//! Bouncer — a headless input debouncer for Windows.
//!
//! The crate is split into a **pure core** (all decision logic, no OS) and a thin
//! **imperative shell** (`platform`, `ui`) per [ADR-0001]. This file only declares
//! the module skeleton; behavior is added test-first, slice by slice (see the repo
//! issues). See `DESIGN.md` for the spec and `CONTEXT.md` for the glossary.
//!
//! [ADR-0001]: ../docs/adr/0001-pure-engine-on-hook-thread-channel-control.md

// TODO(scaffolding): remove once the modules are wired together (issues #3+).
// During scaffolding the stub types are defined but not yet used.
#![allow(dead_code)]

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
