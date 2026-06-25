use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::get_input_heightmap;
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let a = get_input_heightmap(ctx.inputs, "a")?;
    let b = get_input_heightmap(ctx.inputs, "b")?;
    let mask = get_input_heightmap(ctx.inputs, "mask")?;
    let hm = apply_chooser(&a, &b, &mask);
    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// MaskSelect: select between A and B based on a mask (0=A, 1=B, interpolated in between).
pub(crate) fn apply_chooser(a: &Heightmap, b: &Heightmap, mask: &Heightmap) -> Heightmap {
    let w = a.width().min(b.width()).min(mask.width());
    let h = a.height().min(b.height()).min(mask.height());
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let va = a.get(x, y).unwrap_or(0.0);
            let vb = b.get(x, y).unwrap_or(0.0);
            let m = mask.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            data[(y as usize) * (w as usize) + (x as usize)] = va * (1.0 - m) + vb * m;
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType};

    #[test]
    fn test_chooser() {
        let executor = crate::CpuExecutor;
        let a = Heightmap::frbar_data(4, 4, vec![0.2; 16]).unwrap();
        let b = Heightmap::frbar_data(4, 4, vec![0.8; 16]).unwrap();
        // Mask: top half 0.0 (choose a), bottom half 1.0 (choose b)
        let mut mask_data = vec![0.0_f32; 16];
        for v in mask_data[8..16].iter_mut() {
            *v = 1.0;
        }
        let mask = Heightmap::frbar_data(4, 4, mask_data).unwrap();

        let inputs = HashMap::from([
            ("a".to_string(), PortValue::Heightmap(a)),
            ("b".to_string(), PortValue::Heightmap(b)),
            ("mask".to_string(), PortValue::Heightmap(mask)),
        ]);

        let result = executor
            .execute(&NodeType::MaskSelect, &HashMap::new(), &inputs, 4, 4, 4, 4)
            .unwrap();
        match result.get("output").unwrap() {
            PortValue::Heightmap(hm) => {
                // Top half = a (0.2), bottom half = b (0.8)
                assert!((hm.get(0, 0).unwrap() - 0.2).abs() < 0.01);
                assert!((hm.get(0, 3).unwrap() - 0.8).abs() < 0.01);
            }
            _ => panic!("Expected heightmap"),
        }
    }
}
