//! Favorite colors `0`–`9` (Qubicle): palette indices kept in the app
//! settings, so they work across documents and sessions.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Ten slots, each an optional palette index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favorites {
    slots: [Option<usize>; Favorites::SLOTS],
}

impl Favorites {
    pub const SLOTS: usize = 10;

    /// Reads the slots; a missing or unreadable file gives empty ones.
    pub fn load(file: &Path) -> Self {
        std::fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .map(|mut f| {
                // Palette indices only; anything else is dropped.
                for s in &mut f.slots {
                    *s = s.filter(|&i| i < rizxel_math::MAX_PALETTE_COLORS);
                }
                f
            })
            .unwrap_or_default()
    }

    /// Writes the slots, creating the folder when needed.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(file, json)
    }

    pub fn get(&self, slot: usize) -> Option<usize> {
        self.slots.get(slot).copied().flatten()
    }

    /// Stores `index` in `slot`; `false` for a slot outside `0..10`.
    pub fn set(&mut self, slot: usize, index: usize) -> bool {
        match self.slots.get_mut(slot) {
            Some(s) => {
                *s = Some(index);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_round_trip_through_the_settings_file() {
        let dir = std::env::temp_dir().join(format!("rizxel_fav_{}", std::process::id()));
        let file = dir.join("favorites.json");
        let mut f = Favorites::default();
        assert!(f.set(0, 12));
        assert!(f.set(9, 255));
        assert!(!f.set(10, 3));
        f.save(&file).unwrap();
        let back = Favorites::load(&file);
        assert_eq!(back, f);
        assert_eq!(back.get(9), Some(255));
        assert_eq!(back.get(5), None);
        // Garbage or out-of-range indices load as empty slots.
        std::fs::write(
            &file,
            b"{\"slots\":[999,1,null,null,null,null,null,null,null,null]}",
        )
        .unwrap();
        let f = Favorites::load(&file);
        assert_eq!((f.get(0), f.get(1)), (None, Some(1)));
        std::fs::write(&file, b"not json").unwrap();
        assert_eq!(Favorites::load(&file), Favorites::default());
        std::fs::remove_dir_all(&dir).ok();
    }
}
