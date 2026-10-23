//! Importing and exporting other file formats from the application state.
//!
//! | Format | Import | Export |
//! |---|---|---|
//! | MagicaVoxel `.vox` | new tab: layers, groups, linked clones, palette | one model per layer, names, hidden flags, palette |
//! | Goxel `.gox` | new tab: layers, groups, clones, shapes, materials | same, natively where Goxel can |
//! | Wavefront `.obj` | new tab: voxelized, exact for voxel-editor exports | `.obj` + `.mtl` + palette `.png` in MagicaVoxel's layout |
//! | `.mtl` | palette and materials of the active document | `.mtl` + palette `.png` |
//! | STL `.stl` | new tab: voxelized, one color | binary STL, Z-up, 1 voxel = 1 mm |

use std::path::Path;

use rizxel_engine::VoxelizeOptions;
use rizxel_io::RzxlError;

use crate::{AppState, Document};

/// Largest file accepted for import.
const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;

/// A file format Rizxel can import from or export to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Rzxl,
    Vox,
    Gox,
    Obj,
    Mtl,
    Stl,
}

impl FileFormat {
    /// Formats offered under File ▸ Import.
    pub const IMPORTS: [FileFormat; 5] = [
        FileFormat::Vox,
        FileFormat::Gox,
        FileFormat::Obj,
        FileFormat::Stl,
        FileFormat::Mtl,
    ];
    /// Formats offered under File ▸ Export.
    pub const EXPORTS: [FileFormat; 5] = [
        FileFormat::Obj,
        FileFormat::Vox,
        FileFormat::Gox,
        FileFormat::Stl,
        FileFormat::Mtl,
    ];

    pub const fn extension(self) -> &'static str {
        match self {
            FileFormat::Rzxl => rizxel_io::RZXL_EXTENSION,
            FileFormat::Vox => rizxel_io::VOX_EXTENSION,
            FileFormat::Gox => rizxel_io::GOX_EXTENSION,
            FileFormat::Obj => rizxel_io::OBJ_EXTENSION,
            FileFormat::Mtl => rizxel_io::MTL_EXTENSION,
            FileFormat::Stl => rizxel_io::STL_EXTENSION,
        }
    }

    /// Name shown in menus and file dialogs.
    pub const fn label(self) -> &'static str {
        match self {
            FileFormat::Rzxl => "Rizxel Project",
            FileFormat::Vox => "MagicaVoxel",
            FileFormat::Gox => "Goxel",
            FileFormat::Obj => "Wavefront OBJ",
            FileFormat::Mtl => "Material Library (MTL)",
            FileFormat::Stl => "STL",
        }
    }

    /// What an export writes, for menus and tooltips.
    pub const fn export_note(self) -> &'static str {
        match self {
            FileFormat::Obj => "OBJ + MTL + palette PNG (MagicaVoxel layout, scale 0.1)",
            FileFormat::Mtl => "MTL + palette PNG",
            FileFormat::Vox => "one model per layer",
            FileFormat::Gox => "layers, groups, clones, shapes, materials",
            FileFormat::Stl => "binary, Z-up, 1 voxel = 1 mm",
            FileFormat::Rzxl => "",
        }
    }

    /// Format of `path` by its extension (case-insensitive).
    pub fn from_path(path: &Path) -> Option<FileFormat> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        [
            FileFormat::Rzxl,
            FileFormat::Vox,
            FileFormat::Gox,
            FileFormat::Obj,
            FileFormat::Mtl,
            FileFormat::Stl,
        ]
        .into_iter()
        .find(|f| f.extension() == ext)
    }
}

fn read_limited(path: &Path) -> Result<Vec<u8>, RzxlError> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_IMPORT_BYTES {
        return Err(RzxlError::BodyTooLarge(len.min(u32::MAX as u64) as u32));
    }
    Ok(std::fs::read(path)?)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported".to_owned())
}

