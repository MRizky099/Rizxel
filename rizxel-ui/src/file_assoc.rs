//! *Settings ▸ General ▸ Associate .rzxl files*: per-user registration of
//! this copy of Rizxel as the handler of `.rzxl`, and of no other extension,
//! for portable builds. The installers register the
//! same entries themselves. Only on the user's click, never automatically,
//! and always undoable.
//!
//! - Windows: keys under `HKCU\Software\Classes` (no admin rights), written
//!   with `reg.exe`. The `.rzxl` document icon is the executable's icon
//!   resource 2 (see `build.rs`).
//! - Linux: a `.desktop` file and a shared-mime-info package under
//!   `$XDG_DATA_HOME` (`~/.local/share`), then the desktop databases are
//!   refreshed when their tools exist.

use std::path::{Path, PathBuf};

/// ProgID of Rizxel projects (Windows) and the desktop/app id elsewhere.
pub const PROG_ID: &str = "Rizxel.Project";
/// ProgID of the *Open with* entry for `.vox` and `.gox` that 0.9 and 0.10
/// wrote. No longer registered; [`register`] and [`unregister`] remove it.
pub const LEGACY_MODEL_PROG_ID: &str = "Rizxel.Model";
pub const APP_ID: &str = "io.github.mrizky099.rizxel";
pub const MIME_TYPE: &str = "application/x-rizxel";
/// Extensions 0.9 and 0.10 added *Open with* entries for.
pub const LEGACY_OPEN_WITH: [&str; 2] = ["vox", "gox"];

// ---------------------------------------------------------------- Windows

/// One registry value under `HKCU\Software\Classes`. `name: None` is the
/// key's default value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue {
    pub key: String,
    pub name: Option<String>,
    pub value: String,
}

fn reg(key: &str, name: Option<&str>, value: &str) -> RegValue {
    RegValue {
        key: key.to_owned(),
        name: name.map(str::to_owned),
        value: value.to_owned(),
    }
}

/// The values that make `exe` the `.rzxl` handler.
pub fn registry_values(exe: &Path) -> Vec<RegValue> {
    let exe = exe.display().to_string();
    let open = format!("\"{exe}\" \"%1\"");
    vec![
        reg(".rzxl", None, PROG_ID),
        reg(".rzxl", Some("Content Type"), MIME_TYPE),
        reg(".rzxl\\OpenWithProgids", Some(PROG_ID), ""),
        reg(PROG_ID, None, "Rizxel Project"),
        reg(
            &format!("{PROG_ID}\\DefaultIcon"),
            None,
            &format!("\"{exe}\",-2"),
        ),
        reg(&format!("{PROG_ID}\\shell\\open\\command"), None, &open),
        reg(
            "Applications\\rizxel.exe",
            Some("FriendlyAppName"),
            "Rizxel",
        ),
        reg(
            "Applications\\rizxel.exe\\shell\\open\\command",
            None,
            &open,
        ),
        reg(
            "Applications\\rizxel.exe\\SupportedTypes",
            Some(".rzxl"),
            "",
        ),
    ]
}

/// Where registry values go: `reg.exe` on the real hive, or a map in tests.
pub trait Registry {
    fn get(&self, key: &str, name: Option<&str>) -> Option<String>;
    fn set(&mut self, value: &RegValue) -> std::io::Result<()>;
    fn delete_value(&mut self, key: &str, name: Option<&str>) -> std::io::Result<()>;
    fn delete_key(&mut self, key: &str) -> std::io::Result<()>;
}

/// Registers `exe` as the `.rzxl` handler, dropping the `.vox`/`.gox`
/// entries an older Rizxel wrote.
pub fn register(reg: &mut dyn Registry, exe: &Path) -> std::io::Result<()> {
    remove_legacy(reg)?;
    for v in registry_values(exe) {
        reg.set(&v)?;
    }
    Ok(())
}

