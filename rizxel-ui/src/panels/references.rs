//! References dock panel: the active document's reference images (list,
//! show / hide all, add from files) and the selected image's placement.

use std::collections::HashMap;

use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::{AppState, Document};
use rizxel_math::{DepthMode, ImageData, RefPlane, ReferenceImage, SliceFilter};

use super::UiState;
use crate::theme;
use crate::widgets::{icon_button, IconStyle};

/// Height of one reference row.
const ROW_HEIGHT: f32 = 36.0;
/// Thumbnail side in points.
const THUMB: f32 = 28.0;
/// Thumbnail texture side in pixels.
const THUMB_PX: u32 = 64;

/// Panel thumbnails by [`ImageData::key`]; the `ImageData` keeps the key
/// from being reused while cached.
#[derive(Default)]
pub struct Thumbnails {
    textures: HashMap<usize, (ImageData, TextureHandle)>,
}

impl Thumbnails {
    fn get(&mut self, ctx: &egui::Context, image: &ImageData) -> Option<TextureHandle> {
        if let Some((_, t)) = self.textures.get(&image.key()) {
            return Some(t.clone());
        }
        let rgba = rizxel_app::decode_reference(image).ok()?;
        let full = image::RgbaImage::from_raw(rgba.width, rgba.height, rgba.pixels)?;
        let small =
            image::imageops::thumbnail(&full, THUMB_PX.min(rgba.width), THUMB_PX.min(rgba.height));
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [small.width() as usize, small.height() as usize],
            small.as_raw(),
        );
        let texture = ctx.load_texture(
            format!("reference_thumb_{}", image.key()),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.textures
            .insert(image.key(), (image.clone(), texture.clone()));
        Some(texture)
    }

    /// Frees thumbnails of images no open document uses.
    fn retain(&mut self, state: &AppState) {
        self.textures.retain(|key, _| {
            state
                .documents
                .iter()
                .any(|d| d.data.references.iter().any(|r| r.image.key() == *key))
        });
    }
}

/// Drag payload for reordering reference rows.
#[derive(Clone, Copy)]
struct RefDrag(usize);

enum RowAction {
    Select(usize),
    ToggleVisible(usize),
    ToggleLocked(usize),
    StartRename(usize),
    Duplicate(usize),
    Replace(usize),
    ResetSize(usize),
    Move { from: usize, to: usize },
    Delete(usize),
}

pub fn show(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    ui_state.reference_thumbs.retain(state);
    if state.doc().is_none() {
        return;
    }
    let mut action = None;
    egui::Frame::NONE.inner_margin(8).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        header(ui, state, ui_state);
        let Some(doc) = state.doc() else { return };
        let count = doc.data.references.len();
        if count == 0 {
            ui.label(
                RichText::new(
                    "Add PNG or JPEG concept art to trace, or drop image files on the viewport.",
                )
                .size(11.0)
                .color(theme::p().text_dim),
            );
            return;
        }
        // Later references draw on top: list them first.
        for i in (0..count).rev() {
            if let Some(a) = row(ui, state, ui_state, i) {
                action = Some(a);
            }
        }
        ui.add_space(6.0);
        properties(ui, state, ui_state);
    });

    let Some(action) = action else { return };
    if let RowAction::Replace(i) = action {
        replace_image(state, i);
        return;
    }
    let Some(doc) = state.doc_mut() else { return };
    if !matches!(action, RowAction::Select(_) | RowAction::StartRename(_)) {
        ui_state.renaming_reference = None;
    }
    match action {
        RowAction::Select(i) => doc.active_reference = i,
        RowAction::ToggleVisible(i) => {
            doc.update_reference(i, false, |r| r.visible = !r.visible);
        }
        RowAction::ToggleLocked(i) => {
            doc.update_reference(i, false, |r| r.locked = !r.locked);
        }
        RowAction::StartRename(i) => {
            doc.active_reference = i;
            ui_state.renaming_reference = Some((i, doc.data.references[i].name.clone()));
        }
        RowAction::Duplicate(i) => {
            doc.duplicate_reference(i);
        }
        RowAction::ResetSize(i) => {
            let bounds = doc.data.bounds;
            doc.labeled("Reset Reference Size", |d| {
                d.update_reference(i, false, |r| r.fit_to(bounds))
            });
        }
        RowAction::Move { from, to } => {
            doc.move_reference(from, to);
        }
        RowAction::Delete(i) => {
            doc.remove_reference(i);
        }
        RowAction::Replace(_) => {}
    }
}

