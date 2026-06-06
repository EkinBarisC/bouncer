# Bouncer — Design & Spec (v1)

> **Bouncer** is a headless, tray-resident **input debouncer** for Windows that suppresses
> keyboard *and* mouse "chatter" — repeat events that arrive faster than a human could
> physically produce them (worn switch bounce, the mouse double-click bug).
>
> The name is a double meaning: a club *bouncer* rejects unwanted entries at the door, and
> the app *de-bounces* the input stream.

**Status:** design locked, implementation not started.
**Platform:** Windows-only for v1 (cross-platform architecture in place, other OSes deferred).
**Language:** Rust. **UI:** egui/eframe. **License/source:** open source.

---

## 1. Problem statement

A physical key or mouse switch degrades over time and "chatters": a single physical
actuation bounces and registers as multiple `down/up` cycles within a few milliseconds.
A human produces at most one `down→up` per ~80–120 ms for the *same* key; chatter fires in
~5–25 ms. Bouncer sits in the OS input path, recognises these physically-impossible repeats,
and swallows them — while never touching legitimate input.

Reframe: this is not a "keyboard chatter fixer," it's an **input debouncer** — one engine,
two device classes (keyboard + mouse), one timing-based algorithm.

---

## 2. Scope

### In scope (v1)
- Keyboard chatter suppression via `WH_KEYBOARD_LL`.
- Mouse chatter / double-click-bug suppression via `WH_MOUSE_LL`.
- Global filtering (no per-device selection — see Decision D2).
- Tray-resident headless process, egui settings window on demand.
- Autostart-on-login (toggleable), single-instance.
- Global panic hotkey (full pass-through escape), rebindable.
- In-memory suppressed-event counter + opt-in diagnostic calibration view.
- TOML config persistence (the only disk artifact).

### Explicitly OUT of v1 (deferred)
- **Per-device keyboard selection** — would require a kernel filter driver (admin, signing,
  anti-cheat risk). Global timing-based filtering is sufficient because chatter is defined by
  *inhuman timing* regardless of source device.
- **Controllers / gamepads** — not routed through `WH_*_LL`; would need a virtual-device
  driver (e.g. ViGEm) + re-emission. Also, the common controller defect (stick drift) is an
  *analog* problem, not a discrete double-fire you can debounce.
- **macOS / Linux implementations** — architecture is ready behind the hook trait, but not built.
  (macOS: `CGEventTap`; Linux: `evdev` + `uinput` virtual device.)
- **Code-signing certificate** — no budget for v1; SmartScreen warning documented instead.
- **Per-key thresholds** — single global threshold per device class is enough.
- **Installer / MSI** — portable single `.exe` instead.
- **Auto-update / telemetry / any network code** — deliberately none (privacy + trust).

---

## 3. Key design decisions (decision log)

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | Windows-only v1, cross-platform later | `WH_*_LL` hooks are a clean, no-driver, no-admin path; per-OS impls hide behind a trait. |
| D2 | Global filtering, **no** per-device selection | `WH_*_LL` can't identify source device; per-device needs a kernel driver. Timing alone catches chatter regardless of source. |
| D3 | Keyboard **+ mouse** in scope; controllers out | Mouse double-click bug uses the identical `WH_MOUSE_LL` architecture (nearly free). Controllers need a virtual-device driver. |
| D4 | Release-anchored, per-key debounce; default kb 30 ms, mouse 40 ms; clamp ≤100 ms | Real chatter <25 ms; fastest legit same-key repeat ~60 ms. Comfortable gap. Two thresholds because kb/mouse baselines differ. |
| D5 | Rust + egui, single binary | Zero-GC hot path (prevents Windows silently dropping a slow hook), tiny always-on footprint, clean cross-platform, light UI for a tray tool. |
| D6 | TDD the pure core; manual + injection test the shell; **inject the clock** | Functional core / imperative shell. Engine must never call `Instant::now()` — time is a parameter, or tests become flaky races. |
| D7 | Single tray-resident process, GUI on demand, autostart-on-login, single-instance | Headless by default; hooks need an interactive desktop + message loop (not a session-0 service). |
| D8 | Defense-in-depth safety: panic hotkey, fail-open, eviction detect+reinstall, threshold clamp | The app swallows input; a bug must never be able to lock the user out. Fail-open is an inviolable invariant. |
| D9 | Observability: in-memory counter (resets every boot) + opt-in auto-expiring diagnostic ring buffer | Proof-of-value + threshold calibration without becoming a keylogger. |
| D10 | Privacy invariants (see §7) | A global hook is behaviourally indistinguishable from spyware; trust must be designed in and provable. |
| D11 | TOML config in `%APPDATA%\Bouncer\config.toml`; defensive load | The single disk artifact; contains only settings, zero sensitive data. |
| D12 | Production **ignores injected events**; test-only mode processes them | Chatter is physical; injected events (remappers, macro tools) are never chatter. Test mode enables `SendInput`-driven integration tests. |
| D13 | Distribution: unsigned portable `.exe` + GitHub Releases + SHA256; signing deferred | No budget for certs. Open source + "build it yourself" + README disclosure mitigate the SmartScreen speed-bump. |

