//! # rizxel-io (L2)
//!
//! Serialization layer:
//! - [`format_rzxl`]: the native `.rzxl` project format (default save format).
//! - [`format_obj`]: Wavefront OBJ mesh export.

pub mod format_obj;
pub mod format_rzxl;

pub use format_obj::{export_obj, write_obj_file};
pub use format_rzxl::{
    generator, linear_index, load_rzxl, read_rzxl, runs_from_volume, save_rzxl, volume_from_runs,
    write_rzxl, LayerData, ProjectFile, RzxlError, VoxelRun, MAX_GROUP_DEPTH, MAX_LAYERS,
    MAX_MATERIALS, MAX_NAME_CHARS, RZXL_EXTENSION, RZXL_VERSION,
};
