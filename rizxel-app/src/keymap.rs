//! Keyboard shortcuts: every command the keyboard can trigger, its default
//! chords and the user's rebindings (*Settings ▸ Keymap*).
//!
//! Chords are stored as text (`"Ctrl+Shift+S"`); the key part is one of
//! [`KEY_NAMES`], which the UI maps to its toolkit's keys. `Ctrl` is the
//! platform's command key (`Cmd` on macOS).

use std::collections::BTreeMap;
use std::fmt;

use rizxel_engine::Tool;

/// Keys a chord can use, in their canonical spelling.
pub const KEY_NAMES: &[&str] = &[
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "Space",
    "Tab",
    "Enter",
    "Escape",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Up",
    "Down",
    "Left",
    "Right",
    "Slash",
    "Period",
    "Comma",
    "Minus",
    "Equals",
    "Semicolon",
    "Quote",
    "Backslash",
    "OpenBracket",
    "CloseBracket",
    "Backtick",
];

/// A key with modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyChord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// One of [`KEY_NAMES`].
    key: &'static str,
}

impl KeyChord {
    /// `None` when `key` isn't in [`KEY_NAMES`] (case-insensitive).
    pub fn new(ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<Self> {
        let key = canonical_key(key)?;
        Some(Self {
            ctrl,
            shift,
            alt,
            key,
        })
    }

    fn plain(key: &'static str) -> Self {
        Self::new(false, false, false, key).expect("known key")
    }

    fn ctrl(key: &'static str) -> Self {
        Self::new(true, false, false, key).expect("known key")
    }

    fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// The key part (canonical spelling).
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// Parses `"Ctrl+Shift+S"` (modifiers in any order, any case; `Cmd` is
    /// `Ctrl`).
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        // `Ctrl++` would split into empty parts; `Equals` is the spelling.
        let (key, mods) = parts.split_last()?;
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                _ => return None,
            }
        }
        Self::new(ctrl, shift, alt, key)
    }

    /// Chords no command can take: Ctrl/Cmd with `X`, `C` or `V` and the
    /// Windows `Insert`/`Delete` clipboard keys (the windowing layer turns
    /// them into clipboard events, so only the clipboard commands have
    /// them), and plain `Enter` and `Esc`, which apply and cancel.
    pub fn is_reserved(&self) -> bool {
        let plain = !self.ctrl && !self.shift && !self.alt;
        (self.ctrl && matches!(self.key, "X" | "C" | "V"))
            || (self.ctrl && self.key == "Insert")
            || (self.shift && matches!(self.key, "Insert" | "Delete"))
            || (plain && matches!(self.key, "Enter" | "Escape"))
    }

    /// Text shown in menus and the keymap editor: `"Ctrl+Shift+S"`, `"/"`.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str(if cfg!(target_os = "macos") {
                "Cmd+"
            } else {
                "Ctrl+"
            });
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        out.push_str(key_label(self.key));
        out
    }
}

/// Stored spelling: `Ctrl+Alt+Shift+Key`, the same on every platform.
impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        f.write_str(self.key)
    }
}

fn canonical_key(key: &str) -> Option<&'static str> {
    KEY_NAMES
        .iter()
        .copied()
        .find(|k| k.eq_ignore_ascii_case(key))
        .or(match key {
            "/" => Some("Slash"),
            "." => Some("Period"),
            "," => Some("Comma"),
            "-" => Some("Minus"),
            "=" => Some("Equals"),
            ";" => Some("Semicolon"),
            "'" => Some("Quote"),
            "\\" => Some("Backslash"),
            "[" => Some("OpenBracket"),
            "]" => Some("CloseBracket"),
            "`" => Some("Backtick"),
            "Esc" | "esc" => Some("Escape"),
            _ => None,
        })
}

