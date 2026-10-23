//! Dockable workspace, built on CADCraft's shared `craft_ui::docking` layer.
//!
//! Every panel can be dragged by its tab, re-docked as a tab or to any side of
//! another group, floated into its own window, resized by its dividers, closed
//! and reopened from *View > Panels*. The viewport is the protected canvas: it
//! cannot move or close, but other panels split around it.

use craft_ui::docking::{
    Action, DockArea, DockStyle, Layout, Location, Node, PanelLimits, Permissions, Placement,
    StackEntry, Zone,
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
}

impl Default for Workspace {
    /// Option bar on top, tools rail on the left, viewport in the middle and
    /// a collapsible inspector stack on the right — the prototype's arrangement.
    fn default() -> Self {
        let inspector = Node::Stack {
            entries: vec![
                StackEntry {
                    panel: Panel::Palette,
                    open: true,
                    height: Some(320.0),
                },
                StackEntry {
                    panel: Panel::Layers,
                    open: true,
                    height: None,
                },
                StackEntry {
                    panel: Panel::References,
                    open: false,
                    height: Some(320.0),
                },
                StackEntry {
                    panel: Panel::Bounds,
                    open: false,
                    height: Some(200.0),
                },
                StackEntry {
                    panel: Panel::History,
                    open: false,
                    height: Some(220.0),
                },
            ],
        };
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
        let style = style(ui);
        let output = DockArea::new(Id::new(AREA_ID)).show_with_limits(
            ui,
            &self.layout,
            &style,
            |panel| panel.label().to_owned(),
            permissions,
            |panel| panel.limits(),
            |ui, panel| body(ui, panel),
        );
        let mut error = output.error.map(|e| format!("Panel layout error: {e}"));
        for action in output.actions {
            if let Err(e) = self.apply(action) {
                error = Some(format!("Panel move was not applied: {e}"));
            }
        }
        error
    }

    /// Back to the default arrangement, cancelling any drag in flight.
    pub fn reset(&mut self, ctx: &egui::Context) {
        craft_ui::docking::cancel_drag::<Panel>(ctx, Id::new(AREA_ID));
        *self = Self::default();
    }
}

fn style(ui: &Ui) -> DockStyle {
    let mut style = DockStyle::from_ui(ui);
    style.tab_height = TAB_HEIGHT;
    style.min_pane = 48.0;
    style.background = theme::BG_PANEL;
    style.tab_background = theme::BG_HEADER;
    style.active_background = theme::BG_PANEL;
    style.text = theme::TEXT;
    style.inactive_text = theme::TEXT_DIM;
    style.border = Stroke::new(1.0, theme::BORDER);
    style.accent = theme::ACCENT;
    style.font = theme::regular(12.0);
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