---

## 4. Architecture

Bouncer is **two threads** plus a **pure core**, following functional-core / imperative-shell.

```
                                   BOUNCER (single process)
 ┌─────────────────────────────────────────────────────────────────────────────┐
 │                                                                               │
 │   HOOK THREAD (imperative shell, hot path)        UI / TRAY THREAD            │
 │  ┌──────────────────────────────────────┐        ┌──────────────────────┐    │
 │  │ message loop: GetMessage()           │        │ system tray icon     │    │
 │  │                                      │        │  • Enable / Pause    │    │
 │  │ WH_KEYBOARD_LL ─┐                    │        │  • Settings…         │    │
 │  │ WH_MOUSE_LL    ─┤                    │        │  • Quit              │    │
 │  │                 ▼                    │        │                      │    │
 │  │   ┌────────────────────────────┐     │        │ egui window (on      │    │
 │  │   │  PANIC HOTKEY check FIRST  │     │        │ demand):             │    │
 │  │   │  (pass-through if engaged) │     │        │  • kb/mouse sliders  │    │
 │  │   └───────────┬────────────────┘     │        │  • enable/pause      │    │
 │  │               ▼                      │        │  • autostart toggle  │    │
 │  │   ┌────────────────────────────┐     │        │  • panic-hotkey bind │    │
 │  │   │   Debouncer (PURE CORE)    │     │        │  • live counter      │    │
 │  │   │   verdict = decide(event)  │     │        │  • diagnostic mode + │    │
 │  │   │   → Pass | Suppress        │     │        │    gap histogram     │    │
 │  │   │   (fail-open on any doubt) │     │        └──────────┬───────────┘    │
 │  │   └───────────┬────────────────┘     │                   │                │
 │  │      Pass → return 0 (let through)   │                   │                │
 │  │      Suppress → return 1 (swallow)   │                   │                │
 │  └───────────────┬──────────────────────┘                   │                │
 │                  │                                           │                │
 │                  │   stats / suppressed events (mpsc) ──────►│                │
 │                  │◄──── config updates, pause cmd (mpsc) ────┤                │
 │                  │                                           │                │
 │   eviction watchdog → reinstall hook if Windows drops it     │                │
 │                                                              ▼                │
 │                                                  config.toml (%APPDATA%)      │
 └─────────────────────────────────────────────────────────────────────────────┘

 KEY PROPERTIES
  • Hook callback is allocation-free and fast → Windows never evicts it for timeout.
  • Panic check happens BEFORE any suppression logic → escape works even if engine misbehaves.
  • Channels decouple UI from the input path → GUI can never stall keystrokes.
  • Process death = Windows tears down hooks automatically → a crash is SAFE (keyboard restored).
  • Injected events (LLKHF_INJECTED) pass straight through in production.
```

### Core/shell boundary (see ADR-0001)

All decision logic is a single **pure `Engine`** on the hook thread, called synchronously from
the hook callback. The shell is dumb glue; presentation state lives on the UI side; threads talk
only over `Command`/`Report` channels (never a shared lock).