fn key_label(key: &'static str) -> &'static str {
    match key {
        "Slash" => "/",
        "Period" => ".",
        "Comma" => ",",
        "Minus" => "-",
        "Equals" => "=",
        "Semicolon" => ";",
        "Quote" => "'",
        "Backslash" => "\\",
        "OpenBracket" => "[",
        "CloseBracket" => "]",
        "Backtick" => "`",
        "Escape" => "Esc",
        k => k,
    }
}

/// A command the keyboard can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    New,
    Open,
    Save,
    SaveAs,
    Settings,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteInPlace,
    PasteToNewLayer,
    Delete,
    SelectAll,
    Deselect,
    InvertSelection,
    SelectionToLayer,
    SelectionToLayerCut,
    MaskMode,
    Group,
    Ungroup,
    RotateCcw,
    RotateCw,
    FlipX,
    FlipY,
    FlipZ,
    SliceMode,
    WorkPlane,
    ZenMode,
    RenderMode,
    Projection,
    FrameSelection,
    FrameWorkArea,
    HomeView,
    FloorGrid,
    VoxelGrid,
    ModelBounds,
    Ghosts,
    References,
    UserGuide,
    Tool(Tool),
    RecallFavorite(u8),
    StoreFavorite(u8),
}

/// Section of the keymap editor and the *Keyboard Shortcuts* window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionGroup {
    File,
    Edit,
    Selection,
    Layer,
    Transform,
    View,
    Tools,
    Favorites,
    Help,
}

impl ActionGroup {
    pub const ALL: [ActionGroup; 9] = [
        ActionGroup::File,
        ActionGroup::Edit,
        ActionGroup::Selection,
        ActionGroup::Layer,
        ActionGroup::Transform,
        ActionGroup::View,
        ActionGroup::Tools,
        ActionGroup::Favorites,
        ActionGroup::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ActionGroup::File => "File",
            ActionGroup::Edit => "Edit",
            ActionGroup::Selection => "Selection",
            ActionGroup::Layer => "Layer",
            ActionGroup::Transform => "Transform",
            ActionGroup::View => "View",
            ActionGroup::Tools => "Tools",
            ActionGroup::Favorites => "Favorite colors",
            ActionGroup::Help => "Help",
        }
    }
}

impl ActionId {
    /// Every action, in editor order.
    pub fn all() -> Vec<ActionId> {
        use ActionId::*;
        let mut all = vec![
            New,
            Open,
            Save,
            SaveAs,
            Settings,
            Undo,
            Redo,
            Cut,
            Copy,
            Paste,
            PasteInPlace,
            PasteToNewLayer,
            Delete,
            SelectAll,
            Deselect,
            InvertSelection,
            SelectionToLayer,
            SelectionToLayerCut,
            MaskMode,
            Group,
            Ungroup,
            RotateCcw,
            RotateCw,
            FlipX,
            FlipY,
            FlipZ,
            SliceMode,
            WorkPlane,
            ZenMode,
            RenderMode,
            Projection,
            FrameSelection,
            FrameWorkArea,
            HomeView,
            FloorGrid,
            VoxelGrid,
            ModelBounds,
            Ghosts,
            References,
        ];
        all.extend(rizxel_engine::Tool::ALL.into_iter().map(Tool));
        all.extend((0..10).map(RecallFavorite));
        all.extend((0..10).map(StoreFavorite));
        all.push(UserGuide);
        all
    }

