//! Importing and exporting other file formats from the application state.
//!
//! | Format | Import | Export |
//! |---|---|---|
//! | MagicaVoxel `.vox` | new tab: layers, groups, linked clones, palette | one model per layer, names, hidden flags, palette |
//! | Goxel `.gox` | new tab: layers, groups, clones, shapes, materials | same, natively where Goxel can |
//! | Wavefront `.obj` | new tab: voxelized, exact for voxel-editor exports | `.obj` + `.mtl` (Fast load), or + palette `.png` (MagicaVoxel layout) |
//! | `.mtl` | palette and materials of the active document | `.mtl` + palette `.png` |
//! | STL `.stl` | new tab: voxelized, one color | binary STL, Z-up, 1 voxel = 1 mm |
//! | glTF `.glb` / `.gltf` | — | one node per visible layer, groups, instanced clones |
//! | Qubicle `.qb` | new tab: one layer per matrix | one matrix per visible layer |
//! | Qubicle `.qef` | new tab: one layer, palette order kept | merged visible layers |
//! | Qubicle `.qbt` | new tab: matrices, models/compounds as groups | matrices, groups as compounds |
//! | Stanford `.ply` | new tab: voxelized like STL, vertex colors | binary, Z-up, vertex colors |
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
    Qb,
    Qef,
    Qbt,
    Ply,
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
    pub obj_layout: rizxel_io::ObjLayout,
    /// One file per visible layer (OBJ, STL, PLY; glTF has nodes).
    pub per_layer: bool,
}

/// A mesh read for the Voxelizer dialog.
#[derive(Debug, Clone)]
pub struct PendingMesh {
    pub path: std::path::PathBuf,
    pub triangles: Vec<rizxel_engine::Triangle>,
    pub up: rizxel_io::UpAxis,
    /// The 256×1 palette texture, for voxel-editor OBJ exports.
    pub palette: Option<rizxel_math::Palette>,
}

/// Voxelizer dialog settings.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VoxelizerSettings {
    pub voxelize: VoxelizeOptions,
    /// Quarter turns about X, Y and Z.
    pub turns: rizxel_io::QuarterTurns,
}

/// What the Voxelizer would make with some settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelizerPreview {
    pub size: glam::UVec3,
    pub voxels: usize,
    pub exact: bool,
}

impl PendingMesh {
    /// Voxelizes without building a document (the dialog's live count).
    pub fn preview(&self, settings: &VoxelizerSettings) -> Result<VoxelizerPreview, RzxlError> {
        let (size, voxels, exact) =
            rizxel_io::preview_mesh(&self.triangles, self.up, settings.turns, settings.voxelize)?;
        Ok(VoxelizerPreview {
            size,
            voxels,
            exact,
        })
    }
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
    pub const IMPORTS: [FileFormat; 9] = [
        FileFormat::Vox,
        FileFormat::Gox,
        FileFormat::Qb,
        FileFormat::Qbt,
        FileFormat::Qef,
        FileFormat::Obj,
        FileFormat::Stl,
        FileFormat::Ply,
        FileFormat::Mtl,
    ];
    /// Formats offered under File ▸ Export.
    pub const EXPORTS: [FileFormat; 11] = [
        FileFormat::Glb,
        FileFormat::Gltf,
        FileFormat::Obj,
        FileFormat::Vox,
        FileFormat::Gox,
        FileFormat::Qb,
        FileFormat::Qbt,
        FileFormat::Qef,
        FileFormat::Stl,
        FileFormat::Ply,
        FileFormat::Mtl,
    ];

    /// Every format, for extension lookup.
    pub const ALL: [FileFormat; 12] = [
        FileFormat::Rzxl,
        FileFormat::Vox,
        FileFormat::Gox,
        FileFormat::Obj,
        FileFormat::Mtl,
        FileFormat::Stl,
        FileFormat::Glb,
        FileFormat::Gltf,
        FileFormat::Qb,
        FileFormat::Qef,
        FileFormat::Qbt,
        FileFormat::Ply,
    ];

