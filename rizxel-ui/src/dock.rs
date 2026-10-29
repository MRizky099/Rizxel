//! Dockable workspace, built on CADCraft's shared `craft_ui::docking` layer.
//!
//! Every panel can be dragged by its tab, re-docked as a tab or to any side of
//! another group, floated into its own window, resized by its dividers, closed
//! and reopened from *View > Panels*. The viewport is the protected canvas: it
//! cannot move or close, but other panels split around it. Every group is a
//! tab strip, so any panel dropped on a strip joins it as a sub-tab; a
//! double-click on a tab collapses its group to the strip.

use craft_ui::docking::{
    Action, DockArea, DockStyle, Layout, Location, Node, PanelLimits, Permissions, Placement,
    TabOverflow, TabSizing, Zone,
};
use craft_ui::layout::{SplitAxis, SplitSize};
use egui::{Id, Stroke, Ui};
use serde::{Deserialize, Serialize};

use rizxel_app::ViewMode;

use crate::theme;

/// Every dockable region of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Panel {
    Viewport,
    Tools,
    Options,
    Palette,
    Layers,
    Bounds,
    History,
    References,
    /// Render settings and the Render button (Render mode only).
    Render,
}

impl Panel {
    /// Panels offered in *View > Panels*, in menu order.
    pub const ALL: [Panel; 9] = [
        Panel::Viewport,
        Panel::Tools,
        Panel::Options,
        Panel::Palette,
        Panel::Layers,
        Panel::References,
        Panel::Bounds,
        Panel::History,
        Panel::Render,
    ];

    /// Panels a mode offers in *View > Panels*: Render mode has no editing
    /// or tracing panels, Model mode no Render panel.
    pub fn offered(mode: ViewMode) -> Vec<Panel> {
        let render_only = |p: &Panel| *p == Panel::Render;
        let model_only = |p: &Panel| {
            matches!(
                p,
                Panel::Tools | Panel::Palette | Panel::Bounds | Panel::References
            )
        };
        Panel::ALL
            .into_iter()
            .filter(|p| match mode {
                ViewMode::Model => !render_only(p),
                ViewMode::Render => !model_only(p),
            })
            .collect()
    }

    pub fn label(self) -> &'static str {
        match self {
            Panel::Viewport => "Viewport",
            Panel::Tools => "Tools",
            Panel::Options => "Tool Options",
            Panel::Palette => "Color Palette",
            Panel::Layers => "Layers",
            Panel::Bounds => "Bound Model",
            Panel::History => "History",
            Panel::References => "References",
            Panel::Render => "Render",
        }
    }

    pub fn permissions(self) -> Permissions {
        match self {
            Panel::Viewport => Permissions::PROTECTED,
            _ => Permissions::default(),
        }
    }

    /// Side of the viewport a panel reopens on when its old spot is gone.
    fn default_side(self) -> Zone {
        match self {
            Panel::Tools => Zone::Left,
            Panel::Options => Zone::Top,
            _ => Zone::Right,
        }
    }

    fn limits(self) -> PanelLimits {
        let min = match self {
            Panel::Viewport => egui::vec2(240.0, 160.0),
            Panel::Tools => egui::vec2(48.0, 120.0),
            Panel::Options => egui::vec2(200.0, theme::OPTION_BAR_HEIGHT),
            Panel::Palette | Panel::Layers | Panel::Bounds | Panel::History | Panel::References => {
                egui::vec2(200.0, 80.0)
            }
            Panel::Render => egui::vec2(240.0, 120.0),
        };
        PanelLimits {
            min,
            max: egui::vec2(1_000_000.0, 1_000_000.0),
        }
    }
}

fn permissions(panel: &Panel) -> Permissions {
    panel.permissions()
}

fn tabs(panel: Panel) -> Node<Panel> {
    Node::Tabs {
        panels: vec![panel],
        active: 0,
    }
}

fn split(axis: SplitAxis, size: SplitSize, first: Node<Panel>, second: Node<Panel>) -> Node<Panel> {
    Node::Split {
        axis,
        size,
        first: Box::new(first),
        second: Box::new(second),
    }
}

