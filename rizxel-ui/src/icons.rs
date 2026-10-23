//! Pixel-art icon textures sliced from `docs/Pixel-Art Voxel Editor Icon Sheet-2.png`
//! into `assets/icons/*.png` (128x128 RGBA). Embedded at compile time.

use egui::{Color32, ColorImage, Rect, TextureFilter, TextureHandle, TextureOptions, Ui};
use rizxel_engine::{Tool, VoxelOp};

/// Every icon in `assets/icons/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Cursor,
    Brush,
    Line,
    Shape,
    Plane,
    Move,
    Transform,
    ColorPicker,
    Selection,
    FuzzySelect,
    Extrude,
    Fill,
    Settings,
    Undo,
    Redo,
    Add,
    Subtract,
    Join,
    Paint,
}

macro_rules! icon_sources {
    ($($variant:ident => $file:literal),* $(,)?) => {
        const SOURCES: &[(Icon, &str, &[u8])] = &[
            $((
                Icon::$variant,
                $file,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/icons/",
                    $file,
                    ".png"
                )),
            )),*
        ];
    };
}

// Order must match the `Icon` declaration order (index = discriminant).
icon_sources! {
    Cursor => "cursor",
    Brush => "brush",
    Line => "line",
    Shape => "shape",
    Plane => "plane",
    Move => "move",
    Transform => "transform",
    ColorPicker => "color_picker",
    Selection => "selection",
    FuzzySelect => "fuzzy_select",
    Extrude => "extrude",
    Fill => "fill",
    Settings => "settings",
    Undo => "undo",
    Redo => "redo",
    Add => "add",
    Subtract => "subtract",
    Join => "join",
    Paint => "paint",
}

impl From<Tool> for Icon {
    fn from(tool: Tool) -> Self {
        match tool {
            Tool::Cursor => Icon::Cursor,
            Tool::Brush => Icon::Brush,
            Tool::Line => Icon::Line,
            Tool::Shape => Icon::Shape,
            Tool::Plane => Icon::Plane,
            Tool::Move => Icon::Move,
            Tool::ColorPicker => Icon::ColorPicker,
            Tool::Selection => Icon::Selection,
            Tool::FuzzySelect => Icon::FuzzySelect,
            Tool::Extrude => Icon::Extrude,
            Tool::Fill => Icon::Fill,
        }
    }
}

impl From<VoxelOp> for Icon {
    fn from(op: VoxelOp) -> Self {
        match op {
            VoxelOp::Add => Icon::Add,
            VoxelOp::Subtract => Icon::Subtract,
            VoxelOp::Join => Icon::Join,
            VoxelOp::Paint => Icon::Paint,
        }
    }
}

/// Loaded GPU textures, indexed by `Icon as usize`.
pub struct Icons {
    textures: Vec<TextureHandle>,
}

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        // Icons are drawn at 18–26 px from 128 px sources: mipmaps avoid aliasing.
        let options = TextureOptions {
            mipmap_mode: Some(TextureFilter::Linear),
            ..TextureOptions::LINEAR
        };
        let textures = SOURCES
            .iter()
            .map(|(_, name, bytes)| {
                let image = decode(bytes).unwrap_or_else(|err| {
                    log::error!("failed to decode icon `{name}`: {err}");
                    ColorImage::filled([1, 1], Color32::TRANSPARENT)
                });
                ctx.load_texture(format!("icon_{name}"), image, options)
            })
            .collect();
        Self { textures }
    }

    pub fn get(&self, icon: impl Into<Icon>) -> &TextureHandle {
        &self.textures[icon.into() as usize]
    }
}

fn decode(bytes: &[u8]) -> Result<ColorImage, image::ImageError> {
    let rgba = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)?.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

/// Paints `texture` centered in `rect` at `size` px, multiplied by `tint`.
pub fn paint(ui: &Ui, rect: Rect, texture: &TextureHandle, size: f32, tint: Color32) {
    let target = Rect::from_center_size(rect.center(), egui::Vec2::splat(size));
    egui::Image::new(texture).tint(tint).paint_at(ui, target);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_match_enum_order_and_decode() {
        for (i, (icon, name, bytes)) in SOURCES.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{name} out of order");
            let img = decode(bytes).expect(name);
            assert_eq!(img.size, [128, 128], "{name}");
        }
        assert_eq!(SOURCES.len(), Icon::Paint as usize + 1);
    }
}
