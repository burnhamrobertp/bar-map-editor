//! # bar-compute
//!
//! GPU compute shaders and CPU fallback implementations for terrain generation.
//! Provides noise generation, erosion simulation, and texture operations.

pub mod coast;
pub mod device;
pub mod erosion;
pub mod gpu_erosion;
pub mod gpu_filters;
pub mod gpu_lightmap;
pub mod gpu_noise;
pub mod lightmap;
pub mod noise;

pub use coast::{coast_erosion, CoastErosionParams};
pub use device::{ComputeDevice, ComputeError, GpuContext};
pub use erosion::{
    hydraulic_erosion, thermal_erosion, FlowErosionParams, HydraulicErosionMaps,
    HydraulicErosionParams, ThermalErosionParams,
};
pub use gpu_erosion::{GpuErosionError, GpuErosionPipeline};
pub use gpu_filters::{GpuFilterError, GpuFilterPipeline};
pub use gpu_lightmap::{GpuLightmapError, GpuLightmapPipeline};
pub use gpu_noise::GpuNoisePipeline;
pub use lightmap::{bake_lightmap_cpu, LightmapParams};
pub use noise::{generate_noise_cpu, NoiseParams, NoiseType};