/// Dock layout plus where each closed panel used to live.
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub layout: Layout<Panel>,
    /// View mode this layout belongs to (its default and dock area id).
    mode: ViewMode,
    hidden: Vec<(Panel, Location<Panel>)>,
    /// Groups collapsed to their tab strip by a double-click.
    collapsed: Vec<Collapsed>,
}

/// A group shrunk to its tab strip, found again by any panel it held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Collapsed {
    panel: Panel,
    /// Split size the group had before collapsing, restored on expand.
    restore: SplitSize,
    /// Split size while collapsed; a divider drag that changes it expands.
    size: SplitSize,
}

impl Default for Workspace {
    /// Option bar on top, tools rail on the left, viewport in the middle and
    /// two tab groups on the right: Palette and References above, Layers,
    /// Bound Model and History below.
    fn default() -> Self {
        let inspector = split(
            SplitAxis::Vertical,
            SplitSize::FixedFirst(320.0),
            Node::Tabs {
                panels: vec![Panel::Palette, Panel::References],
                active: 0,
            },
            Node::Tabs {
                panels: vec![Panel::Layers, Panel::Bounds, Panel::History],
                active: 0,
            },
        );
        let center = split(
            SplitAxis::Horizontal,
            SplitSize::FixedSecond(288.0),
            tabs(Panel::Viewport),
            inspector,
        );
        let body = split(
            SplitAxis::Horizontal,
            SplitSize::FixedFirst(64.0),
            tabs(Panel::Tools),
            center,
        );
        let root = split(
            SplitAxis::Vertical,
            SplitSize::FixedFirst(theme::OPTION_BAR_HEIGHT + TAB_HEIGHT),
            tabs(Panel::Options),
            body,
        );
        Self {
            layout: Layout {
                root: Some(root),
                floating: Vec::new(),
            },
            mode: ViewMode::Model,
            hidden: Vec::new(),
            collapsed: Vec::new(),
        }
    }
}

impl Workspace {
    /// Render-mode layout: render bar on top, viewport in the middle, the
    /// Render panel on the right above Layers and History. No Tools,
    /// Palette or Bound Model.
    pub fn render() -> Self {
        let inspector = split(
            SplitAxis::Vertical,
            SplitSize::FixedSecond(240.0),
            tabs(Panel::Render),
            Node::Tabs {
                panels: vec![Panel::Layers, Panel::History],
                active: 0,
            },
        );
        let body = split(
            SplitAxis::Horizontal,
            SplitSize::FixedSecond(320.0),
            tabs(Panel::Viewport),
            inspector,
        );
        let root = split(
            SplitAxis::Vertical,
            SplitSize::FixedFirst(theme::OPTION_BAR_HEIGHT + TAB_HEIGHT),
            tabs(Panel::Options),
            body,
        );
        Self {
            layout: Layout {
                root: Some(root),
                floating: Vec::new(),
            },
            mode: ViewMode::Render,
            hidden: Vec::new(),
            collapsed: Vec::new(),
        }
    }

    /// The default layout of `mode`.
    pub fn for_mode(mode: ViewMode) -> Self {
        match mode {
            ViewMode::Model => Self::default(),
            ViewMode::Render => Self::render(),
        }
    }

    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    /// Dock area id: one per mode, so drag state never crosses layouts.
    fn area_id(&self) -> Id {
        Id::new(AREA_ID).with(self.mode == ViewMode::Render)
    }
}

/// One dock layout per view mode. Switching modes swaps them, and each keeps
/// the user's own changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Workspaces {
    model: Workspace,
    render: Workspace,
}

impl Default for Workspaces {
    fn default() -> Self {
        Self {
            model: Workspace::default(),
            render: Workspace::render(),
        }
    }
}

impl Workspaces {
    pub fn get(&self, mode: ViewMode) -> &Workspace {
        match mode {
            ViewMode::Model => &self.model,
            ViewMode::Render => &self.render,
        }
    }

