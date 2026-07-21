# Ubiquitous Language

The canonical vocabulary for **Bouncer**, a headless input debouncer for Windows.
Use these terms exactly — in code, commits, UI copy, and conversation.

## Core phenomenon

| Term                | Definition                                                                                              | Aliases to avoid                          |
| ------------------- | ------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| **Chatter**         | Repeat down/up events from one physical switch arriving faster than a human can produce (worn switch, mouse double-click bug) | bounce, glitch, double-fire, ghosting     |
| **Double-click bug**| Mouse chatter specifically — one physical click registering as two. *A subtype of chatter, not its own thing.* | (don't treat as separate from chatter)    |
| **Legitimate input**| Any event a human actually intended; must always pass untouched                                         | real input, valid input, good input       |

## The decision (pure core)

| Term              | Definition                                                                                          | Aliases to avoid                       |
| ----------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------- |
| **Engine**        | The pure core that decides each event's fate; owns the Debouncer, PanicDetector, and Mode           | core, brain, processor                 |
| **Debouncer**     | Per-key, release-anchored chatter logic inside the Engine (`last_up_ms` lookup vs threshold)         | filter, deduper                        |
| **Verdict**       | The Engine's ruling on one event: **Pass** or **Suppress**                                           | decision, result, action               |
| **Suppress**      | Drop a chatter event so it never reaches the foreground app (`return 1` from the hook)               | swallow, block, filter, eat, reject    |
| **Pass**          | Let an event through untouched (`return 0`)                                                          | allow, accept, forward, let through    |
| **Outcome**       | The Engine's full reply: a Verdict plus an optional Mode change                                      | response, output                       |
| **Fail-open**     | The inviolable safety rule: suppress only when *certain*; any doubt or error → Pass                  | fail-safe, graceful degrade            |

## Events & timing

| Term             | Definition                                                                                       | Aliases to avoid              |
| ---------------- | ------------------------------------------------------------------------------------------------ | ----------------------------- |
| **InputEvent**   | The platform-agnostic unit the Engine sees: `{ device, code, kind, timestamp_ms }`               | event struct, signal          |
| **Device class** | Keyboard *or* mouse — the two categories Bouncer debounces, each with its own threshold           | device type, input type       |
| **KeyId**        | The identity a Debouncer keys its state on: a keyboard key code *or* a mouse button               | key (when ambiguous), button  |
| **Down / Up**    | The two event kinds; only a Down can ever be suppressed, an Up always passes                       | press/release (mixed freely)  |
| **Threshold**    | Minimum human-possible gap (ms) after a key's Up before its next Down counts as real; kb 30, mouse 40, clamped ≤100 | delay, debounce time, window  |
| **Release-anchored** | The timing model: the gap is measured from the *last Up* of that key, not the last Down       | (no synonym — state the model) |
| **Injected event** | An event synthesized by software (remapper, macro tool, `SendInput`); never chatter, so production always passes it | fake event, synthetic input   |

## Modes & control

| Term       | Definition                                                                                          | Aliases to avoid                |
| ---------- | --------------------------------------------------------------------------------------------------- | ------------------------------- |
| **Mode**   | The Engine's operating state: **Active**, **Paused**, or **Panic**                                  | state, status (when imprecise)  |
| **Active** | Normal operation — Bouncer is suppressing chatter                                                   | enabled, on, running            |
| **Paused** | User-chosen rest — all events pass through; Bouncer is idle but installed                            | disabled, off, stopped          |
| **Panic**  | Emergency escape Mode — full pass-through triggered by the panic hotkey                              | bypass, safe mode               |
| **Pass-through** | The *behavior* of letting every event through. Both Paused and Panic are pass-through Modes; the term names what happens, not which Mode | passthrough, transparent        |
| **Panic hotkey** | The rebindable chord (default `Ctrl+Alt+Shift+F12`) that flips the Engine to Panic Mode; checked first in the hot path | kill switch, escape key, hotkey |
| **Chord**  | A simultaneous, edge-triggered key combination (≥1 modifier + 1 key) — how the panic hotkey is detected | combo, shortcut, sequence       |
| **PanicDetector** | The pure-core component matching the configured chord against live key state                 | hotkey handler, combo matcher   |

## Platform & resilience

| Term            | Definition                                                                                       | Aliases to avoid           |
| --------------- | ------------------------------------------------------------------------------------------------ | -------------------------- |
| **Hook**        | A Windows low-level input hook (`WH_KEYBOARD_LL` / `WH_MOUSE_LL`) — Bouncer's tap into the input path | listener, interceptor, tap |
| **Hook thread** | The imperative-shell thread running the message loop and hook callbacks (the hot path)            | worker, input thread       |
| **Eviction**    | Windows silently dropping a hook it judged too slow                                               | timeout, drop, disconnect  |
| **Watchdog**    | The mechanism that detects eviction and reinstalls the hook                                       | monitor, healthcheck       |
| **HookBackend** | The per-OS trait the platform layer implements (Windows hooks, Linux grab+replay); the seam for future macOS support | platform driver, adapter |
| **Grab**        | Linux: the exclusive `EVIOCGRAB` claim on a device, so its events reach only Bouncer              | capture, lock, take        |
| **Replay**      | Linux: re-emitting a passed event on Bouncer's `uinput` virtual device, which the desktop reads   | forward, re-inject, mirror |
| **Command / Report** | The channel messages between threads: Commands flow UI→hook, Reports flow hook→UI            | message, event (overloaded)|

## Observability & privacy

| Term                  | Definition                                                                                  | Aliases to avoid             |
| --------------------- | ------------------------------------------------------------------------------------------- | ---------------------------- |
| **Suppressed counter**| In-memory, per-device-class tally of suppressed events; resets every boot, never persisted   | stats, metrics, log          |
| **Diagnostic mode**   | Opt-in, visibly-indicated, auto-expiring (1 hr) capture of recent suppressed events for calibration | debug mode, logging, capture |
| **Suppression gap**   | The measured ms between a key's Up and the suppressed Down — the data point a histogram plots | interval, delta              |
| **Ring buffer**       | The fixed ~500-entry in-memory store of suppressed events backing diagnostic mode            | log, cache, history          |

## Relationships

- An **Engine** owns exactly one **Debouncer**, one **PanicDetector**, and one **Mode**.
- The **Engine** turns each **InputEvent** into one **Outcome** (a **Verdict** plus optional **Mode** change).
- A **Verdict** is **Pass** or **Suppress**; **Suppress** is only legal for a **Down**, never an **Up**.
- A **Debouncer** keeps **release-anchored** state per **KeyId**, compared against the **Threshold** for that **device class**.
- The **panic hotkey** is a **chord** matched by the **PanicDetector**; matching it puts the **Engine** in **Panic** Mode (a **pass-through** Mode).
- The **counter**, **ring buffer**, and histogram are UI-side state built from the **Report** stream — *never* inside the **Engine**.
- An **injected event** bypasses suppression in production; only test-only mode debounces it.

## Example dialogue

> **Dev:** "When the **Debouncer** sees a **Down**, what does it compare against the **threshold**?"

> **Domain expert:** "The gap since that **KeyId**'s last **Up** — we're **release-anchored**. If it's under the **threshold** for that **device class**, the **verdict** is **Suppress**; otherwise **Pass**."

> **Dev:** "And a held key that auto-repeats?"

> **Domain expert:** "Those **Downs** arrive with no **Up** in between, so the **Debouncer** never sees a recent release — it naturally **Passes**. We never suppress an auto-repeat or an **Up**."

> **Dev:** "If the **Engine** is unsure — say a weird timestamp?"

> **Domain expert:** "**Fail-open.** Any doubt is a **Pass**. Suppressing legitimate input is the one thing we can never do. That's also why the **panic hotkey** is checked *first* in the hot path — even if the **Engine** misbehaves, the **chord** still drops you into **Panic** **pass-through**."

> **Dev:** "Is **Panic** the same as **Paused**, then? Both pass everything."

> **Domain expert:** "Same *behavior*, different *intent*. **Paused** is the user resting Bouncer deliberately; **Panic** is the emergency escape. Both are **pass-through**, but they're distinct **Modes** and the tray shows them differently."

## Flagged ambiguities

- **"suppress" vs "swallow" / "block" / "filter" / "eat"** — the design prose uses *swallow* casually, but the canonical verb is **Suppress** (and it's a **Verdict** value). Use *Suppress* in code and *swallow* never in identifiers.
- **"Pass-through" is a behavior, not a Mode.** Both **Paused** and **Panic** are pass-through. Don't say "pass-through mode" as if it were a single state — name the **Mode** (Paused/Panic) and use **pass-through** only for the shared behavior.
- **`enabled` (config flag) vs `Active` (Mode).** The persisted `enabled = false` maps to runtime **Mode::Paused**. Keep the TOML flag and the runtime Mode distinct — the flag is durable settings, the Mode is live state.
- **"key" is overloaded.** It can mean a keyboard key or the generic **KeyId** (keyboard key *or* mouse button). When the mouse is in play, say **KeyId** or **button**, not "key".
- **"double-click bug" is not a separate feature.** It is mouse **chatter** handled by the identical algorithm. Avoid implying a dedicated mouse code path.
- **"event" is overloaded** — it appears as **InputEvent** (the core's input), as **Report** items (hook→UI), and as raw Windows messages. Qualify it (**InputEvent** / **Report** / Windows message) whenever the context isn't obvious.
- **"threshold" is singular but holds two values** — keyboard (30) and mouse (40). Always pair it with the **device class** when the value matters.
