//! UI-side observability state, built from the `Report` stream (never in the
//! Engine, per ADR-0001). All in-memory: the counter resets every boot and nothing
//! keystroke-related touches disk (DESIGN.md §7 privacy invariant 4).
//!
//! Three pieces, all driven by `Report::Suppressed`:
//! - a **per-device session counter** (always counting, never cleared by the user),
//! - an opt-in **ring buffer** of the last [`RING_CAPACITY`] suppressed samples,
//!   filled only while diagnostic recording is active, and
//! - a **gap histogram** over those samples for threshold calibration.
//!
//! Diagnostic recording is opt-in and auto-expires after [`DIAGNOSTIC_TTL`]. Time
//! is passed in (an `Instant`) so this stays pure and deterministically testable.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::core::{Device, KeyId};
use crate::messages::Report;

/// The diagnostic ring buffer holds at most this many suppressed samples; the
/// oldest is evicted past the cap (DESIGN.md §7: "~last 500 suppressed events").
pub const RING_CAPACITY: usize = 500;

/// Diagnostic recording auto-expires this long after it is switched on.
pub const DIAGNOSTIC_TTL: Duration = Duration::from_secs(60 * 60);

/// Width of one histogram bucket, in milliseconds.
pub const HISTOGRAM_BUCKET_MS: u64 = 5;

/// Number of histogram buckets, spanning `0..100 ms` (the threshold clamp range,
/// `MAX_THRESHOLD_MS`). A gap at or beyond the top lands in the last bucket.
pub const HISTOGRAM_BUCKETS: usize = 20;

/// One recorded suppression: which key, the measured gap, and when it landed.
/// `key` is the suppressed key/button id; nothing here identifies *passed* input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub key: KeyId,
    pub gap_ms: u64,
    pub at: Instant,
}

/// Session suppressed counters + the diagnostic ring buffer + the gap histogram.
#[derive(Debug, Default)]
pub struct Stats {
    keyboard: u64,
    mouse: u64,
    buffer: VecDeque<Sample>,
    /// `Some(deadline)` while recording; recording is over once `now >= deadline`.
    recording_until: Option<Instant>,
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Suppressed-keyboard-events this session.
    pub fn keyboard_suppressed(&self) -> u64 {
        self.keyboard
    }

    /// Suppressed-mouse-events this session.
    pub fn mouse_suppressed(&self) -> u64 {
        self.mouse
    }

    /// The diagnostic ring buffer, oldest first.
    pub fn samples(&self) -> &VecDeque<Sample> {
        &self.buffer
    }

    /// Fold one `Report` into the stats. Only `Report::Suppressed` matters; it always
    /// bumps the device counter, and additionally appends a sample while recording.
    pub fn record(&mut self, report: &Report, now: Instant) {
        if let Report::Suppressed {
            device,
            key,
            gap_ms,
        } = *report
        {
            match device {
                Device::Keyboard => self.keyboard += 1,
                Device::Mouse => self.mouse += 1,
            }
            if self.is_recording(now) {
                if self.buffer.len() == RING_CAPACITY {
                    self.buffer.pop_front();
                }
                self.buffer.push_back(Sample {
                    key,
                    gap_ms,
                    at: now,
                });
            }
        }
    }

    /// Begin (or restart) diagnostic recording; it expires at `now + DIAGNOSTIC_TTL`.
    pub fn start_recording(&mut self, now: Instant) {
        self.recording_until = Some(now + DIAGNOSTIC_TTL);
    }

    /// Stop recording now (a manual toggle-off). Leaves the buffer intact.
    pub fn stop_recording(&mut self) {
        self.recording_until = None;
    }

    /// Whether diagnostic recording is currently live (set, and not yet expired).
    pub fn is_recording(&self, now: Instant) -> bool {
        self.recording_until.is_some_and(|deadline| now < deadline)
    }

    /// Time left before auto-expiry, for the UI countdown; `None` when not recording
    /// (or already expired).
    pub fn time_remaining(&self, now: Instant) -> Option<Duration> {
        self.recording_until
            .filter(|&deadline| now < deadline)
            .map(|deadline| deadline - now)
    }