/// Removes the `.vox`/`.gox` *Open with* entries of 0.9 and 0.10.
fn remove_legacy(reg: &mut dyn Registry) -> std::io::Result<()> {
    reg.delete_key(LEGACY_MODEL_PROG_ID)?;
    for ext in LEGACY_OPEN_WITH {
        reg.delete_value(
            &format!(".{ext}\\OpenWithProgids"),
            Some(LEGACY_MODEL_PROG_ID),
        )?;
        reg.delete_value(
            "Applications\\rizxel.exe\\SupportedTypes",
            Some(&format!(".{ext}")),
        )?;
    }
    Ok(())
}

/// Whether `.rzxl` opens with `exe` now.
pub fn is_registered(reg: &dyn Registry, exe: &Path) -> bool {
    let wanted = registry_values(exe);
    let command = &wanted[5];
    reg.get(".rzxl", None).as_deref() == Some(PROG_ID)
        && reg.get(&command.key, None).as_deref() == Some(command.value.as_str())
}

/// Removes what [`register`] wrote. `.rzxl` loses its default only while
/// it still points to Rizxel; other programs' entries stay.
pub fn unregister(reg: &mut dyn Registry) -> std::io::Result<()> {
    reg.delete_key(PROG_ID)?;
    remove_legacy(reg)?;
    reg.delete_key("Applications\\rizxel.exe")?;
    reg.delete_value(".rzxl\\OpenWithProgids", Some(PROG_ID))?;
    if reg.get(".rzxl", None).as_deref() == Some(PROG_ID) {
        reg.delete_value(".rzxl", None)?;
        reg.delete_value(".rzxl", Some("Content Type"))?;
    }
    Ok(())
}

/// Test double: keys and values in memory.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MemoryRegistry {
    pub values: std::collections::BTreeMap<(String, Option<String>), String>,
}

impl Registry for MemoryRegistry {
    fn get(&self, key: &str, name: Option<&str>) -> Option<String> {
        self.values
            .get(&(key.to_owned(), name.map(str::to_owned)))
            .cloned()
    }

    fn set(&mut self, v: &RegValue) -> std::io::Result<()> {
        self.values
            .insert((v.key.clone(), v.name.clone()), v.value.clone());
        Ok(())
    }

    fn delete_value(&mut self, key: &str, name: Option<&str>) -> std::io::Result<()> {
        self.values
            .remove(&(key.to_owned(), name.map(str::to_owned)));
        Ok(())
    }

    fn delete_key(&mut self, key: &str) -> std::io::Result<()> {
        let sub = format!("{key}\\");
        self.values
            .retain(|(k, _), _| k != key && !k.starts_with(&sub));
        Ok(())
    }
}

/// The user's real `HKCU\Software\Classes`, through `reg.exe`.
pub struct RegExe;

const CLASSES: &str = r"HKCU\Software\Classes";

/// `reg.exe` arguments for one value.
pub fn reg_add_args(v: &RegValue) -> Vec<String> {
    let mut args = vec!["add".to_owned(), format!(r"{CLASSES}\{}", v.key)];
    match &v.name {
        Some(n) => args.extend(["/v".to_owned(), n.clone()]),
        None => args.push("/ve".to_owned()),
    }
    args.extend([
        "/t".to_owned(),
        "REG_SZ".to_owned(),
        "/d".to_owned(),
        v.value.clone(),
        "/f".to_owned(),
    ]);
    args
}

fn run_reg(args: &[String]) -> std::io::Result<std::process::Output> {
    let mut cmd = std::process::Command::new("reg");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up from the GUI.
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output()
}

