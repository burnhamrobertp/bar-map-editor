use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_float, get_input_heightmap};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let direction = get_float(ctx.params, "direction", 0.0);
    let width = get_float(ctx.params, "width", 90.0);
    let falloff = get_float(ctx.params, "falloff", 30.0).max(1e-4);
    let hm = apply_select_aspect(&input, direction, width, falloff);

    Ok(HashMap::from([("output".to_string(), PortValue::Heightmap(hm))]))
}

/// Aspect-direction mask. High where terrain faces `direction` degrees
/// (0=North/up, 90=East, 180=South, 270=West).
pub(crate) fn apply_select_aspect(input: &Heightmap, direction: f32, width: f32, falloff: f32) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let data = input.data();

    let out: Vec<f32> = (0..h)
        .flat_map(|py| {
            (0..w).map(move |px| {
                let xm = px.saturating_sub(1);
                let xp = (px + 1).min(w - 1);
                let ym = py.saturating_sub(1);
                let yp = (py + 1).min(h - 1);
                let dx = (data[py * w + xp] - data[py * w + xm]) / (xp - xm).max(1) as f32;
                let dy = (data[yp * w + px] - data[ym * w + px]) / (yp - ym).max(1) as f32;
                if dx * dx + dy * dy < 1e-12 {
                    return 0.0;
                }
                // atan2(dx, -dy): 0=North, 90=East, 180=South, 270=West.
                let aspect = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                let mut diff = (aspect - direction).abs().rem_euclid(360.0);
                if diff > 180.0 {
                    diff = 360.0 - diff;
                }
                let half = width * 0.5;
                if diff <= half {
                    1.0
                } else if diff <= half + falloff {
                    let t = (diff - half) / falloff;
                    1.0 - t * t * (3.0 - 2.0 * t)
                } else {
                    0.0
                }
            })
        })
        .collect();
    Heightmap::frbar_data(w as u32, h as u32, out).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType, ParamValue};

    #[test]
    fn select_aspect_east_facing_slopes() {
        let executor = crate::CpuExecutor;
        // Ramp increasing left-to-right: slopes face east (90 deg).
        let data: Vec<f32> = (0..8 * 8).map(|i| (i % 8) as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        // Select east-facing (direction=90), full-strength band=60 deg.
        let params = HashMap::from([
            ("direction".to_string(), ParamValue::Float(90.0)),
            ("width".to_string(), ParamValue::Float(60.0)),
            ("falloff".to_string(), ParamValue::Float(30.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::SelectAspect, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        // Interior pixels (not at edge) should have a high mask value.
        let centre = out.get(4, 4).unwrap();
        assert!(
            centre > 0.8,
            "east-facing slope should score high, got {centre}"
        );
    }

    #[test]
    fn select_aspect_opposite_direction_is_zero() {
        let executor = crate::CpuExecutor;
        // Ramp increasing left-to-right: slopes face east (90 deg).
        let data: Vec<f32> = (0..8 * 8).map(|i| (i % 8) as f32 / 7.0).collect();
        let hm = Heightmap::frbar_data(8, 8, data).unwrap();
        // Select west-facing (direction=270), tight band.
        let params = HashMap::from([
            ("direction".to_string(), ParamValue::Float(270.0)),
            ("width".to_string(), ParamValue::Float(30.0)),
            ("falloff".to_string(), ParamValue::Float(10.0)),
        ]);
        let inputs = HashMap::from([("input".to_string(), PortValue::Heightmap(hm))]);
        let result = executor
            .execute(&NodeType::SelectAspect, &params, &inputs, 8, 8, 8, 8)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        let centre = out.get(4, 4).unwrap();
        assert!(
            centre < 0.01,
            "west selector on east-facing slope should be 0, got {centre}"
        );
    }
}
