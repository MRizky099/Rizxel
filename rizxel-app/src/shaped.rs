//! *Export Shaped Mesh* (Phase 11b): the model drawn with Render mode's
//! voxel shape (bricks with studs, rounded cubes, spheres, cylinders) as a
//! real mesh. Only offered in Render mode with a shape other than Cube;
//! Model-mode exports never look at the render settings.

use std::path::Path;

use rizxel_engine::{shape_mesh, shape_triangle_count, Mesh, ShapeMeshOptions, ShapeQuality};
use rizxel_io::shaped_export::{shaped_glb, shaped_obj, shaped_stl, ShapedExportOptions};
use rizxel_io::{Pivot, RzxlError};
use rizxel_math::VoxelShape;

use crate::document::Document;
use crate::render_job::write_file;
use crate::{AppState, ViewMode};

/// Options of the shaped export dialog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedExport {
    pub quality: ShapeQuality,
    pub studs: bool,
    /// Use the render settings' cell size (else full cells).
    pub use_cell: bool,
    /// Units per voxel.
    pub scale: f32,
    pub pivot: Pivot,
}

impl Default for ShapedExport {
    fn default() -> Self {
        Self {
            quality: ShapeQuality::Medium,
            studs: true,
            use_cell: true,
            scale: rizxel_io::DEFAULT_OBJ_SCALE,
            pivot: Pivot::default(),
        }
    }
}

/// Formats of the shaped export, by file extension.
pub const SHAPED_EXTENSIONS: [&str; 3] = ["obj", "stl", "glb"];

impl Document {
    /// Shape options for this document's render settings.
    pub fn shape_options(&self, e: &ShapedExport) -> ShapeMeshOptions {
        let d = &self.data.render.display;
        ShapeMeshOptions {
            shape: d.shape,
            cell: if e.use_cell { d.cell } else { 1.0 },
            bevel: d.bevel,
            quality: e.quality,
            studs: e.studs,
        }
    }

    /// The visible layers drawn as `opts.shape`, in voxel space.
    pub fn shaped_mesh(&self, opts: &ShapeMeshOptions) -> Mesh {
        self.with_visible_layers(|layers| {
            let layers: Vec<_> = layers
                .iter()
                .map(|(i, v)| (*v, self.material_slot(*i)))
                .collect();
            shape_mesh(&layers, &self.data.palette, opts)
        })
    }

    /// Triangles [`Document::shaped_mesh`] would have.
    pub fn shaped_triangles(&self, opts: &ShapeMeshOptions) -> usize {
        self.with_visible_layers(|layers| {
            let layers: Vec<_> = layers.iter().map(|(_, v)| (*v, 0)).collect();
            shape_triangle_count(&layers, opts)
        })
    }

    /// Writes the shaped mesh to `path` (`.obj` + `.mtl`, `.stl` or `.glb`).
    pub fn export_shaped(&self, path: &Path, e: &ShapedExport) -> Result<String, RzxlError> {
        let opts = self.shape_options(e);
        let mesh = self.shaped_mesh(&opts);
        let placement = ShapedExportOptions {
            scale: e.scale,
            origin: e.pivot.origin(self.data.bounds),
        };
        let ext = path
            .extension()
            .map(|x| x.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let io = |e: std::io::Error| RzxlError::Io(e);
        match ext.as_str() {
            "obj" => {
                let mtl_path = path.with_extension("mtl");
                let mtl_name = mtl_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let (obj, mtl) = shaped_obj(&mesh, &self.data.materials, &placement, &mtl_name)?;
                write_file(path, obj.as_bytes()).map_err(io)?;
                write_file(&mtl_path, mtl.as_bytes()).map_err(io)?;
            }
            "stl" => write_file(path, &shaped_stl(&mesh, &placement)?).map_err(io)?,
            "glb" => {
                let bytes = shaped_glb(&mesh, &self.data.materials, &placement, &self.name)?;
                write_file(path, &bytes).map_err(io)?
            }
            other => {
                return Err(RzxlError::Invalid(format!(
                    "shaped meshes export to OBJ, STL or GLB, not .{other}"
                )))
            }
        }
        Ok(format!(
            "Exported {} {} shape ({} triangles)",
            path.display(),
            opts.shape.label().to_lowercase(),
            mesh.triangle_count()
        ))
    }
}

impl AppState {
    /// *Export Shaped Mesh* is offered: Render mode with a shape other than
    /// Cube.
    pub fn can_export_shaped(&self) -> bool {
        self.view_mode == ViewMode::Render
            && self
                .doc()
                .is_some_and(|d| d.data.render.display.shape != VoxelShape::Cube)
    }

    /// Exports the active document's shaped mesh; refused outside Render
    /// mode or for the cube shape.
    pub fn export_shaped(&mut self, path: &Path, e: &ShapedExport) -> Result<String, RzxlError> {
        if !self.can_export_shaped() {
            return Err(RzxlError::Invalid(
                "shaped export needs Render mode and a shape other than Cube".into(),
            ));
        }
        let doc = self
            .doc()
            .ok_or_else(|| RzxlError::Invalid("no document".into()))?;
        let message = doc.export_shaped(path, e)?;
        self.status_message = Some(message.clone());
        Ok(message)
    }
}
