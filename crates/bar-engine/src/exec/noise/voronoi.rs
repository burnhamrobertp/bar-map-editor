use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{
    get_float, get_optional_heightmap, get_string, get_uint, scale_by_field,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let hm = generate_voronoi(ctx.params, ctx.hm_w, ctx.hm_h);
    let hm = scale_by_field(hm, ctrl.as_ref());
    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Generate Voronoi (plateau/cell) terrain.
/// Params: frequency, seed, mode ("f1", "f2", "f2_f1", "cell")
pub(crate) fn generate_voronoi(
    params: &HashMap<String, ParamValue>,
    width: u32,
    height: u32,
) -> Heightmap {
    let frequency = get_float(params, "frequency", 8.0);
    let seed = get_uint(params, "seed", 0);
    let mode = get_string(params, "mode", "f1");

    // Simple Voronoi via random cell points
    let num_cells = (frequency * frequency) as usize;
    let mut rng_state: u64 = seed as u64 ^ 0xDEAD_BEEF;

    let mut cell_points: Vec<(f32, f32, f32)> = Vec::with_capacity(num_cells);
    for _ in 0..num_cells {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let cx = ((rng_state >> 32) as f32) / (u32::MAX as f32);
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let cy = ((rng_state >> 32) as f32) / (u32::MAX as f32);
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let cval = ((rng_state >> 32) as f32) / (u32::MAX as f32);
        cell_points.push((cx, cy, cval));
    }

    let mut data = vec![0.0f32; (width as usize) * (height as usize)];
    for y in 0..height {
        for x in 0..width {
            let px = x as f32 / width as f32;
            let py = y as f32 / height as f32;

            let mut d1 = f32::MAX;
            let mut d2 = f32::MAX;
            let mut closest_val = 0.0f32;

            for &(cx, cy, cval) in &cell_points {
                let dx = px - cx;
                let dy = py - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < d1 {
                    d2 = d1;
                    d1 = dist;
                    closest_val = cval;
                } else if dist < d2 {
                    d2 = dist;
                }
            }

            let v = match mode {
                "f2" => (d2 * frequency).min(1.0),
                "f2_f1" => ((d2 - d1) * frequency).min(1.0),
                "cell" => closest_val,
                _ => (d1 * frequency).min(1.0), // "f1"
            };

            data[(y as usize) * (width as usize) + (x as usize)] = v.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(width, height, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::NodeExecutor;
    use bar_graph::NodeType;

    #[test]
    fn test_voronoi_generator() {
        let executor = crate::CpuExecutor;
        let params = HashMap::from([
            ("frequency".to_string(), ParamValue::Float(4.0)),
            ("seed".to_string(), ParamValue::UInt(42)),
            ("mode".to_string(), ParamValue::String("f1".to_string())),
        ]);
        let result = executor
            .execute(&NodeType::Voronoi, &params, &HashMap::new(), 64, 64, 64, 64)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                assert_eq!(hm.width(), 64);
                assert_eq!(hm.height(), 64);
                // Should have variation
                let min = hm.data().iter().cloned().fold(f32::INFINITY, f32::min);
                let max = hm.data().iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                assert!(max - min > 0.1, "Voronoi should have variation");
            }
            _ => panic!("Expected heightmap"),
        }
    }
}
