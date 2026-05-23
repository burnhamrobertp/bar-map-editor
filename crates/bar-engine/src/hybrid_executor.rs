//! HybridExecutor: uses GPU for noise, erosion, and blur when available,
//! falls back to CPU for everything else.

use std::collections::HashMap;

use bar_compute::{
    FlowErosionParams, GpuContext, GpuErosionPipeline, GpuFilterPipeline, GpuNoisePipeline,
    NoiseParams, NoiseType, ThermalErosionParams,
};
use bar_data::Heightmap;
use bar_graph::{EvalError, NodeExecutor, NodeType, ParamValue, PortValue};

use crate::executor::CpuExecutor;

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
    cpu_fallback: CpuExecutor,
}

impl HybridExecutor {
    /// Create a new HybridExecutor with a shared GPU context.
    pub fn new(gpu_context: GpuContext) -> Self {
        let noise_pipeline = GpuNoisePipeline::new(&gpu_context.device);
        let erosion_pipeline = GpuErosionPipeline::new(&gpu_context.device);
        let filter_pipeline = GpuFilterPipeline::new(&gpu_context.device);
        Self {
            gpu_context,
            noise_pipeline,
            erosion_pipeline,
            filter_pipeline,
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
                NodeType::HydraulicErosion => {
                    return self.execute_gpu_hydraulic_erosion(params, inputs);
                }
                NodeType::ThermalErosion => {
                    return self.execute_gpu_thermal_erosion(params, inputs);
                }
                NodeType::Blur => {
                    return self.execute_gpu_blur(params, inputs);
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
        let frequency = get_float(params, "frequency", 2.0);
        let octaves = get_float(params, "octaves", 6.0) as u32;
        let persistence = get_float(params, "persistence", 0.5);
        let lacunarity = get_float(params, "lacunarity", 2.0);
        let seed = get_float(params, "seed", 0.0) as u32;

        let noise_params = NoiseParams {
            width,
            height,
            noise_type,
            frequency,
            octaves,
            persistence,
            lacunarity,
            seed,
            offset_x: 0.0,
            offset_y: 0.0,
        };

        let heightmap = self
            .noise_pipeline
            .generate(&self.gpu_context, &noise_params, noise_type)
            .map_err(|e| EvalError::Compute(format!("GPU noise failed: {e}")))?;

        let mut outputs = HashMap::new();
        outputs.insert("output".to_string(), PortValue::Heightmap(heightmap));
        Ok(outputs)
    }

    /// Execute hydraulic erosion on the GPU using the virtual-pipe flow model.
    fn execute_gpu_hydraulic_erosion(
        &self,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let input = get_input_heightmap(inputs)?;

        // Map node params to flow sim params. The node exposes the keys most
        // relevant to visible output; physical constants use sensible defaults.
        let iterations = get_uint(params, "iterations", 50_000);
        let flow_params = FlowErosionParams {
            // Scale droplet count → flow steps (different order of magnitude)
            iterations: (iterations / 1_000).clamp(5, 200),
            rain_rate: 0.012,
            evaporation_rate: get_float(params, "evaporation_rate", 0.015),
            sediment_capacity: get_float(params, "capacity_factor", 1.0),
            erosion_rate: get_float(params, "erosion_rate", 0.3),
            deposition_rate: get_float(params, "deposition_rate", 0.3),
            min_tilt: 0.01,
            gravity: 9.8,
            dt: 0.02,
            pipe_length: 1.0,
        };

        let result = self
            .erosion_pipeline
            .hydraulic_flow_erode(&self.gpu_context, &input, &flow_params)
            .map_err(|e| EvalError::Compute(format!("GPU hydraulic erosion failed: {e}")))?;

        let mut outputs = HashMap::new();
        outputs.insert("output".to_string(), PortValue::Heightmap(result));
        Ok(outputs)
    }

    /// Execute thermal erosion on the GPU.
    fn execute_gpu_thermal_erosion(
        &self,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let input = get_input_heightmap(inputs)?;
        let erosion_params = ThermalErosionParams {
            iterations: get_uint(params, "iterations", 50),
            talus_angle: get_float(params, "talus_angle", 0.004),
            erosion_rate: get_float(params, "erosion_rate", 0.5),
        };

        let result = self
            .erosion_pipeline
            .thermal_erode(&self.gpu_context, &input, &erosion_params)
            .map_err(|e| EvalError::Compute(format!("GPU thermal erosion failed: {e}")))?;

        let mut outputs = HashMap::new();
        outputs.insert("output".to_string(), PortValue::Heightmap(result));
        Ok(outputs)
    }

    /// Execute box blur on the GPU.
    fn execute_gpu_blur(
        &self,
        params: &HashMap<String, ParamValue>,
        inputs: &HashMap<String, PortValue>,
    ) -> Result<HashMap<String, PortValue>, EvalError> {
        let input = get_input_heightmap(inputs)?;
        let radius = get_float(params, "radius", 2.0).round() as u32;

        let result = self
            .filter_pipeline
            .box_blur(&self.gpu_context, &input, radius)
            .map_err(|e| EvalError::Compute(format!("GPU blur failed: {e}")))?;

        let mut outputs = HashMap::new();
        outputs.insert("output".to_string(), PortValue::Heightmap(result));
        Ok(outputs)
    }
}

fn get_float(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    params
        .get(key)
        .and_then(|v| match v {
            ParamValue::Float(f) => Some(*f),
            _ => None,
        })
        .unwrap_or(default)
}

fn get_uint(params: &HashMap<String, ParamValue>, key: &str, default: u32) -> u32 {
    params
        .get(key)
        .and_then(|v| match v {
            ParamValue::UInt(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(default)
}

fn get_input_heightmap(inputs: &HashMap<String, PortValue>) -> Result<Heightmap, EvalError> {
    inputs
        .get("input")
        .and_then(|v| match v {
            PortValue::Heightmap(h) => Some(h.clone()),
            _ => None,
        })
        .ok_or_else(|| EvalError::Compute("missing 'input' heightmap".to_string()))
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