    /// Stable name in `settings.json`.
    pub fn id(self) -> String {
        use ActionId::*;
        let s = match self {
            New => "file.new",
            Open => "file.open",
            Save => "file.save",
            SaveAs => "file.save_as",
            Settings => "file.settings",
            Undo => "edit.undo",
            Redo => "edit.redo",
            Cut => "edit.cut",
            Copy => "edit.copy",
            Paste => "edit.paste",
            PasteInPlace => "edit.paste_in_place",
            PasteToNewLayer => "edit.paste_to_new_layer",
            Delete => "edit.delete",
            SelectAll => "selection.all",
            Deselect => "selection.none",
            InvertSelection => "selection.invert",
            SelectionToLayer => "selection.to_layer",
            SelectionToLayerCut => "selection.to_layer_cut",
            MaskMode => "selection.mask",
            Group => "layer.group",
            Ungroup => "layer.ungroup",
            RotateCcw => "transform.rotate_ccw",
            RotateCw => "transform.rotate_cw",
            FlipX => "transform.flip_x",
            FlipY => "transform.flip_y",
            FlipZ => "transform.flip_z",
            SliceMode => "view.slice_mode",
            WorkPlane => "view.work_plane",
            ZenMode => "view.zen",
            RenderMode => "view.render_mode",
            Projection => "view.projection",
            FrameSelection => "view.frame_selection",
            FrameWorkArea => "view.frame_work_area",
            HomeView => "view.home",
            FloorGrid => "view.floor_grid",
            VoxelGrid => "view.voxel_grid",
            ModelBounds => "view.model_bounds",
            Ghosts => "view.ghosts",
            References => "view.references",
            UserGuide => "help.user_guide",
            Tool(t) => return format!("tool.{}", t.label().to_ascii_lowercase().replace(' ', "_")),
            RecallFavorite(i) => return format!("favorite.recall.{i}"),
            StoreFavorite(i) => return format!("favorite.store.{i}"),
        };
        s.to_owned()
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().into_iter().find(|a| a.id() == id)
    }

    pub fn label(self) -> String {
        use ActionId::*;
        let s = match self {
            New => "New project",
            Open => "Open project",
            Save => "Save",
            SaveAs => "Save as",
            Settings => "Settings",
            Undo => "Undo",
            Redo => "Redo",
            Cut => "Cut (selection or layer)",
            Copy => "Copy (selection or layer)",
            Paste => "Paste at the cursor",
            PasteInPlace => "Paste in place",
            PasteToNewLayer => "Paste to new layer",
            Delete => "Delete selected voxels",
            SelectAll => "Select all",
            Deselect => "Deselect",
            InvertSelection => "Invert selection",
            SelectionToLayer => "Selection to a new layer (copy)",
            SelectionToLayerCut => "Selection to a new layer (cut)",
            MaskMode => "Mask Mode: protect the selection",
            Group => "Group the active layer",
            Ungroup => "Ungroup",
            RotateCcw => "Rotate 90° counterclockwise (facing axis)",
            RotateCw => "Rotate 90° clockwise (facing axis)",
            FlipX => "Flip X",
            FlipY => "Flip Y",
            FlipZ => "Flip Z",
            SliceMode => "Slice Mode on / off",
            WorkPlane => "Show or hide the work plane",
            ZenMode => "Zen mode",
            RenderMode => "Model / Render mode",
            Projection => "Perspective / orthographic",
            FrameSelection => "Frame selection (or the model)",
            FrameWorkArea => "Frame work area (model bounds)",
            HomeView => "Home view (reset camera)",
            FloorGrid => "Show or hide the floor grid",
            VoxelGrid => "Show or hide the voxel grid on the model",
            ModelBounds => "Show or hide the model bounds",
            Ghosts => "Ghosts: other layers semi-transparent",
            References => "Show or hide all reference images",
            UserGuide => "User guide (opens in the browser)",
            Tool(t) => return format!("{} tool", t.label()),
            RecallFavorite(i) => return format!("Select favorite color {i}"),
            StoreFavorite(i) => return format!("Store the selected color as favorite {i}"),
        };
        s.to_owned()
    }

    pub fn group(self) -> ActionGroup {
        use ActionId::*;
        match self {
            New | Open | Save | SaveAs | Settings => ActionGroup::File,
            Undo | Redo | Cut | Copy | Paste | PasteInPlace | PasteToNewLayer | Delete => {
                ActionGroup::Edit
            }
            SelectAll | Deselect | InvertSelection | SelectionToLayer | SelectionToLayerCut
            | MaskMode => ActionGroup::Selection,
            Group | Ungroup => ActionGroup::Layer,
            RotateCcw | RotateCw | FlipX | FlipY | FlipZ => ActionGroup::Transform,
            SliceMode | WorkPlane | ZenMode | RenderMode | Projection | FrameSelection
            | FrameWorkArea | HomeView | FloorGrid | VoxelGrid | ModelBounds | Ghosts
            | References => ActionGroup::View,
            Tool(_) => ActionGroup::Tools,
            RecallFavorite(_) | StoreFavorite(_) => ActionGroup::Favorites,
            UserGuide => ActionGroup::Help,
        }
    }

