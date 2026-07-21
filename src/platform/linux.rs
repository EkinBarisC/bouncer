//! Linux backend: exclusive `evdev` grabs feeding the pure `Engine`, with the
//! surviving events replayed on a `uinput` virtual device.
//!
//! There is no OS-level "suppress this event" hook on Linux the way `WH_*_LL` gives
//! one on Windows, so suppression is done by interposition: we take an exclusive
//! grab (`EVIOCGRAB`) on each keyboard/mouse, so its events reach us and nobody
//! else, and re-emit the ones the Engine passes through a virtual device the rest of
//! the system sees instead. Chatter is simply never re-emitted.
//!
//! This keeps ADR-0001's shape — the Engine is owned by this thread and the verdict
//! is computed synchronously in the read loop, with no lock on the input path — but
//! the loop is a `poll(2)` over the grabbed device fds rather than an OS message
//! pump.
//!
//! **Fail-open:** a grabbed device whose events we stop forwarding is a dead
//! keyboard, so every failure path here tears the backend down rather than limping.
//! Dropping a [`RawDevice`] closes its fd, which releases the kernel grab — so an
//! error return, a panic, or a `kill -9` all restore normal input automatically.

use crate::core::{Engine, EventKind, InputEvent as CoreEvent, KeyId, Verdict};
use crate::messages::{Command, Report};
use crate::platform::evdev_keycode;
use crate::platform::{BackendError, HookBackend};

use evdev::raw_stream::{self, RawDevice};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode as EvKey, MiscCode, RelativeAxisCode};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

/// The name our virtual device reports. Also the marker that stops us grabbing our
/// own output device during discovery.
const VIRTUAL_DEVICE_NAME: &str = "Bouncer Virtual Input";

/// How long `poll` waits before looping. Bounds how long a `Command` (a threshold
/// change, a Shutdown) waits when the user is not typing; input itself wakes the
/// poll immediately, so this never costs latency on the hot path.
const POLL_TIMEOUT_MS: i32 = 100;

/// Time given to udev/libinput to notice the new virtual device before we grab the
/// real ones. Without it the first keystrokes after startup are emitted into a
/// device nothing is listening to yet, and are lost.
const UINPUT_SETTLE: Duration = Duration::from_millis(300);

/// One grabbed input device.
struct Source {
    path: PathBuf,
    dev: RawDevice,
}

/// The Linux evdev/uinput backend.
#[derive(Default)]
pub struct LinuxBackend;

impl LinuxBackend {
    pub fn new() -> Self {
        LinuxBackend
    }
}

impl HookBackend for LinuxBackend {
    fn run(
        self,
        mut engine: Engine,
        commands: Receiver<Command>,
        reports: Sender<Report>,
    ) -> Result<(), BackendError> {
        let mut sources = discover()?;
        // Build the output device *before* grabbing anything: if uinput is
        // unavailable we must fail while the user still has a working keyboard.
        let mut virtual_device = build_virtual_device(&sources)?;
        std::thread::sleep(UINPUT_SETTLE);

        for source in &mut sources {
            source.dev.grab().map_err(|e| {
                format!(
                    "failed to grab {} exclusively: {e} \
                     (another process may already hold a grab on it)",
                    source.path.display()
                )
            })?;
        }

        pump(
            &mut sources,
            &mut virtual_device,
            &mut engine,
            &commands,
            &reports,
        )
        // `sources` drops here: every fd closes, every grab is released.
    }
}

/// Find the keyboards and mice worth grabbing.
///
/// The filter is deliberately narrow: a device qualifies only if it reports `KEY_A`
/// (a keyboard) or `BTN_LEFT` (a mouse) and has no absolute axes. That excludes
/// touchpads, tablets and gamepads — switch chatter is a mechanical-switch problem,
/// and grabbing an `EV_ABS` device would mean mirroring its axis ranges onto the
/// virtual device for no benefit. Devices we skip are untouched and keep working.
fn discover() -> Result<Vec<Source>, BackendError> {
    let sources: Vec<Source> = raw_stream::enumerate()
        .filter(|(_, dev)| is_target(dev))
        .map(|(path, dev)| Source { path, dev })
        .collect();

    if sources.is_empty() {
        return Err(
            "no keyboard or mouse found under /dev/input. If you have one plugged in, \
             this is almost certainly a permissions problem: add yourself to the `input` \
             group (see packaging/linux/README.md) and log back in."
                .to_string(),
        );
    }
    Ok(sources)
}

