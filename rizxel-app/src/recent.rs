//! Recently opened and saved files (*File ▸ Open Recent*).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Most recent first, de-duplicated, at most [`RecentFiles::MAX`] entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentFiles {
    paths: Vec<PathBuf>,
}

impl RecentFiles {
    pub const MAX: usize = 10;

    /// Reads the list; a missing or unreadable file gives an empty one.
    pub fn load(file: &Path) -> Self {
        std::fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .map(|mut r| {
                r.paths.truncate(Self::MAX);
                r
            })
            .unwrap_or_default()
    }

    /// Writes the list, creating the folder when needed.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(file, json)
    }

    /// Moves `path` to the top (absolute, so the same file is never listed
    /// twice), dropping the oldest entry beyond [`RecentFiles::MAX`].
    pub fn push(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.paths.retain(|p| !same_file(p, &path));
        self.paths.insert(0, path);
        self.paths.truncate(Self::MAX);
    }

    /// Drops entries whose file no longer exists. Returns whether any went.
    pub fn prune(&mut self) -> bool {
        let before = self.paths.len();
        self.paths.retain(|p| p.is_file());
        self.paths.len() != before
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// Paths are compared case-insensitively on Windows, where the file system is.
fn same_file(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_dedupes_and_caps() {
        let mut r = RecentFiles::default();
        for i in 0..12 {
            r.push(Path::new(&format!("/m/{i}.rzxl")));
        }
        assert_eq!(r.paths().len(), RecentFiles::MAX);
        assert!(r.paths()[0].ends_with("11.rzxl"));
        r.push(Path::new("/m/5.rzxl"));
        assert_eq!(r.paths().len(), RecentFiles::MAX);
        assert!(r.paths()[0].ends_with("5.rzxl"));
        assert_eq!(
            r.paths().iter().filter(|p| p.ends_with("5.rzxl")).count(),
            1
        );
    }

    #[test]
    fn save_load_and_prune() {
        let dir = std::env::temp_dir().join(format!("rizxel_recent_{}", std::process::id()));
        let existing = dir.join("a.rzxl");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&existing, b"x").unwrap();
        let mut r = RecentFiles::default();
        r.push(&dir.join("missing.rzxl"));
        r.push(&existing);
        let file = dir.join("sub").join("recent.json");
        r.save(&file).unwrap();
        let mut back = RecentFiles::load(&file);
        assert_eq!(back, r);
        assert!(back.prune());
        assert_eq!(back.paths(), std::slice::from_ref(&existing));
        assert!(RecentFiles::load(&dir.join("nope.json")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
