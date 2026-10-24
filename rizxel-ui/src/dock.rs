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

use crate::theme;

/// Every dockable region of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Viewport,
    Tools,
    Options,
    Palette,
    Layers,
    Bounds,
    History,
    References,
}

impl Panel {
    /// Panels offered in *View > Panels*, in menu order.
    pub const ALL: [Panel; 8] = [
        Panel::Viewport,
        Panel::Tools,
        Panel::Options,
        Panel::Palette,
        Panel::Layers,
        Panel::References,
        Panel::Bounds,
        Panel::History,
    ];

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
#[derive(Debug, Clone)]
pub struct Workspace {
    pub layout: Layout<Panel>,
    hidden: Vec<(Panel, Location<Panel>)>,
    /// Groups collapsed to their tab strip by a double-click.
    collapsed: Vec<Collapsed>,
}

/// A group shrunk to its tab strip, found again by any panel it held.
#[derive(Debug, Clone, PartialEq)]
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
            hidden: Vec::new(),
            collapsed: Vec::new(),
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
        let output = DockArea::new(Id::new(AREA_ID)).show_with_limits(
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
        let tab_id = |panel: &Panel| Id::new(AREA_ID).with(("tab", panel));
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
        craft_ui::docking::cancel_drag::<Panel>(ctx, Id::new(AREA_ID));
        *self = Self::default();
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
        let ws = Workspace::default();
        assert!(ws.layout.validate().is_ok());
        for panel in Panel::ALL {
            assert!(ws.is_visible(panel), "{panel:?} missing");
        }
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