/// Whether a device is one we should grab (see [`discover`]).
fn is_target(dev: &RawDevice) -> bool {
    if dev.name() == Some(VIRTUAL_DEVICE_NAME) {
        return false; // our own output device
    }
    if dev.supported_absolute_axes().is_some() {
        return false; // touchpad / tablet / gamepad
    }
    dev.supported_keys()
        .is_some_and(|keys| keys.contains(EvKey::KEY_A) || keys.contains(EvKey::BTN_LEFT))
}

/// Build the virtual device the desktop will see in place of the grabbed hardware.
///
/// It advertises the *union* of everything the grabbed devices can emit: the kernel
/// silently drops an emitted event whose code the device never declared, and a
/// dropped event is lost input.
fn build_virtual_device(sources: &[Source]) -> Result<VirtualDevice, BackendError> {
    let mut keys = AttributeSet::<EvKey>::new();
    let mut axes = AttributeSet::<RelativeAxisCode>::new();
    let mut misc = AttributeSet::<MiscCode>::new();

    for source in sources {
        if let Some(supported) = source.dev.supported_keys() {
            supported.iter().for_each(|k| keys.insert(k));
        }
        if let Some(supported) = source.dev.supported_relative_axes() {
            supported.iter().for_each(|a| axes.insert(a));
        }
        if let Some(supported) = source.dev.misc_properties() {
            supported.iter().for_each(|m| misc.insert(m));
        }
    }

    let hint = |e: std::io::Error| {
        format!(
            "failed to create the uinput device: {e} \
             (is the `uinput` module loaded and /dev/uinput writable? \
             see packaging/linux/README.md)"
        )
    };
    VirtualDevice::builder()
        .map_err(hint)?
        .name(VIRTUAL_DEVICE_NAME)
        .with_keys(&keys)
        .map_err(hint)?
        .with_relative_axes(&axes)
        .map_err(hint)?
        .with_msc(&misc)
        .map_err(hint)?
        .build()
        .map_err(hint)
}

/// The read loop: block in `poll` until a device has events or the timeout expires,
/// run each event past the Engine, and replay the survivors.
fn pump(
    sources: &mut Vec<Source>,
    virtual_device: &mut VirtualDevice,
    engine: &mut Engine,
    commands: &Receiver<Command>,
    reports: &Sender<Report>,
) -> Result<(), BackendError> {
    let start = Instant::now();
    // Every buffer the loop needs is allocated once here and reused, so a keystroke
    // costs no allocation — the same discipline the Windows hook callback keeps.
    let mut forward: Vec<InputEvent> = Vec::with_capacity(32);
    let mut ready: Vec<usize> = Vec::with_capacity(sources.len());
    let mut failed: Vec<usize> = Vec::new();
    let mut fds = poll_set(sources);

    loop {
        while let Ok(cmd) = commands.try_recv() {
            if apply_command(engine, cmd) {
                return Ok(()); // Shutdown
            }
        }

        poll_sources(&mut fds, &mut ready)?;
        if ready.is_empty() {
            continue;
        }

        // One timestamp per wake-up: everything read here arrived in the same
        // kernel report, well under the millisecond the thresholds are measured in.
        // A monotonic reading (not the event's own `timeval`, which is CLOCK_REALTIME
        // and can jump under NTP) keeps the Engine's gap arithmetic sane.
        let now_ms = start.elapsed().as_millis() as u64;

        for &index in &ready {
            forward.clear();
            let events = match sources[index].dev.fetch_events() {
                Ok(events) => events,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    failed.push(index); // unplugged, most likely
                    continue;
                }
            };

            for event in events {
                if !suppressed(event, engine, reports, now_ms) {
                    forward.push(event);
                }
            }
            virtual_device
                .emit(&forward)
                .map_err(|e| format!("failed to replay input on the virtual device: {e}"))?;
        }

        if !failed.is_empty() {
            // Descending, so each removal leaves the lower indices valid.
            for index in failed.drain(..).rev() {
                let gone = sources.remove(index);
                eprintln!("bouncer: stopped watching {}", gone.path.display());
            }
            if sources.is_empty() {
                return Err("every watched device disappeared".to_string());
            }
            fds = poll_set(sources);
        }
    }
}

