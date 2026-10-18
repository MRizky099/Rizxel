//! Resize handles for the frameless window.
//!
//! The OS no longer provides resize borders when decorations are disabled, so
//! thin hit zones along the window edges start a native resize via
//! [`ViewportCommand::BeginResize`].

use egui::{
    CursorIcon, Id, LayerId, Order, Pos2, Rect, ResizeDirection, Sense, Stroke, StrokeKind, Vec2,
    ViewportCommand,
};

use crate::theme;

/// Thickness of edge hit zones (px).
const EDGE: f32 = 5.0;
/// Size of corner hit zones (px).
const CORNER: f32 = 12.0;

fn is_maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

/// Registers resize hit zones on the window edges and corners.
pub fn handle_edges(ctx: &egui::Context) {
    if is_maximized(ctx) {
        return;
    }
    let r = ctx.content_rect();

    // (direction, zone rect, cursor) — corners first so they win on overlap.
    let zones = [
        (
            ResizeDirection::NorthWest,
            Rect::from_min_size(r.min, Vec2::splat(CORNER)),
            CursorIcon::ResizeNorthWest,
        ),
        (
            ResizeDirection::NorthEast,
            Rect::from_min_size(Pos2::new(r.max.x - CORNER, r.min.y), Vec2::splat(CORNER)),
            CursorIcon::ResizeNorthEast,
        ),
        (
            ResizeDirection::SouthWest,
            Rect::from_min_size(Pos2::new(r.min.x, r.max.y - CORNER), Vec2::splat(CORNER)),
            CursorIcon::ResizeSouthWest,
        ),
        (
            ResizeDirection::SouthEast,
            Rect::from_min_size(r.max - Vec2::splat(CORNER), Vec2::splat(CORNER)),
            CursorIcon::ResizeSouthEast,
        ),
        (
            ResizeDirection::North,
            Rect::from_min_max(r.min, Pos2::new(r.max.x, r.min.y + EDGE)),
            CursorIcon::ResizeNorth,
        ),
        (
            ResizeDirection::South,
            Rect::from_min_max(Pos2::new(r.min.x, r.max.y - EDGE), r.max),
            CursorIcon::ResizeSouth,
        ),
        (
            ResizeDirection::West,
            Rect::from_min_max(r.min, Pos2::new(r.min.x + EDGE, r.max.y)),
            CursorIcon::ResizeWest,
        ),
        (
            ResizeDirection::East,
            Rect::from_min_max(Pos2::new(r.max.x - EDGE, r.min.y), r.max),
            CursorIcon::ResizeEast,
        ),
    ];

    // A foreground area so the zones sit above every panel.
    egui::Area::new(Id::new("window_resize_zones"))
        .order(Order::Foreground)
        .fixed_pos(r.min)
        .interactable(true)
        .show(ctx, |ui| {
            for (i, (dir, rect, cursor)) in zones.into_iter().enumerate() {
                let resp = ui.interact(rect, Id::new(("resize_zone", i)), Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ctx.set_cursor_icon(cursor);
                }
                if resp.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
                }
            }
        });
}

/// 1px border around the window (frameless windows otherwise blend into the desktop).
pub fn paint_border(ctx: &egui::Context) {
    if is_maximized(ctx) {
        return;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("window_border")));
    painter.rect_stroke(
        ctx.content_rect(),
        0.0,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
}