/// *Add Image…*, *Show All / Hide All*, delete and *Align Work Plane*.
fn header(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    let (count, any_visible, active) = state.doc().map_or((0, false, 0), |d| {
        (
            d.data.references.len(),
            d.data.references.iter().any(|r| r.visible),
            d.active_reference,
        )
    });
    let has = active < count;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let style = IconStyle::small();
        if icon_button(ui, icon::IMAGE, style, true)
            .on_hover_text("Add images... (PNG or JPEG; onto the plane the camera faces most)")
            .clicked()
        {
            add_images(state);
            ui_state.renaming_reference = None;
        }
        let (glyph, tip) = if any_visible {
            (icon::EYE, "Hide all references (Alt+R)")
        } else {
            (icon::EYE_SLASH, "Show all references (Alt+R)")
        };
        if icon_button(ui, glyph, style, count > 0)
            .on_hover_text(tip)
            .clicked()
        {
            state.toggle_all_references();
        }
        let del = style.hover_color(theme::p().danger);
        if icon_button(ui, icon::TRASH, del, has)
            .on_hover_text("Delete the selected reference")
            .clicked()
        {
            if let Some(d) = state.doc_mut() {
                d.remove_reference(active);
            }
            ui_state.renaming_reference = None;
        }
        if icon_button(ui, icon::CROSSHAIR, style, has)
            .on_hover_text("Align the work plane to the selected reference (draw onto it)")
            .clicked()
        {
            if let Some(d) = state.doc_mut() {
                d.align_work_plane_to_reference(active);
            }
            state.status_message = Some("Work plane aligned to the reference".to_owned());
        }
    });
}

fn image_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Images", &rizxel_app::IMAGE_EXTENSIONS)
}

/// *Add Image…*: one or more files, each onto the plane the camera faces.
fn add_images(state: &mut AppState) {
    let Some(paths) = image_dialog()
        .set_title("Add Reference Images")
        .pick_files()
    else {
        return;
    };
    add_files(state, &paths);
}

/// Adds image files as references (dialog or drop). Reports the last result.
pub fn add_files(state: &mut AppState, paths: &[std::path::PathBuf]) {
    for path in paths {
        state.status_message = Some(match state.add_reference_file(path, None) {
            Ok(msg) => msg,
            Err(e) => {
                log::error!("reference {}: {e}", path.display());
                format!("Failed to add {}: {e}", path.display())
            }
        });
    }
}

fn replace_image(state: &mut AppState, index: usize) {
    let Some(path) = image_dialog().set_title("Replace Image").pick_file() else {
        return;
    };
    state.status_message = Some(match state.replace_reference_file(index, &path) {
        Ok(msg) => msg,
        Err(e) => format!("Failed to replace the image: {e}"),
    });
}

/// Final index of a row dragged from `from` and dropped above (`true`) or
/// below row `target`. Rows run top first, so "above" is a higher index.
fn drop_index(from: usize, target: usize, above: bool) -> usize {
    match (above, from > target) {
        (true, true) => target + 1,
        (true, false) | (false, true) => target,
        (false, false) => target - 1,
    }
}