    pub fn get_mut(&mut self, mode: ViewMode) -> &mut Workspace {
        match mode {
            ViewMode::Model => &mut self.model,
            ViewMode::Render => &mut self.render,
        }
    }
}

// ------------------------------------------------------------ layout.json

/// Version of `layout.json`; a file with another one is ignored.
pub const LAYOUT_VERSION: u32 = 1;

/// Size, position and maximized flag of the main window, in logical points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub size: [f32; 2],
    /// Outer top-left corner; `None` lets the OS center the window.
    pub position: Option<[f32; 2]>,
    pub maximized: bool,
}

impl WindowState {
    /// Smallest window size restored (the window's minimum size).
    pub const MIN_SIZE: [f32; 2] = [800.0, 480.0];

    /// The state to restore, or `None` when the saved values are unusable.
    pub fn sane(self) -> Option<Self> {
        let finite = self.size.iter().all(|v| v.is_finite())
            && self
                .position
                .is_none_or(|p| p.iter().all(|v| v.is_finite()));
        if !finite {
            return None;
        }
        let size = [
            self.size[0].clamp(Self::MIN_SIZE[0], 16_384.0),
            self.size[1].clamp(Self::MIN_SIZE[1], 16_384.0),
        ];
        // A window far off any plausible desktop is re-centered.
        let position = self
            .position
            .filter(|p| p.iter().all(|v| (-4_000.0..=32_000.0).contains(v)));
        Some(Self {
            size,
            position,
            maximized: self.maximized,
        })
    }
}

/// What `layout.json` keeps of one workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedWorkspace {
    layout: Layout<Panel>,
    #[serde(default)]
    hidden: Vec<(Panel, Location<Panel>)>,
    #[serde(default)]
    collapsed: Vec<Collapsed>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LayoutFile {
    version: u32,
    model: Option<SavedWorkspace>,
    render: Option<SavedWorkspace>,
    window: Option<WindowState>,
}

impl Workspace {
    fn saved(&self) -> SavedWorkspace {
        SavedWorkspace {
            layout: self.layout.clone(),
            hidden: self.hidden.clone(),
            collapsed: self.collapsed.clone(),
        }
    }

    /// A saved workspace for `mode`, or `None` when it is broken: invalid
    /// tree, no viewport, or panels the mode doesn't offer.
    fn from_saved(mode: ViewMode, saved: SavedWorkspace) -> Option<Self> {
        saved.layout.validate().ok()?;
        let offered = Panel::offered(mode);
        let panels = saved.layout.panels();
        let fits = saved.layout.contains(&Panel::Viewport)
            && panels.iter().all(|p| offered.contains(p))
            && saved
                .hidden
                .iter()
                .all(|(p, _)| offered.contains(p) && !panels.contains(&p));
        let mut ws = Self {
            layout: saved.layout,
            mode,
            hidden: saved.hidden,
            collapsed: saved.collapsed,
        };
        ws.prune_collapsed();
        fits.then_some(ws)
    }
}

impl Workspaces {
    /// The layouts saved in `file`. A missing, corrupt or outdated file, or
    /// a broken layout of one mode, gives that mode's default.
    pub fn load(file: &std::path::Path) -> (Self, Option<WindowState>) {
        let Some(saved) = read_layout_file(file) else {
            return (Self::default(), None);
        };
        let pick = |mode, ws: Option<SavedWorkspace>| {
            ws.and_then(|w| Workspace::from_saved(mode, w))
                .unwrap_or_else(|| {
                    log::info!("layout: {mode:?} layout unusable; using the default");
                    Workspace::for_mode(mode)
                })
        };
        let docks = Self {
            model: pick(ViewMode::Model, saved.model),
            render: pick(ViewMode::Render, saved.render),
        };
        (docks, saved.window.and_then(WindowState::sane))
    }

    /// Writes both layouts and the window state to `file`.
    pub fn save(&self, file: &std::path::Path, window: Option<WindowState>) -> std::io::Result<()> {
        let data = LayoutFile {
            version: LAYOUT_VERSION,
            model: Some(self.model.saved()),
            render: Some(self.render.saved()),
            window,
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(&data).map_err(std::io::Error::other)?;
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, file)
    }
}

