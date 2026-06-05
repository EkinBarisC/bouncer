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

### Module layout (proposed)

```
bouncer/
├── Cargo.toml
├── DESIGN.md                 ← this file
├── README.md
├── .github/workflows/ci.yml  ← fmt --check, clippy -D warnings, tests
├── deny.toml                 ← cargo-deny config
└── src/
    ├── main.rs               ← wiring: spawn hook thread + UI thread, single-instance mutex
    ├── core/                 ← PURE, no OS, fully unit-tested
    │   ├── mod.rs
    │   ├── debouncer.rs       ← decide(event) -> Verdict ; per-key release-anchored state
    │   ├── event.rs           ← InputEvent { device, code, kind, timestamp_ms }
    │   └── verdict.rs         ← Verdict::{Pass, Suppress}
    ├── config.rs             ← TOML load/save, defaults, clamp
    ├── stats.rs              ← in-memory counter + diagnostic ring buffer
    ├── platform/             ← imperative shell, per-OS behind a trait
    │   ├── mod.rs             ← trait HookBackend { install, uninstall, set_paused, ... }
    │   └── windows.rs         ← WH_KEYBOARD_LL / WH_MOUSE_LL impl + eviction watchdog
    └── ui/
        ├── tray.rs
        └── settings.rs        ← egui window
```

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

1. **Global panic hotkey** → engine flips to full pass-through (hooks stay installed, every
   event passes). Checked **first** in the hot path. Default: an obscure-but-memorable combo,
   **rebindable**. Keyboard-based (never depends on the mouse, since mouse may be the bug).
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
panic_hotkey          = "Ctrl+Alt+Shift+Pause"  # rebindable
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

## 12. Open / soft areas (candidates for further grilling)

- Exact egui settings-window layout.
- Precise cross-platform `HookBackend` trait shape (what the shell must expose).
- Default panic-hotkey combo (currently a placeholder `Ctrl+Alt+Shift+Pause`).
- Tray icon states (enabled vs paused vs diagnostic-active) and tooltip content.
- v1 task breakdown / milestone ordering for implementation.