```
Engine::on_event(event: InputEvent) -> Outcome { verdict: Pass | Suppress, mode_change: Option<Mode> }
    Engine owns: Debouncer (per-key release-anchored state) + PanicDetector (combo match) + Mode.
    Pure: no OS, no clock (timestamp arrives on the event), no I/O → fully unit-testable.

trait HookBackend {
    fn run(self, engine: Engine,
           commands: Receiver<Command>,
           reports:  Sender<Report>) -> Result<()>;   // blocks; runs OS message loop until Shutdown
}

Command  (UI → hook thread): SetThresholds | SetMode | SetDiagnostic | RebindPanic | Shutdown
Report   (hook thread → UI): Suppressed { device, key, gap_ms } | ModeChanged(Mode)
                             | HookEvicted | HookReinstalled
```

The counter, diagnostic ring buffer, and histogram are **UI-side state** built from the `Report`
stream — not in the Engine. Only `windows.rs` knows `WH_*_LL`, virtual-key codes, and the
`PostThreadMessageW` that wakes the blocking `GetMessage` loop when a `Command` arrives.

### Module layout (proposed)

```
bouncer/
├── Cargo.toml
├── DESIGN.md                 ← this file
├── README.md
├── CONTEXT.md                ← glossary (canonical domain terms)
├── docs/adr/                 ← architecture decision records
├── .github/workflows/ci.yml  ← fmt --check, clippy -D warnings, tests
├── deny.toml                 ← cargo-deny config
└── src/
    ├── main.rs               ← wiring: single-instance mutex, spawn hook + UI threads, channels
    ├── core/                 ← PURE, no OS, fully unit-tested
    │   ├── mod.rs
    │   ├── engine.rs          ← Engine::on_event(event) -> Outcome ; owns Debouncer+PanicDetector+Mode
    │   ├── debouncer.rs       ← per-key release-anchored chatter decision
    │   ├── panic.rs           ← PanicDetector: matches configured combo against live key state
    │   ├── event.rs           ← InputEvent { device, code, kind, timestamp_ms }
    │   ├── mode.rs            ← Mode::{Active, Paused, Panic}
    │   └── verdict.rs         ← Verdict::{Pass, Suppress}, Outcome
    ├── messages.rs           ← Command / Report enums (platform-agnostic)
    ├── config.rs             ← TOML load/save, defaults, clamp
    ├── stats.rs              ← UI-side: counter + diagnostic ring buffer + histogram (from Reports)
    ├── platform/             ← imperative shell, per-OS behind HookBackend
    │   ├── mod.rs             ← trait HookBackend
    │   └── windows.rs         ← WH_KEYBOARD_LL / WH_MOUSE_LL impl + eviction watchdog + msg loop
    └── ui/
        ├── tray.rs
        └── settings.rs        ← egui window
```

### Tray surface

The tray is a projection of the Mode enum + diagnostic overlay.

- **Four visual states:** Active (normal/colored), Active+Diagnostic (normal + recording badge —
  diagnostic must always be visibly indicated), Paused (greyed/muted), Panic (red/alert).
- **Click behavior:** left-click → open Settings window; right-click → context menu.
- **Context menu (state-aware):** Pause/Resume (label flips with Mode; in Panic it clears Panic),
  Diagnostic mode (checkable; greyed while Paused), Settings…, Quit.
- **Tooltip = live status line:** Mode + session counter, and in non-Active modes how to recover,
  e.g. `Bouncer — PANIC (pass-through) · press Ctrl+Alt+Shift+F12 to resume`.
- **Quit confirmation:** "Quit Bouncer? Chatter protection will stop." with a **"Don't ask again"**
  checkbox (persisted as `confirm_on_quit`). Default: ask.

### Settings window (egui)

One small (~420px), non-modal, resizable window; opening it never changes Mode. Four labeled
groups top-to-bottom (no in-app trust banner — that lives in the README):

- **Status** — colored dot for current Mode + primary **Pause/Resume** button; live session
  suppressed counts (keyboard · mouse), fed from the `Report` stream.
- **Tuning** — per device class: an enable checkbox (`debounce_keyboard` / `debounce_mouse`) + a
  0–100 ms slider with the numeric `ms` value shown.
- **Diagnostics** — Diagnostic-mode toggle (greyed while Paused; shows expiry countdown when on);
  a **histogram of suppression gaps with a vertical line marking the current threshold** (makes
  calibration visual); a **Clear** button.
