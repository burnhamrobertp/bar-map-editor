//! Noise kernels shared within the family.

use std::collections::HashMap;

use bar_compute::{generate_noise_cpu, NoiseParams, NoiseType};
use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue};

use crate::exec::shared::{get_float, get_uint};

/// Build `NoiseParams` from node params. The single source of truth shared by
/// the CPU generator here and the GPU path in `hybrid_executor`, so the editor
/// and the CLI export read every param (esp. the UInt seed/octaves) identically.
pub(crate) fn build_noise_params(
    noise_type: NoiseType,
    params: &HashMap<String, ParamValue>,
    width: u32,
    height: u32,
) -> NoiseParams {
    NoiseParams {
        width,
        height,
        noise_type,
        octaves: get_uint(params, "octaves", 6),
        lacunarity: get_float(params, "lacunarity", 2.0),
        persistence: get_float(params, "persistence", 0.5),
        frequency: get_float(params, "frequency", 4.0),
        seed: get_uint(params, "seed", 0),
        offset_x: get_float(params, "offset_x", 0.0),
        offset_y: get_float(params, "offset_y", 0.0),
        steepness: get_float(params, "steepness", 0.5),
        elevation: get_float(params, "elevation", 0.5),
        offset: get_float(params, "offset", 0.0),
        gain: get_float(params, "gain", 0.5),
    }
}

pub(crate) fn generate_noise(
    noise_type: NoiseType,
    params: &HashMap<String, ParamValue>,
    width: u32,
    height: u32,
) -> Result<Heightmap, EvalError> {
    let noise_params = build_noise_params(noise_type, params, width, height);

    generate_noise_cpu(&noise_params).map_err(|e| EvalError::Compute(e.to_string()))
}