    /// Empty the ring buffer. Does **not** touch the session counter (DESIGN.md §7:
    /// Clear empties the buffer; the counter only resets on restart).
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Bucket the recorded gaps into [`HISTOGRAM_BUCKETS`] bins of
    /// [`HISTOGRAM_BUCKET_MS`] each. A gap at/above the top bucket is clamped into
    /// the last bin, so the histogram always sums to the buffer length.
    pub fn histogram(&self) -> [u32; HISTOGRAM_BUCKETS] {
        let mut bins = [0u32; HISTOGRAM_BUCKETS];
        for s in &self.buffer {
            let idx = (s.gap_ms / HISTOGRAM_BUCKET_MS) as usize;
            bins[idx.min(HISTOGRAM_BUCKETS - 1)] += 1;
        }
        bins
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: KeyId = 0x41;
    const LMB: KeyId = 0x01;

    fn suppressed(device: Device, key: KeyId, gap_ms: u64) -> Report {
        Report::Suppressed {
            device,
            key,
            gap_ms,
        }
    }

    // --- AC: a Report::Suppressed increments the right device counter ---

    #[test]
    fn suppressed_report_increments_the_matching_device_counter() {
        let mut s = Stats::new();
        let now = Instant::now();
        s.record(&suppressed(Device::Keyboard, A, 5), now);
        s.record(&suppressed(Device::Keyboard, A, 7), now);
        s.record(&suppressed(Device::Mouse, LMB, 9), now);

        assert_eq!(s.keyboard_suppressed(), 2);
        assert_eq!(s.mouse_suppressed(), 1);
    }

    #[test]
    fn non_suppressed_reports_change_nothing() {
        let mut s = Stats::new();
        let now = Instant::now();
        s.record(&Report::ModeChanged(crate::core::Mode::Panic), now);
        s.record(&Report::HookEvicted, now);
        assert_eq!(s.keyboard_suppressed(), 0);
        assert_eq!(s.mouse_suppressed(), 0);
        assert!(s.samples().is_empty());
    }

    // --- AC: the counter counts even when not recording; the buffer does not ---

    #[test]
    fn counter_counts_without_recording_but_buffer_stays_empty() {
        let mut s = Stats::new();
        let now = Instant::now();
        // Not recording: counter ticks, buffer untouched.
        s.record(&suppressed(Device::Keyboard, A, 5), now);
        assert_eq!(s.keyboard_suppressed(), 1);
        assert!(s.samples().is_empty());
    }

    #[test]
    fn samples_are_recorded_only_while_recording() {
        let mut s = Stats::new();
        let now = Instant::now();
        s.start_recording(now);
        s.record(&suppressed(Device::Keyboard, A, 5), now);
        assert_eq!(s.samples().len(), 1);
        assert_eq!(
            *s.samples().back().unwrap(),
            Sample {
                key: A,
                gap_ms: 5,
                at: now
            }
        );
    }

    // --- AC: ring buffer caps at capacity (oldest evicted) ---

    #[test]
    fn ring_buffer_caps_at_capacity_evicting_oldest() {
        let mut s = Stats::new();
        let now = Instant::now();
        s.start_recording(now);
        // Push one more than capacity; gaps encode insertion order (mod is fine here).
        for i in 0..(RING_CAPACITY as u64 + 1) {
            s.record(&suppressed(Device::Keyboard, A, i % 100), now);
        }
        assert_eq!(s.samples().len(), RING_CAPACITY);
        // The very first sample (gap 0) was evicted; the front is now the second (gap 1).
        assert_eq!(s.samples().front().unwrap().gap_ms, 1);
        // The counter, by contrast, saw every event.
        assert_eq!(s.keyboard_suppressed(), RING_CAPACITY as u64 + 1);
    }

    // --- AC: histogram buckets gaps into expected bins ---

    #[test]
    fn histogram_buckets_gaps_into_expected_bins() {
        let mut s = Stats::new();
        let now = Instant::now();
        s.start_recording(now);
        // 5-ms buckets: 0..5 → bin0, 5..10 → bin1, 95..100 → bin19.
        s.record(&suppressed(Device::Keyboard, A, 0), now); // bin 0
        s.record(&suppressed(Device::Keyboard, A, 4), now); // bin 0
        s.record(&suppressed(Device::Keyboard, A, 5), now); // bin 1
        s.record(&suppressed(Device::Keyboard, A, 97), now); // bin 19
        s.record(&suppressed(Device::Keyboard, A, 250), now); // clamps into bin 19

        let h = s.histogram();
        assert_eq!(h[0], 2);
        assert_eq!(h[1], 1);
        assert_eq!(h[19], 2);
        assert_eq!(h.iter().sum::<u32>(), s.samples().len() as u32);
    }

    // --- AC: diagnostic mode auto-expires after 1 hour ---

    #[test]
    fn recording_auto_expires_after_the_ttl() {
        let mut s = Stats::new();
        let t0 = Instant::now();
        s.start_recording(t0);

        assert!(s.is_recording(t0));
        assert!(s.is_recording(t0 + Duration::from_secs(59 * 60)));
        // At/after the TTL it is no longer recording…
        assert!(!s.is_recording(t0 + DIAGNOSTIC_TTL));
        assert!(!s.is_recording(t0 + Duration::from_secs(61 * 60)));

        // …and a suppression past expiry still counts but is not buffered.
        s.record(
            &suppressed(Device::Keyboard, A, 5),
            t0 + Duration::from_secs(61 * 60),
        );
        assert_eq!(s.keyboard_suppressed(), 1);
        assert!(s.samples().is_empty());
    }

    #[test]
    fn time_remaining_counts_down_then_vanishes() {
        let mut s = Stats::new();
        let t0 = Instant::now();
        assert_eq!(s.time_remaining(t0), None); // not recording
        s.start_recording(t0);
        assert_eq!(s.time_remaining(t0), Some(DIAGNOSTIC_TTL));
        assert_eq!(
            s.time_remaining(t0 + Duration::from_secs(60)),
            Some(DIAGNOSTIC_TTL - Duration::from_secs(60))
        );
        assert_eq!(s.time_remaining(t0 + DIAGNOSTIC_TTL), None);
    }

    // --- AC: manual Clear empties the buffer; Clear does not affect the counter ---

    #[test]
    fn clear_empties_the_buffer_but_keeps_the_counter() {
        let mut s = Stats::new();
        let now = Instant::now();
        s.start_recording(now);
        s.record(&suppressed(Device::Keyboard, A, 5), now);
        s.record(&suppressed(Device::Mouse, LMB, 8), now);
        assert_eq!(s.samples().len(), 2);

        s.clear();
        assert!(s.samples().is_empty());
        // The session counter is untouched by Clear.
        assert_eq!(s.keyboard_suppressed(), 1);
        assert_eq!(s.mouse_suppressed(), 1);
    }

    #[test]
    fn stop_recording_halts_buffering_without_clearing() {
        let mut s = Stats::new();
        let now = Instant::now();
        s.start_recording(now);
        s.record(&suppressed(Device::Keyboard, A, 5), now);
        s.stop_recording();
        assert!(!s.is_recording(now));
        // The earlier sample survives; new ones aren't buffered.
        s.record(&suppressed(Device::Keyboard, A, 6), now);
        assert_eq!(s.samples().len(), 1);
    }
}
