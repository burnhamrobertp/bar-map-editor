//! Texture + map-layer nodes: splat composition, height-to-colour colorizers,
//! and the normal / grass / specular map generators. Most emit a `Color`
//! buffer; the analytical maps emit a `Heightmap`.

pub mod shared;

pub mod auto_texture;
pub mod color_ramp;
pub mod grass_map;
pub mod layer_blend;
pub mod lightmap_bake;
pub mod normal_map;
pub mod rock_soil;
pub mod specular_map;
pub mod terrain_splat;
pub mod texture_weightmap;
pub mod vegetation;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[
    &terrain_splat::DEF,
    &auto_texture::DEF,
    &rock_soil::DEF,
    &vegetation::DEF,
    &layer_blend::DEF,
    &texture_weightmap::DEF,
    &color_ramp::DEF,
    &lightmap_bake::DEF,
    &normal_map::DEF,
    &grass_map::DEF,
    &specular_map::DEF,
];
