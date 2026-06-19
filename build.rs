// Embed the Windows exe icon. On other platforms this is a no-op.
fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            // Don't fail the build if the resource compiler is unavailable; the exe
            // just falls back to the default icon.
            println!("cargo:warning=icon embed skipped: {e}");
        }
    }
}
