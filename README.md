# Bouncer

A headless, tray-resident **input debouncer** for Windows. It suppresses keyboard *and* mouse
**chatter** — repeat events that fire faster than humanly possible (worn switch bounce, the
mouse double-click bug) — while never touching your legitimate input.

> The name has a double meaning: a club *bouncer* rejects unwanted entries at the door, and the
> app *de-bounces* your input stream.

**no admin · no driver · no network · suppressed-events-only · open source**

## Why

A worn keyboard or mouse switch starts to "chatter": one physical press registers as several
events within a few milliseconds, so you get doubled letters, dropped game inputs, or accidental
double-clicks. New hardware is the usual fix. Bouncer fixes it in software instead — silently, in
the background, without a driver, without admin rights, and without trusting a closed-source tool
with a hook into your keyboard.

## How it works

Bouncer installs Windows low-level keyboard/mouse hooks (`WH_KEYBOARD_LL` / `WH_MOUSE_LL`) — no
driver, no admin rights. A key/button *down* that arrives faster than a configurable threshold
after that same key's *release* is physically impossible for a human, so it's chatter and gets
suppressed. Everything else passes through untouched. Held keys (auto-repeat) and intentional
double-clicks are unaffected by design.

- **Keyboard threshold:** default 30 ms
- **Mouse threshold:** default 40 ms (the double-click bug)
- **Panic hotkey:** `Ctrl+Alt+Shift+F12` (rebindable) instantly drops Bouncer into full
  pass-through if anything ever feels off.

## Features

- Keyboard **and** mouse chatter suppression through one timing-based engine.
- Independent, live-adjustable thresholds per device class (clamped to ≤100 ms so a bad value
  can't black-hole your input).
- System-tray control: Pause/Resume, Settings, Quit — with four visual states (Active,
  Active+Diagnostic, Paused, Panic).
- Panic hotkey for instant emergency pass-through, plus fail-open safety (any doubt → pass) and
  an eviction watchdog that reinstalls the hook if Windows ever drops it.
- Opt-in **Diagnostic mode**: an in-memory, auto-expiring histogram of suppression gaps to help
  you calibrate your threshold.
- Start-on-login (toggleable), single-instance, and a single human-readable `config.toml`.

## Usage

Bouncer runs headless from the system tray.

- **Left-click** the tray icon to open Settings (thresholds, panic-hotkey rebind, autostart,
  diagnostics).
- **Right-click** for the context menu (Pause/Resume, Diagnostic mode, Settings, Quit).
- Press the **panic hotkey** any time to force full pass-through; press it again to resume.

Settings persist to `%APPDATA%\Bouncer\config.toml` — the only file Bouncer ever writes.

## Privacy

Bouncer is **not** a keylogger and is built so you can prove it:

- Keystrokes that pass through are **never** recorded, stored, or transmitted.
- Only *suppressed* events (random switch bounces) are ever counted — sparse noise that can't
  reconstruct typed text.
- **No network code at all.** Nothing to phone home with.
- Nothing keystroke-related ever touches disk. The only file written is `config.toml` (settings).
- Diagnostic mode is opt-in, visibly indicated, in-memory only, and auto-expires.
- Open source — audit it yourself.

## A note on SmartScreen / antivirus

Any program that installs a global keyboard hook looks, to AV heuristics, like spyware — that's
just the shape of this category. Bouncer is unsigned, so Windows SmartScreen may show an
"unrecognized app" warning on first run. You can build it yourself from source to be sure
(see below).

## Building

Requires a stable Rust toolchain (1.92+) on Windows.

```sh
cargo build --release   # binary in target/release/bouncer.exe
cargo test              # run the suite
cargo clippy -- -D warnings
cargo fmt --check
```

The `SendInput`-driven end-to-end test installs a real global hook and synthesizes input, so it
is opt-in:

```sh
cargo test --features integration-test
```

## Contributing

Bouncer follows a **functional-core / imperative-shell** design: all decision logic lives in a
pure, fully unit-tested `core` (no OS, no clock — time is a parameter), behind a thin
platform/UI shell. New behavior is built test-first.

- [DESIGN.md](DESIGN.md) — full spec, decision log, architecture diagram, and roadmap.
- [docs/adr/](docs/adr/) — architecture decision records.
- [UBIQUITOUS_LANGUAGE.md](UBIQUITOUS_LANGUAGE.md) — the canonical glossary; please use these
  terms in code and commits.

## Status

Core engine, Windows keyboard + mouse backends, tray, settings window, diagnostics, and the
hardening layer (panic hotkey, fail-open, eviction watchdog, single-instance) are implemented.
Remaining before a tagged release: packaging a portable `.exe` and GitHub Releases with SHA256
checksums. See [DESIGN.md](DESIGN.md) §12 for the milestone map.

## License

Licensed under the [MIT License](LICENSE-MIT).
