use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::combiners::shared::combine_mode_heightmaps;
use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_string,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let a = get_input_heightmap(ctx.inputs, "a")?;
    let b = get_input_heightmap(ctx.inputs, "b")?;
    let factor = get_float(ctx.params, "factor", 0.5).clamp(0.0, 1.0);
    let mode = get_string(ctx.params, "mode", "blend");
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let combined = combine_mode_heightmaps(&a, &b, mode, factor);
    let hm = apply_modulation(&a, combined, ctrl.as_ref(), mask.as_ref());
    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_data::Heightmap;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    fn const_hm(w: u32, h: u32, v: f32) -> Heightmap {
        Heightmap::frbar_data(w, h, vec![v; (w as usize) * (h as usize)]).unwrap()
    }

    /// Run a node and return its `output` heightmap.
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

    fn ab_inputs(av: f32, bv: f32) -> HashMap<String, PortValue> {
        HashMap::from([
            ("a".to_string(), PortValue::Heightmap(const_hm(2, 2, av))),
            ("b".to_string(), PortValue::Heightmap(const_hm(2, 2, bv))),
        ])
    }

    #[test]
    fn test_blend_combiner() {
        let executor = crate::CpuExecutor;
        let a = Heightmap::frbar_data(4, 4, vec![0.0; 16]).unwrap();
        let b = Heightmap::frbar_data(4, 4, vec![1.0; 16]).unwrap();

        let inputs = HashMap::from([
            ("a".to_string(), PortValue::Heightmap(a)),
            ("b".to_string(), PortValue::Heightmap(b)),
        ]);
        let params = HashMap::from([("factor".to_string(), ParamValue::Float(0.5))]);

        let result = executor
            .execute(&NodeType::Blend, &params, &inputs, 4, 4, 4, 4)
            .unwrap();
        let output = result.get("output").unwrap();
        match output {
            PortValue::Heightmap(hm) => {
                assert!((hm.get(0, 0).unwrap() - 0.5).abs() < 0.01);
            }
            _ => panic!("Expected heightmap"),
        }
    }

    #[test]
    fn combine_modes_compute_expected_ops() {
        // factor=1.0 applies the op fully: result == op(a,b).
        let f = |mode: &str, a: f32, b: f32| {
            run_node(
                NodeType::Blend,
                &[
                    ("mode", ParamValue::String(mode.into())),
                    ("factor", ParamValue::Float(1.0)),
                ],
                ab_inputs(a, b),
                2,
                2,
            )
            .get(0, 0)
            .unwrap()
        };
        assert!((f("add", 0.3, 0.4) - 0.7).abs() < 1e-5);
        assert!((f("subtract", 0.7, 0.2) - 0.5).abs() < 1e-5);
        assert!((f("multiply", 0.5, 0.4) - 0.2).abs() < 1e-5);
        assert!((f("divide", 0.2, 0.5) - 0.4).abs() < 1e-5);
        assert!((f("average", 0.2, 0.8) - 0.5).abs() < 1e-5);
        assert!((f("screen", 0.5, 0.5) - 0.75).abs() < 1e-5);
        assert!((f("difference", 0.2, 0.7) - 0.5).abs() < 1e-5);
        assert!((f("max", 0.2, 0.8) - 0.8).abs() < 1e-5);
        assert!((f("min", 0.2, 0.8) - 0.2).abs() < 1e-5);
    }

    #[test]
    fn combine_blend_default_matches_old_lerp() {
        // The default mode preserves the historical blend_heightmaps behaviour.
        let v = run_node(
            NodeType::Blend,
            &[
                ("mode", ParamValue::String("blend".into())),
                ("factor", ParamValue::Float(0.5)),
            ],
            ab_inputs(0.2, 0.8),
            2,
            2,
        )
        .get(0, 0)
        .unwrap();
        assert!((v - 0.5).abs() < 1e-5);
    }

    #[test]
    fn combine_factor_is_wm_strength() {
        // factor (Strength) lerps from A toward op(a,b): 0 => A passthrough.
        let at_zero = run_node(
            NodeType::Blend,
            &[
                ("mode", ParamValue::String("add".into())),
                ("factor", ParamValue::Float(0.0)),
            ],
            ab_inputs(0.2, 0.8),
            2,
            2,
        )
        .get(0, 0)
        .unwrap();
        assert!((at_zero - 0.2).abs() < 1e-5);
        // factor 0.5: lerp(0.2, (0.2+0.8).min(1)=1.0, 0.5) = 0.6.
        let at_half = run_node(
            NodeType::Blend,
            &[
                ("mode", ParamValue::String("add".into())),
                ("factor", ParamValue::Float(0.5)),
            ],
            ab_inputs(0.2, 0.8),
            2,
            2,
        )
        .get(0, 0)
        .unwrap();
        assert!((at_half - 0.6).abs() < 1e-5);
    }
}
