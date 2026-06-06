# PRD — Bouncer v1 (test-driven)

> Scope: the v1 build of Bouncer, written test-first. The **Testing Decisions** section is the
> centerpiece — this PRD exists to drive the TDD loop described in [DESIGN.md](DESIGN.md) §9.
> Domain terms follow [CONTEXT.md](CONTEXT.md); architecture follows
> [docs/adr/0001](docs/adr/0001-pure-engine-on-hook-thread-channel-control.md).

## Problem Statement

A user's keyboard or mouse switch degrades and **chatters**: one physical actuation registers as
several events within a few milliseconds (worn switches; the mouse double-click bug). The user
sees doubled letters, dropped game inputs, and accidental double-clicks. They want this fixed
silently in the background, without buying new hardware, without installing a driver, granting
admin, or trusting a closed-source tool that hooks their keyboard.

## Solution

**Bouncer** — a headless, tray-resident input debouncer for Windows that **suppresses** chatter on
the keyboard and mouse while **passing** all legitimate input, using driverless, no-admin Windows
low-level hooks. It is configurable (two thresholds), safe (panic hotkey, fail-open), observable
(suppressed counter + calibration histogram), and provably non-spyware (no network, nothing
keystroke-related on disk, open source).

Because the decision logic is a **pure Engine** (ADR-0001), it can be built and proven test-first
*before* any OS code exists — which is the spine of this PRD.

## User Stories

**Chatter suppression (the core promise)**

1. As a user with a worn keyboard switch, I want a repeated key-down that arrives faster than I
   could physically type to be **suppressed**, so that one press produces one character.
2. As a user, I want a key-down that arrives *after* the threshold has elapsed since that key's
   release to **pass**, so that deliberate fast typing is never eaten.
3. As a gamer holding a key, I want auto-repeat (downs with no intervening release) to always
   **pass**, so that held movement keys never stutter.
4. As a fast typist, I want rolling across *different* keys milliseconds apart to **pass**, so that
   the per-key timing of one key never affects another.
5. As a user, I want the suppressed chatter down's paired release to also be discarded, so that no
   orphan up event reaches my applications.
6. As a user with a failing mouse, I want a second button-down that arrives within the mouse
   threshold of the previous button-up to be **suppressed**, so that the double-click bug stops.
7. As a user, I want an intentional ~200 ms double-click to **pass**, so that real double-clicks
   still work.

**Tuning**

8. As a user, I want independent keyboard (default 30 ms) and mouse (default 40 ms) thresholds, so
   that each device class is tuned to its own human baseline.
9. As a user editing config by hand, I want any threshold above 100 ms clamped to 100 ms, so that a
   fat-fingered value can't black-hole my keyboard.

**Safety**

10. As a user whose setup feels wrong, I want a panic hotkey chord to instantly put Bouncer into
    **Panic** Mode (everything passes), so that I get my keyboard back immediately.
11. As a user, I want the panic chord to fire once per press (edge-triggered), so that holding it
    doesn't flip Panic on and off rapidly.
12. As a user, I want the panic chord's own keys consumed, so that triggering Panic doesn't leak
    `Ctrl+Alt+Shift+F12` into whatever app has focus.
13. As a user, I want Panic to never persist across reboot, so that I never boot stuck in emergency
    pass-through.
14. As a user, I want any unexpected/error state in the engine to default to **pass** (fail-open),
    so that a bug can never lock me out of my own input.
15. As a user rebinding the panic hotkey, I want binds without at least one modifier + one key
    rejected, so that I can't accidentally bind something triggerable by normal use.

**State & control**

16. As a user, I want to **Pause** Bouncer deliberately and have it stay paused across reboots, so
    that I can turn protection off when I want.
17. As a user, I want Diagnostic mode to only operate while Active, so that it never pretends to
    record while nothing is being suppressed.

**Observability & privacy**

