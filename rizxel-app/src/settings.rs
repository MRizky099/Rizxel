//! User preferences (*Settings* dialog), stored as `settings.json` in the
//! app data folder.
//!
//! The file is versioned. Keys this version doesn't know are kept when it is
//! written again (a newer Rizxel may have added them), and a value that
//! doesn't parse falls back to its default with a log line, so one bad entry
//! never resets the rest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::keymap::Keymap;

/// Version written to `settings.json`.
pub const SETTINGS_VERSION: u32 = 1;

/// Range of the undo limit (steps per document).
pub const UNDO_LIMIT_RANGE: std::ops::RangeInclusive<usize> = 16..=4096;
/// Range of the UI scale.
pub const UI_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.75..=2.0;
/// Longest autosave interval in minutes (`0` turns autosave off).
pub const MAX_AUTOSAVE_MINUTES: u32 = 60;
/// Range of the voxel and floor grid line widths, in pixels.
pub const GRID_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 0.5..=4.0;
/// Range of the voxel grid opacity.
pub const GRID_OPACITY_RANGE: std::ops::RangeInclusive<f32> = 0.1..=1.0;

/// Color theme. Only the dark theme exists so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
}

/// How the mouse moves the viewport camera.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraControls {
    /// Right drag orbits, middle drag pans, the wheel zooms.
    #[default]
    Rizxel,
    /// Also `Alt` + left drag orbits, `Alt` + middle pans and `Alt` + right
    /// zooms (Maya, Qubicle). The tools' own `Alt` (color pick) is off.
    AltMouse,
}

impl CameraControls {
    pub const ALL: [CameraControls; 2] = [CameraControls::Rizxel, CameraControls::AltMouse];