/// Only the window state of `file` (read before the window opens).
pub fn load_window_state(file: &std::path::Path) -> Option<WindowState> {
    read_layout_file(file)?.window.and_then(WindowState::sane)
}

fn read_layout_file(file: &std::path::Path) -> Option<LayoutFile> {
    let bytes = std::fs::read(file).ok()?;
    match serde_json::from_slice::<LayoutFile>(&bytes) {
        Ok(f) if f.version == LAYOUT_VERSION => Some(f),
        Ok(f) => {
            log::info!("layout: version {} ignored", f.version);
            None
        }
        Err(e) => {
            log::warn!(
                "layout: {} unreadable ({e}); using the defaults",
                file.display()
            );
            None
        }
    }
}

/// Height of a dock tab strip.
const TAB_HEIGHT: f32 = 26.0;
const AREA_ID: &str = "rizxel-docking";

impl Workspace {
    /// Applies one docking action atomically; closing remembers the spot.
    pub fn apply(&mut self, action: Action<Panel>) -> Result<(), String> {
        let mut next = self.clone();
        if let Action::Close { panel } = &action {
            let location = next.layout.location(panel).map_err(|e| e.to_string())?;
            next.hidden.retain(|(id, _)| id != panel);
            next.hidden.push((*panel, location));
        }
        next.layout
            .apply_with_permissions(action, permissions)
            .map_err(|e| e.to_string())?;
        *self = next;
        Ok(())
    }

    pub fn is_visible(&self, panel: Panel) -> bool {
        self.layout.contains(&panel)
    }

    /// Shows or hides a panel. Reopening restores its last group, side, size
    /// or floating rect, falling back to its default side of the viewport.
    pub fn set_visible(&mut self, panel: Panel, visible: bool) -> Result<(), String> {
        if self.is_visible(panel) == visible {
            return Ok(());
        }
        if !visible {
            return self.apply(Action::Close { panel });
        }
        let mut next = self.clone();
        let location = next
            .hidden
            .iter()
            .find(|(id, _)| *id == panel)
            .map(|(_, l)| l.clone());
        next.hidden.retain(|(id, _)| *id != panel);
        let restored = location.is_some_and(|l| next.layout.restore(panel, &l).is_ok());
        if !restored {
            next.layout
                .apply_with_permissions(
                    Action::OpenAt {
                        panel,
                        anchor: Panel::Viewport,
                        placement: Placement::Split(panel.default_side()),
                    },
                    permissions,
                )
                .map_err(|e| e.to_string())?;
        }
        let _ = next.layout.apply(Action::Activate { panel });
        *self = next;
        Ok(())
    }

    /// Draws the workspace and applies the gestures it produced. Errors are
    /// returned so the caller can surface them in the status bar.
    pub fn show(&mut self, ui: &mut Ui, mut body: impl FnMut(&mut Ui, &Panel)) -> Option<String> {
        self.prune_collapsed();
        let style = style(ui);
        let collapsed: Vec<Panel> = Panel::ALL
            .into_iter()
            .filter(|p| self.is_collapsed(*p))
            .collect();
        let area = self.area_id();
        let output = DockArea::new(area).show_with_limits(
            ui,
            &self.layout,
            &style,
            |panel| panel.label().to_owned(),
            permissions,
            |panel| {
                let mut limits = panel.limits();
                if collapsed.contains(panel) {
                    limits.min.y = 0.0;
                }
                limits
            },
            |ui, panel| {
                if !collapsed.contains(panel) {
                    body(ui, panel);
                }
            },
        );
        let mut error = output.error.map(|e| format!("Panel layout error: {e}"));
        for action in output.actions {
            if let Err(e) = self.apply(action) {
                error = Some(format!("Panel move was not applied: {e}"));
            }
        }
        // A double-click on a tab collapses its group to the strip, or
        // expands it again.
        let tab_id = |panel: &Panel| area.with(("tab", panel));
        if let Some(panel) = Panel::ALL.into_iter().find(|p| {
            ui.ctx()
                .read_response(tab_id(p))
                .is_some_and(|r| r.double_clicked())
        }) {
            self.toggle_collapsed(panel);
        }
        error
    }