impl AppState {
    /// Imports `path` by its extension. Models open in a new tab (replacing
    /// a pristine untitled one); an `.mtl` replaces the active document's
    /// palette and materials (undoable). Returns a status message.
    pub fn import(&mut self, path: &Path) -> Result<String, RzxlError> {
        let format = FileFormat::from_path(path).ok_or_else(|| {
            RzxlError::Invalid(format!("unsupported file type: {}", path.display()))
        })?;
        let name = stem(path);
        // How OBJ/STL meshes were voxelized, for the status message.
        let mut mesh_note = String::new();
        let mut voxelized =
            |(project, info): (rizxel_io::ProjectFile, rizxel_io::MeshImportInfo)| {
                mesh_note = if info.exact {
                    ", exact: the mesh's own voxel grid".to_owned()
                } else {
                    format!(
                        ", voxelized to {} voxels on the longest side",
                        VoxelizeOptions::default().resolution
                    )
                };
                project
            };
        let project = match format {
            FileFormat::Rzxl => {
                self.open(path)?;
                return Ok(format!("Opened {}", path.display()));
            }
            FileFormat::Mtl => {
                let text = String::from_utf8_lossy(&read_limited(path)?).into_owned();
                let load = rizxel_io::sibling_loader(path);
                let images =
                    |p: &str| load(p).and_then(|b| rizxel_io::png_io::decode_rgba(&b).ok());
                let (palette, materials) =
                    rizxel_io::palette_from_mtl(&rizxel_io::read_mtl(&text), &images)?;
                let colors = palette.len() - 1;
                let count = materials.len();
                if let Some(doc) = self.doc_mut() {
                    doc.replace_palette_and_materials(palette, materials);
                }
                return Ok(format!(
                    "Imported {colors} colors and {count} materials from {}",
                    path.display()
                ));
            }
            FileFormat::Vox => rizxel_io::read_vox(&read_limited(path)?, &name)?,
            FileFormat::Gox => rizxel_io::read_gox(&read_limited(path)?, &name)?,
            FileFormat::Obj => {
                let text = String::from_utf8_lossy(&read_limited(path)?).into_owned();
                let mesh = rizxel_io::read_obj(&text, &rizxel_io::sibling_loader(path))?;
                voxelized(rizxel_io::project_from_mesh(
                    mesh.triangles,
                    rizxel_io::UpAxis::Y,
                    mesh.palette,
                    VoxelizeOptions::default(),
                    &name,
                )?)
            }
            FileFormat::Stl => {
                let tris = rizxel_io::read_stl(&read_limited(path)?, rizxel_io::uncolored())?;
                voxelized(rizxel_io::project_from_mesh(
                    tris,
                    rizxel_io::UpAxis::Z,
                    None,
                    VoxelizeOptions::default(),
                    &name,
                )?)
            }
        };
        let mut doc = Document::from_project(project, None)?;
        if self.structure.fit_on_import {
            // An undo step of its own, so the original placement is one Ctrl+Z away.
            let _ = doc.fit_bounds_to_content();
        }
        doc.mark_unsaved();
        self.remember_recent(path);
        let summary = format!(
            "Imported {} ({} voxels, {} layers{mesh_note})",
            path.display(),
            doc.visible_voxel_count(),
            doc.data.layers.len()
        );
        self.cancel_active_edit();
        let replace_pristine = self.documents.len() == 1
            && self.documents[0].path.is_none()
            && !self.documents[0].is_dirty();
        if replace_pristine {
            let mut doc = doc;
            doc.mask_selection = self.options.mask_selection;
            self.documents[0] = doc;
            self.active_doc = 0;
        } else {
            self.push_document(doc);
        }
        Ok(summary)
    }