    pub fn label(self) -> &'static str {
        match self {
            CameraControls::Rizxel => "Rizxel (right orbit, middle pan)",
            CameraControls::AltMouse => "Alt + mouse (Alt+left orbit, Alt+middle pan)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub theme: Theme,
    /// Multiplies the size of everything in the UI.
    pub ui_scale: f32,
    /// Undo steps kept per document.
    pub undo_limit: usize,
    /// Autosave a dirty document after this many minutes (`0`: never).
    pub autosave_minutes: u32,
    /// Show the start screen when no file was opened from the command line.
    pub start_screen: bool,
    /// Clicking a voxel layer in the Layers panel selects all its voxels.
    pub layer_click_selects_voxels: bool,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            ui_scale: 1.0,
            undo_limit: crate::history::DEFAULT_UNDO_LIMIT,
            autosave_minutes: 2,
            start_screen: true,
            layer_click_selects_voxels: true,
            other: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewportSettings {
    /// Floor grid shown at start (`G` still toggles it).
    pub floor_grid: bool,
    /// Voxel grid on the model at start.
    pub voxel_grid: bool,
    /// Voxel grid line width in pixels.
    pub voxel_grid_width: f32,
    /// Voxel grid line opacity.
    pub voxel_grid_opacity: f32,
    /// Floor grid line width in pixels.
    pub floor_grid_width: f32,
    /// Model-mode background, `None` for the theme's.
    pub background: Option<[u8; 3]>,
    /// Start in orthographic projection.
    pub orthographic: bool,
    pub camera_controls: CameraControls,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Default for ViewportSettings {
    fn default() -> Self {
        Self {
            floor_grid: true,
            voxel_grid: false,
            voxel_grid_width: 1.0,
            voxel_grid_opacity: 0.35,
            floor_grid_width: 1.0,
            background: None,
            orthographic: false,
            camera_controls: CameraControls::Rizxel,
            other: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PathSettings {
    /// Folder the export dialogs start in, `None` for the system's choice.
    pub export_folder: Option<PathBuf>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// Everything in `settings.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub general: GeneralSettings,
    pub viewport: ViewportSettings,
    pub keymap: Keymap,
    pub paths: PathSettings,
    /// Top-level keys of a newer version, written back unchanged.
    other: Map<String, Value>,
    /// Keymap entries that didn't apply (unknown actions), kept as written.
    keymap_other: BTreeMap<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            general: GeneralSettings::default(),
            viewport: ViewportSettings::default(),
            keymap: Keymap::default(),
            paths: PathSettings::default(),
            other: Map::new(),
            keymap_other: BTreeMap::new(),
        }
    }
}

const KNOWN_KEYS: [&str; 5] = ["version", "general", "viewport", "keymap", "paths"];

impl Settings {
    /// Reads `file`; a missing or broken file gives the defaults.
    pub fn load(file: &Path) -> Self {
        match std::fs::read(file) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(v) => Self::from_value(v),
                Err(e) => {
                    log::warn!(
                        "settings: {} is not JSON ({e}); using defaults",
                        file.display()
                    );
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                log::warn!("settings: {}: {e}; using defaults", file.display());
                Self::default()
            }
        }
    }

    /// Parses settings JSON leniently (see the module docs).
    pub fn from_value(v: Value) -> Self {
        let Value::Object(mut obj) = v else {
            log::warn!("settings: not an object; using defaults");
            return Self::default();
        };
        if let Some(version) = obj.get("version").and_then(Value::as_u64) {
            if version > u64::from(SETTINGS_VERSION) {
                log::info!("settings: version {version} is newer; reading what this version knows");
            }
        }
        let general = section::<GeneralSettings>("general", obj.get("general"));
        let viewport = section::<ViewportSettings>("viewport", obj.get("viewport"));
        let paths = section::<PathSettings>("paths", obj.get("paths"));
        let mut overrides = BTreeMap::new();
        let mut keymap_other = BTreeMap::new();
        if let Some(Value::Object(km)) = obj.get("keymap") {
            for (id, chords) in km {
                let known = crate::keymap::ActionId::from_id(id).is_some();
                match serde_json::from_value::<Vec<String>>(chords.clone()) {
                    Ok(list) if known => {
                        overrides.insert(id.clone(), list);
                    }
                    _ if !known => {
                        keymap_other.insert(id.clone(), chords.clone());
                    }
                    _ => log::warn!("settings: keymap {id}: not a list of chords"),
                }
            }
        }
        obj.retain(|k, _| !KNOWN_KEYS.contains(&k.as_str()));
        let mut s = Self {
            general,
            viewport,
            keymap: Keymap::from_overrides(&overrides),
            paths,
            other: obj,
            keymap_other,
        };
        s.sanitize();
        s
    }

    pub fn to_value(&self) -> Value {
        let mut obj = self.other.clone();
        obj.insert("version".into(), SETTINGS_VERSION.into());
        let put = |obj: &mut Map<String, Value>, k: &str, v: Value| {
            obj.insert(k.to_owned(), v);
        };
        put(&mut obj, "general", to_value(&self.general));
        put(&mut obj, "viewport", to_value(&self.viewport));
        let mut km: Map<String, Value> = self
            .keymap_other
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (id, chords) in self.keymap.overrides() {
            km.insert(id, chords.into());
        }
        put(&mut obj, "keymap", Value::Object(km));
        put(&mut obj, "paths", to_value(&self.paths));
        Value::Object(obj)
    }

    /// Writes the settings, creating the folder when needed.
    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(&self.to_value()).map_err(std::io::Error::other)?;
        // Through a temp file, so a crash mid-write leaves the old settings.
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, file)
    }

    /// Clamps values into their ranges.
    pub fn sanitize(&mut self) {
        let g = &mut self.general;
        if !g.ui_scale.is_finite() {
            g.ui_scale = 1.0;
        }
        g.ui_scale = g
            .ui_scale
            .clamp(*UI_SCALE_RANGE.start(), *UI_SCALE_RANGE.end());
        g.undo_limit = g
            .undo_limit
            .clamp(*UNDO_LIMIT_RANGE.start(), *UNDO_LIMIT_RANGE.end());
        g.autosave_minutes = g.autosave_minutes.min(MAX_AUTOSAVE_MINUTES);
        let v = &mut self.viewport;
        let clamp = |x: f32, r: &std::ops::RangeInclusive<f32>, default: f32| {
            if x.is_finite() {
                x.clamp(*r.start(), *r.end())
            } else {
                default
            }
        };
        v.voxel_grid_width = clamp(v.voxel_grid_width, &GRID_WIDTH_RANGE, 1.0);
        v.floor_grid_width = clamp(v.floor_grid_width, &GRID_WIDTH_RANGE, 1.0);
        v.voxel_grid_opacity = clamp(v.voxel_grid_opacity, &GRID_OPACITY_RANGE, 0.35);
    }

    /// Autosave interval, `None` when autosave is off.
    pub fn autosave_interval(&self) -> Option<Duration> {
        (self.general.autosave_minutes > 0)
            .then(|| Duration::from_secs(u64::from(self.general.autosave_minutes) * 60))
    }
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Reads one section key by key: a key whose value doesn't fit keeps the
/// default (logged), the others still apply.
fn section<T: Serialize + DeserializeOwned + Default>(name: &str, v: Option<&Value>) -> T {
    let file = match v {
        Some(Value::Object(file)) => file,
        None => return T::default(),
        Some(_) => {
            log::warn!("settings: {name} is not an object; using defaults");
            return T::default();
        }
    };
    let Value::Object(mut merged) = to_value(&T::default()) else {
        return T::default();
    };
    for (k, val) in file {
        let mut trial = merged.clone();
        trial.insert(k.clone(), val.clone());
        if serde_json::from_value::<T>(Value::Object(trial)).is_ok() {
            merged.insert(k.clone(), val.clone());
        } else {
            log::warn!("settings: {name}.{k} = {val} is invalid; using the default");
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{ActionId, KeyChord};

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rizxel_settings_{name}_{}", std::process::id()))
    }

    #[test]
    fn round_trip() {
        let dir = temp("rt");
        let file = dir.join("settings.json");
        let mut s = Settings::default();
        s.general.undo_limit = 100;
        s.general.autosave_minutes = 5;
        s.viewport.camera_controls = CameraControls::AltMouse;
        s.viewport.background = Some([10, 20, 30]);
        s.viewport.voxel_grid_width = 2.5;
        s.viewport.voxel_grid_opacity = 0.6;
        s.viewport.floor_grid_width = 1.5;
        s.general.layer_click_selects_voxels = false;
        s.paths.export_folder = Some(PathBuf::from("/exports"));
        s.keymap
            .set(ActionId::Undo, vec![KeyChord::parse("Ctrl+U").unwrap()])
            .unwrap();
        s.save(&file).unwrap();
        assert_eq!(Settings::load(&file), s);
        assert_eq!(s.autosave_interval(), Some(Duration::from_secs(300)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        assert_eq!(
            Settings::load(&temp("missing").join("x.json")),
            Settings::default()
        );
        let dir = temp("broken");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(&file, b"{ not json").unwrap();
        assert_eq!(Settings::load(&file), Settings::default());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_keys_are_preserved() {
        let v = serde_json::json!({
            "version": 7,
            "cloud": { "sync": true },
            "general": { "undo_limit": 64, "future_flag": "x" },
            "keymap": { "tool.lasso": ["Q"], "edit.undo": ["Ctrl+U"] },
        });
        let s = Settings::from_value(v);
        assert_eq!(s.general.undo_limit, 64);
        assert_eq!(
            s.keymap.label(ActionId::Undo),
            KeyChord::parse("Ctrl+U").unwrap().label()
        );
        let out = s.to_value();
        assert_eq!(out["version"], SETTINGS_VERSION);
        assert_eq!(out["cloud"]["sync"], true);
        assert_eq!(out["general"]["future_flag"], "x");
        assert_eq!(out["keymap"]["tool.lasso"], serde_json::json!(["Q"]));
        assert_eq!(out["keymap"]["edit.undo"], serde_json::json!(["Ctrl+U"]));
    }

    #[test]
    fn invalid_values_fall_back_per_key() {
        let v = serde_json::json!({
            "general": { "undo_limit": "lots", "autosave_minutes": 10, "ui_scale": 9.0 },
            "viewport": { "camera_controls": "joystick", "voxel_grid": true },
            "keymap": { "edit.redo": "Ctrl+Y" },
            "paths": 3,
        });
        let s = Settings::from_value(v);
        assert_eq!(s.general.undo_limit, crate::history::DEFAULT_UNDO_LIMIT);
        assert_eq!(s.general.autosave_minutes, 10);
        assert_eq!(s.general.ui_scale, *UI_SCALE_RANGE.end());
        assert_eq!(s.viewport.camera_controls, CameraControls::Rizxel);
        assert!(s.viewport.voxel_grid);
        assert_eq!(s.keymap, Keymap::default());
        assert_eq!(s.paths, PathSettings::default());
    }

    #[test]
    fn older_files_get_the_new_defaults() {
        // A 0.10.0 settings file: no grid style, no layer click setting.
        let v = serde_json::json!({
            "version": 1,
            "general": { "undo_limit": 64 },
            "viewport": { "voxel_grid": true, "floor_grid": false },
        });
        let s = Settings::from_value(v);
        assert!(s.general.layer_click_selects_voxels);
        assert_eq!(s.viewport.voxel_grid_width, 1.0);
        assert_eq!(s.viewport.voxel_grid_opacity, 0.35);
        assert_eq!(s.viewport.floor_grid_width, 1.0);
        let out = Settings::from_value(serde_json::json!({
            "viewport": { "voxel_grid_width": 40.0, "voxel_grid_opacity": 0.0 },
        }));
        assert_eq!(out.viewport.voxel_grid_width, *GRID_WIDTH_RANGE.end());
        assert_eq!(out.viewport.voxel_grid_opacity, *GRID_OPACITY_RANGE.start());
    }
}