fn row(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState, i: usize) -> Option<RowAction> {
    let doc = state.doc()?;
    let r = doc.data.references.get(i)?.clone();
    let selected = i == doc.active_reference;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    response.dnd_set_drag_payload(RefDrag(i));
    if selected {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::p().bg_selected);
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(1.0, theme::p().accent_line),
            StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::p().bg_header);
    }
    let mut action = None;

    // Thumbnail.
    let thumb_rect = Rect::from_center_size(
        rect.left_center() + egui::vec2(4.0 + THUMB / 2.0, 0.0),
        egui::vec2(THUMB, THUMB),
    );
    match ui_state.reference_thumbs.get(ui.ctx(), &r.image) {
        Some(texture) => {
            let aspect = r.image.width as f32 / r.image.height.max(1) as f32;
            let size = if aspect >= 1.0 {
                egui::vec2(THUMB, THUMB / aspect)
            } else {
                egui::vec2(THUMB * aspect, THUMB)
            };
            let fit = Rect::from_center_size(thumb_rect.center(), size);
            let tint = if r.visible {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(90)
            };
            ui.painter().image(
                texture.id(),
                fit,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        }
        None => {
            ui.painter().rect_stroke(
                thumb_rect,
                2.0,
                Stroke::new(1.0, theme::p().text_dim),
                StrokeKind::Inside,
            );
        }
    }

    // Lock and eye on the right, plane badge before them.
    let lock_rect = Rect::from_center_size(
        rect.right_center() - egui::vec2(14.0, 0.0),
        egui::vec2(18.0, 18.0),
    );
    let eye_rect = lock_rect.translate(egui::vec2(-22.0, 0.0));
    let eye_glyph = if r.visible {
        icon::EYE
    } else {
        icon::EYE_SLASH
    };
    let eye = ui
        .put(eye_rect, |ui: &mut Ui| {
            icon_button(ui, eye_glyph, IconStyle::small(), true)
        })
        .on_hover_text(if r.visible { "Hide" } else { "Show" });
    if eye.clicked() {
        action = Some(RowAction::ToggleVisible(i));
    }
    let lock_glyph = if r.locked {
        icon::LOCK
    } else {
        icon::LOCK_OPEN
    };
    let lock = ui
        .put(lock_rect, |ui: &mut Ui| {
            icon_button(ui, lock_glyph, IconStyle::small(), true)
        })
        .on_hover_text(if r.locked {
            "Unlock (allow moving and resizing)"
        } else {
            "Lock in place"
        });
    if lock.clicked() {
        action = Some(RowAction::ToggleLocked(i));
    }
    let badge_galley = ui.painter().layout_no_wrap(
        r.plane.label().to_owned(),
        theme::semibold(11.0),
        theme::p().text_strong,
    );
    let badge_size = badge_galley.size() + egui::vec2(8.0, 2.0);
    let badge = Rect::from_min_size(
        egui::pos2(
            eye_rect.left() - 6.0 - badge_size.x,
            rect.center().y - badge_size.y / 2.0,
        ),
        badge_size,
    );
    let axis_color = [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][r.plane.normal_axis()];
    ui.painter().rect_filled(
        badge,
        egui::CornerRadius::same(6),
        axis_color.gamma_multiply(0.6),
    );
    ui.painter().galley(
        badge.min + egui::vec2(4.0, 1.0),
        badge_galley,
        theme::p().text_strong,
    );

    // Name (inline rename on double-click).
    let name_rect = Rect::from_min_max(
        egui::pos2(thumb_rect.right() + 8.0, rect.top() + 6.0),
        egui::pos2(badge.left() - 6.0, rect.bottom() - 6.0),
    );
    let renaming = matches!(&ui_state.renaming_reference, Some((r, _)) if *r == i);
    if renaming {
        if let Some((_, buffer)) = ui_state.renaming_reference.as_mut() {
            let edit = ui.put(
                name_rect,
                egui::TextEdit::singleline(buffer)
                    .font(theme::regular(12.0))
                    .margin(egui::vec2(4.0, 2.0)),
            );
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            let cancel = ui.input(|inp| inp.key_pressed(egui::Key::Escape));
            if edit.lost_focus() || cancel {
                let (_, name) = ui_state.renaming_reference.take().expect("renaming");
                if !cancel && !name.trim().is_empty() {
                    if let Some(d) = state.doc_mut() {
                        d.update_reference(i, false, |r| r.name = name);
                    }
                }
            }
        }
    } else if name_rect.width() > 0.0 {
        let mut text = RichText::new(&r.name).size(12.0);
        text = match (selected, r.visible) {
            (true, _) => text
                .color(theme::p().text_strong)
                .font(theme::semibold(12.0)),
            (false, false) => text.color(theme::p().text_dim).italics(),
            (false, true) => text.color(theme::p().text_dim),
        };
        let galley = egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            name_rect.width(),
            egui::TextStyle::Body,
        );
        let pos = egui::pos2(name_rect.left(), rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, theme::p().text);
    }

    // Drag a row onto another to change the draw order.
    let above = |ui: &Ui| {
        ui.input(|inp| inp.pointer.interact_pos())
            .is_some_and(|p| p.y < rect.center().y)
    };
    if let Some(from) = response.dnd_hover_payload::<RefDrag>().map(|p| p.0) {
        if from != i {
            let y = if above(ui) {
                rect.top() - 2.0
            } else {
                rect.bottom() + 2.0
            };
            ui.painter()
                .hline(rect.x_range(), y, Stroke::new(2.0, theme::p().accent_line));
        }
    }
    if let Some(from) = response.dnd_release_payload::<RefDrag>().map(|p| p.0) {
        if from != i {
            action = Some(RowAction::Move {
                from,
                to: drop_index(from, i, above(ui)),
            });
        }
    }
    if action.is_none() && !renaming {
        if response.double_clicked() {
            action = Some(RowAction::StartRename(i));
        } else if response.clicked() {
            action = Some(RowAction::Select(i));
        }
    }
    response
        .on_hover_text(
            "Double-click to rename, drag to change the draw order, right-click for more",
        )
        .context_menu(|ui| {
            ui.set_min_width(170.0);
            let mut item = |ui: &mut Ui, label: &str, a: RowAction| {
                if ui.button(label).clicked() {
                    action = Some(a);
                    ui.close();
                }
            };
            item(ui, "Rename", RowAction::StartRename(i));
            item(ui, "Duplicate", RowAction::Duplicate(i));
            item(ui, "Replace Image...", RowAction::Replace(i));
            item(ui, "Reset Size", RowAction::ResetSize(i));
            ui.separator();
            item(ui, "Delete", RowAction::Delete(i));
        });
    action
}

