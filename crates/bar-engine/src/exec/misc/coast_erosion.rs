use std::collections::HashMap;

use bar_compute::{coast_erosion, CoastErosionParams};
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::{
    apply_modulation, get_float, get_input_heightmap, get_optional_heightmap, get_uint,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");

    let params = CoastErosionParams {
        sea_level: get_float(ctx.params, "sea_level", 0.3),
        beach_size: get_float(ctx.params, "beach_size", 0.05),
        inland_height_influence: get_float(ctx.params, "inland_height_influence", 0.3),
        underwater_smoothing: get_uint(ctx.params, "underwater_smoothing", 3),
    };

    let result = coast_erosion(&input, &params);
    let hm = apply_modulation(&input, result, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bar_data::Heightmap;
    use bar_graph::{ParamValue, PortValue};

    use crate::exec::ExecCtx;

    fn run(params: &[(&str, ParamValue)], inputs: &HashMap<String, PortValue>) -> Heightmap {
        let params: HashMap<String, ParamValue> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        let ctx = ExecCtx {
            params: &params,
            inputs,
            hm_w: 4,
            hm_h: 4,
            tex_w: 4,
            tex_h: 4,
        };
        match super::exec(&ctx).unwrap().remove("output").unwrap() {
            PortValue::Heightmap(h) => h,
            _ => panic!("expected heightmap output"),
        }
    }

    #[test]
    fn flattens_seabed_and_keeps_unit_range() {
        // 4x4: a jagged submerged seabed (below sea=0.3) plus some land.
        let data = vec![
            0.05, 0.22, 0.03, 0.20, //
            0.18, 0.02, 0.24, 0.06, //
            0.40, 0.55, 0.70, 0.90, //
            0.45, 0.60, 0.75, 0.95, //
        ];
        let input = Heightmap::frbar_data(4, 4, data.clone()).unwrap();
        let mut inputs = HashMap::new();
        inputs.insert("input".to_string(), PortValue::Heightmap(input));

        let out = run(
            &[
                ("sea_level", ParamValue::Float(0.3)),
                ("beach_size", ParamValue::Float(0.05)),
                ("inland_height_influence", ParamValue::Float(0.3)),
                ("underwater_smoothing", ParamValue::UInt(6)),
            ],
            &inputs,
        );

        assert_eq!(out.width(), 4);
        assert_eq!(out.height(), 4);
        for &v in out.data() {
            assert!((0.0..=1.0).contains(&v), "value {v} out of [0,1]");
        }

        // The submerged region (first two rows) should be flatter than the
        // jagged input there.
        let seabed_var = |d: &[f32]| {
            let s = &d[0..8];
            let m = s.iter().sum::<f32>() / 8.0;
            s.iter().map(|v| (v - m).powi(2)).sum::<f32>() / 8.0
        };
        assert!(
            seabed_var(out.data()) < seabed_var(&data),
            "coast erosion should smooth the seabed"
        );
    }

    #[test]
    fn missing_input_errors() {
        let inputs = HashMap::new();
        let params: HashMap<String, ParamValue> = HashMap::new();
        let ctx = ExecCtx {
            params: &params,
            inputs: &inputs,
            hm_w: 4,
            hm_h: 4,
            tex_w: 4,
            tex_h: 4,
        };
        assert!(super::exec(&ctx).is_err());
    }
}
