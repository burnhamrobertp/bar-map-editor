use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{apply_modulation, get_input_heightmap, get_optional_heightmap};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let hm = apply_normalize(&input);
    let hm = apply_modulation(&input, hm, ctrl.as_ref(), mask.as_ref());

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Normalize: remap all values to fill the 0..1 range.
pub(crate) fn apply_normalize(input: &Heightmap) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let data_in = input.data();

    let mut min_val = f32::MAX;
    let mut max_val = f32::MIN;
    for &v in data_in {
        if v < min_val {
            min_val = v;
        }
        if v > max_val {
            max_val = v;
        }
    }

    let range = max_val - min_val;
    let data: Vec<f32> = if range.abs() < 1e-8 {
        vec![0.5; data_in.len()]
    } else {
        data_in.iter().map(|&v| (v - min_val) / range).collect()
    };

    Heightmap::frbar_data(w, h, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    #[test]
    fn test_normalize_filter() {
        let executor = crate::CpuExecutor;
        // Input with values in [0.3, 0.7] -- normalize should stretch to [0, 1]
        let data: Vec<f32> = (0..64).map(|i| 0.3 + 0.4 * (i as f32 / 63.0)).collect();
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);

        let result = executor
            .execute(&NodeType::Normalize, &HashMap::<String, ParamValue>::new(), &inputs, 8, 8, 8, 8)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                let min = hm.data().iter().cloned().fold(f32::INFINITY, f32::min);
                let max = hm.data().iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                assert!(min.abs() < 0.001, "Min should be ~0, got {min}");
                assert!((max - 1.0).abs() < 0.001, "Max should be ~1, got {max}");
            }
            _ => panic!("Expected heightmap"),
        }
    }
}
