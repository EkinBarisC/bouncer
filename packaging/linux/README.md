# Bouncer on Linux

The Linux build is a **headless daemon**: same engine, same `config.toml`, no tray
icon and no Settings window yet. Edit the config file and restart it.

## How it works

Linux has no equivalent of the Windows low-level hook that can veto an event, so
Bouncer suppresses chatter by interposition:

1. It takes an **exclusive grab** (`EVIOCGRAB`) on each keyboard and mouse, so their
   events reach Bouncer and nothing else.
2. It creates one **uinput virtual device** that advertises everything those devices
   can emit, and replays each event the engine passes.
3. Chatter is simply never replayed.

Consequences worth knowing:

- Bouncer must be running for your keyboard to work at all — *while it holds the
  grab*. It never leaves you stranded: the grab is held by an open file descriptor,
  so if the process exits, crashes, or is `kill -9`'d, the kernel releases it
  immediately and your hardware goes back to normal.
- Applications see one device named `Bouncer Virtual Input` instead of your real
  keyboard and mouse. Per-device settings in your desktop environment (and tools
  that key off device names) will follow that name.
- Touchpads, tablets, and gamepads are **not** grabbed — chatter is a
  mechanical-switch problem. They keep working exactly as before.
- Hotplug is not handled yet: devices are enumerated once at startup, so a keyboard
  plugged in later is not debounced until you restart Bouncer.

## Setup

Bouncer runs as your own user — it does **not** need root.

```sh
# 1. Read access to input devices.
sudo usermod -aG input "$USER"

# 2. Write access to /dev/uinput.
sudo install -m 0644 packaging/linux/99-bouncer-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger

# 3. Make sure the uinput module is loaded at boot.
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
sudo modprobe uinput
```

Log out and back in (or reboot) so your new `input` group membership takes effect,
then:

```sh
cargo build --release
./target/release/bouncer
```

> Adding yourself to the `input` group lets any program you run read every keystroke
> on the machine. That is the price of a userspace debouncer; the alternative is
> running Bouncer as root, which is worse.

## Running it in the background

```ini
# ~/.config/systemd/user/bouncer.service
[Unit]
Description=Bouncer input debouncer

[Service]
ExecStart=%h/.local/bin/bouncer
Restart=on-failure

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now bouncer
```

## Troubleshooting

| Message | Cause |
| --- | --- |
| `no keyboard or mouse found under /dev/input` | Not in the `input` group, or the group change hasn't taken effect — log out and back in. |
| `failed to create the uinput device` | `/dev/uinput` missing (`modprobe uinput`) or not writable (udev rule not installed / not reloaded). |
| `failed to grab … exclusively` | Something else already holds a grab on that device — another remapper such as kmonad, keyd, or interception-tools. |
