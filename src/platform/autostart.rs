//! Start-on-login registration (issue #10).
//!
//! Bouncer registers itself under the per-user `Run` key
//! (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) — no admin, no installer,
//! and removed cleanly when the user turns it off. This is OS glue (manually
//! verified); the desired-state → action mapping is straightforward set/delete.

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Bouncer";

/// Apply the desired autostart state: write the current exe path under the Run key
/// when enabling, delete the value when disabling. Idempotent.
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu
        .create_subkey(RUN_KEY)
        .map_err(|e| format!("open Run key: {e}"))?;

    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        // Quote the path so a space in the install dir doesn't split the command.
        let command = format!("\"{}\"", exe.display());
        run.set_value(VALUE_NAME, &command)
            .map_err(|e| format!("set Run value: {e}"))
    } else {
        match run.delete_value(VALUE_NAME) {
            Ok(()) => Ok(()),
            // Already absent → already in the desired state.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("delete Run value: {e}")),
        }
    }
}
