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
    /// directory (features that need it are then off).
    pub fn new() -> Option<Self> {
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
}
