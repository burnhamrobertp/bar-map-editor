use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{
    apply_invert,
    get_bool,
    get_float,
    get_input_heightmap,
    get_optional_heightmap,
    get_string,
    scale_by_field,
};
use crate::exec::selectors::shared::{compute_height_select, compute_slope_map};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let min_deg = get_float(ctx.params, "min_slope", 0.0);
    let max_deg = get_float(ctx.params, "max_slope", 30.0);
    let falloff_deg = get_float(ctx.params, "falloff", 10.0);
    let smooth = get_string(ctx.params, "falloff_type", "linear") == "smooth";
    let invert = get_bool(ctx.params, "invert", false);
    // Slope as bar's normalised 0..1 field, then select the band.
    // Degree thresholds map linearly onto 0..90deg == 0..1 (effective,
    // not literal -- bar slope is unitless gradient magnitude).
    let slope = compute_slope_map(&input);
    let (lo, hi, fo) = (min_deg / 90.0, max_deg / 90.0, falloff_deg / 90.0);
    let mut hm = compute_height_select(&slope, lo, hi, fo, smooth);
    if invert {
        hm = apply_invert(&hm);
    }
    let hm = scale_by_field(hm, ctrl.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
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
        let p: HashMap<String, ParamValue> =
            params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let out = crate::CpuExecutor.execute(&nt, &p, &inputs, w, h, w, h).unwrap();
        match out.get("output").unwrap() {
            PortValue::Heightmap(hm) => hm.clone(),
            _ => panic!("expected heightmap output"),
        }
    }

    #[test]
    fn slope_select_picks_steep_rejects_flat() {
        let sel_sum = |hm: Heightmap| -> f32 {
            let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
            run_node(
                NodeType::SlopeSelect,
                &[
                    ("min_slope", ParamValue::Float(10.0)),
                    ("max_slope", ParamValue::Float(90.0)),
                    ("falloff", ParamValue::Float(5.0)),
                ],
                inputs,
                8,
                8,
            )
            .data()
            .iter()
            .sum()
        };
        let flat = const_hm(8, 8, 0.5);
        let mut steep_d = vec![0.0f32; 64];
        for y in 0..8 {
            for x in 0..8 {
                steep_d[y * 8 + x] = x as f32 / 7.0; // steady ramp -> high slope
            }
        }
        let steep = Heightmap::frbar_data(8, 8, steep_d).unwrap();
        assert!(sel_sum(flat) < 0.5, "flat terrain should select ~nothing");
        assert!(sel_sum(steep) > 5.0, "steep ramp should select substantially");
    }
}
