use std::collections::HashMap;

use bar_compute::{hydraulic_erosion, HydraulicErosionParams};
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_string, get_uint,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hardness = get_optional_heightmap(ctx.inputs, "hardness");

    // Only "droplet" is implemented; the param exists for forward-compat with a
    // future "pipe" model. Read it so the descriptor and executor stay paired.
    let _method = get_string(ctx.params, "method", "droplet");

    let params_e = HydraulicErosionParams {
        num_droplets: get_uint(ctx.params, "iterations", 50_000),
        inertia: get_float(ctx.params, "inertia", 0.05),
        capacity_factor: get_float(ctx.params, "capacity_factor", 4.0),
        min_capacity: get_float(ctx.params, "min_capacity", 0.01),
        deposition_rate: get_float(ctx.params, "deposition_rate", 0.3),
        erosion_rate: get_float(ctx.params, "erosion_rate", 0.3),
        evaporation_rate: get_float(ctx.params, "evaporation_rate", 0.01),
        gravity: get_float(ctx.params, "gravity", 4.0),
        max_lifetime: get_uint(ctx.params, "max_lifetime", 30),
        erosion_radius: get_uint(ctx.params, "erosion_radius", 3),
        seed: get_uint(ctx.params, "seed", 0),
        river_depth: get_float(ctx.params, "river_depth", 0.0),
    };
    let result = hydraulic_erosion(&input, &params_e, hardness.as_ref())
        .map_err(|e| EvalError::Compute(e.to_string()))?;
    let hm = apply_modulation(&input, result.heightmap, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([
        ("output".to_string(), PortValue::Heightmap(hm)),
        ("flow".to_string(), PortValue::Heightmap(result.flow)),
        ("wear".to_string(), PortValue::Heightmap(result.wear)),
        ("deposit".to_string(), PortValue::Heightmap(result.deposit)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_data::Heightmap;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    fn run_node(
        nt: NodeType,
        params: &[(&str, ParamValue)],
        inputs: HashMap<String, PortValue>,
        w: u32,
        h: u32,
    ) -> Heightmap {
        let p: HashMap<String, ParamValue> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        let out = crate::CpuExecutor
            .execute(&nt, &p, &inputs, w, h, w, h)
            .unwrap();
        match out.get("output").unwrap() {
            PortValue::Heightmap(hm) => hm.clone(),
            _ => panic!("expected heightmap output"),
        }
    }

    #[test]
    fn erosion_reads_new_params_and_seed_changes_result() {
        let mut d = vec![0.0f32; 64];
        for y in 0..8 {
            for x in 0..8 {
                d[y * 8 + x] = (((x * 3 + y * 7) % 5) as f32) / 5.0;
            }
        }
        let hm = Heightmap::frbar_data(8, 8, d).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let with_seed = |s: u32| {
            run_node(
                NodeType::HydraulicErosion,
                &[
                    ("iterations", ParamValue::UInt(2000)),
                    ("capacity_factor", ParamValue::Float(6.0)),
                    ("inertia", ParamValue::Float(0.1)),
                    ("gravity", ParamValue::Float(6.0)),
                    ("erosion_radius", ParamValue::UInt(2)),
                    ("max_lifetime", ParamValue::UInt(20)),
                    ("seed", ParamValue::UInt(s)),
                ],
                inputs.clone(),
                8,
                8,
            )
        };
        let out1 = with_seed(1);
        let out2 = with_seed(999);
        let diff: f32 = out1
            .data()
            .iter()
            .zip(out2.data())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(
            diff > 0.0,
            "the surfaced `seed` param must affect erosion output"
        );
    }

    #[test]
    fn hydraulic_erosion_emits_four_output_ports() {
        let executor = crate::CpuExecutor;
        let data: Vec<f32> = (0..64 * 64)
            .map(|i| {
                let x = (i % 64) as f32 / 63.0;
                let y = (i / 64) as f32 / 63.0;
                ((x - 0.5) * (x - 0.5) + (y - 0.5) * (y - 0.5)).sqrt()
            })
            .collect();
        let hm = Heightmap::frbar_data(64, 64, data).unwrap();
        let params = HashMap::from([("iterations".to_string(), ParamValue::UInt(2000))]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(
                &NodeType::HydraulicErosion,
                &params,
                &inputs,
                64,
                64,
                64,
                64,
            )
            .unwrap();
        for port in ["output", "flow", "wear", "deposit"] {
            let val = result.get(port).expect(port);
            let PortValue::Heightmap(out) = val else {
                panic!("{port} should be Heightmap");
            };
            for &v in out.data() {
                assert!((0.0..=1.0).contains(&v), "{port} value {v} out of range");
            }
        }
    }
}
