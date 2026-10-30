//! Per-user application data folder: recent files and autosave recovery.

use std::path::{Path, PathBuf};

/// Locations under the app data folder (`%APPDATA%\Rizxel`,
/// `~/.local/share/rizxel`, `~/Library/Application Support/Rizxel`).
/// Folders are created when something is written, never up front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// The platform's app data folder, or `None` when the OS reports no home
    /// directory (features that need it are then off). `RIZXEL_DATA_DIR`
    /// points it elsewhere (testing, or a second profile).
    pub fn new() -> Option<Self> {
        if let Some(dir) = std::env::var_os("RIZXEL_DATA_DIR").filter(|d| !d.is_empty()) {
            return Some(Self::with_root(dir));
        }
        let name = if cfg!(any(target_os = "windows", target_os = "macos")) {
            "Rizxel"
        } else {
            "rizxel"
        };
        directories::BaseDirs::new().map(|d| Self::with_root(d.data_dir().join(name)))
    }

    /// Everything under `root` (tests use a temp folder).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Autosave copies and their metadata.
    pub fn recovery_dir(&self) -> PathBuf {
        self.root.join("recovery")
    }

    /// The recent files list.
    pub fn recent_file(&self) -> PathBuf {
        self.root.join("recent.json")
    }

    /// Favorite colors (`0`–`9`).
    pub fn favorites_file(&self) -> PathBuf {
        self.root.join("favorites.json")
    }

    /// Render setting presets saved by the user.
    pub fn render_presets_file(&self) -> PathBuf {
        self.root.join("render_presets.json")
    }

    /// User preferences (*Settings*).
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Dock layouts and window state.
    pub fn layout_file(&self) -> PathBuf {
        self.root.join("layout.json")
    }

    /// Start screen thumbnails of recent files.
    pub fn thumbnails_dir(&self) -> PathBuf {
        self.root.join("thumbnails")
    }

    /// Thumbnail of the project at `project` (named by a hash of its
    /// absolute path, compared case-insensitively on Windows).
    pub fn thumbnail_file(&self, project: &Path) -> PathBuf {
        let path = std::path::absolute(project).unwrap_or_else(|_| project.to_path_buf());
        let mut key = path.to_string_lossy().into_owned();
        if cfg!(windows) {
            key = key.to_lowercase();
        }
        // FNV-1a: stable across runs and Rust versions (unlike `DefaultHasher`).
        let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        self.thumbnails_dir().join(format!("{hash:016x}.png"))
    }

    /// The single-instance lock (port and token of the running window).
    pub fn instance_file(&self) -> PathBuf {
        self.root.join("instance.lock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_names_are_stable_per_path() {
        let p = AppPaths::with_root("/data");
        let a = p.thumbnail_file(Path::new("/m/castle.rzxl"));
        assert_eq!(a, p.thumbnail_file(Path::new("/m/castle.rzxl")));
        assert_ne!(a, p.thumbnail_file(Path::new("/m/tower.rzxl")));
        assert!(a.starts_with(p.thumbnails_dir()));
        assert_eq!(a.extension().unwrap(), "png");
    }
}