    /// True when the active document can be exported as `format`.
    pub fn can_export(&self, format: FileFormat) -> bool {
        let Some(doc) = self.doc() else {
            return false;
        };
        match format {
            FileFormat::Obj | FileFormat::Stl => doc.visible_voxel_count() > 0,
            // Cheap check (menus ask every frame); hidden layers count too.
            FileFormat::Vox => {
                doc.visible_voxel_count() > 0
                    || doc.data.layers.iter().any(|l| match l.kind() {
                        rizxel_math::LayerKind::Voxels => !l.volume.is_empty(),
                        rizxel_math::LayerKind::Group { .. } => false,
                        _ => true,
                    })
            }
            FileFormat::Gox | FileFormat::Mtl | FileFormat::Rzxl => true,
        }
    }

    /// Exports the active document to `path`. OBJ and MTL also write their
    /// companion files next to it. Returns a status message.
    pub fn export_active(&self, path: &Path, format: FileFormat) -> Result<String, RzxlError> {
        let doc = self
            .doc()
            .ok_or_else(|| RzxlError::Invalid("no document is open".into()))?;
        let detail = match format {
            FileFormat::Obj => {
                let tris = rizxel_io::write_obj_set(
                    path,
                    &doc.merged_visible_volume(),
                    doc.data.bounds,
                    &doc.data.palette,
                    rizxel_io::DEFAULT_OBJ_SCALE,
                )?;
                format!("{tris} triangles, with .mtl and .png")
            }
            FileFormat::Mtl => {
                let stem = stem(path);
                let png = rizxel_io::format_obj::palette_texture(&doc.data.palette)?;
                let mtl = rizxel_io::format_mtl::write_palette_mtl(&format!("{stem}.png"));
                std::fs::write(path, mtl)?;
                std::fs::write(path.with_file_name(format!("{stem}.png")), png)?;
                "with palette .png".to_owned()
            }
            FileFormat::Vox => {
                let scene = doc.export_scene();
                let models = scene.layers.iter().filter(|l| !l.volume.is_empty()).count();
                std::fs::write(path, rizxel_io::write_vox(&scene)?)?;
                format!("{models} models")
            }
            FileFormat::Gox => {
                std::fs::write(path, rizxel_io::write_gox(&doc.to_project())?)?;
                format!("{} layers", doc.data.layers.len())
            }
            FileFormat::Stl => {
                let bytes = rizxel_io::write_stl(
                    &doc.merged_visible_volume(),
                    doc.data.bounds,
                    rizxel_io::DEFAULT_STL_SCALE,
                )?;
                let tris = (bytes.len() - 84) / 50;
                std::fs::write(path, bytes)?;
                format!("{tris} triangles")
            }
            FileFormat::Rzxl => {
                rizxel_io::save_rzxl(path, &doc.to_project())?;
                "project copy".to_owned()
            }
        };
        Ok(format!("Exported {} ({detail})", path.display()))
    }

    /// True when the active document has visible voxels to export as OBJ.
    pub fn can_export_obj(&self) -> bool {
        self.can_export(FileFormat::Obj)
    }