    /// Whether `panel`'s group is collapsed to its tab strip.
    pub fn is_collapsed(&self, panel: Panel) -> bool {
        self.collapsed_entry(panel).is_some()
    }

    fn collapsed_entry(&self, panel: Panel) -> Option<usize> {
        let group = group_of(&self.layout, panel)?;
        let slot = self
            .layout
            .root
            .as_ref()
            .and_then(|root| group_slot(root, panel, &mut Vec::new()))?;
        self.collapsed
            .iter()
            .position(|c| group.contains(&c.panel) && slot.size == c.size)
    }

    /// Drops collapse records whose group moved, was resized or split up.
    fn prune_collapsed(&mut self) {
        let keep: Vec<bool> = self
            .collapsed
            .iter()
            .map(|c| self.collapsed_entry(c.panel).is_some())
            .collect();
        let mut keep = keep.into_iter();
        self.collapsed.retain(|_| keep.next().unwrap_or(false));
    }

    /// Collapses `panel`'s group to its tab strip, or expands it. Only a group
    /// stacked above or below another one can collapse; returns whether
    /// anything changed.
    pub fn toggle_collapsed(&mut self, panel: Panel) -> bool {
        self.prune_collapsed();
        let Some(slot) = self
            .layout
            .root
            .as_ref()
            .and_then(|root| group_slot(root, panel, &mut Vec::new()))
        else {
            return false;
        };
        if let Some(index) = self.collapsed_entry(panel) {
            let entry = self.collapsed.remove(index);
            return self
                .layout
                .apply(Action::ResizeSplit {
                    path: slot.path,
                    size: entry.restore,
                })
                .is_ok();
        }
        let size = if slot.second {
            SplitSize::FixedSecond(TAB_HEIGHT)
        } else {
            SplitSize::FixedFirst(TAB_HEIGHT)
        };
        let applied = self
            .layout
            .apply(Action::ResizeSplit {
                path: slot.path,
                size,
            })
            .is_ok();
        if applied {
            self.collapsed.push(Collapsed {
                panel,
                restore: slot.size,
                size,
            });
        }
        applied
    }

    /// Back to the default arrangement, cancelling any drag in flight.
    pub fn reset(&mut self, ctx: &egui::Context) {
        craft_ui::docking::cancel_drag::<Panel>(ctx, self.area_id());
        *self = Self::for_mode(self.mode);
    }
}

/// Members of the tab group holding `panel`, docked or floating.
fn group_of(layout: &Layout<Panel>, panel: Panel) -> Option<Vec<Panel>> {
    fn walk(node: &Node<Panel>, panel: Panel) -> Option<Vec<Panel>> {
        match node {
            Node::Tabs { panels, .. } => panels.contains(&panel).then(|| panels.clone()),
            Node::Stack { entries } => entries
                .iter()
                .any(|e| e.panel == panel)
                .then(|| vec![panel]),
            Node::Split { first, second, .. } => walk(first, panel).or_else(|| walk(second, panel)),
        }
    }
    layout
        .root
        .as_ref()
        .and_then(|root| walk(root, panel))
        .or_else(|| {
            layout
                .floating
                .iter()
                .find(|f| f.panels.contains(&panel))
                .map(|f| f.panels.clone())
        })
}

/// A tab group that is a direct child of a top/bottom split.
struct GroupSlot {
    /// Path from the root to that split: `false` first child, `true` second.
    path: Vec<bool>,
    /// The group is the split's second (bottom) child.
    second: bool,
    size: SplitSize,
}