18. As a user, I want a live count of suppressed events this session, so that I can see Bouncer
    earning its keep.
19. As a user, I want the counter to reset every boot and never touch disk, so that nothing about
    my typing is persisted.
20. As a user calibrating, I want a histogram of suppression gaps with my current threshold marked,
    so that I can see my chatter clusters safely below the threshold.
21. As a user, I want Diagnostic mode to be opt-in, visibly indicated, and to auto-expire after an
    hour (or clear on demand), so that it can never silently record.
22. As a privacy-conscious user, I want passed-through keystrokes to never be recorded, counted by
    identity, or transmitted, so that Bouncer is provably not a keylogger.

**Persistence & lifecycle**

23. As a user, I want my settings saved to a single readable config file and re-applied on launch,
    so that I configure once.
24. As a user, I want a corrupt or partial config to fall back to safe defaults rather than crash,
    so that the app always starts.
25. As a user, I want config changes applied live without a restart, so that tuning is immediate.
26. As a user, I want Bouncer to start on login (toggleable), so that protection is always on.
27. As a user, I want only one instance to run, so that input isn't double-filtered.
28. As a user, I want a confirmation before quitting (with "don't ask again"), so that I don't
    accidentally drop protection.

## Implementation Decisions

- **Architecture per ADR-0001:** a single pure **Engine** (`on_event(InputEvent) -> Outcome`) owns
  the `Debouncer`, `PanicDetector`, and current `Mode`, runs on the hook thread, and is called
  synchronously from the hook callback. No clock, no OS, no I/O inside the core.
- **Modules to build (deep, isolation-testable first):**
  - `Debouncer` — per-key/per-button release-anchored decision; input is `(InputEvent, thresholds)`,
    output is `Verdict::{Pass, Suppress}`. Holds `last_up` per key.
  - `PanicDetector` — tracks the live held-key set; emits a toggle when the configured chord becomes
    fully held (edge-triggered); signals that the triggering events are consumed.
  - `Engine` — composes the above + `Mode` gating; emits `Outcome { verdict, mode_change }`.
  - `Config` — TOML (de)serialization with defaulting + clamp (≤100 ms) applied on load.
  - `Stats` (UI-side) — consumes the `Report` stream into a session counter, a bounded ring buffer
    (~500 suppressed events), a gap histogram, and diagnostic expiry/clear.
  - `HookBackend` trait + `windows.rs` — OS glue: `WH_KEYBOARD_LL` / `WH_MOUSE_LL`, `Command`/
    `Report` channels, `PostThreadMessageW` wake, eviction watchdog. Production ignores injected
    events; a test-only mode processes them.
- **Mode model:** `Mode::{Active, Paused, Panic}` (Paused persisted, Panic never persisted) + an
  orthogonal `diagnostic: bool` overlay meaningful only while Active.
- **Thresholds:** two independent values (keyboard 30 ms, mouse 40 ms), clamped to `0..=100` on
  load and on UI change.
- **Control:** `Command` (SetThresholds, SetMode, SetDiagnostic, RebindPanic, Shutdown) and
  `Report` (Suppressed{device,key,gap_ms}, ModeChanged, HookEvicted, HookReinstalled) cross threads
  by channel only — no shared lock on the input path.
- **Injected-event policy:** events flagged injected (`LLKHF_INJECTED`) pass straight through in
  production; the integration-test build processes them so `SendInput` can drive end-to-end tests.

## Testing Decisions

**What makes a good test here:** assert **external behavior**, not internals. For the pure modules
that means feeding a synthetic sequence of `InputEvent`s (with **injected timestamps** — the core
never reads a clock) and asserting the returned `Verdict` / `Outcome` / emitted `Report`s. Tests
must not reach into private state or assert on `HashMap` contents. Time is a test input, never a
`sleep`.

**Modules under test (unit, TDD red-green-refactor):**

