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
