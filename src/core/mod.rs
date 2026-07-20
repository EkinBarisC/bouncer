//! The pure core: all decision logic, with no OS calls, no clock, and no I/O.
//!
//! Timestamps arrive *on* the event (`InputEvent::timestamp_ms`) so the core is
//! fully deterministic and unit-testable with synthetic event streams.

pub mod debouncer;
pub mod engine;
pub mod event;
pub mod hotkey;
pub mod mode;
pub mod panic;
pub mod verdict;

#[cfg(test)]
pub(crate) mod test_util;

pub use debouncer::{Debouncer, Thresholds};
pub use engine::Engine;
pub use event::{Device, EventKind, InputEvent, KeyCode, KeyId, MouseButton};
pub use mode::Mode;
pub use panic::{ChordError, PanicChord, PanicDetector, PanicReaction};
pub use verdict::{Outcome, Verdict};