    /// The clipboard commands keep their chords: the windowing layer turns
    /// them into clipboard events before the keymap sees a key.
    pub fn is_fixed(self) -> bool {
        use ActionId::*;
        matches!(
            self,
            Cut | Copy | Paste | PasteInPlace | PasteToNewLayer | FlipX
        )
    }

    /// Holding the chord repeats the command (undo, redo, rotate).
    pub fn repeats(self) -> bool {
        use ActionId::*;
        matches!(self, Undo | Redo | RotateCcw | RotateCw)
    }

    /// Chords before any rebinding.
    pub fn default_chords(self) -> Vec<KeyChord> {
        use ActionId::*;
        type K = KeyChord;
        let digit = |i: u8| KEY_NAMES[26 + i as usize];
        match self {
            New => vec![K::ctrl("N")],
            Open => vec![K::ctrl("O")],
            Save => vec![K::ctrl("S")],
            SaveAs => vec![K::ctrl("S").with_shift()],
            Settings => vec![K::ctrl("Comma")],
            Undo => vec![K::ctrl("Z")],
            Redo => vec![K::ctrl("Y"), K::ctrl("Z").with_shift()],
            Cut => vec![K::ctrl("X")],
            Copy => vec![K::ctrl("C")],
            Paste => vec![K::ctrl("V")],
            PasteInPlace => vec![K::ctrl("V").with_alt()],
            PasteToNewLayer => vec![K::ctrl("V").with_shift()],
            Delete => vec![K::plain("Delete"), K::plain("Backspace")],
            SelectAll => vec![K::ctrl("A")],
            Deselect => vec![K::ctrl("D")],
            InvertSelection => vec![K::ctrl("I")],
            SelectionToLayer => vec![K::ctrl("J")],
            SelectionToLayerCut => vec![K::ctrl("J").with_shift()],
            MaskMode => vec![K::plain("M").with_shift()],
            Group => vec![K::ctrl("G")],
            Ungroup => vec![K::ctrl("G").with_shift()],
            RotateCcw => vec![K::ctrl("R")],
            RotateCw => vec![K::ctrl("R").with_shift()],
            FlipX => vec![K::ctrl("X").with_alt()],
            FlipY => vec![K::ctrl("Y").with_alt()],
            FlipZ => vec![K::ctrl("Z").with_alt()],
            SliceMode => vec![K::plain("Space")],
            WorkPlane => vec![K::plain("Slash")],
            ZenMode => vec![K::ctrl("F").with_shift()],
            RenderMode => vec![K::plain("F5")],
            // egui reports the numpad digits as the main ones.
            Projection => vec![K::plain("5")],
            FrameSelection => vec![K::plain("Period")],
            FrameWorkArea => vec![K::plain("Home")],
            HomeView => vec![K::plain("Home").with_shift()],
            FloorGrid => vec![K::plain("G")],
            VoxelGrid => vec![K::plain("G").with_shift()],
            ModelBounds => vec![K::plain("B").with_shift()],
            Ghosts => vec![K::plain("H").with_shift()],
            References => vec![K::plain("R").with_alt()],
            UserGuide => vec![K::plain("F1")],
            Tool(t) => {
                let letter = t.shortcut().to_string();
                vec![K::new(false, false, false, &letter).expect("tool keys are letters")]
            }
            RecallFavorite(i) => vec![K::ctrl(digit(i))],
            StoreFavorite(i) => vec![K::ctrl(digit(i)).with_shift()],
        }
    }
}

/// Two actions sharing a chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub chord: KeyChord,
    pub actions: Vec<ActionId>,
}

