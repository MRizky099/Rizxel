//! # rizxel-io (L2)
//!
//! Serialization layer:
//! - [`format_rzxl`]: the native `.rzxl` project format (default save format).
//! - [`format_vox`]: MagicaVoxel `.vox` import/export.
//! - [`format_gox`]: Goxel `.gox` import/export.
//! - [`format_obj`] / [`format_mtl`]: Wavefront OBJ + MTL export in the
//!   MagicaVoxel layout, and OBJ import.
//! - [`format_stl`]: binary STL export, binary/ASCII STL import.
//! - [`mesh_import`]: voxelizes imported OBJ/STL meshes into a project.

pub mod format_gox;
pub mod format_mtl;
pub mod format_obj;
pub mod format_rzxl;
pub mod format_stl;
pub mod format_vox;
pub mod interchange;
pub mod mesh_import;
pub mod png_io;

pub use format_gox::{read_gox, write_gox, GOX_EXTENSION};
pub use format_mtl::{palette_from_mtl, read_mtl, MtlMaterial, MTL_EXTENSION};
pub use format_obj::{
    build_obj_export, export_obj, read_obj, sibling_loader, write_obj_file, write_obj_set,
    ObjExport, ObjMesh, DEFAULT_OBJ_SCALE, OBJ_EXTENSION,
};
pub use format_rzxl::{
    generator, linear_index, load_rzxl, read_rzxl, runs_from_volume, save_rzxl, volume_from_runs,
    write_rzxl, LayerData, ProjectFile, RzxlError, VoxelRun, MAX_GROUP_DEPTH, MAX_LAYERS,
    MAX_MATERIALS, MAX_NAME_CHARS, RZXL_EXTENSION, RZXL_VERSION,
};
pub use format_stl::{read_stl, write_stl, DEFAULT_STL_SCALE, STL_EXTENSION};
pub use format_vox::{read_vox, write_vox, VOX_EXTENSION};
pub use interchange::{evaluate_layer, ExportLayer, ExportScene};
pub use mesh_import::{project_from_mesh, uncolored, MeshImportInfo, UpAxis};
