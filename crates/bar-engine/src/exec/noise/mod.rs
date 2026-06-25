//! Executors for the noise + value generators.

use std::collections::HashMap;

use bar_compute::NoiseType;
use bar_graph::{EvalError, NodeType, PortValue};

use super::{ExecCtx, ExecFn};
use crate::exec::shared::{get_optional_heightmap, scale_by_field};
use shared::generate_noise;

pub mod constant;
pub mod gradient;
pub mod perlin;
pub mod ridged;
pub mod shared;
pub mod simplex;
pub mod voronoi;
pub mod worley;

/// Shared body for the FBM generators (Perlin/Simplex/Worley/Ridged): generate
/// the fractal noise, then scale by the optional `control` field.
pub(crate) fn run_noise(
    noise_type: NoiseType,
    ctx: &ExecCtx,
) -> Result<HashMap<String, PortValue>, EvalError> {
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let hm = generate_noise(noise_type, ctx.params, ctx.hm_w, ctx.hm_h)?;
    let hm = scale_by_field(hm, ctrl.as_ref());
    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

pub fn register(m: &mut HashMap<NodeType, ExecFn>) {
    m.insert(NodeType::PerlinNoise, perlin::exec);
    m.insert(NodeType::SimplexNoise, simplex::exec);
    m.insert(NodeType::WorleyNoise, worley::exec);
    m.insert(NodeType::RidgedNoise, ridged::exec);
    m.insert(NodeType::Constant, constant::exec);
    m.insert(NodeType::Voronoi, voronoi::exec);
    m.insert(NodeType::Gradient, gradient::exec);
}
