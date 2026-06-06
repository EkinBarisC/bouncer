# Pure decision Engine on the hook thread, controlled by channels (no shared lock)

## Context

The `WH_KEYBOARD_LL` / `WH_MOUSE_LL` callback must return its verdict (pass vs suppress)
**synchronously** — Windows blocks, holding up every input event on the machine, until the
callback returns. The decision therefore cannot be delegated to another thread and awaited.

## Decision

All decision logic lives in a single **pure `Engine`** (wrapping the `Debouncer`, a
`PanicDetector`, and the current `Mode`) that runs **on the hook thread** and is called
synchronously from the hook callback: `Engine::on_event(InputEvent) -> Outcome { verdict,
mode_change }`. The Engine is pure — no OS calls, no clock, no I/O — so chatter suppression,
panic-hotkey toggling, and Pause/Panic gating are all unit-testable with synthetic events.

Presentation state (the suppressed **counter**, the diagnostic **ring buffer**, the gap
**histogram**) does **not** live in the Engine. The Engine emits lightweight `Report`s; the
UI/stats side owns and builds that state from the Report stream.

The hook thread and UI thread communicate **only through `Command`/`Report` channels**, never a
shared `Arc<Mutex<Engine>>`. The UI sends `Command`s (SetThresholds, SetMode, SetDiagnostic,
RebindPanic, Shutdown); the hook thread applies them to its own Engine between events and sends
`Report`s back.

## Considered options

- **`Arc<Mutex<Engine>>` shared between threads** — simpler wiring, fewer message types.
  Rejected: any lock contention on the input hot path, however brief, adds latency that can get
  the hook evicted by Windows' `LowLevelHooksTimeout`, and it breaks the guarantee that a slow
  or stalled UI thread can never delay a keystroke.

## Consequences

- The OS-specific `HookBackend` trait shrinks to essentially one blocking method:
  `run(self, engine, commands: Receiver<Command>, reports: Sender<Report>) -> Result<()>`.
  `Engine`, `Command`, `Report`, `InputEvent` are all platform-agnostic core types; only the
  trait impl knows `WH_*_LL`, virtual-key codes, and the `PostThreadMessageW` used to wake the
  blocking `GetMessage` loop when a `Command` arrives.
- The input hot path can never be stalled by the UI — no lock is ever acquired while deciding.