    /// True for formats that bake the model into a triangle mesh.
    pub const fn is_mesh(self) -> bool {
        matches!(
            self,
            FileFormat::Obj
                | FileFormat::Stl
                | FileFormat::Glb
                | FileFormat::Gltf
                | FileFormat::Ply
        )
    }

    /// True for mesh formats that can write one file per layer.
    pub const fn allows_per_layer(self) -> bool {
        matches!(self, FileFormat::Obj | FileFormat::Stl | FileFormat::Ply)
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
            FileFormat::Qb => rizxel_io::QB_EXTENSION,
            FileFormat::Qef => rizxel_io::QEF_EXTENSION,
            FileFormat::Qbt => rizxel_io::QBT_EXTENSION,
            FileFormat::Ply => rizxel_io::PLY_EXTENSION,
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
            FileFormat::Qb => "Qubicle Binary",
            FileFormat::Qef => "Qubicle Exchange",
            FileFormat::Qbt => "Qubicle Binary Tree",
            FileFormat::Ply => "Stanford PLY",
        }
    }

    /// What an export writes, for menus and tooltips.
    pub const fn export_note(self) -> &'static str {
        match self {
            FileFormat::Obj => "OBJ + MTL, Fast load or MagicaVoxel layout, scale 0.1",
            FileFormat::Mtl => "MTL + palette PNG",
            FileFormat::Vox => "one model per layer, groups kept",
            FileFormat::Gox => "layers, groups, clones, shapes, materials",
            FileFormat::Stl => "binary, Z-up, 1 voxel = 1 mm",
            FileFormat::Glb => "one file: a node per layer, 1 voxel = 0.1 m",
            FileFormat::Gltf => ".gltf + .bin: a node per layer, 1 voxel = 0.1 m",
            FileFormat::Qb => "one matrix per visible layer",
            FileFormat::Qef => "one matrix, palette order kept",
            FileFormat::Qbt => "matrices, groups as compounds",
            FileFormat::Ply => "binary, Z-up, vertex colors, scale 0.1",
            FileFormat::Rzxl => "",
        }
    }

    /// Format of `path` by its extension (case-insensitive).
    pub fn from_path(path: &Path) -> Option<FileFormat> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        FileFormat::ALL.into_iter().find(|f| f.extension() == ext)
    }
}

/// Writes the visible layers of `scene` as the mesh `format`. Returns the
/// detail for the status message.
fn export_mesh(
    path: &Path,
    format: FileFormat,
    scene: &rizxel_io::ExportScene,
    options: &ExportOptions,
) -> Result<String, RzxlError> {
    let scale = options.scale_for(format);
    let origin = match format {
        FileFormat::Stl | FileFormat::Ply => options.pivot.origin_zup(scene.bounds),
        _ => options.pivot.origin(scene.bounds),
    };
    Ok(match format {
        FileFormat::Obj => {
            let obj = rizxel_io::ObjOptions {
                layout: options.obj_layout,
                scale,
                origin,
            };
            let e = rizxel_io::write_obj(path, scene, &obj)?;
            let files = if e.png.is_empty() {
                ".mtl"
            } else {
                ".mtl and .png"
            };
            format!(
                "{} triangles, {} layout, with {files}",
                e.triangles,
                options.obj_layout.label()
            )
        }
        FileFormat::Stl => {
            let bytes =
                rizxel_io::write_stl_with(&scene.merged_visible(), scene.bounds, scale, origin)?;
            let tris = (bytes.len() - 84) / 50;
            std::fs::write(path, bytes)?;
            format!("{tris} triangles")
        }
        FileFormat::Ply => {
            let bytes =
                rizxel_io::write_ply(&scene.merged_visible(), &scene.palette, scale, origin)?;
            std::fs::write(path, bytes)?;
            "binary, Z-up".to_owned()
        }
        FileFormat::Glb | FileFormat::Gltf => {
            let gltf = rizxel_io::GltfOptions {
                scale,
                colors: options.gltf_colors,
                origin,
            };
            let e = rizxel_io::write_gltf_file(path, scene, &gltf)?;
            let bin = if format == FileFormat::Gltf {
                ", with .bin"
            } else {
                ""
            };
            format!("{} nodes, {} triangles{bin}", e.mesh_nodes, e.triangles)
        }
        _ => {
            return Err(RzxlError::Invalid(format!(
                "{} is not a mesh format",
                format.label()
            )))
        }
    })
}

