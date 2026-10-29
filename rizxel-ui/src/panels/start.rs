//! Start screen: New, Open and the recent files with their thumbnails.
//! Shown at start when no file came from the command line (*Settings ▸
//! General ▸ Start screen*); any action or `Esc` closes it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{RichText, Sense, TextureHandle, Ui};
use egui_phosphor::regular as icon;
use rizxel_app::AppState;

use crate::{actions, theme, thumbnails};

/// Open start screen and its thumbnail textures.
#[derive(Default)]
pub struct StartScreen {
    /// `None`: no thumbnail file (or unreadable); loaded once per path.
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
}

impl StartScreen {
    fn thumb(
        &mut self,
        ctx: &egui::Context,
        state: &AppState,
        path: &Path,
    ) -> Option<TextureHandle> {
        self.thumbs
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                let file = state.paths()?.thumbnail_file(path);
                let image = thumbnails::load(&file)?;
                Some(ctx.load_texture(
                    format!("start_thumb:{}", path.display()),
                    image,
                    egui::TextureOptions::LINEAR,
                ))
            })
            .clone()
    }
}

const CARD: egui::Vec2 = egui::vec2(
    thumbnails::SIZE[0] as f32,
    thumbnails::SIZE[1] as f32 + 36.0,
);

/// What the user picked.
enum Choice {
    New,
    Open,
    Recent(PathBuf),
    Close,
}

/// Draws the start screen while it is open.
pub fn show(ctx: &egui::Context, state: &mut AppState, screen: &mut Option<StartScreen>) {
    let Some(s) = screen else {
        return;
    };
    state.prune_recent();
    let recent: Vec<PathBuf> = state.recent.paths().to_vec();
    let mut choice = None;
    let mut show_again = state.settings.general.start_screen;
    let modal = egui::Modal::new(egui::Id::new("start_screen")).show(ctx, |ui| {
        ui.set_width(4.0 * (CARD.x + 12.0) + 180.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(icon::CUBE)
                    .font(theme::icon(28.0))
                    .color(theme::ACCENT),
            );
            ui.vertical(|ui| {
                ui.label(RichText::new("Rizxel").font(theme::semibold(18.0)));
                ui.label(
                    RichText::new(format!("Voxel Editor v{}", env!("CARGO_PKG_VERSION")))
                        .color(theme::TEXT_DIM),
                );
            });
        });
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(160.0);
                let big = |ui: &mut Ui, glyph: &str, text: &str| {
                    ui.add_sized(
                        [160.0, 34.0],
                        egui::Button::new(RichText::new(format!("{glyph}  {text}")).size(14.0)),
                    )
                    .clicked()
                };
                if big(ui, icon::FILE_PLUS, "New Project") {
                    choice = Some(Choice::New);
                }
                if big(ui, icon::FOLDER_OPEN, "Open...") {
                    choice = Some(Choice::Open);
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.label(RichText::new("Recent").font(theme::semibold(13.0)));
                if recent.is_empty() {
                    ui.label(
                        RichText::new("Projects you open or save appear here.")
                            .color(theme::TEXT_DIM),
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(2.0 * (CARD.y + 12.0))
                    .show(ui, |ui| {
                        egui::Grid::new("start_recent")
                            .spacing([12.0, 12.0])
                            .show(ui, |ui| {
                                for (i, path) in recent.iter().enumerate() {
                                    let thumb = s.thumb(ctx, state, path);
                                    if card(ui, path, thumb.as_ref()).clicked() {
                                        choice = Some(Choice::Recent(path.clone()));
                                    }
                                    if i % 4 == 3 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
            });
        });
        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut show_again, "Show at start");
            if ui.button("Close").clicked() {
                choice = Some(Choice::Close);
            }
        });
    });
    if show_again != state.settings.general.start_screen {
        let mut settings = state.settings.clone();
        settings.general.start_screen = show_again;
        state.apply_settings(settings);
        if let Err(e) = state.save_settings() {
            log::warn!("settings: {e}");
        }
    }
    if modal.should_close() {
        choice.get_or_insert(Choice::Close);
    }
    let Some(choice) = choice else {
        return;
    };
    *screen = None;
    match choice {
        Choice::New => {} // The empty untitled document is already open.
        Choice::Open => actions::open(state),
        Choice::Recent(path) => actions::open_path(state, &path),
        Choice::Close => {}
    }
}

/// A recent file: thumbnail (or a placeholder), name and folder.
fn card(ui: &mut Ui, path: &Path, thumb: Option<&TextureHandle>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, file_name.clone())
    });
    let hovered = response.hovered();
    let painter = ui.painter_at(rect);
    let fill = if hovered {
        theme::HOVER
    } else {
        theme::BG_BUTTON
    };
    painter.rect_filled(rect, theme::RADIUS, fill);
    let image_rect =
        egui::Rect::from_min_size(rect.min, egui::vec2(CARD.x, thumbnails::SIZE[1] as f32));
    match thumb {
        Some(t) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(t.id(), image_rect, uv, egui::Color32::WHITE);
        }
        None => {
            painter.rect_filled(image_rect, theme::RADIUS, theme::WORKSPACE);
            painter.text(
                image_rect.center(),
                egui::Align2::CENTER_CENTER,
                icon::CUBE,
                theme::icon(36.0),
                theme::TEXT_DIM,
            );
        }
    }
    let name = path.file_stem().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let folder = path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let text_top = image_rect.bottom() + 4.0;
    painter.text(
        egui::pos2(rect.left() + 6.0, text_top),
        egui::Align2::LEFT_TOP,
        name,
        theme::semibold(12.0),
        theme::TEXT_STRONG,
    );
    painter.text(
        egui::pos2(rect.left() + 6.0, text_top + 16.0),
        egui::Align2::LEFT_TOP,
        folder,
        theme::regular(10.0),
        theme::TEXT_DIM,
    );
    if hovered {
        painter.rect_stroke(
            rect,
            theme::RADIUS,
            egui::Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(path.display().to_string())
}
