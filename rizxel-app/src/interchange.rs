//! Importing and exporting other file formats from the application state.
//!
//! | Format | Import | Export |
//! |---|---|---|
//! | MagicaVoxel `.vox` | new tab: layers, groups, linked clones, palette | one model per layer, names, hidden flags, palette |
//! | Goxel `.gox` | new tab: layers, groups, clones, shapes, materials | same, natively where Goxel can |
//! | Wavefront `.obj` | new tab: voxelized, exact for voxel-editor exports | `.obj` + `.mtl` + palette `.png` in MagicaVoxel's layout |
//! | `.mtl` | palette and materials of the active document | `.mtl` + palette `.png` |
//! | STL `.stl` | new tab: voxelized, one color | binary STL, Z-up, 1 voxel = 1 mm |
//! | glTF `.glb` / `.gltf` | — | one node per visible layer, groups, instanced clones |
//! | PNG / JPEG | reference image in the active document | — |

use std::path::Path;

use rizxel_engine::VoxelizeOptions;
use rizxel_io::RzxlError;
use rizxel_math::RefPlane;

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
    Glb,
    Gltf,
}

/// Settings for mesh exports, set in the export dialog (or `convert`
/// flags). Formats ignore the settings they do not use.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ExportOptions {
    /// Units per voxel; `None` uses the format's default (OBJ and glTF 0.1,
    /// STL 1 mm).
    pub scale: Option<f32>,
    pub pivot: rizxel_io::Pivot,
    pub gltf_colors: rizxel_io::GltfColors,
}

impl ExportOptions {
    /// The scale used for `format`.
    pub fn scale_for(&self, format: FileFormat) -> f32 {
        self.scale
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or_else(|| format.default_scale())
    }
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
    pub const EXPORTS: [FileFormat; 7] = [
        FileFormat::Glb,
        FileFormat::Gltf,
        FileFormat::Obj,
        FileFormat::Vox,
        FileFormat::Gox,
        FileFormat::Stl,
        FileFormat::Mtl,
    ];

    /// Every format, for extension lookup.
    pub const ALL: [FileFormat; 8] = [
        FileFormat::Rzxl,
        FileFormat::Vox,
        FileFormat::Gox,
        FileFormat::Obj,
        FileFormat::Mtl,
        FileFormat::Stl,
        FileFormat::Glb,
        FileFormat::Gltf,
    ];

    /// True for formats that bake the model into a triangle mesh.
    pub const fn is_mesh(self) -> bool {
        matches!(
            self,
            FileFormat::Obj | FileFormat::Stl | FileFormat::Glb | FileFormat::Gltf
        )
    }

    /// Default units per voxel of a mesh export.
    pub const fn default_scale(self) -> f32 {
        match self {
            FileFormat::Stl => rizxel_io::DEFAULT_STL_SCALE,
            FileFormat::Glb | FileFormat::Gltf => rizxel_io::DEFAULT_GLTF_SCALE,
            _ => rizxel_io::DEFAULT_OBJ_SCALE,
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            FileFormat::Rzxl => rizxel_io::RZXL_EXTENSION,
            FileFormat::Vox => rizxel_io::VOX_EXTENSION,
            FileFormat::Gox => rizxel_io::GOX_EXTENSION,
            FileFormat::Obj => rizxel_io::OBJ_EXTENSION,
            FileFormat::Mtl => rizxel_io::MTL_EXTENSION,
            FileFormat::Stl => rizxel_io::STL_EXTENSION,
            FileFormat::Glb => rizxel_io::GLB_EXTENSION,
            FileFormat::Gltf => rizxel_io::GLTF_EXTENSION,
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
            FileFormat::Glb => "glTF Binary",
            FileFormat::Gltf => "glTF",
        }
    }

    /// What an export writes, for menus and tooltips.
    pub const fn export_note(self) -> &'static str {
        match self {
            FileFormat::Obj => "OBJ + MTL + palette PNG (MagicaVoxel layout, scale 0.1)",
            FileFormat::Mtl => "MTL + palette PNG",
            FileFormat::Vox => "one model per layer, groups kept",
            FileFormat::Gox => "layers, groups, clones, shapes, materials",
            FileFormat::Stl => "binary, Z-up, 1 voxel = 1 mm",
            FileFormat::Glb => "one file: a node per layer, 1 voxel = 0.1 m",
            FileFormat::Gltf => ".gltf + .bin: a node per layer, 1 voxel = 0.1 m",
            FileFormat::Rzxl => "",
        }
    }

    /// Format of `path` by its extension (case-insensitive).
    pub fn from_path(path: &Path) -> Option<FileFormat> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        FileFormat::ALL.into_iter().find(|f| f.extension() == ext)
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

/// Extensions of the images [`AppState::add_reference_file`] accepts.
pub const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// True for a path with a PNG or JPEG extension.
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

impl AppState {
    /// Plane the camera faces most (the one whose normal is the facing
    /// axis): where dropped images go.
    pub fn facing_plane(&self) -> RefPlane {
        match self.facing_axis() {
            0 => RefPlane::Yz,
            1 => RefPlane::Xz,
            _ => RefPlane::Xy,
        }
    }