/// Placement of the selected reference. Field and slider drags are one
/// undo step each; a locked reference shows its values read-only.
fn properties(ui: &mut Ui, state: &mut AppState, ui_state: &mut UiState) {
    let Some(doc) = state.doc_mut() else { return };
    let index = doc.active_reference;
    let Some(r) = doc.data.references.get(index).cloned() else {
        return;
    };
    let bounds = doc.data.bounds;
    let mut new = r.clone();
    let mut merge = false;
    let mut label = "Reference Placement";
    ui.label(
        RichText::new(format!(
            "{} — {}×{} px",
            r.name, r.image.width, r.image.height
        ))
        .size(11.0)
        .color(theme::p().text_muted),
    );
    ui.add_enabled_ui(!r.locked, |ui| {
        // Plane.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Plane").size(12.0).color(theme::p().text));
            for plane in RefPlane::ALL {
                let tip = match plane {
                    RefPlane::Xz => "Floor (seen from the top)",
                    RefPlane::Xy => "Front / back (seen from the front)",
                    RefPlane::Yz => "Side (seen from the right)",
                };
                if ui
                    .selectable_label(new.plane == plane, plane.label())
                    .on_hover_text(tip)
                    .clicked()
                    && new.plane != plane
                {
                    new.plane = plane;
                    new.offset = glam::Vec3::ZERO;
                    new.fit_to(bounds);
                    label = "Reference Plane";
                }
            }
        });
        // Offset, normal axis highlighted.
        let normal = new.plane.normal_axis();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Offset").size(12.0).color(theme::p().text));
            for (axis, (name, color)) in [
                ("X", theme::AXIS_X),
                ("Y", theme::AXIS_Y),
                ("Z", theme::AXIS_Z),
            ]
            .into_iter()
            .enumerate()
            {
                let text = if axis == normal {
                    RichText::new(name).font(theme::semibold(12.0)).color(color)
                } else {
                    RichText::new(name)
                        .size(11.0)
                        .color(color.gamma_multiply(0.6))
                };
                ui.label(text).on_hover_text(if axis == normal {
                    "Plane position along its normal"
                } else {
                    "Slides the image on its plane"
                });
                let resp = ui.add(
                    egui::DragValue::new(&mut new.offset[axis])
                        .speed(0.1)
                        .max_decimals(2),
                );
                merge |= resp.dragged() && !resp.drag_started() || resp.has_focus();
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Snap plane")
                    .size(12.0)
                    .color(theme::p().text),
            );
            for (which, text) in [(0u8, "Min"), (1, "Center"), (2, "Max")] {
                if ui
                    .small_button(text)
                    .on_hover_text("Move the plane to this face of the bounds")
                    .clicked()
                {
                    new.snap_to_face(bounds, which);
                    label = "Snap Reference to Bounds";
                }
            }
        });
        // Size with aspect lock, or pixels per voxel.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Size").size(12.0).color(theme::p().text));
            let aspect = r.size.y / r.size.x.max(f32::EPSILON);
            let lock = !ui_state.reference_free_aspect;
            let w = ui.add(
                egui::DragValue::new(&mut new.size.x)
                    .speed(0.1)
                    .range(0.01..=4096.0)
                    .max_decimals(2)
                    .prefix("W "),
            );
            let h = ui.add(
                egui::DragValue::new(&mut new.size.y)
                    .speed(0.1)
                    .range(0.01..=4096.0)
                    .max_decimals(2)
                    .prefix("H "),
            );
            if lock && w.changed() {
                new.size.y = new.size.x * aspect;
            } else if lock && h.changed() {
                new.size.x = new.size.y / aspect.max(f32::EPSILON);
            }
            for resp in [&w, &h] {
                merge |= resp.dragged() && !resp.drag_started() || resp.has_focus();
            }
            let glyph = if lock { icon::LINK } else { icon::LINK_BREAK };
            if icon_button(ui, glyph, IconStyle::small(), true)
                .on_hover_text("Keep the aspect ratio")
                .clicked()
            {
                ui_state.reference_free_aspect = !ui_state.reference_free_aspect;
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Pixels per voxel")
                    .size(12.0)
                    .color(theme::p().text),
            );
            let mut ppv = r.image.width as f32 / r.size.x.max(f32::EPSILON);
            let resp = ui.add(
                egui::DragValue::new(&mut ppv)
                    .speed(0.1)
                    .range(0.01..=4096.0)
                    .max_decimals(2),
            );
            if resp.changed() && ppv > 0.0 {
                new.size = glam::Vec2::new(r.image.width as f32 / ppv, r.image.height as f32 / ppv);
            }
            merge |= resp.dragged() && !resp.drag_started() || resp.has_focus();
        });
        // Turns and flips.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.label(RichText::new("Orient").size(12.0).color(theme::p().text));
            if icon_button(ui, icon::ARROW_COUNTER_CLOCKWISE, IconStyle::small(), true)
                .on_hover_text("Rotate 90° counterclockwise")
                .clicked()
            {
                new.rotation = (new.rotation + 1) % 4;
                label = "Rotate Reference";
            }
            if icon_button(ui, icon::ARROW_CLOCKWISE, IconStyle::small(), true)
                .on_hover_text("Rotate 90° clockwise")
                .clicked()
            {
                new.rotation = (new.rotation + 3) % 4;
                label = "Rotate Reference";
            }
            if ui.selectable_label(new.flip_u, "Flip H").clicked() {
                new.flip_u = !new.flip_u;
                label = "Flip Reference";
            }
            if ui.selectable_label(new.flip_v, "Flip V").clicked() {
                new.flip_v = !new.flip_v;
                label = "Flip Reference";
            }
        });
    });
    // Look: allowed while locked.
    ui.horizontal(|ui| {
        ui.label(RichText::new("Opacity").size(12.0).color(theme::p().text));
        let mut percent = new.opacity * 100.0;
        let resp = ui.add(
            egui::Slider::new(&mut percent, 0.0..=100.0)
                .suffix("%")
                .fixed_decimals(0),
        );
        if resp.changed() {
            new.opacity = (percent / 100.0).clamp(0.0, 1.0);
            merge |= !resp.drag_started();
            label = "Reference Opacity";
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Depth").size(12.0).color(theme::p().text));
        egui::ComboBox::from_id_salt("reference_depth")
            .selected_text(new.depth.label())
            .show_ui(ui, |ui| {
                for mode in DepthMode::ALL {
                    ui.selectable_value(&mut new.depth, mode, mode.label());
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Slice Mode")
                .size(12.0)
                .color(theme::p().text),
        );
        egui::ComboBox::from_id_salt("reference_slice")
            .selected_text(new.slice.label())
            .show_ui(ui, |ui| {
                for filter in SliceFilter::ALL {
                    ui.selectable_value(&mut new.slice, filter, filter.label());
                }
            });
    });
    if new != r && apply_change(doc, index, new, merge, label) {
        ui.ctx().request_repaint();
    }
}

fn apply_change(
    doc: &mut Document,
    index: usize,
    new: ReferenceImage,
    merge: bool,
    label: &str,
) -> bool {
    doc.labeled(label, |d| d.set_reference(index, new, merge))
}

#[cfg(test)]
mod tests {
    use super::drop_index;

    #[test]
    fn drop_index_places_rows_next_to_the_target() {
        assert_eq!(drop_index(0, 2, true), 2);
        assert_eq!(drop_index(0, 2, false), 1);
        assert_eq!(drop_index(3, 1, true), 2);
        assert_eq!(drop_index(3, 1, false), 1);
    }
}
