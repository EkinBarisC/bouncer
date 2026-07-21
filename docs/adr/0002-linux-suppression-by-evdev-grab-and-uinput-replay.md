# Linux suppression by exclusive evdev grab + uinput replay

## Context

Bouncer's whole value is *vetoing* an event. Windows offers that directly: a
`WH_KEYBOARD_LL` callback returns `LRESULT(1)` and the keystroke never reaches
anyone. Linux has no equivalent — neither X11 nor Wayland lets a client suppress an
event it did not originate, and neither sees the physical device layer where chatter
actually happens. There is no hook to install.

What Linux does offer is `EVIOCGRAB`: an exclusive grab on an `/dev/input/eventN`
node that routes that device's events to the grabbing process **and nobody else**,
plus `uinput`, which lets a process create a device the rest of the system reads.

## Decision

Suppress by **interposition**. The Linux backend:

1. enumerates `/dev/input/event*` and selects keyboards (`KEY_A`) and mice
   (`BTN_LEFT`) with no absolute axes;
2. creates one `uinput` virtual device advertising the union of everything those
   devices can emit;
3. takes an exclusive grab on each real device;
4. `poll(2)`s the grabbed fds, runs each key/button transition past the same pure
   `Engine`, and re-emits every event the Engine passes on the virtual device.

Chatter is suppressed by *not replaying it*. Everything else — pointer motion,
scroll, auto-repeat, `MSC_SCAN`, `SYN_REPORT` — is forwarded byte-for-byte.

The `poll` loop replaces the Windows `GetMessage` pump but keeps ADR-0001's shape
exactly: the Engine is owned by the backend thread, the verdict is computed
synchronously in the read loop, and control still arrives only over the
`Command`/`Report` channels.

## Considered options

- **Grab devices and re-emit — chosen.** The only approach that can suppress a
  physical key on both X11 and Wayland, and the one every comparable tool
  (interception-tools, kmonad, keyd) converged on.
- **X11 `XRecord` / `XInput2`.** Can observe but not veto, and is X11-only. Useless
  for the core feature and dead on Wayland.
- **A kernel module or a patched HID driver.** Suppresses perfectly, but requires
  out-of-tree code, signing, and root — against the project's no-driver stance
  (DESIGN.md D1).
- **libinput quirks / `debounce` in the compositor.** Not user-configurable per
  threshold, compositor-specific, and absent on most desktops.

## Consequences

- **Bouncer becomes load-bearing for input while it runs.** Mitigated structurally:
  the grab is held by an open fd, so process exit, panic (`panic = "abort"`), or
  `kill -9` all release it and restore the hardware immediately. There is no
  shutdown handshake that can fail and leave a user without a keyboard. The backend
  therefore treats every error as terminal rather than limping on.
- The uinput device must exist **before** any grab is taken, and must be given a
  moment for udev to enumerate it — otherwise the first keystrokes are emitted into
  a device nothing is listening to yet.
- Applications see `Bouncer Virtual Input` in place of the real devices. Per-device
  desktop settings key off that name.
- It needs read access to `/dev/input/event*` and write access to `/dev/uinput` —
  the `input` group plus a udev rule (`packaging/linux/`), not root.
- Devices are enumerated once at startup: no hotplug yet. A keyboard plugged in
  later is not debounced until Bouncer restarts.
- The Windows eviction watchdog has no analogue here — there is no hook for the OS
  to silently evict. `platform/watchdog.rs` stays Windows-only in practice.
