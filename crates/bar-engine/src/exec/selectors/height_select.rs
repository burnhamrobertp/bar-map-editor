use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::selectors::shared::compute_height_select;
use crate::exec::shared::{
    apply_invert, get_bool, get_float, get_input_heightmap, get_optional_heightmap, get_string,
    scale_by_field,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let low = get_float(ctx.params, "low", 0.3);
    let high = get_float(ctx.params, "high", 0.7);
    let falloff = get_float(ctx.params, "falloff", 0.1);
    let smooth = get_string(ctx.params, "falloff_type", "linear") == "smooth";
    let invert = get_bool(ctx.params, "invert", false);
    let mut hm = compute_height_select(&input, low, high, falloff, smooth);
    if invert {
        hm = apply_invert(&hm);
    }
    let hm = scale_by_field(hm, ctrl.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

#[cfg(test)]
mod tests {
    use bar_data::Heightmap;
    use bar_graph::{NodeExecutor, NodeType, ParamValue, PortValue};
    use std::collections::HashMap;

    fn const_hm(w: u32, h: u32, v: f32) -> Heightmap {
        Heightmap::frbar_data(w, h, vec![v; (w as usize) * (h as usize)]).unwrap()
    }

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
    fn height_select_invert_flips_selection() {
        let inputs = HashMap::from([(
            "input".to_string(),
            PortValue::Heightmap(const_hm(2, 2, 0.5)),
        )]);
        let base = &[
            ("low", ParamValue::Float(0.4)),
            ("high", ParamValue::Float(0.6)),
            ("falloff", ParamValue::Float(0.1)),
        ][..];
        let sel = run_node(NodeType::HeightSelect, base, inputs.clone(), 2, 2);
        assert!((sel.get(0, 0).unwrap() - 1.0).abs() < 1e-5); // 0.5 is in band
        let mut inv = base.to_vec();
        inv.push(("invert", ParamValue::Bool(true)));
        let sel_i = run_node(NodeType::HeightSelect, &inv, inputs, 2, 2);
        assert!((sel_i.get(0, 0).unwrap() - 0.0).abs() < 1e-5);
    }

    #[test]
    fn height_select_smooth_falloff_differs_from_linear() {
        // Value 0.85, band [0,0.5], falloff 0.5 -> dist 0.35, ramp t=0.3.
        // linear -> 0.3; smooth (smoothstep) -> 0.3^2*(3-0.6) = 0.216.
        let inputs = HashMap::from([(
            "input".to_string(),
            PortValue::Heightmap(const_hm(2, 2, 0.85)),
        )]);
        let common = [
            ("low", ParamValue::Float(0.0)),
            ("high", ParamValue::Float(0.5)),
            ("falloff", ParamValue::Float(0.5)),
        ];
        let lin = run_node(NodeType::HeightSelect, &common, inputs.clone(), 2, 2)
            .get(0, 0)
            .unwrap();
        let mut sm = common.to_vec();
        sm.push(("falloff_type", ParamValue::String("smooth".into())));
        let smooth = run_node(NodeType::HeightSelect, &sm, inputs, 2, 2)
            .get(0, 0)
            .unwrap();
        assert!(
            (lin - 0.3).abs() < 1e-4,
            "linear ramp expected 0.3, got {lin}"
        );
        assert!(
            (smooth - 0.216).abs() < 1e-3,
            "smooth ramp expected 0.216, got {smooth}"
        );
    }
}