1. **Debouncer**
   - down at `t` where `t - last_up[K] < threshold` → **Suppress**.
   - down at `t` where `t - last_up[K] >= threshold` → **Pass**.
   - boundary: gap exactly `== threshold` → **Pass** (threshold is exclusive).
   - held-key auto-repeat (downs, no intervening up) → always **Pass**.
   - two different keys milliseconds apart → **both Pass** (per-key isolation).
   - suppressing a down also discards its paired up (no orphan up).
   - keyboard vs mouse use their own threshold (30 vs 40 defaults).
   - clamp: a threshold passed in above 100 behaves as 100.
   - first-ever down for a key (no prior up recorded) → **Pass**.
2. **PanicDetector**
   - all chord keys become held → emits **toggle once**.
   - chord held continuously → **no second toggle** (edge-triggered).
   - release one chord key then re-press the full chord → toggles **again**.
   - partial chord (subset held) → **no toggle**.
   - triggering events reported as **consumed**.
   - rebind validation: bind with no modifier, or a single key, is **rejected**.
3. **Engine** (`on_event`)
   - while `Active`: delegates to Debouncer (suppress/pass as above).
   - while `Paused`: **every** event passes; nothing suppressed.
   - while `Panic`: **every** event passes; nothing suppressed.
   - panic chord while Active → `Outcome.mode_change == Some(Panic)`.
   - panic chord while Panic → `mode_change == Some(Active)` (toggle back).
4. **Fail-open property test** (`proptest` or similar)
   - over arbitrary event sequences, `on_event` never panics; and for any state the engine cannot
     classify as certain chatter, the verdict is **Pass**. (The safety invariant, as a property.)
5. **Config**
   - round-trip: serialize → deserialize → equal.
   - missing file → defaults.
   - corrupt/partial TOML → defaults (no panic); unknown keys ignored; missing keys defaulted.
   - `threshold = 9999` on load → clamped to 100.
6. **Stats** (UI-side)
   - one `Report::Suppressed` → counter for that device increments by one.
   - ring buffer caps at capacity (oldest evicted, newest retained).
   - histogram buckets a stream of gaps into the expected bins.
   - diagnostic expiry clears the buffer; manual clear empties it; counter is unaffected by clear.

**Integration test (the one shell test that matters), `SendInput`-driven, Windows-only:**

7. With the hook installed in test-only mode (processes injected events):
   - inject `down, up, down` where the 2nd down is 5 ms after the up → the 2nd down is **not**
     observed downstream (suppressed end-to-end).
   - inject two presses spaced 200 ms apart → **both** observed (passed).
   - inject a mouse `down,up,down,up` double-click 8 ms apart → second click **suppressed**;
     250 ms apart → **both** pass.

**Prior art:** none in-repo yet (greenfield). The pure-core tests follow the standard Rust
`#[cfg(test)]` module + table-driven cases convention; property tests use `proptest`. The
integration test lives under `tests/` and is gated to Windows.

**Coverage:** `cargo-llvm-cov` is meaningful only on the pure core (modules 1–6); the shell
(module 7 target) is covered by the single integration test, not line coverage.

## Out of Scope

- Per-device keyboard selection (needs a kernel driver); controllers/gamepads (need a virtual
  device); macOS/Linux backends (trait ready, impls deferred).
- Code signing; installer/MSI; auto-update; any network code or telemetry.
- Per-key thresholds; exporting diagnostic data.
- UI snapshot/pixel testing of the egui window and tray (verified manually per DESIGN.md §9).

## Further Notes

- The build order (DESIGN.md §12) front-loads a throwaway **Spike-0** hook proof to de-risk the OS
  mechanism, then **M2 (pure core, TDD)** before any production OS code — this PRD's modules 1–4 are
  exactly M2's deliverable and should be fully green before M3 (the keyboard backend) begins.
- Test 7 (the `SendInput` integration test) is the acceptance gate for M3/M4 and proves the
  injected-event policy works in both directions (ignored in production, processed under test).
