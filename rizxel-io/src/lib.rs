//! # rizxel-io (L2)
//!
//! Serialization layer:
//! - [`format_rzxl`]: the native `.rzxl` project format (default save format).
//! - [`format_vox`]: MagicaVoxel `.vox` import/export.
//! - [`format_gox`]: Goxel `.gox` import/export.
//! - [`format_gltf`]: glTF 2.0 `.glb` / `.gltf` export.
//! - [`format_obj`] / [`format_mtl`]: Wavefront OBJ + MTL export (Fast load
//!   or MagicaVoxel layout) and OBJ import.
//! - [`format_ply`]: Stanford `.ply` export and import.
//! - [`format_qb`]: Qubicle Binary `.qb` import/export.
//! - [`format_qbt`]: Qubicle Binary Tree `.qbt` import/export.
//! - [`format_qef`]: Qubicle Exchange `.qef` import/export.
//! - [`format_stl`]: binary STL export, binary/ASCII STL import.
//! - [`mesh_import`]: voxelizes imported OBJ/STL meshes into a project.
//! - [`format_palette`]: GIMP `.gpl` and `.hex` palette files.
//! - [`slices`]: Export Slices (one PNG per voxel slice).
//! - [`heightmap`]: grayscale image -> voxel terrain.
//! - [`image_io`]: PNG and JPEG reference images.

pub mod format_gltf;
pub mod format_gox;
pub mod format_mtl;
pub mod format_obj;
pub mod format_palette;
pub mod format_ply;
pub mod format_qb;
pub mod format_qbt;
pub mod format_qef;
pub mod format_rzxl;
pub mod format_stl;
pub mod format_vox;
pub mod heightmap;
pub mod image_io;
pub mod interchange;
pub mod mesh_export;
pub mod mesh_import;
pub mod png_io;
mod qubicle;
pub mod slices;
#[cfg(test)]
mod test_samples;

pub use format_gltf::{
    build_gltf, write_gltf_file, GltfColors, GltfExport, GltfOptions, DEFAULT_GLTF_SCALE,
    GLB_EXTENSION, GLTF_EXTENSION,
};
pub use format_gox::{read_gox, write_gox, GOX_EXTENSION};
pub use format_mtl::{palette_from_mtl, read_mtl, MtlMaterial, MTL_EXTENSION};
pub use format_obj::{
    build_obj, build_obj_export, export_obj, read_obj, sibling_loader, write_obj, write_obj_file,
    write_obj_set, ObjExport, ObjLayout, ObjMesh, ObjOptions, DEFAULT_OBJ_SCALE, FAST_LOAD_MARK,
    OBJ_EXTENSION,
};
pub use format_palette::{
    load_palette_file, read_gpl, read_hex, save_palette_file, write_gpl, write_hex, PaletteFile,
    GPL_EXTENSION, HEX_EXTENSION,
};
pub use format_ply::{read_ply, write_ply, PLY_EXTENSION};
pub use format_qb::{read_qb, write_qb, QB_EXTENSION};
pub use format_qbt::{read_qbt, write_qbt, QBT_EXTENSION};
pub use format_qef::{read_qef, write_qef, QEF_EXTENSION};
pub use format_rzxl::{
    generator, linear_index, load_rzxl, read_rzxl, runs_from_volume, save_rzxl, volume_from_runs,
    write_rzxl, LayerData, ProjectFile, RzxlError, VoxelRun, MAX_GROUP_DEPTH, MAX_LAYERS,
    MAX_MATERIALS, MAX_NAME_CHARS, MAX_REFERENCES, MAX_REFERENCE_BYTES, RZXL_EXTENSION,
    RZXL_VERSION,
};
pub use format_stl::{read_stl, write_stl, DEFAULT_STL_SCALE, STL_EXTENSION};
pub use format_vox::{read_vox, write_vox, VOX_EXTENSION};
pub use heightmap::{project_from_heightmap, HeightmapOptions};
pub use image_io::{
    check_reference, decode_fitted, decode_reference, import_image, ImportedImage,
    MAX_REFERENCE_SIDE,
};
pub use interchange::{evaluate_layer, ExportGroup, ExportLayer, ExportScene};
pub use mesh_export::Pivot;
pub use mesh_import::{
    oriented, preview_mesh, project_from_mesh, project_from_mesh_turned, uncolored, MeshImportInfo,
    QuarterTurns, UpAxis,
};
pub use slices::{export_slices, Slice, SliceAxis, MAX_SLICE_SCALE};