impl Registry for RegExe {
    fn get(&self, key: &str, name: Option<&str>) -> Option<String> {
        let mut args = vec!["query".to_owned(), format!(r"{CLASSES}\{key}")];
        match name {
            Some(n) => args.extend(["/v".to_owned(), n.to_owned()]),
            None => args.push("/ve".to_owned()),
        }
        let out = run_reg(&args).ok()?;
        if !out.status.success() {
            return None;
        }
        // `    (Default)    REG_SZ    value`
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.split_once("REG_SZ").map(|(_, v)| v.trim().to_owned()))
    }

    fn set(&mut self, v: &RegValue) -> std::io::Result<()> {
        let out = run_reg(&reg_add_args(v))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ))
        }
    }

    fn delete_value(&mut self, key: &str, name: Option<&str>) -> std::io::Result<()> {
        let mut args = vec!["delete".to_owned(), format!(r"{CLASSES}\{key}")];
        match name {
            Some(n) => args.extend(["/v".to_owned(), n.to_owned()]),
            None => args.push("/ve".to_owned()),
        }
        args.push("/f".to_owned());
        // Missing values are fine.
        run_reg(&args).map(|_| ())
    }

    fn delete_key(&mut self, key: &str) -> std::io::Result<()> {
        run_reg(&[
            "delete".to_owned(),
            format!(r"{CLASSES}\{key}"),
            "/f".to_owned(),
        ])
        .map(|_| ())
    }
}

// ------------------------------------------------------------------ Linux

/// The `.desktop` entry for `exe`.
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rizxel\n\
         Comment=Voxel art editor\n\
         Exec=\"{}\" %f\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Graphics;3DGraphics;\n\
         MimeType={MIME_TYPE};\n\
         StartupWMClass=rizxel\n",
        exe.display(),
    )
}

/// The shared-mime-info package declaring `*.rzxl`.
pub fn mime_package() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n\
         \x20 <mime-type type=\"{MIME_TYPE}\">\n\
         \x20   <comment>Rizxel project</comment>\n\
         \x20   <icon name=\"{APP_ID}.project\"/>\n\
         \x20   <glob pattern=\"*.rzxl\"/>\n\
         \x20 </mime-type>\n\
         </mime-info>\n"
    )
}

/// Files written under the XDG data folder.
pub fn xdg_files(data_home: &Path) -> [PathBuf; 4] {
    [
        data_home.join(format!("applications/{APP_ID}.desktop")),
        data_home.join(format!("mime/packages/{APP_ID}.xml")),
        data_home.join(format!("icons/hicolor/256x256/apps/{APP_ID}.png")),
        data_home.join(format!(
            "icons/hicolor/256x256/mimetypes/{APP_ID}.project.png"
        )),
    ]
}

/// Writes the desktop entry, MIME package and icons for `exe` under
/// `data_home`.
pub fn install_xdg(data_home: &Path, exe: &Path) -> std::io::Result<()> {
    let [desktop, mime, app_icon, doc_icon] = xdg_files(data_home);
    let write = |p: &Path, bytes: &[u8]| -> std::io::Result<()> {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, bytes)
    };
    write(&desktop, desktop_entry(exe).as_bytes())?;
    write(&mime, mime_package().as_bytes())?;
    write(
        &app_icon,
        include_bytes!("../../assets/Rizxel_icon_256x256.png"),
    )?;
    write(
        &doc_icon,
        include_bytes!("../../assets/rizxel_document.png"),
    )?;
    Ok(())
}

/// Removes what [`install_xdg`] wrote.
pub fn uninstall_xdg(data_home: &Path) -> std::io::Result<()> {
    for f in xdg_files(data_home) {
        match std::fs::remove_file(&f) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Whether the desktop entry under `data_home` launches `exe`.
pub fn xdg_registered(data_home: &Path, exe: &Path) -> bool {
    std::fs::read_to_string(&xdg_files(data_home)[0]).is_ok_and(|s| s == desktop_entry(exe))
}

/// `$XDG_DATA_HOME`, or `~/.local/share`.
fn xdg_data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.data_local_dir().to_path_buf()))
}

/// Refreshes the MIME and desktop databases and makes Rizxel the default
/// for `.rzxl` (best effort: missing tools are skipped).
fn refresh_xdg(data_home: &Path) {
    let run = |cmd: &str, args: &[&std::ffi::OsStr]| {
        if let Err(e) = std::process::Command::new(cmd).args(args).output() {
            log::info!("{cmd}: {e}");
        }
    };
    run(
        "update-mime-database",
        &[data_home.join("mime").as_os_str()],
    );
    run(
        "update-desktop-database",
        &[data_home.join("applications").as_os_str()],
    );
    let desktop = format!("{APP_ID}.desktop");
    run(
        "xdg-mime",
        &["default".as_ref(), desktop.as_ref(), MIME_TYPE.as_ref()],
    );
}

