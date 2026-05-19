//! # bar-data
//!
//! Core data types for BAR map editor: heightmap buffers, color textures,
//! .sd7/.smf/.smt format I/O, and image import/export.

pub mod coastmap;
pub mod color;
pub mod heightmap;
pub mod s3o;
pub mod sd7;
pub mod skybox;
pub mod smt;
pub mod water_assets;

pub use coastmap::{bake_coastmap, COAST_DISTANCE_TEXELS, FULL_DEPTH_ELMOS};
pub use color::ColorBuffer;
pub use heightmap::Heightmap;
pub use s3o::{parse_s3o, S3oError, S3oMesh, S3oVertex};
pub use sd7::{SmfFeaturePlacement, SmfHeader, SmfMap};
pub use skybox::{
    load_dds_2d, load_dds_2d_bytes, load_dds_2d_with_mips, load_dds_cubemap, Cubemap, DdsMip,
    SkyboxError,
};
pub use smt::{
    assemble_bc1_linear, compress_image_dxt1, decode_dxt1_block, decode_smf_minimap_base,
    decode_tile_dxt1, generate_minimap_dxt1, read_smt, read_smt_raw, write_smt, DXT1_TILE_BYTES,
    MINIMAP_BASE_DXT1_BYTES, MINIMAP_SIZE,
};
pub use water_assets::{
    load_from_archive as load_water_assets_from_archive,
    load_from_engine_dir as load_water_assets_from_engine_dir, locate_bitmaps_archive,
    WaterAssetError, WaterAssetSet, WaterTexture,
};