/// Ask the Engine about one raw event; `true` means drop it instead of replaying it.
///
/// Only real key/button transitions are decisions — auto-repeats, pointer motion,
/// scroll, `MSC_SCAN` and `SYN_REPORT` are forwarded untouched, so the replayed
/// stream is byte-identical to the hardware's apart from the missing chatter.
fn suppressed(
    event: InputEvent,
    engine: &mut Engine,
    reports: &Sender<Report>,
    now_ms: u64,
) -> bool {
    if event.event_type() != EventType::KEY {
        return false;
    }
    let kind = match event.value() {
        0 => EventKind::Up,
        1 => EventKind::Down,
        // `2` is a kernel-generated auto-repeat, not a physical switch transition,
        // so it is never chatter — forward it untouched.
        _ => return false,
    };

    let device = evdev_keycode::device_class(event.code());
    let key: KeyId = evdev_keycode::to_keycode(event.code());
    let outcome = engine.on_event(CoreEvent {
        device,
        key,
        kind,
        timestamp_ms: now_ms,
        // A grabbed hardware device only ever delivers physical events; synthetic
        // input goes to other devices, which we never read.
        injected: false,
    });

    if let Some(mode) = outcome.mode_change {
        let _ = reports.send(Report::ModeChanged(mode)); // off the hot path
    }
    if let Some(gap_ms) = outcome.chatter_gap_ms {
        let _ = reports.send(Report::Suppressed {
            device,
            key,
            gap_ms,
        });
    }
    outcome.verdict == Verdict::Suppress
}

/// The `pollfd` array for the current device set. Rebuilt only when a device is
/// dropped, since the fds are otherwise stable for the life of the backend.
fn poll_set(sources: &[Source]) -> Vec<libc::pollfd> {
    sources
        .iter()
        .map(|s| libc::pollfd {
            fd: s.dev.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect()
}

/// Block until one or more devices have events, filling `ready` with their indices.
/// An empty `ready` means the poll timed out (or was interrupted by a signal).
fn poll_sources(fds: &mut [libc::pollfd], ready: &mut Vec<usize>) -> Result<(), BackendError> {
    ready.clear();

    // SAFETY: `fds` is a valid, correctly-sized array of `pollfd` for its length.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, POLL_TIMEOUT_MS) };
    if n < 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            return Ok(()); // a signal; just go round again
        }
        return Err(format!("poll on the input devices failed: {err}"));
    }

    ready.extend(
        fds.iter()
            .enumerate()
            .filter(|(_, fd)| fd.revents & libc::POLLIN != 0)
            .map(|(i, _)| i),
    );
    Ok(())
}

/// Apply one `Command` to the Engine. Returns `true` for `Shutdown`.
fn apply_command(engine: &mut Engine, cmd: Command) -> bool {
    match cmd {
        Command::SetThresholds(thresholds) => {
            engine.set_thresholds(thresholds);
            false
        }
        Command::SetMode(mode) => {
            engine.set_mode(mode);
            false
        }
        Command::SetDiagnostic(_) => false, // UI-side stats overlay (#11)
        Command::RebindPanic(chord) => {
            engine.set_panic_chord(chord);
            false
        }
        Command::Shutdown => true,
    }
}