// ------------------------------------------------------------------- UI

/// The executable to register: the AppImage itself when running from one.
fn this_exe() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

/// Whether this platform offers the setting (macOS uses the bundle's
/// `Info.plist`).
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));

/// Whether `.rzxl` opens with this copy now.
pub fn associated() -> bool {
    let Some(exe) = this_exe() else {
        return false;
    };
    if cfg!(windows) {
        is_registered(&RegExe, &exe)
    } else {
        xdg_data_home().is_some_and(|d| xdg_registered(&d, &exe))
    }
}

/// Associates `.rzxl` with this copy (`on`) or removes the association.
pub fn set_associated(on: bool) -> std::io::Result<()> {
    let exe = this_exe().ok_or_else(|| std::io::Error::other("executable path unknown"))?;
    if cfg!(windows) {
        let mut reg = RegExe;
        if on {
            register(&mut reg, &exe)?;
        } else {
            unregister(&mut reg)?;
        }
        // Explorer picks up the new icons (best effort).
        let _ = std::process::Command::new("ie4uinit.exe")
            .arg("-show")
            .output();
        Ok(())
    } else {
        let data = xdg_data_home().ok_or_else(|| std::io::Error::other("no data folder"))?;
        if on {
            install_xdg(&data, &exe)?;
        } else {
            uninstall_xdg(&data)?;
        }
        refresh_xdg(&data);
        Ok(())
    }
}

