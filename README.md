# Bouncer

A headless, tray-resident **input debouncer** for Windows. It suppresses keyboard *and* mouse
**chatter** — repeat events that fire faster than humanly possible (worn switch bounce, the
mouse double-click bug) — while never touching your legitimate input.

> The name has a double meaning: a club *bouncer* rejects unwanted entries at the door, and the
> app *de-bounces* your input stream.

**no admin · no driver · no network · suppressed-events-only · open source**

## Status

🚧 Design locked, implementation not started. See [DESIGN.md](DESIGN.md) for the full spec,
decision log, and architecture diagram.

## How it works (short version)

Bouncer installs Windows low-level keyboard/mouse hooks (`WH_KEYBOARD_LL` / `WH_MOUSE_LL`) — no
driver, no admin rights. A key/button *down* that arrives faster than a configurable threshold
after that same key's *release* is physically impossible for a human, so it's chatter and gets
swallowed. Everything else passes through untouched. Held keys and intentional double-clicks are
unaffected by design.

- **Keyboard threshold:** default 30 ms
- **Mouse threshold:** default 40 ms (the double-click bug)
- **Panic hotkey:** instantly disables all suppression (pass-through) if anything ever feels off.

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
just the shape of this category. Bouncer is unsigned (no code-signing budget for v1), so Windows
SmartScreen may show an "unrecognized app" warning on first run. You can build it yourself from
source to be sure:

```sh
cargo build --release
```

## Building

Requires a stable Rust toolchain.

```sh
cargo build --release   # binary in target/release/
cargo test              # run the suite
cargo clippy -- -D warnings
cargo fmt --check
```

## License

TBD (will be an OSI-approved open-source license).
