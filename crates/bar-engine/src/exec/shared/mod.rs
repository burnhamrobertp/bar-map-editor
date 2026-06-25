//! Cross-cutting compute helpers shared by multiple node families.
//!
//! Family-local kernels stay in their family modules (or `executor.rs`); this
//! module holds only helpers reached from more than one place.

pub mod inputs;
pub mod modulation;
pub mod params;
pub mod texture_assembly;

pub(crate) use inputs::{
    get_input_color, get_input_heightmap, get_input_scalar, get_optional_heightmap,
};
pub(crate) use modulation::{apply_invert, apply_modulation, scale_by_field};
pub(crate) use params::{get_bool, get_float, get_string, get_uint};
pub(crate) use texture_assembly::assemble_texture_preview;
