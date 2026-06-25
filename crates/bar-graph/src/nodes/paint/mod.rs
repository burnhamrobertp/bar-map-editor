//! Paint + import source nodes: hand-painted heightmaps and textures, imported
//! SMT textures, and external file inputs. All are source nodes (no required
//! input); the painted / imported variants carry managed binary assets.

pub mod file_input;
pub mod imported_texture;
pub mod painted_heightmap;
pub mod painted_texture;

use crate::nodes::def::NodeDef;

pub static NODES: &[&NodeDef] = &[
    &painted_heightmap::DEF,
    &painted_texture::DEF,
    &imported_texture::DEF,
    &file_input::DEF,
];