/// File-name-safe version of a layer name.
fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('_').to_owned();
    if s.is_empty() {
        "layer".to_owned()
    } else {
        s
    }
}

/// *Per-layer files*: one `stem_layer.ext` per visible layer with voxels,
/// all placed in the same frame so they line up when loaded together.
fn export_per_layer(
    path: &Path,
    format: FileFormat,
    scene: &rizxel_io::ExportScene,
    options: &ExportOptions,
) -> Result<String, RzxlError> {
    let base = stem(path);
    let ext = format.extension();
    let mut used: Vec<String> = Vec::new();
    let mut written = 0;
    for (i, layer) in scene.layers.iter().enumerate() {
        if !layer.visible || layer.volume.is_empty() {
            continue;
        }
        let mut name = format!("{base}_{}", safe_name(&layer.name));
        if used.contains(&name) {
            name = format!("{name}_{}", i + 1);
        }
        used.push(name.clone());
        let single = rizxel_io::ExportScene {
            layers: vec![rizxel_io::ExportLayer {
                group: None,
                instance_of: None,
                ..layer.clone()
            }],
            groups: Vec::new(),
            ..scene.clone()
        };
        export_mesh(
            &path.with_file_name(format!("{name}.{ext}")),
            format,
            &single,
            options,
        )?;
        written += 1;
    }
    if written == 0 {
        return Err(RzxlError::Invalid(
            "nothing to export: the visible layers contain no voxels".into(),
        ));
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    Ok(format!(
        "Exported {written} {} files ({base}_*.{ext}) to {}",
        format.label(),
        dir.display()
    ))
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
            FileFormat::Ply => {
                let tris = rizxel_io::read_ply(&read_limited(path)?, rizxel_io::uncolored())?;
                voxelized(rizxel_io::project_from_mesh(
                    tris,
                    rizxel_io::UpAxis::Z,
                    None,
                    VoxelizeOptions::default(),
                    &name,
                )?)
            }
            FileFormat::Qb => rizxel_io::read_qb(&read_limited(path)?, &name)?,
            FileFormat::Qbt => rizxel_io::read_qbt(&read_limited(path)?, &name)?,
            FileFormat::Qef => {
                let text = String::from_utf8_lossy(&read_limited(path)?).into_owned();
                rizxel_io::read_qef(&text, &name)?
            }
        };
        self.open_imported(project, path, &mesh_note)
    }

    /// Opens an imported project in a new tab (replacing a pristine untitled
    /// one) and returns the status message.
    fn open_imported(
        &mut self,
        project: rizxel_io::ProjectFile,
        path: &Path,
        note: &str,
    ) -> Result<String, RzxlError> {
        let mut doc = Document::from_project(project, None)?;
        if self.structure.fit_on_import {
            // An undo step of its own, so the original placement is one Ctrl+Z away.
            let _ = doc.fit_bounds_to_content();
        }
        doc.mark_unsaved();
        self.remember_recent(path);
        let summary = format!(
            "Imported {} ({} voxels, {} layers{note})",
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

    /// Reads an OBJ, STL or PLY file for the Voxelizer dialog, without
    /// voxelizing it yet.
    pub fn load_mesh(&self, path: &Path) -> Result<PendingMesh, RzxlError> {
        let format = FileFormat::from_path(path);
        let (triangles, up, palette) = match format {
            Some(FileFormat::Obj) => {
                let text = String::from_utf8_lossy(&read_limited(path)?).into_owned();
                let mesh = rizxel_io::read_obj(&text, &rizxel_io::sibling_loader(path))?;
                (mesh.triangles, rizxel_io::UpAxis::Y, mesh.palette)
            }
            Some(FileFormat::Stl) => (
                rizxel_io::read_stl(&read_limited(path)?, rizxel_io::uncolored())?,
                rizxel_io::UpAxis::Z,
                None,
            ),
            Some(FileFormat::Ply) => (
                rizxel_io::read_ply(&read_limited(path)?, rizxel_io::uncolored())?,
                rizxel_io::UpAxis::Z,
                None,
            ),
            _ => {
                return Err(RzxlError::Invalid(format!(
                    "not a mesh file (OBJ, STL or PLY): {}",
                    path.display()
                )))
            }
        };
        Ok(PendingMesh {
            path: path.to_path_buf(),
            triangles,
            up,
            palette,
        })
    }

    /// Voxelizes a mesh loaded with [`Self::load_mesh`] into a new tab.
    pub fn import_mesh(
        &mut self,
        mesh: &PendingMesh,
        settings: &VoxelizerSettings,
    ) -> Result<String, RzxlError> {
        let (project, info) = rizxel_io::project_from_mesh_turned(
            mesh.triangles.clone(),
            mesh.up,
            settings.turns,
            mesh.palette.clone(),
            settings.voxelize,
            &stem(&mesh.path),
        )?;
        let note = if info.exact {
            ", exact: the mesh's own voxel grid".to_owned()
        } else {
            format!(
                ", voxelized to {} voxels on the longest side",
                settings.voxelize.resolution
            )
        };
        self.open_imported(project, &mesh.path, &note)
    }

    /// *Import ▸ Heightmap*: the grayscale PNG or JPEG `path` (scaled down to
    /// 256 per side) becomes a terrain in a new tab, colored from
    /// `color` when given.
    pub fn import_heightmap(
        &mut self,
        path: &Path,
        color: Option<&Path>,
        options: rizxel_io::HeightmapOptions,
    ) -> Result<String, RzxlError> {
        let side = rizxel_math::BoundSize::MAX;
        let height = rizxel_io::decode_fitted(&read_limited(path)?, side)?;
        let color = match color {
            Some(c) => Some(rizxel_io::decode_fitted(&read_limited(c)?, 1024)?),
            None => None,
        };
        let project =
            rizxel_io::project_from_heightmap(&height, color.as_ref(), options, &stem(path))?;
        let note = format!(", heightmap {}×{}", height.width, height.height);
        self.open_imported(project, path, &note)
    }

    /// True when the active document can be exported as `format`.
    pub fn can_export(&self, format: FileFormat) -> bool {
        let Some(doc) = self.doc() else {
            return false;
        };
        match format {
            FileFormat::Obj
            | FileFormat::Stl
            | FileFormat::Glb
            | FileFormat::Gltf
            | FileFormat::Ply
            | FileFormat::Qb
            | FileFormat::Qef
            | FileFormat::Qbt => doc.visible_voxel_count() > 0,
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
        if format.is_mesh() {
            let scene = doc.export_scene();
            if options.per_layer && format.allows_per_layer() {
                return export_per_layer(path, format, &scene, options);
            }
            let detail = export_mesh(path, format, &scene, options)?;
            return Ok(format!("Exported {} ({detail})", path.display()));
        }
        let detail = match format {
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
            FileFormat::Qb | FileFormat::Qbt => {
                let scene = doc.export_scene();
                let bytes = if format == FileFormat::Qb {
                    rizxel_io::write_qb(&scene)?
                } else {
                    rizxel_io::write_qbt(&scene)?
                };
                std::fs::write(path, bytes)?;
                let n = scene
                    .layers
                    .iter()
                    .filter(|l| l.visible && !l.volume.is_empty())
                    .count();
                format!("{n} matrices")
            }
            FileFormat::Qef => {
                std::fs::write(path, rizxel_io::write_qef(&doc.export_scene())?)?;
                "one matrix".to_owned()
            }
            FileFormat::Obj
            | FileFormat::Stl
            | FileFormat::Ply
            | FileFormat::Glb
            | FileFormat::Gltf => unreachable!("mesh formats return above"),
            FileFormat::Rzxl => {
                rizxel_io::save_rzxl(path, &doc.to_project())?;
                "project copy".to_owned()
            }
        };
        Ok(format!("Exported {} ({detail})", path.display()))
    }

    /// *Export Slices*: one PNG per slice of the visible layers along
    /// `axis`, `scale` pixels per voxel, into the folder `dir` (created when
    /// missing). Returns a status message.
    pub fn export_slices(
        &self,
        dir: &Path,
        axis: rizxel_io::SliceAxis,
        scale: u32,
    ) -> Result<String, RzxlError> {
        let doc = self
            .doc()
            .ok_or_else(|| RzxlError::Invalid("no document is open".into()))?;
        let slices = rizxel_io::export_slices(
            &doc.merged_visible_volume(),
            &doc.data.palette,
            axis,
            scale,
            &doc.name,
        )?;
        std::fs::create_dir_all(dir)?;
        for s in &slices {
            std::fs::write(dir.join(&s.file_name), &s.png)?;
        }
        Ok(format!(
            "Exported {} {} slices to {}",
            slices.len(),
            axis.label(),
            dir.display()
        ))
    }

    /// True when the active document has visible voxels to export as OBJ.
    pub fn can_export_obj(&self) -> bool {
        self.can_export(FileFormat::Obj)
    }

    /// Exports the visible layers of the active document as OBJ + MTL in
    /// the default (Fast load) layout. Returns the number of triangles.
    pub fn export_active_obj(&self, path: &Path) -> Result<usize, RzxlError> {
        let doc = self
            .doc()
            .ok_or_else(|| RzxlError::Invalid("no document is open".into()))?;
        let scene = doc.export_scene();
        let options = rizxel_io::ObjOptions::new(scene.bounds);
        Ok(rizxel_io::write_obj(path, &scene, &options)?.triangles)
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
            FileFormat::Qb,
            FileFormat::Qbt,
            FileFormat::Qef,
            FileFormat::Ply,
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
                FileFormat::Obj | FileFormat::Stl | FileFormat::Ply => {
                    assert_eq!(doc.visible_voxel_count(), visible, "{format:?}");
                }
                // Qubicle files carry the visible layers (clones baked).
                FileFormat::Qb | FileFormat::Qbt => {
                    assert_eq!(doc.visible_voxel_count(), visible, "{format:?}");
                    assert!(doc.data.layers.iter().all(|l| l.visible));
                    assert_eq!(doc.data.layers.len(), 2, "{format:?}: block + clone");
                }
                FileFormat::Qef => {
                    assert_eq!(doc.visible_voxel_count(), visible);
                    assert_eq!(doc.data.layers.len(), 1);
                }
                _ => unreachable!(),
            }
        }
        // The default Fast load OBJ needs only its .mtl.
        assert!(d.join("model.mtl").exists() && !d.join("model.png").exists());
        let magica = ExportOptions {
            obj_layout: rizxel_io::ObjLayout::MagicaVoxel,
            ..ExportOptions::default()
        };
        let path = d.join("magica.obj");
        let msg = src
            .export_active_with(&path, FileFormat::Obj, &magica)
            .unwrap();
        assert!(msg.contains("MagicaVoxel layout"), "{msg}");
        assert!(d.join("magica.png").exists());
        let mut dst = AppState::default();
        dst.import(&path).unwrap();
        assert_eq!(dst.doc().unwrap().visible_voxel_count(), visible);
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
    fn voxelizer_and_heightmap_imports_open_new_tabs() {
        let d = dir().join("voxelizer");
        std::fs::create_dir_all(&d).unwrap();
        let src = sample_state();
        let stl = d.join("m.stl");
        src.export_active(&stl, FileFormat::Stl).unwrap();
        let mut s = AppState::default();
        let mesh = s.load_mesh(&stl).unwrap();
        let exact = VoxelizerSettings::default();
        let p = mesh.preview(&exact).unwrap();
        assert!(p.exact);
        assert_eq!(p.voxels, src.doc().unwrap().visible_voxel_count());
        let coarse = VoxelizerSettings {
            voxelize: VoxelizeOptions {
                detect_grid: false,
                resolution: 4,
            },
            turns: [0, 1, 0],
        };
        let q = mesh.preview(&coarse).unwrap();
        assert!(!q.exact && q.size.max_element() == 4);
        let msg = s.import_mesh(&mesh, &coarse).unwrap();
        assert!(msg.contains("voxelized to 4"), "{msg}");
        assert!(s.load_mesh(&d.join("m.vox")).is_err());

        // Heightmap: a 2×1 PNG, black and white.
        let png =
            rizxel_io::png_io::encode_rgba(2, 1, &[0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
        let hm = d.join("terrain.png");
        std::fs::write(&hm, png).unwrap();
        let msg = s
            .import_heightmap(&hm, None, rizxel_io::HeightmapOptions { max_height: 6 })
            .unwrap();
        assert!(msg.contains("6 voxels"), "{msg}");
        assert_eq!(s.doc().unwrap().data.bounds.y(), 6);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn mesh_options_apply_to_every_mesh_format() {
        let d = dir().join("options");
        std::fs::create_dir_all(&d).unwrap();
        let src = sample_state();
        // Per-layer files: the block and its clone (the hidden layer is out).
        for format in [FileFormat::Obj, FileFormat::Stl, FileFormat::Ply] {
            let options = ExportOptions {
                per_layer: true,
                ..ExportOptions::default()
            };
            let path = d.join(format!("parts.{}", format.extension()));
            let msg = src.export_active_with(&path, format, &options).unwrap();
            assert!(msg.contains("Exported 2"), "{msg}");
            let files: Vec<_> = std::fs::read_dir(&d)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|x| x == format.extension())
                })
                .collect();
            assert_eq!(files.len(), 2, "{format:?}");
        }
        // Scale and pivot reach STL: corner pivot, 2 units per voxel.
        let options = ExportOptions {
            scale: Some(2.0),
            pivot: rizxel_io::Pivot::Corner,
            ..ExportOptions::default()
        };
        let path = d.join("big.stl");
        src.export_active_with(&path, FileFormat::Stl, &options)
            .unwrap();
        let tris =
            rizxel_io::read_stl(&std::fs::read(&path).unwrap(), rizxel_io::uncolored()).unwrap();
        let lo = tris
            .iter()
            .flat_map(|t| t.p)
            .fold(glam::Vec3::splat(f32::MAX), |a, b| a.min(b));
        let hi = tris
            .iter()
            .flat_map(|t| t.p)
            .fold(glam::Vec3::splat(f32::MIN), |a, b| a.max(b));
        assert!(
            lo.min_element() >= -1e-4,
            "corner pivot: all positive, {lo}"
        );
        // The 4×3 block is 8 units wide on X.
        assert!(hi.x >= 8.0, "{hi}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn slices_are_written_into_a_folder() {
        let d = dir().join("slices");
        let s = sample_state();
        let msg = s.export_slices(&d, rizxel_io::SliceAxis::Y, 2).unwrap();
        let height = s.doc().unwrap().data.bounds.y() as usize;
        assert!(msg.contains(&format!("{height} Y slices")), "{msg}");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), height);
        assert!(AppState::default()
            .export_slices(&d, rizxel_io::SliceAxis::Y, 1)
            .is_err());
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
