//! Embeds the multi-size application icon into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../assets/Rizxel_icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/Rizxel_icon.ico");
        // A missing resource compiler should not break the build; only the
        // Explorer icon is lost (the runtime window icon still works).
        if let Err(err) = res.compile() {
            println!("cargo:warning=failed to embed Windows icon: {err}");
        }
    }
}
