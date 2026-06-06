//! The pure core: all decision logic, with no OS calls, no clock, and no I/O.
//!
//! Timestamps arrive *on* the event (`InputEvent::timestamp_ms`) so the core is
//! fully deterministic and unit-testable with synthetic event streams.

pub mod debouncer;
pub mod engine;
pub mod event;
pub mod mode;
pub mod panic;
pub mod verdict;

pub use debouncer::{Debouncer, Thresholds};
pub use engine::Engine;
pub use event::{Device, EventKind, InputEvent, KeyId};
pub use mode::Mode;
pub use panic::PanicDetector;
pub use verdict::{Outcome, Verdict};
