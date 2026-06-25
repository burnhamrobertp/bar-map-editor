//! Executors for the paint + import source nodes.

use std::collections::HashMap;

use bar_graph::NodeType;

use super::ExecFn;

pub mod file_input;
pub mod imported_texture;
pub mod painted_heightmap;
pub mod painted_texture;
pub mod shared;

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::PaintedHeightmap, painted_heightmap::exec);
    m.insert(NodeType::PaintedTexture, painted_texture::exec);
    m.insert(NodeType::ImportedTexture, imported_texture::exec);
    m.insert(NodeType::FileInput, file_input::exec);
}