    /// Exports the visible layers of the active document as OBJ + MTL +
    /// palette PNG (MagicaVoxel layout). Returns the number of triangles.
    pub fn export_active_obj(&self, path: &Path) -> Result<usize, RzxlError> {
        let doc = self
            .doc()
            .ok_or_else(|| RzxlError::Invalid("no document is open".into()))?;
        rizxel_io::write_obj_set(
            path,
            &doc.merged_visible_volume(),
            doc.data.bounds,
            &doc.data.palette,
            rizxel_io::DEFAULT_OBJ_SCALE,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec3;
    use rizxel_math::{BoundSize, LayerKind, Material};

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rizxel_interchange_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample_state() -> AppState {
        let mut s = AppState::default();
        let d = s.doc_mut().unwrap();
        d.set_bounds(BoundSize::new(12, 10, 8), false);
        let block: Vec<_> = (0..12)
            .map(|i| (IVec3::new(i % 4, i / 4, 1), if i < 6 { 9 } else { 40 }))
            .collect();
        d.set_voxels(0, block, false);
        d.add_linked_clone(0, IVec3::new(5, 0, 2)).unwrap();
        d.add_layer();
        let top = d.active_layer;
        d.set_voxels(top, [(IVec3::new(8, 7, 6), 200)], false);
        d.toggle_layer_visible(top);
        s
    }

    #[test]
    fn every_format_roundtrips_through_disk() {
        let d = dir();
        let src = sample_state();
        let visible = src.doc().unwrap().visible_voxel_count();
        for format in [
            FileFormat::Vox,
            FileFormat::Gox,
            FileFormat::Obj,
            FileFormat::Stl,
        ] {
            let path = d.join(format!("model.{}", format.extension()));
            assert!(src.can_export(format), "{format:?}");
            let msg = src.export_active(&path, format).unwrap();
            assert!(msg.starts_with("Exported"), "{msg}");
            let mut dst = AppState::default();
            dst.import(&path).unwrap();
            let doc = dst.doc().unwrap();
            assert!(doc.is_dirty(), "imports are unsaved");
            assert!(doc.path.is_none());
            match format {
                // Layered formats keep hidden layers; Goxel also keeps the
                // linked clone (MagicaVoxel export bakes it into a model, so
                // the bounds never grow on re-import).
                FileFormat::Vox | FileFormat::Gox => {
                    assert_eq!(doc.visible_voxel_count(), visible, "{format:?}");
                    assert_eq!(doc.data.bounds, src.doc().unwrap().data.bounds);
                    assert!(doc.data.layers.iter().any(|l| !l.visible));
                    let clones = doc
                        .data
                        .layers
                        .iter()
                        .any(|l| matches!(l.props.kind, LayerKind::Clone { .. }));
                    assert_eq!(clones, format == FileFormat::Gox, "{format:?}");
                }
                // Mesh formats carry the visible surface only; the solid
                // block and its clone come back as the same voxels.
                FileFormat::Obj | FileFormat::Stl => {
                    assert_eq!(doc.visible_voxel_count(), visible, "{format:?}");
                }
                _ => unreachable!(),
            }
        }
        // OBJ export wrote its companions.
        assert!(d.join("model.mtl").exists() && d.join("model.png").exists());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn mtl_import_replaces_palette_and_materials_undoably() {
        let d = dir().join("mtl");
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("colors.mtl");
        std::fs::write(
            &path,
            "newmtl Red\nKd 1 0 0\nPm 0.5\nnewmtl Green\nKd 0 1 0\n",
        )
        .unwrap();
        let mut s = sample_state();
        let old = s.doc().unwrap().data.palette.clone();
        let msg = s.import(&path).unwrap();
        assert!(msg.contains("2 colors and 2 materials"), "{msg}");
        let doc = s.doc().unwrap();
        assert_eq!(
            doc.data.palette.get(1),
            Some(rizxel_math::Rgba8::rgb(255, 0, 0))
        );
        assert_eq!(
            doc.data.palette.len(),
            old.len(),
            "unlisted slots keep their colors"
        );
        assert_eq!(doc.data.palette.get(40), old.get(40));
        assert_eq!(
            doc.data.materials[0],
            Material {
                metallic: 0.5,
                ..Material::new("Red")
            }
        );
        s.undo();
        assert_eq!(s.doc().unwrap().data.palette, old);
        assert!(s.doc().unwrap().data.materials.is_empty());

        // Export MTL writes the library and its palette texture.
        let out = d.join("pal.mtl");
        s.export_active(&out, FileFormat::Mtl).unwrap();
        assert!(d.join("pal.png").exists());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn unknown_and_empty_cases() {
        let mut s = AppState::default();
        assert!(s.import(Path::new("model.xyz")).is_err());
        assert!(!s.can_export(FileFormat::Obj));
        assert!(!s.can_export(FileFormat::Vox));
        assert!(s.can_export(FileFormat::Gox));
        assert_eq!(
            FileFormat::from_path(Path::new("A.VOX")),
            Some(FileFormat::Vox)
        );
    }
}