fn group_slot(node: &Node<Panel>, panel: Panel, path: &mut Vec<bool>) -> Option<GroupSlot> {
    let Node::Split {
        axis,
        size,
        first,
        second,
    } = node
    else {
        return None;
    };
    let holds = |n: &Node<Panel>| matches!(n, Node::Tabs { panels, .. } if panels.contains(&panel));
    if *axis == SplitAxis::Vertical {
        for (child, is_second) in [(first, false), (second, true)] {
            if holds(child) {
                return Some(GroupSlot {
                    path: path.clone(),
                    second: is_second,
                    size: *size,
                });
            }
        }
    }
    for (child, is_second) in [(first, false), (second, true)] {
        path.push(is_second);
        let found = group_slot(child, panel, path);
        path.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

fn style(ui: &Ui) -> DockStyle {
    let mut style = DockStyle::from_ui(ui);
    style.tab_height = TAB_HEIGHT;
    // Every panel sets its own minimum size (`Panel::limits`); a zero floor
    // lets a collapsed group shrink to its tab strip.
    style.min_pane = 0.0;
    style.background = theme::BG_PANEL;
    style.tab_background = theme::BG_HEADER;
    style.active_background = theme::BG_PANEL;
    style.text = theme::TEXT;
    style.inactive_text = theme::TEXT_DIM;
    style.border = Stroke::new(1.0, theme::BORDER);
    style.accent = theme::ACCENT;
    style.font = theme::regular(12.0);
    // Tabs share their strip's width; strips with too many tabs scroll.
    style.tab_sizing = TabSizing::Fill;
    style.tab_overflow = TabOverflow::Scroll;
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_holds_every_panel_once() {
        for mode in [ViewMode::Model, ViewMode::Render] {
            let ws = Workspace::for_mode(mode);
            assert!(ws.layout.validate().is_ok());
            for panel in Panel::ALL {
                let offered = Panel::offered(mode).contains(&panel);
                assert_eq!(ws.is_visible(panel), offered, "{mode:?}: {panel:?}");
            }
        }
    }

    #[test]
    fn render_layout_keeps_layers_next_to_the_render_panel() {
        let ws = Workspace::render();
        let groups = groups(&ws);
        assert!(groups.contains(&vec![Panel::Render]));
        assert!(groups.contains(&vec![Panel::Layers, Panel::History]));
        for hidden in [Panel::Tools, Panel::Palette, Panel::Bounds] {
            assert!(!ws.is_visible(hidden), "{hidden:?}");
        }
    }

    #[test]
    fn switching_modes_restores_each_layout() {
        let mut docks = Workspaces::default();
        docks
            .get_mut(ViewMode::Model)
            .set_visible(Panel::Palette, false)
            .unwrap();
        docks
            .get_mut(ViewMode::Render)
            .set_visible(Panel::History, false)
            .unwrap();
        // Back and forth: each mode keeps its own change.
        assert!(!docks.get(ViewMode::Model).is_visible(Panel::Palette));
        assert!(docks.get(ViewMode::Model).is_visible(Panel::History));
        assert!(!docks.get(ViewMode::Render).is_visible(Panel::History));
        assert!(docks.get(ViewMode::Render).is_visible(Panel::Render));
        // Reset goes back to the mode's own default.
        let ctx = egui::Context::default();
        docks.get_mut(ViewMode::Render).reset(&ctx);
        assert_eq!(
            docks.get(ViewMode::Render).layout,
            Workspace::render().layout
        );
        assert!(!docks.get(ViewMode::Model).is_visible(Panel::Palette));
    }

    fn tab_groups(node: &Node<Panel>, out: &mut Vec<Vec<Panel>>) {
        match node {
            Node::Tabs { panels, .. } => out.push(panels.clone()),
            Node::Stack { .. } => panic!("stack in the layout"),
            Node::Split { first, second, .. } => {
                tab_groups(first, out);
                tab_groups(second, out);
            }
        }
    }

    fn groups(ws: &Workspace) -> Vec<Vec<Panel>> {
        let mut out = Vec::new();
        tab_groups(ws.layout.root.as_ref().unwrap(), &mut out);
        out
    }

    #[test]
    fn default_layout_is_tab_groups_only() {
        let groups = groups(&Workspace::default());
        assert!(groups.contains(&vec![Panel::Palette, Panel::References]));
        assert!(groups.contains(&vec![Panel::Layers, Panel::Bounds, Panel::History]));
    }

    #[test]
    fn dropping_on_a_strip_makes_sub_tabs() {
        let mut ws = Workspace::default();
        ws.apply(Action::Move {
            panel: Panel::Palette,
            anchor: Panel::Layers,
            placement: Placement::Tab { before: None },
        })
        .unwrap();
        let groups = groups(&ws);
        let layers = groups.iter().find(|g| g.contains(&Panel::Layers)).unwrap();
        assert!(layers.contains(&Panel::Palette) && layers.contains(&Panel::Bounds));
        assert_eq!(
            groups
                .iter()
                .flatten()
                .filter(|p| **p == Panel::Palette)
                .count(),
            1
        );
    }

    #[test]
    fn dropping_on_an_edge_splits() {
        let mut ws = Workspace::default();
        ws.apply(Action::Move {
            panel: Panel::Bounds,
            anchor: Panel::Layers,
            placement: Placement::Split(Zone::Right),
        })
        .unwrap();
        let groups = groups(&ws);
        assert!(groups.contains(&vec![Panel::Bounds]));
        assert!(groups.contains(&vec![Panel::Layers, Panel::History]));
        assert!(ws.layout.validate().is_ok());
    }

    #[test]
    fn groups_collapse_to_their_strip_and_back() {
        let mut ws = Workspace::default();
        let before = ws.layout.clone();
        assert!(ws.toggle_collapsed(Panel::Bounds));
        assert!(ws.is_collapsed(Panel::Layers) && ws.is_collapsed(Panel::History));
        assert!(!ws.is_collapsed(Panel::Palette));
        assert!(ws.toggle_collapsed(Panel::History));
        assert!(!ws.is_collapsed(Panel::Layers));
        assert_eq!(ws.layout, before);

        // The top group collapses too; the viewport and tools rail can't.
        assert!(ws.toggle_collapsed(Panel::Palette));
        assert!(ws.is_collapsed(Panel::References));
        assert!(!ws.toggle_collapsed(Panel::Tools));
        assert!(!ws.toggle_collapsed(Panel::Viewport));

        // Moving a panel out ends the collapse record of the group it left.
        ws.apply(Action::Move {
            panel: Panel::Palette,
            anchor: Panel::Layers,
            placement: Placement::Tab { before: None },
        })
        .unwrap();
        ws.apply(Action::Move {
            panel: Panel::References,
            anchor: Panel::Layers,
            placement: Placement::Tab { before: None },
        })
        .unwrap();
        ws.prune_collapsed();
        assert!(ws.collapsed.is_empty());
    }

    #[test]
    fn closed_panel_reopens_where_it_was() {
        let mut ws = Workspace::default();
        let before = ws.layout.clone();
        ws.set_visible(Panel::Layers, false).unwrap();
        assert!(!ws.is_visible(Panel::Layers));
        ws.set_visible(Panel::Layers, true).unwrap();
        assert_eq!(ws.layout, before);
    }

    #[test]
    fn references_panel_floats_and_reopens() {
        let mut ws = Workspace::default();
        ws.apply(Action::Float {
            panel: Panel::References,
            rect: [50.0, 50.0, 300.0, 400.0],
        })
        .unwrap();
        assert_eq!(ws.layout.floating.len(), 1);
        let floating = ws.layout.clone();
        ws.set_visible(Panel::References, false).unwrap();
        assert!(!ws.is_visible(Panel::References));
        ws.set_visible(Panel::References, true).unwrap();
        assert_eq!(ws.layout, floating, "reopens where it floated");
        ws.apply(Action::Move {
            panel: Panel::References,
            anchor: Panel::Layers,
            placement: Placement::Tab { before: None },
        })
        .unwrap();
        assert!(ws.layout.floating.is_empty() && ws.is_visible(Panel::References));
    }

    fn temp_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("rizxel_layout_{name}_{}", std::process::id()))
            .join("layout.json")
    }

    #[test]
    fn layout_and_window_survive_a_save() {
        let file = temp_file("rt");
        let mut docks = Workspaces::default();
        let model = docks.get_mut(ViewMode::Model);
        model.set_visible(Panel::History, false).unwrap();
        model
            .apply(Action::Float {
                panel: Panel::References,
                rect: [100.0, 120.0, 300.0, 200.0],
            })
            .unwrap();
        assert!(model.toggle_collapsed(Panel::Layers));
        let window = WindowState {
            size: [1280.0, 720.0],
            position: Some([40.0, 30.0]),
            maximized: true,
        };
        docks.save(&file, Some(window)).unwrap();
        let (back, w) = Workspaces::load(&file);
        assert_eq!(back, docks);
        assert_eq!(w, Some(window));
        assert_eq!(load_window_state(&file), Some(window));
        // The closed panel still reopens where it was.
        let mut back = back;
        back.get_mut(ViewMode::Model)
            .set_visible(Panel::History, true)
            .unwrap();
        assert!(groups(back.get(ViewMode::Model)).contains(&vec![
            Panel::Layers,
            Panel::Bounds,
            Panel::History
        ]));
        std::fs::remove_dir_all(file.parent().unwrap()).ok();
    }

    #[test]
    fn broken_layout_files_fall_back_to_the_defaults() {
        let file = temp_file("broken");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        for bytes in [
            &b"{ nope"[..],
            br#"{"version": 99, "model": null, "render": null, "window": null}"#,
        ] {
            std::fs::write(&file, bytes).unwrap();
            assert_eq!(Workspaces::load(&file), (Workspaces::default(), None));
        }
        assert_eq!(
            Workspaces::load(&temp_file("missing")).0,
            Workspaces::default()
        );

        // A Render panel in the Model layout breaks only that layout.
        let mut docks = Workspaces::default();
        docks.render.set_visible(Panel::History, false).unwrap();
        let mut bad = docks.clone();
        bad.model = Workspace::render();
        bad.save(&file, None).unwrap();
        let (back, _) = Workspaces::load(&file);
        assert_eq!(back.model, Workspace::default());
        assert_eq!(back.render, docks.render);

        // Duplicate panels fail validation.
        let mut dup = Workspaces::default();
        if let Some(Node::Split { first, .. }) = &mut dup.model.layout.root {
            **first = tabs(Panel::Viewport);
        }
        dup.save(&file, None).unwrap();
        assert_eq!(Workspaces::load(&file).0.model, Workspace::default());
        std::fs::remove_dir_all(file.parent().unwrap()).ok();
    }

    #[test]
    fn window_state_is_sanitized() {
        let w = WindowState {
            size: [10.0, f32::NAN],
            position: None,
            maximized: false,
        };
        assert_eq!(w.sane(), None);
        let w = WindowState {
            size: [100.0, 100.0],
            position: Some([-90_000.0, 0.0]),
            maximized: false,
        };
        let s = w.sane().unwrap();
        assert_eq!(s.size, WindowState::MIN_SIZE);
        assert_eq!(s.position, None);
    }

    #[test]
    fn viewport_is_protected() {
        let mut ws = Workspace::default();
        assert!(ws
            .apply(Action::Close {
                panel: Panel::Viewport
            })
            .is_err());
        let float = Action::Float {
            panel: Panel::Viewport,
            rect: [0.0, 0.0, 300.0, 300.0],
        };
        assert!(ws.apply(float).is_err());
        assert!(ws.is_visible(Panel::Viewport));
    }

    #[test]
    fn panels_float_and_redock() {
        let mut ws = Workspace::default();
        ws.apply(Action::Float {
            panel: Panel::Palette,
            rect: [100.0, 100.0, 300.0, 400.0],
        })
        .unwrap();
        assert_eq!(ws.layout.floating.len(), 1);
        ws.apply(Action::Move {
            panel: Panel::Palette,
            anchor: Panel::Tools,
            placement: Placement::Tab { before: None },
        })
        .unwrap();
        assert!(ws.layout.floating.is_empty());
        assert!(ws.is_visible(Panel::Palette));
    }
}
