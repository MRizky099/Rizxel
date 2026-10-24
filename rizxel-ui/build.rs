//! Embeds the multi-size application icon and the version details shown in
//! Explorer's *Properties ▸ Details* into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../assets/Rizxel_icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/Rizxel_icon.ico");
        res.set("ProductName", "Rizxel");
        res.set("FileDescription", "Rizxel - Voxel Editor");
        res.set("CompanyName", "Muhammad Rizky Abdillah (MRizky099)");
        res.set(
            "LegalCopyright",
            "© 2026 Muhammad Rizky Abdillah (MRizky099)",
        );
        // A missing resource compiler should not break the build; only the
        // Explorer icon and details are lost (the runtime window icon still works).
        if let Err(err) = res.compile() {
            println!("cargo:warning=failed to embed Windows resources: {err}");
        }
    }
}
