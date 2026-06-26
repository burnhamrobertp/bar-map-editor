//! Executors for the texture / map-layer nodes.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod auto_texture;
pub mod color_ramp;
pub mod grass_map;
pub mod layer_blend;
pub mod lightmap_bake;
pub mod rock_soil;
pub mod shared;
pub mod specular_map;
pub mod terrain_splat;
pub mod texture_weightmap;
pub mod vegetation;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::TerrainSplat, terrain_splat::exec);
    m.insert(NodeType::AutoTexture, auto_texture::exec);
    m.insert(NodeType::RockSoil, rock_soil::exec);
    m.insert(NodeType::Vegetation, vegetation::exec);
    m.insert(NodeType::LayerBlend, layer_blend::exec);
    m.insert(NodeType::TextureWeightmap, texture_weightmap::exec);
    m.insert(NodeType::ColorRamp, color_ramp::exec);
    m.insert(NodeType::LightmapBake, lightmap_bake::exec);
    m.insert(NodeType::GrassMap, grass_map::exec);
    m.insert(NodeType::SpecularMap, specular_map::exec);
}
