//! # bar-render
//!
//! 3D terrain viewport rendering with wgpu.
//! Handles terrain mesh generation, camera controls, and display modes.

pub mod camera;
pub mod feature_lights;
pub mod features;
pub mod picking;
pub mod renderer;
pub mod shadow;
pub mod terrain;

pub use camera::Camera;
pub use feature_lights::{lights_for_feature_def, FeatureLightConfig};
pub use features::{FeatureInstance, FeatureRenderer, FeatureTexture};
pub use picking::{
    camera_ray, pick_feature, pick_terrain, terrain_y_at_world_xz, PickResult, PickableFeature,
};
pub use renderer::{PreviewFrame, SmfLighting, TerrainRenderer, TerrainUpdateParams};
pub use terrain::TerrainVertex;
