//! # bar-render
//!
//! 3D terrain viewport rendering with wgpu.
//! Handles terrain mesh generation, camera controls, and display modes.

pub mod camera;
pub mod features;
pub mod picking;
pub mod renderer;
pub mod terrain;

pub use camera::Camera;
pub use features::{FeatureInstance, FeatureRenderer};
pub use picking::{pick_terrain, PickResult};
pub use renderer::{PreviewFrame, SmfLighting, TerrainRenderer, TerrainUpdateParams};
pub use terrain::TerrainVertex;