/// The *Associate .rzxl files* row of *Settings ▸ General*. Applies at
/// once (it isn't part of the settings file).
pub fn settings_section(ui: &mut egui::Ui, status: &mut Option<String>) {
    if !SUPPORTED {
        return;
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new("File association").font(crate::theme::semibold(12.0)));
    let on = associated();
    ui.horizontal(|ui| {
        let text = if on {
            ".rzxl files open with this copy of Rizxel"
        } else {
            ".rzxl files are not associated with this copy"
        };
        ui.label(text);
    });
    ui.horizontal(|ui| {
        let label = if on {
            "Remove Association"
        } else {
            "Associate .rzxl Files with This Copy"
        };
        if ui
            .button(label)
            .on_hover_text("Per user, no admin rights. Only .rzxl; other formats are left alone.")
            .clicked()
        {
            *status = Some(match set_associated(!on) {
                Ok(()) if on => "Association removed".to_owned(),
                Ok(()) => "Associated .rzxl files with this copy".to_owned(),
                Err(e) => format!("Failed: {e}"),
            });
        }
    });
    if let Some(s) = status {
        ui.label(egui::RichText::new(s.as_str()).color(crate::theme::TEXT_DIM));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_writer_produces_the_expected_keys() {
        let exe = Path::new(r"C:\Tools\Rizxel\rizxel.exe");
        let mut reg = MemoryRegistry::default();
        // Another program's .vox handler must survive.
        reg.set(&RegValue {
            key: ".vox".into(),
            name: None,
            value: "MagicaVoxel.vox".into(),
        })
        .unwrap();
        assert!(!is_registered(&reg, exe));
        register(&mut reg, exe).unwrap();
        assert!(is_registered(&reg, exe));
        assert_eq!(reg.get(".rzxl", None).as_deref(), Some(PROG_ID));
        assert_eq!(
            reg.get(r"Rizxel.Project\shell\open\command", None)
                .as_deref(),
            Some(r#""C:\Tools\Rizxel\rizxel.exe" "%1""#)
        );
        assert_eq!(
            reg.get(r"Rizxel.Project\DefaultIcon", None).as_deref(),
            Some(r#""C:\Tools\Rizxel\rizxel.exe",-2"#)
        );
        assert!(
            // The other program's `.vox` default is not ours.
            reg.values.iter().all(|((k, n), _)| !k.starts_with(r".vox\")
                && !k.starts_with(r".gox\")
                && !k.starts_with(LEGACY_MODEL_PROG_ID)
                && n.as_deref().is_none_or(|n| n != ".vox" && n != ".gox")),
            "only .rzxl is registered"
        );
        assert!(!is_registered(&reg, Path::new(r"D:\other\rizxel.exe")));

        unregister(&mut reg).unwrap();
        let mut left = MemoryRegistry::default();
        left.set(&RegValue {
            key: ".vox".into(),
            name: None,
            value: "MagicaVoxel.vox".into(),
        })
        .unwrap();
        assert_eq!(reg, left, "only the other program's entry is left");
    }

    #[test]
    fn register_and_unregister_drop_the_legacy_vox_gox_entries() {
        let legacy = [
            (r".vox\OpenWithProgids", Some(LEGACY_MODEL_PROG_ID)),
            (r".gox\OpenWithProgids", Some(LEGACY_MODEL_PROG_ID)),
            (r"Applications\rizxel.exe\SupportedTypes", Some(".vox")),
            (r"Applications\rizxel.exe\SupportedTypes", Some(".gox")),
            (r"Rizxel.Model\shell\open\command", None),
        ];
        let old = || {
            let mut r = MemoryRegistry::default();
            for (key, name) in legacy {
                r.set(&reg(key, name, "")).unwrap();
            }
            r
        };
        let gone = |reg: &MemoryRegistry| legacy.iter().all(|&(k, n)| reg.get(k, n).is_none());
        let mut r = old();
        register(&mut r, Path::new(r"C:\a\rizxel.exe")).unwrap();
        assert!(gone(&r));
        let mut r = old();
        unregister(&mut r).unwrap();
        assert!(r.values.is_empty(), "nothing of an older Rizxel is left");
    }

    #[test]
    fn unregister_keeps_a_foreign_rzxl_default() {
        let mut reg = MemoryRegistry::default();
        register(&mut reg, Path::new(r"C:\a\rizxel.exe")).unwrap();
        reg.set(&RegValue {
            key: ".rzxl".into(),
            name: None,
            value: "Other.App".into(),
        })
        .unwrap();
        unregister(&mut reg).unwrap();
        assert_eq!(reg.get(".rzxl", None).as_deref(), Some("Other.App"));
    }

    #[test]
    fn reg_exe_arguments() {
        let v = &registry_values(Path::new(r"C:\r\rizxel.exe"))[0];
        assert_eq!(
            reg_add_args(v),
            [
                r"add",
                r"HKCU\Software\Classes\.rzxl",
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                PROG_ID,
                "/f"
            ]
        );
        let v = &registry_values(Path::new(r"C:\r\rizxel.exe"))[1];
        assert_eq!(&reg_add_args(v)[2..4], ["/v", "Content Type"]);
    }

    #[test]
    fn xdg_files_install_and_uninstall() {
        let dir = std::env::temp_dir().join(format!("rizxel_xdg_{}", std::process::id()));
        let exe = Path::new("/opt/Rizxel.AppImage");
        install_xdg(&dir, exe).unwrap();
        assert!(xdg_registered(&dir, exe));
        assert!(!xdg_registered(&dir, Path::new("/elsewhere/rizxel")));
        let desktop = std::fs::read_to_string(&xdg_files(&dir)[0]).unwrap();
        assert!(desktop.contains("MimeType=application/x-rizxel;\n"));
        assert!(desktop.contains("Exec=\"/opt/Rizxel.AppImage\" %f"));
        let mime = std::fs::read_to_string(&xdg_files(&dir)[1]).unwrap();
        assert!(mime.contains("<glob pattern=\"*.rzxl\"/>"));
        uninstall_xdg(&dir).unwrap();
        assert!(xdg_files(&dir).iter().all(|f| !f.exists()));
        uninstall_xdg(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}