- **Settings** — Start-on-login checkbox; Panic-hotkey display + **Rebind** (capture state that
  validates ≥1 modifier + 1 key before accepting); Confirm-on-quit checkbox.

---

## 5. Core algorithm (the pure `Debouncer`)

State: `last_up_ms: HashMap<KeyId, u64>` (per key code / per mouse button).

```
on event (key/button K, kind, t):
    if kind == Up:
        record last_up_ms[K] = t
        return Pass                       # never suppress an up
    if kind == Down:
        if K in last_up_ms and (t - last_up_ms[K]) < threshold(device):
            return Suppress               # chatter: down too soon after this key's up
        else:
            return Pass
    # auto-repeat downs arrive with NO intervening up → last_up_ms unchanged
    # → naturally Pass (held keys / game key-hold never suppressed)
```

Invariants (each a test):
- Down < threshold after same-key Up → **Suppress**.
- Down ≥ threshold after same-key Up → **Pass**.
- Held-key auto-repeat (downs, no ups) → **always Pass**.
- Two *different* keys close together → **both Pass** (per-key state).
- Intentional ~200 ms mouse double-click → **Pass**; ~8 ms double-click → second **Suppress**.
- Any unknown/error state → **Pass** (fail-open).
- Suppressing a Down also discards its paired Up (no orphan up reaches the app).

`threshold(device)` = `keyboardThresholdMs` (def 30) or `mouseThresholdMs` (def 40), clamped ≤100.

---

## 6. Safety (defense-in-depth)

1. **Global panic hotkey** → Engine flips to **Panic** Mode (full pass-through; hooks stay
   installed, every event passes). Checked **first** in the hot path. Default
   **`Ctrl+Alt+Shift+F12`** (universal keys — no Pause/ScrollLock that laptops lack), rebindable.
   Detection is a **simultaneous chord**, **edge-triggered** (flips once per chord, not while
   held), and the triggering events are **consumed** (not leaked to the foreground app). Rebinds
   are validated to require ≥1 modifier + 1 non-modifier key. Keyboard-based (never depends on
   the mouse, since the mouse may be the bug).
2. **Fail-open invariant** — suppress only when *certain*; any doubt/error → Pass. Tested.
3. **Hook-eviction detection + auto-reinstall** — if Windows drops a slow/evicted hook, detect
   and reinstall; never silently die.
4. **Threshold clamp ≤100 ms** — applied on config load too, so a hand-edited `threshold=9999`
   can't black-hole the keyboard.
5. **Crash = safe** — process death makes Windows tear down the hooks; keyboard restored.

---

## 7. Observability & privacy

### Observability
- **In-memory suppressed counter**, per device class, shown in UI + tray tooltip.
  **Resets every boot** — nothing persisted.
- **Opt-in diagnostic mode** (auto-expires after 1 hour): an in-memory **ring buffer**
  (~last 500 *suppressed* events) `(key, gap_ms, timestamp)` driving a live **suppression-gap
  histogram** for threshold calibration. Cleared on timeout **or** via a manual UI "Clear"
  button. **No export.**

### Privacy invariants (write these in the README, loud)
1. **Passed-through keystrokes are never recorded, stored, counted-by-identity, or transmitted.**
2. **Only *suppressed* events may carry identity** (about a failing physical switch, the
   legitimate purpose). A suppressed-only log is sparse random switch-bounce — physically
   incapable of reconstructing typed text.
3. **No network code at all** in v1 — no telemetry, no update check, no socket. Easier to
   *prove* "not a keylogger" when there's nothing to phone home with.
4. **Nothing keystroke-related ever touches disk.** Counter + diagnostic buffer are memory-only.
5. **Diagnostic mode is opt-in, visibly indicated, and auto-expires.**
6. **Open source** — the strongest "I am not a keylogger" proof is an auditable public repo.

---

## 8. Persistence

Exactly one disk artifact: `config.toml` at `%APPDATA%\Bouncer\config.toml` (via the
`directories` crate, so macOS/Linux paths come free later).