/// Why a rebinding was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeymapError {
    #[error("{0} is reserved (clipboard, Enter and Esc keep their meaning)")]
    Reserved(String),
    #[error("{0} keeps its clipboard shortcut")]
    Fixed(String),
}

/// Chords per action: the defaults plus the user's rebindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    /// Every action once, in [`ActionId::all`] order.
    bindings: Vec<(ActionId, Vec<KeyChord>)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: ActionId::all()
                .into_iter()
                .map(|a| (a, a.default_chords()))
                .collect(),
        }
    }
}

impl Keymap {
    /// Defaults with `overrides` (`action id → chords`) applied. Unknown
    /// actions and unparsable chords are skipped with a log line; a fixed
    /// action keeps its default.
    pub fn from_overrides(overrides: &BTreeMap<String, Vec<String>>) -> Self {
        let mut map = Self::default();
        for (id, chords) in overrides {
            let Some(action) = ActionId::from_id(id) else {
                log::warn!("settings: unknown keymap action {id:?}");
                continue;
            };
            let parsed: Option<Vec<KeyChord>> = chords.iter().map(|c| KeyChord::parse(c)).collect();
            match parsed {
                Some(chords) => {
                    if let Err(e) = map.set(action, chords) {
                        log::warn!("settings: keymap {id}: {e}");
                    }
                }
                None => log::warn!("settings: keymap {id}: invalid chord in {chords:?}"),
            }
        }
        map
    }

    /// Actions whose chords differ from the defaults, for `settings.json`.
    pub fn overrides(&self) -> BTreeMap<String, Vec<String>> {
        self.bindings
            .iter()
            .filter(|(a, chords)| a.default_chords() != **chords)
            .map(|(a, chords)| (a.id(), chords.iter().map(ToString::to_string).collect()))
            .collect()
    }

