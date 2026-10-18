//! # rizxel-io (L2)
//!
//! Serialization layer:
//! - [`format_rzxl`]: the native `.rzxl` project format (default save format).
//! - [`format_obj`]: Wavefront OBJ mesh export.

pub mod format_obj;
pub mod format_rzxl;

pub use format_obj::{export_obj, write_obj_file};
pub use format_rzxl::{
    load_rzxl, read_rzxl, save_rzxl, write_rzxl, LayerData, ProjectFile, RzxlError, RZXL_EXTENSION,
    RZXL_VERSION,
};
