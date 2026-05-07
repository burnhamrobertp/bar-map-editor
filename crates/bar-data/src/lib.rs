//! # bar-data
//!
//! Core data types for BAR map editor: heightmap buffers, color textures,
//! .sd7/.smf/.smt format I/O, and image import/export.

pub mod color;
pub mod heightmap;
pub mod sd7;
pub mod smt;

pub use color::ColorBuffer;
pub use heightmap::Heightmap;
pub use sd7::{SmfHeader, SmfMap};
pub use smt::{compress_image_dxt1, decode_tile_dxt1, generate_minimap_dxt1, read_smt, write_smt,
              MINIMAP_SIZE};
