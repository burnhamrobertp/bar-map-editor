//! # bar-render
//!
//! 3D terrain viewport rendering with wgpu.
//! Handles terrain mesh generation, camera controls, and display modes.

pub mod camera;
pub mod color;
pub mod feature_lights;
pub mod features;
pub mod picking;
pub mod renderer;
pub(crate) mod samplers;
pub mod shadow;
pub mod terrain;
pub mod thumbnail;
pub mod widgets;

pub use camera::Camera;
pub use feature_lights::{lights_for_feature_def, FeatureLightConfig};
pub use features::{FeatureInstance, FeatureRenderer, FeatureTexture};
pub use picking::{
    camera_ray, pick_feature, pick_terrain, ray_terrain_occludes, terrain_y_at_world_xz,
    PickResult, PickableFeature,
};
pub use renderer::{PreviewFrame, SmfLighting, TerrainRenderer, TerrainUpdateParams};
pub use terrain::TerrainVertex;
pub use thumbnail::FeatureThumbnailRenderer;