```toml
# Bouncer config — settings only, no sensitive data.
keyboard_threshold_ms = 30   # clamped 0..=100
mouse_threshold_ms    = 40   # clamped 0..=100
enabled               = true # false = paused (pass-through)
debounce_keyboard     = true
debounce_mouse        = true
autostart             = true
panic_hotkey          = "Ctrl+Alt+Shift+F12"    # rebindable; ≥1 modifier + 1 key
confirm_on_quit       = true                    # "Don't ask again" clears this
```

Defensive load: missing/corrupt/partial file → safe defaults (and rewrite a clean file);
unknown keys ignored; missing keys defaulted; clamps applied on load. UI changes write config
**and** push new values to the hook thread over the channel immediately (no restart).

---

## 9. Testing strategy

- **Pure core (`src/core`)** — TDD, exhaustive unit tests over synthetic event streams with
  injected timestamps. Property test the fail-open invariant. This is where coverage matters
  (`cargo-llvm-cov`).
- **Shell (`src/platform/windows.rs`)** — `SendInput`-driven integration tests in a
  **test-only mode** that processes injected events (production ignores them). Inject a
  `down,up,down` chatter pattern (2nd down 5 ms after up) and assert the 2nd down is swallowed
  (never reaches a focused test field / downstream observer).
- **Manual** — tray behaviour, autostart, panic hotkey, eviction/reinstall.

---

## 10. Tooling

- `rustfmt` (CI `--check`)
- `clippy` with `-D warnings`
- `rust-analyzer` (editor LSP)
- `cargo-nextest` (test runner)
- `cargo-deny` / `cargo-audit` (deps, licenses, vulns)
- GitHub Actions CI: fmt-check + clippy + tests on every push
- `cargo-llvm-cov` (coverage on the pure core)

---

## 11. Distribution

- Single self-contained **portable `.exe`** (Rust static link; no installer, no runtime dep).
- **GitHub Releases** + **SHA256 checksums** + prebuilt exe + source.
- **Unsigned** for v1 — README documents the Windows SmartScreen "unrecognized app" warning
  and gives `cargo build --release` "build it yourself" instructions.
- README banner: **"no admin · no driver · no network · suppressed-events-only · open source."**
- The hook approach needs **no elevation and no driver** — a real AV-friendliness + trust win.

---

## 12. v1 roadmap (milestones)

Vertical-slice ordering. The hook is the riskiest unknown, so a throwaway spike de-risks it
*before* the core is built around assumptions.

| # | Milestone | Goal / exit criteria |
|---|-----------|----------------------|
| **Spike-0** | Hook proof (throwaway) | Minimal `WH_KEYBOARD_LL` that suppresses every 2nd press of one key — *prove* install + return-1 + actual swallow works on Win11. Discarded after. |
| **M1** | Scaffolding | Cargo project, module skeleton, CI (fmt-check + clippy `-D warnings` + tests), `deny.toml`. Core types compile. |
| **M2** | Pure core (TDD) | `Debouncer`, `PanicDetector`, `Mode` gating, `Engine::on_event`, fully unit-tested incl. fail-open property test. **Zero OS code.** |
| **M3** | Windows keyboard backend | `HookBackend` + `windows.rs`: real `WH_KEYBOARD_LL` → Engine, `Command`/`Report` channels, `PostThreadMessageW` wake. Headless, threshold hardcoded. `SendInput` integration test proves end-to-end suppression. |
| **M4** | Mouse backend | Add `WH_MOUSE_LL` (double-click-bug) through the same path. |
| **M5** | Tray + Mode control | Tray icon (4 states), menu, Pause/Resume, Quit confirm, panic hotkey end-to-end (chord → consume → toggle Panic). |
| **M6** | Settings window + config | egui layout, TOML load/save (defensive + clamp), live-apply over channels, autostart-on-login, rebind capture. |
| **M7** | Diagnostics | Ring buffer, gap histogram, diagnostic toggle + 1 hr auto-expire. |
| **M8** | Hardening | Eviction watchdog + reinstall, single-instance mutex, defensive config edge cases, fail-open audit. |
| **M9** | Packaging | Portable release `.exe`, GitHub Release + SHA256, README finalize, pick a license. |

> All design soft areas from the initial grilling are now resolved (state model, core/shell
> boundary [ADR-0001], panic hotkey, tray surface, settings layout, roadmap). See `CONTEXT.md`
> for the glossary and `docs/adr/` for recorded decisions.
