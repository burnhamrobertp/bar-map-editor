fn main() {
    // Embed the app icon into the Windows executable so it appears in Explorer and the taskbar.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/bar-map-editor.ico");
        if let Err(e) = res.compile() {
            // Non-fatal: missing rc.exe / llvm-rc is common in cross-compile envs.
            eprintln!("cargo:warning=Could not embed Windows icon: {e}");
        }
    }
}
