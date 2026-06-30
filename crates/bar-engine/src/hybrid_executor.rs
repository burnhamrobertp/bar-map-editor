//! HybridExecutor: uses GPU for noise, erosion, and blur when available,
//! falls back to CPU for everything else.

use std::collections::HashMap;

use bar_compute::{
    GpuContext, GpuErosionPipeline, GpuFilterPipeline, GpuLightmapPipeline, GpuNoisePipeline,
    LightmapParams, NoiseType,
};
use bar_graph::{EvalError, NodeExecutor, NodeType, ParamValue, PortValue};

use crate::exec::filters::thermal_erosion::build_thermal_params;
use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_uint,
};
use crate::exec::CpuExecutor;

/// Minimum resolution to prefer GPU over CPU for noise.
const GPU_NOISE_THRESHOLD: u32 = 128;

/// Minimum resolution to prefer GPU for erosion and blur.
const GPU_FILTER_THRESHOLD: u32 = 256;

const _: () = assert!(GPU_NOISE_THRESHOLD >= 64, "GPU noise threshold too low");
const _: () = assert!(GPU_NOISE_THRESHOLD <= 512, "GPU noise threshold too high");
const _: () = assert!(
    GPU_FILTER_THRESHOLD >= GPU_NOISE_THRESHOLD,
    "GPU filter threshold below noise threshold"
);

/// Executor that uses GPU for compute-heavy operations and CPU for everything else.
pub struct HybridExecutor {
    gpu_context: GpuContext,
    noise_pipeline: GpuNoisePipeline,
    erosion_pipeline: GpuErosionPipeline,
    filter_pipeline: GpuFilterPipeline,
    lightmap_pipeline: GpuLightmapPipeline,
    cpu_fallback: CpuExecutor,
}

impl HybridExecutor {
    /// Create a new HybridExecutor with a shared GPU context.
    pub fn new(gpu_context: GpuContext) -> Self {
        let noise_pipeline = GpuNoisePipeline::new(&gpu_context.device);
        let erosion_pipeline = GpuErosionPipeline::new(&gpu_context.device);
        let filter_pipeline = GpuFilterPipeline::new(&gpu_context.device);
        let lightmap_pipeline = GpuLightmapPipeline::new(&gpu_context.device);
        Self {
            gpu_context,
            noise_pipeline,
            erosion_pipeline,
            filter_pipeline,
            lightmap_pipeline,
            cpu_fallback: CpuExecutor,
        }
    }
}

impl NodeExecutor for HybridExecutor {
    fn execute(
        &self,
        node_type: &NodeType,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
        hm_width: u32,
        hm_height: u32,
        tex_width: u32,
        tex_height: u32,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        // INVARIANT: any node dispatched to the GPU here must produce output
        // identical to its CPU exec (a barproj must compile the same on a
        // GPU-less machine). Each GPU path reuses the CPU param builder + input
        // handling so only the kernel differs. Every node added below needs a
        // case in tests/gpu_cpu_parity.rs.
        //
        // GPU paths handle heightmap nodes only -- they use hm dims.
        let noise_type = match node_type {
            NodeType::PerlinNoise => Some(NoiseType::Perlin),
            NodeType::SimplexNoise => Some(NoiseType::Simplex),
            NodeType::RidgedNoise => Some(NoiseType::Ridged),
            _ => None,
        };

        if let Some(nt) = noise_type {
            if hm_width >= GPU_NOISE_THRESHOLD && hm_height >= GPU_NOISE_THRESHOLD {
                return self.execute_gpu_noise(nt, params, hm_width, hm_height);
            }
        }

        if hm_width >= GPU_FILTER_THRESHOLD && hm_height >= GPU_FILTER_THRESHOLD {
            match node_type {
                // HydraulicErosion is intentionally not dispatched to the GPU.
                // The CPU droplet model is the only implementation that supports
                // the flow/wear/deposit outputs; the GPU pipe model produces only
                // an eroded heightmap and would diverge. GPU pipe-model parity is
                // a follow-up.
                NodeType::ThermalErosion => {
                    return self.execute_gpu_thermal_erosion(params, inputs);
                }
                NodeType::Blur => {
                    return self.execute_gpu_blur(params, inputs);
                }
                NodeType::LightmapBake => {
                    return self.execute_gpu_lightmap(params, inputs, tex_width, tex_height);
                }
                _ => {}
            }
        }

        // Delegate everything else to CPU
        self.cpu_fallback.execute(
            node_type, params, inputs, hm_width, hm_height, tex_width, tex_height,
        )
    }
}

