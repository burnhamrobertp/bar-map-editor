use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_float, get_input_heightmap, get_string};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let mode = get_string(ctx.params, "mode", "ridges");
    let strength = get_float(ctx.params, "strength", 1.0);
    let hm = apply_select_convexity(&input, mode, strength);

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Surface curvature (Laplacian) selector.
pub(crate) fn apply_select_convexity(input: &Heightmap, mode: &str, strength: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let data = input.data();

    // Compute raw Laplacian, collecting its range for normalization.
    let mut raw = vec![0.0f32; w * h];
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for y in 0..h {
        for x in 0..w {
            let c = data[y * w + x];
            let l = data[y * w + x.saturating_sub(1)];
            let r = data[y * w + (x + 1).min(w - 1)];
            let u = data[y.saturating_sub(1) * w + x];
            let d = data[(y + 1).min(h - 1) * w + x];
            // Negative = ridge/peak; positive = valley/bowl.
            let lap = l + r + u + d - 4.0 * c;
            raw[y * w + x] = lap;
            if lap < lo {
                lo = lap;
            }
            if lap > hi {
                hi = lap;
            }
        }
    }

    let range = (hi - lo).max(1e-9);
    let out: Vec<f32> = raw
        .iter()
        .map(|&lap| {
            // Normalize lap to roughly [-1, 1] then scale by strength.
            let norm = lap / range * 2.0 * strength;
            match mode {
                // High on ridges/peaks (negative Laplacian).
                "ridges" => (-norm).clamp(0.0, 1.0),
                // High in valleys/bowls (positive Laplacian).
                "valleys" => norm.clamp(0.0, 1.0),
                // Full map: 0.5 = flat, >0.5 = ridges, <0.5 = valleys.
                _ => (-norm * 0.5 + 0.5).clamp(0.0, 1.0),
            }
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, out).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    #[test]
    fn select_convexity_ridges_mode_peaks_high() {
        let executor = crate::CpuExecutor;
        // Single spike in centre on a flat background.
        // The spike pixel itself has a strongly negative Laplacian
        // (neighbors - 4*center < 0), so "ridges" mode should score it highest.
        let mut data = vec![0.0f32; 16 * 16];
        data[8 * 16 + 8] = 1.0;
        let hm = Heightmap::frbar_data(16, 16, data).unwrap();
        let params = HashMap::from([
            ("mode".to_string(), ParamValue::String("ridges".to_string())),
            ("strength".to_string(), ParamValue::Float(1.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::SelectConvexity, &params, &inputs, 16, 16, 16, 16)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // The spike pixel has the most negative Laplacian; ridges mode maps
        // strongly negative -> high output.
        let spike = out.get(8, 8).unwrap();
        assert!(
            spike > 0.8,
            "spike should score high in ridges mode, got {spike}"
        );
        // Flat background pixel far from spike should be low.
        let flat = out.get(0, 0).unwrap();
        assert!(flat < 0.2, "flat area should score low, got {flat}");
    }
}
