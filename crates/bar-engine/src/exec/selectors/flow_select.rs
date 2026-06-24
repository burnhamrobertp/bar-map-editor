use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_float, get_input_heightmap};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let threshold = get_float(ctx.params, "threshold", 0.2);
    let falloff = get_float(ctx.params, "falloff", 0.15).max(1e-6);
    let hm = apply_flow_select(&input, threshold, falloff);

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Threshold selector for flow/wear/deposit maps.
/// Ramps from 0 at (threshold - falloff) to 1 at threshold.
pub(crate) fn apply_flow_select(input: &Heightmap, threshold: f32, falloff: f32) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let lo = threshold - falloff;
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| ((v - lo) / falloff).clamp(0.0, 1.0))
        .collect();
    Heightmap::frbar_data(w, h, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    #[test]
    fn flow_select_thresholds_correctly() {
        let executor = crate::CpuExecutor;
        // Uniform gradient 0..1 across 8 pixels.
        let data: Vec<f32> = (0..8).map(|i| i as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 1, data).unwrap();
        let params = HashMap::from([
            ("threshold".to_string(), ParamValue::Float(0.5)),
            ("falloff".to_string(), ParamValue::Float(0.25)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::FlowSelect, &params, &inputs, 8, 1, 8, 1)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // v=0 (well below threshold-falloff=0.25) should produce 0.
        assert!(
            out.get(0, 0).unwrap() < 0.01,
            "pixel 0 should be ~0, got {}",
            out.get(0, 0).unwrap()
        );
        // v=1 (above threshold) should produce 1.
        assert!(
            out.get(7, 0).unwrap() > 0.99,
            "pixel 7 should be ~1, got {}",
            out.get(7, 0).unwrap()
        );
    }
}
