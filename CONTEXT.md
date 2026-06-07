# Bouncer

Bouncer is a headless, tray-resident input debouncer for Windows. It watches the keyboard and
mouse event stream and removes "chatter" — repeat events that arrive faster than a human could
physically produce them — while leaving all legitimate input untouched.

## Language

### Core concept

**Chatter**:
A repeat input event caused by a degrading physical switch (or the mouse double-click bug)
that arrives faster than a human could produce it. The thing Bouncer exists to eliminate.
_Avoid_: bounce (the electrical cause), stutter, ghosting, double-fire.

**Debounce**:
The act of removing chatter from the input stream. The product category Bouncer belongs to.

**Suppress**:
To remove a single event from the input stream so it never reaches any other application.
This is the one canonical verb for what Bouncer does to a chatter event.
_Avoid_: swallow, drop, block, eat, filter, kill.

**Pass** / **Pass-through**:
To let an event continue unchanged to the rest of the system. The opposite of suppress.
"Pass-through" also names the whole-stream condition where every event passes (i.e. Paused or
Panic mode).
_Avoid_: allow, forward, let through.

### State model

Bouncer has one primary **Mode** (exactly one at a time) plus one orthogonal overlay.

**Active**:
The normal Mode. Hooks installed, chatter is suppressed.

**Paused**:
A Mode entered by deliberate user toggle. Every event passes. **Persisted** across reboots
(survives restart). Signalled calmly in the UI (neutral icon).
_Avoid_: disabled, off, stopped.

**Panic**:
An emergency pass-through Mode entered by the panic hotkey when something feels wrong — "give
me my keyboard back now." Every event passes. **Not persisted** (Bouncer always boots out of
Panic). Latches until manually cleared. Signalled loudly in the UI (alarming/red icon). Takes
precedence over all other state.
_Avoid_: kill switch (that names the hotkey, not the resulting state), disabled.

**Diagnostic mode**:
An orthogonal overlay (a boolean, not a Mode) meaningful only while Active. While on, Bouncer
records suppressed events into an in-memory ring buffer to drive threshold calibration.
Opt-in, visibly indicated, auto-expiring.
_Avoid_: debug mode, logging mode, capture mode.

### Tuning

**Threshold**:
The per-device-class time window below which a repeat event is judged to be chatter. Two
independent values: keyboard (default 30 ms) and mouse (default 40 ms). Hard-capped at 100 ms.

**Release-anchored**:
The rule that chatter is measured from a key/button's previous *release* (up event), not from
its previous press. A down arriving less than the threshold after the same key's up is chatter.
This is what makes held-key auto-repeat immune to suppression.

**Panic hotkey**:
The user-rebindable global key **chord** that toggles **Panic** Mode (distinct from the Panic
state it triggers). Default `Ctrl+Alt+Shift+F12`. Detected when all its keys are held
simultaneously; edge-triggered (flips once per chord, not while held); its triggering events are
suppressed so they don't leak to the foreground app. Rebinds must include at least one modifier
plus one non-modifier key.

### Decision pipeline

**Engine**:
The single pure decision unit. Composes the `Debouncer` + `PanicDetector` + the current `Mode`
into one synchronous call, `on_event(InputEvent) -> Outcome`, invoked from the hook callback.
Holds all decision state; has no OS, clock, or I/O (the timestamp arrives on the event). See
`docs/adr/0001`.

**Verdict**:
What to do with one event: `Pass` or `Suppress`. The shell maps `Suppress` to "swallow" (return 1
from the hook) and `Pass` to "let through" (return 0).

**Outcome**:
The result of one `Engine::on_event`: the event's `Verdict` plus an optional `mode_change` (e.g.
the panic chord toggling `Panic`). The shell uses `mode_change` to update the tray and emit a
`ModeChanged` report.

**Mode gating**:
The rule the Engine applies on top of the Debouncer: while **Active** it delegates to the
Debouncer; while **Paused** or **Panic** every event passes (nothing suppressed). The panic chord
is honoured in every Mode and toggles `Panic`.

**Fail-open**:
The inviolable safety invariant: any state the Engine cannot positively classify as chatter
resolves to `Pass`, and `on_event` must never panic. A bug must never be able to lock the user
out of their own input (DESIGN.md §6, D8).

## Example dialogue

> **Dev:** When the user hits the panic hotkey, do we tear down the hooks?
>
> **Domain:** No — we enter **Panic** Mode. The hooks stay installed, but every event **passes**.
> We never suppress in Panic.
>
> **Dev:** Same as **Paused**, then?
>
> **Domain:** Functionally yes, both are pass-through. But **Paused** is a deliberate toggle and
> it's persisted — reboot and you're still paused. **Panic** is an emergency and is *never*
> persisted; we always boot into **Active** (or Paused), never Panic.
>
> **Dev:** And if **Diagnostic mode** is on and the user pauses?
>
> **Domain:** Diagnostic only records **suppressed** events, and Paused suppresses nothing, so
> there's nothing to record. Diagnostic is an overlay on **Active** — it's dormant while Paused.

## Build status

How we build: **TDD, one issue → one branch → one PR.** Each slice lands as a RED `test:` commit
(reviewed before implementation) then a GREEN `feat:` commit; the maintainer merges. Heavy deps
are introduced only by the slice that needs them. Roadmap and milestones live in DESIGN.md §12 and
the GitHub issues.

Merged (pure core is essentially complete):

- **#1 Spike** — `WH_KEYBOARD_LL` no-driver hook proof, validated on real hardware (closed).
- **#2 Scaffolding** — lib+bin skeleton, core types, CI (fmt/clippy/test), `deny.toml`.
- **#3 Debouncer** — per-key, release-anchored chatter decision (`core/debouncer.rs`).
- **#4 PanicDetector** — edge-triggered chord detection + rebind validation (`core/panic.rs`).
- **#6 Config** — defensive TOML load/save with defaults + threshold clamp (`config.rs`); adds
  `serde` + `toml`.
- **CLAUDE.md** — behavioral coding guidelines for contributors.

In flight:

- **#5 Engine** — composes Debouncer + PanicDetector + Mode into `Engine::on_event`, with the
  fail-open property test (`proptest`). The last pure-core slice.

Next up:

- **#7 Windows keyboard backend** — first real end-to-end suppression via `WH_KEYBOARD_LL` +
  a `SendInput`-driven integration test; introduces the `windows` crate. Then the mouse backend,
  the channel wiring (`Command`/`Report`), tray, and egui settings window.