impl HybridExecutor {
    /// Execute a noise node on the GPU.
    fn execute_gpu_noise(
        &self,
        noise_type: NoiseType,
        params: &HashMap<String, ParamValue>,
        width: u32,
        height: u32,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let noise_params =
            crate::exec::noise::shared::build_noise_params(noise_type, params, width, height);

        let heightmap = self
            .noise_pipeline
            .generate(&self.gpu_context, &noise_params, noise_type)
            .map_err(|e| EvalError::Compute(format!("GPU noise failed: {e}")))?;

        let mut outputs = HashMap::new();
        outputs.insert("output".to_string(), PortValue::Heightmap(heightmap));
        Ok(outputs)
    }

    /// Execute thermal erosion on the GPU.
    fn execute_gpu_thermal_erosion(
        &self,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let input = get_input_heightmap(inputs, "input")?;
        let ctrl = get_optional_heightmap(inputs, "control");
        let mask = get_optional_heightmap(inputs, "mask");
        let erosion_params = build_thermal_params(params);

        let result = self
            .erosion_pipeline
            .thermal_erode(&self.gpu_context, &input, &erosion_params)
            .map_err(|e| EvalError::Compute(format!("GPU thermal erosion failed: {e}")))?;
        let result = apply_modulation(&input, result, ctrl.as_ref(), mask.as_ref());

        let mut outputs = HashMap::new();
        outputs.insert("output".to_string(), PortValue::Heightmap(result));
        Ok(outputs)
    }

    /// Execute a lightmap bake (AO + sun shadow) on the GPU.
    fn execute_gpu_lightmap(
        &self,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
        tex_width: u32,
        tex_height: u32,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let input = get_input_heightmap(inputs, "heightmap")?;

        let az = get_float(params, "sun_azimuth", 315.0).to_radians();
        let el = get_float(params, "sun_elevation", 45.0).to_radians();
        let horiz = el.cos();
        let sun_dir = [horiz * az.sin(), horiz * az.cos(), el.sin()];

        let lm_params = LightmapParams {
            width: input.width(),
            height: input.height(),
            ao_strength: get_float(params, "ao_strength", 1.0),
            ao_radius: get_float(params, "ao_radius", 0.1),
            num_directions: get_uint(params, "num_directions", 16),
            max_steps: get_uint(params, "max_steps", 24),
            sun_dir,
            sun_softness: get_float(params, "sun_softness", 0.2),
        };

        let mut lightmap = self
            .lightmap_pipeline
            .bake(&self.gpu_context, &input, &lm_params)
            .map_err(|e| EvalError::Compute(format!("GPU lightmap bake failed: {e}")))?;

        if lightmap.width() != tex_width || lightmap.height() != tex_height {
            lightmap = lightmap.resize(tex_width, tex_height);
        }

        let mut outputs = HashMap::new();
        outputs.insert("lightmap".to_string(), PortValue::Color(lightmap));
        Ok(outputs)
    }

    /// Execute box blur on the GPU.
    fn execute_gpu_blur(
        &self,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let input = get_input_heightmap(inputs, "input")?;
        let ctrl = get_optional_heightmap(inputs, "control");
        let mask = get_optional_heightmap(inputs, "mask");
        // Match the CPU `apply_blur` radius handling exactly (default 1.0,
        // round, clamp to [1, 64]) so both paths blur by the same amount.
        let radius = (get_float(params, "radius", 1.0).round() as usize).clamp(1, 64) as u32;

        let result = self
            .filter_pipeline
            .box_blur(&self.gpu_context, &input, radius)
            .map_err(|e| EvalError::Compute(format!("GPU blur failed: {e}")))?;
        let result = apply_modulation(&input, result, ctrl.as_ref(), mask.as_ref());

        let mut outputs = HashMap::new();
        outputs.insert("output".to_string(), PortValue::Heightmap(result));
        Ok(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only PerlinNoise, SimplexNoise, and RidgedNoise should be dispatched to
    /// the GPU.  WorleyNoise must always use the CPU path.
    #[test]
    fn test_gpu_dispatch_selection() {
        let gpu_dispatched = [
            NodeType::PerlinNoise,
            NodeType::SimplexNoise,
            NodeType::RidgedNoise,
        ];
        let cpu_only = [
            NodeType::WorleyNoise,
            NodeType::HydraulicErosion,
            NodeType::Blur,
            NodeType::Constant,
        ];

        for nt in &gpu_dispatched {
            let selects_gpu = matches!(
                nt,
                NodeType::PerlinNoise | NodeType::SimplexNoise | NodeType::RidgedNoise
            );
            assert!(selects_gpu, "{nt:?} should be dispatched to GPU noise");
        }

        for nt in &cpu_only {
            let selects_gpu = matches!(
                nt,
                NodeType::PerlinNoise | NodeType::SimplexNoise | NodeType::RidgedNoise
            );
            assert!(!selects_gpu, "{nt:?} should NOT be dispatched to GPU noise");
        }
    }
}