    pub fn chords(&self, action: ActionId) -> &[KeyChord] {
        self.bindings
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, c)| c.as_slice())
    }

    /// Menu text: the first chord, or empty when unbound.
    pub fn label(&self, action: ActionId) -> String {
        self.chords(action)
            .first()
            .map(KeyChord::label)
            .unwrap_or_default()
    }

    /// Every chord of `action` joined with ` / `.
    pub fn labels(&self, action: ActionId) -> String {
        let labels: Vec<String> = self.chords(action).iter().map(KeyChord::label).collect();
        labels.join(" / ")
    }

    /// Replaces the chords of `action`. Conflicts with other actions are
    /// allowed here (the editor shows them); clipboard keys are not.
    pub fn set(&mut self, action: ActionId, chords: Vec<KeyChord>) -> Result<(), KeymapError> {
        if action.is_fixed() {
            if chords == action.default_chords() {
                return Ok(());
            }
            return Err(KeymapError::Fixed(action.label()));
        }
        if let Some(c) = chords.iter().find(|c| c.is_reserved()) {
            return Err(KeymapError::Reserved(c.label()));
        }
        let mut unique: Vec<KeyChord> = Vec::new();
        for c in chords {
            if !unique.contains(&c) {
                unique.push(c);
            }
        }
        self.slot(action).1 = unique;
        Ok(())
    }

    fn slot(&mut self, action: ActionId) -> &mut (ActionId, Vec<KeyChord>) {
        let i = match self.bindings.iter().position(|(a, _)| *a == action) {
            Some(i) => i,
            None => {
                self.bindings.push((action, Vec::new()));
                self.bindings.len() - 1
            }
        };
        &mut self.bindings[i]
    }

    pub fn reset(&mut self, action: ActionId) {
        self.slot(action).1 = action.default_chords();
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    /// The action bound to `chord`, if exactly one is.
    pub fn action_for(&self, chord: &KeyChord) -> Option<ActionId> {
        let mut found = self
            .bindings
            .iter()
            .filter(|(_, chords)| chords.contains(chord))
            .map(|(a, _)| *a);
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }

    /// Chords bound to more than one action.
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut by_chord: BTreeMap<&KeyChord, Vec<ActionId>> = BTreeMap::new();
        for (action, chords) in &self.bindings {
            for c in chords {
                by_chord.entry(c).or_default().push(*action);
            }
        }
        by_chord
            .into_iter()
            .filter(|(_, actions)| actions.len() > 1)
            .map(|(chord, actions)| Conflict {
                chord: chord.clone(),
                actions,
            })
            .collect()
    }

    /// Whether `action` shares a chord with another action.
    pub fn in_conflict(&self, action: ActionId) -> bool {
        self.conflicts().iter().any(|c| c.actions.contains(&action))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keymap_has_no_conflicts() {
        assert_eq!(Keymap::default().conflicts(), Vec::new());
    }

    #[test]
    fn action_ids_round_trip_and_are_unique() {
        let all = ActionId::all();
        for (i, a) in all.iter().enumerate() {
            assert_eq!(ActionId::from_id(&a.id()), Some(*a), "{a:?}");
            assert!(all[i + 1..].iter().all(|b| b.id() != a.id()));
        }
    }

    #[test]
    fn chords_parse_and_print() {
        let c = KeyChord::parse("shift+ctrl+s").unwrap();
        assert_eq!(c.to_string(), "Ctrl+Shift+S");
        assert_eq!(
            KeyChord::parse("Cmd+Alt+/").unwrap().to_string(),
            "Ctrl+Alt+Slash"
        );
        assert_eq!(KeyChord::parse("Period").unwrap().label(), ".");
        assert!(KeyChord::parse("Hyper+A").is_none());
        assert!(KeyChord::parse("Ctrl+NoSuchKey").is_none());
        assert!(KeyChord::parse("").is_none());
        for a in ActionId::all() {
            for c in a.default_chords() {
                assert_eq!(KeyChord::parse(&c.to_string()), Some(c));
            }
        }
    }

    #[test]
    fn conflicting_chords_are_reported() {
        let mut map = Keymap::default();
        map.set(ActionId::Save, vec![KeyChord::parse("Ctrl+O").unwrap()])
            .unwrap();
        let conflicts = map.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].actions, vec![ActionId::Open, ActionId::Save]);
        assert!(map.in_conflict(ActionId::Open));
        assert_eq!(map.action_for(&KeyChord::parse("Ctrl+O").unwrap()), None);
        map.reset(ActionId::Save);
        assert!(map.conflicts().is_empty());
    }

    #[test]
    fn clipboard_chords_are_reserved() {
        let mut map = Keymap::default();
        let err = map.set(ActionId::Save, vec![KeyChord::parse("Ctrl+C").unwrap()]);
        assert!(matches!(err, Err(KeymapError::Reserved(_))));
        let err = map.set(ActionId::Copy, vec![KeyChord::parse("Ctrl+K").unwrap()]);
        assert!(matches!(err, Err(KeymapError::Fixed(_))));
        assert_eq!(map, Keymap::default());
    }

    #[test]
    fn overrides_round_trip() {
        let mut map = Keymap::default();
        map.set(
            ActionId::Tool(Tool::Brush),
            vec![KeyChord::parse("Q").unwrap()],
        )
        .unwrap();
        map.set(ActionId::Ghosts, Vec::new()).unwrap();
        let o = map.overrides();
        assert_eq!(o.len(), 2);
        assert_eq!(o["tool.brush"], vec!["Q".to_owned()]);
        assert_eq!(Keymap::from_overrides(&o), map);
        assert_eq!(map.label(ActionId::Ghosts), "");
        // Broken entries fall back to the defaults.
        let mut bad = o.clone();
        bad.insert("tool.brush".into(), vec!["Ctrl+Nope".into()]);
        bad.insert("no.such.action".into(), vec!["A".into()]);
        let back = Keymap::from_overrides(&bad);
        assert_eq!(
            back.chords(ActionId::Tool(Tool::Brush)),
            &[KeyChord::plain("B")]
        );
    }
}
