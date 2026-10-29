//! Render setting presets saved by the user, kept in the app data folder
//! (not in project files) so they work across documents.

use std::path::Path;

use rizxel_math::RenderSettings;
use serde::{Deserialize, Serialize};

/// Most user presets kept.
pub const MAX_USER_PRESETS: usize = 64;

/// One named preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPreset {
    pub name: String,
    pub settings: RenderSettings,
}

/// The user's presets, in the order they were saved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserPresets {
    pub presets: Vec<UserPreset>,
}

impl UserPresets {
    /// Reads the presets; a missing or unreadable file gives none. Values
    /// are clamped like a project's.
    pub fn load(file: &Path) -> Self {
        let mut presets = std::fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .unwrap_or_default();
        presets.presets.truncate(MAX_USER_PRESETS);
        for p in &mut presets.presets {
            p.settings.sanitize();
        }
        presets
    }

    /// Writes the presets, creating the folder when needed.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(file, json)
    }

    /// Adds `settings` as `name`, replacing a preset of the same name.
    /// The output settings are not part of a preset. `false` when the list
    /// is full or the name is blank.
    pub fn store(&mut self, name: &str, settings: &RenderSettings) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let settings = RenderSettings {
            output: Default::default(),
            ..settings.clone()
        };
        if let Some(p) = self.presets.iter_mut().find(|p| p.name == name) {
            p.settings = settings;
            return true;
        }
        if self.presets.len() >= MAX_USER_PRESETS {
            return false;
        }
        self.presets.push(UserPreset {
            name: name.to_owned(),
            settings,
        });
        true
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|p| p.name != name);
        self.presets.len() != before
    }

    /// `base` with preset `name`'s settings, keeping `base`'s output.
    pub fn apply(&self, name: &str, base: &RenderSettings) -> Option<RenderSettings> {
        let p = self.presets.iter().find(|p| p.name == name)?;
        Some(RenderSettings {
            output: base.output.clone(),
            ..p.settings.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_store_replace_and_roundtrip() {
        let dir = std::env::temp_dir().join(format!("rizxel_presets_{}", std::process::id()));
        let file = dir.join("render_presets.json");
        let mut presets = UserPresets::default();
        let mut s = RenderSettings::default();
        s.light.sun_azimuth = 10.0;
        s.output.width = 77;
        assert!(presets.store("Mine", &s));
        assert!(!presets.store("  ", &s));
        s.light.sun_azimuth = 20.0;
        assert!(presets.store("Mine", &s));
        assert_eq!(presets.presets.len(), 1);
        assert_eq!(presets.presets[0].settings.output.width, 1920, "no output");
        presets.save(&file).unwrap();
        let back = UserPresets::load(&file);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(back, presets);

        let mut base = RenderSettings::default();
        base.output.width = 500;
        let applied = back.apply("Mine", &base).unwrap();
        assert_eq!(applied.light.sun_azimuth, 20.0);
        assert_eq!(applied.output.width, 500);
        assert!(back.apply("Missing", &base).is_none());
        assert_eq!(
            UserPresets::load(&dir.join("none.json")),
            UserPresets::default()
        );
    }
}