    /// Adds the PNG or JPEG file `path` to the active document as a
    /// reference image on `plane` (the plane the camera faces most when
    /// `None`), fitted to the bounds. One undo step; returns a status
    /// message, which notes when a large image was scaled down.
    pub fn add_reference_file(
        &mut self,
        path: &Path,
        plane: Option<RefPlane>,
    ) -> Result<String, RzxlError> {
        let plane = plane.unwrap_or_else(|| self.facing_plane());
        let imported = rizxel_io::import_image(read_limited(path)?)?;
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Reference".to_owned());
        let doc = self
            .doc_mut()
            .ok_or_else(|| RzxlError::Invalid("no open document".to_owned()))?;
        let label = format!("Add Reference \"{name}\"");
        doc.labeled(label, |d| {
            d.add_reference(name.clone(), imported.image, plane)
        })
        .ok_or_else(|| {
            RzxlError::Invalid(format!(
                "a project holds at most {} reference images",
                rizxel_io::MAX_REFERENCES
            ))
        })?;
        Ok(match imported.downscaled_from {
            Some((w, h)) => format!(
                "Added {name} on {}: scaled down from {w}×{h} to {} px",
                plane.label(),
                rizxel_io::MAX_REFERENCE_SIDE
            ),
            None => format!("Added {name} on {}", plane.label()),
        })
    }

    /// *Replace Image…*: gives reference `index` the image in `path`,
    /// keeping its placement.
    pub fn replace_reference_file(
        &mut self,
        index: usize,
        path: &Path,
    ) -> Result<String, RzxlError> {
        let imported = rizxel_io::import_image(read_limited(path)?)?;
        let doc = self
            .doc_mut()
            .ok_or_else(|| RzxlError::Invalid("no open document".to_owned()))?;
        if !doc.labeled("Replace Reference Image", |d| {
            d.replace_reference_image(index, imported.image)
        }) {
            return Err(RzxlError::Invalid(
                "no such reference image, or it already shows this image".to_owned(),
            ));
        }
        Ok(format!("Replaced the image with {}", path.display()))
    }

    /// *Show All / Hide All* (`Alt+R`).
    pub fn toggle_all_references(&mut self) {
        let Some(doc) = self.doc_mut() else { return };
        if doc.data.references.is_empty() {
            self.status_message =
                Some("No reference images: add one in the References panel".to_owned());
        } else {
            doc.toggle_all_references();
        }
    }

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
            FileFormat::Glb | FileFormat::Gltf => {
                return Err(RzxlError::Invalid(
                    "glTF files can be exported but not imported".into(),
                ))
            }
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
            FileFormat::Obj | FileFormat::Stl | FileFormat::Glb | FileFormat::Gltf => {
                doc.visible_voxel_count() > 0
            }
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

    /// Exports the active document to `path` with the default settings.
    /// OBJ, MTL and `.gltf` also write their companion files next to it.
    /// Returns a status message.
    pub fn export_active(&self, path: &Path, format: FileFormat) -> Result<String, RzxlError> {
        self.export_active_with(path, format, &ExportOptions::default())
    }

    /// [`Self::export_active`] with explicit mesh settings.
    pub fn export_active_with(
        &self,
        path: &Path,
        format: FileFormat,
        options: &ExportOptions,
    ) -> Result<String, RzxlError> {
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
            FileFormat::Glb | FileFormat::Gltf => {
                let scene = doc.export_scene();
                let gltf = rizxel_io::GltfOptions {
                    scale: options.scale_for(format),
                    colors: options.gltf_colors,
                    origin: options.pivot.origin(scene.bounds),
                };
                let e = rizxel_io::write_gltf_file(path, &scene, &gltf)?;
                let bin = if format == FileFormat::Gltf {
                    ", with .bin"
                } else {
                    ""
                };
                format!("{} nodes, {} triangles{bin}", e.mesh_nodes, e.triangles)
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
            // (glTF has no importer; see `gltf_export_writes_nodes_per_layer`.)
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
    fn gltf_export_writes_nodes_per_layer() {
        let d = dir().join("gltf");
        std::fs::create_dir_all(&d).unwrap();
        let src = sample_state();
        for format in [FileFormat::Glb, FileFormat::Gltf] {
            let path = d.join(format!("model.{}", format.extension()));
            assert!(src.can_export(format));
            let msg = src.export_active(&path, format).unwrap();
            // The block and its linked clone; the hidden layer is skipped.
            assert!(msg.contains("2 nodes"), "{msg}");
        }
        assert!(d.join("model.bin").exists());
        let mut dst = AppState::default();
        assert!(dst.import(&d.join("model.glb")).is_err(), "export only");
        let glb = std::fs::read(d.join("model.glb")).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        let text = std::fs::read_to_string(d.join("model.gltf")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            json["meshes"].as_array().unwrap().len(),
            1,
            "clone instanced"
        );
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
