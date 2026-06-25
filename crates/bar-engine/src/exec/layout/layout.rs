use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::layout::raster::{rasterize_primitive_item, rasterize_spline_item};
use crate::exec::shared::{get_float, get_optional_heightmap, get_uint};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let item_count = get_uint(ctx.params, "item_count", 1).min(8) as usize;
    let hm = apply_layout(ctx.params, item_count, ctx.hm_w, ctx.hm_h, mask.as_ref());

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Composite every layout item (primitive shapes + Catmull-Rom
/// splines) into a single [0, 1] coverage field, then map it to the
/// node's output mode and apply the optional mask input.
///
/// Items are read from indexed params (`type_i`, `x_i`, ..., or
/// `points_i` for spline items). Each item contributes its
/// falloff-weighted coverage scaled by `height_i`; items composite by
/// per-pixel max. The node-level `mode` then interprets the field:
/// `ridge`/`mask` pass it through, `valley` inverts it (background 1,
/// shapes 0) so a downstream Multiply carves the terrain.
fn apply_layout(
    params: &HashMap<String, ParamValue>,
    item_count: usize,
    width: u32,
    height: u32,
    mask: Option<&Heightmap>,
) -> Heightmap {
    let mut field = vec![0.0f32; (width * height) as usize];

    let symmetry = match params.get("symmetry") {
        Some(ParamValue::String(s)) => s.as_str(),
        _ => "none",
    };
    let mode = match params.get("mode") {
        Some(ParamValue::String(s)) => s.as_str(),
        _ => "ridge",
    };

    for i in 0..item_count {
        let item_type = match params.get(&format!("type_{i}")) {
            Some(ParamValue::String(s)) => s.as_str(),
            _ => "ellipse",
        };
        let height_i = get_float(params, &format!("height_{i}"), 0.5).clamp(0.0, 1.0);
        let falloff_i = get_float(params, &format!("falloff_{i}"), 0.5).clamp(0.0, 1.0);
        if item_type == "spline" {
            rasterize_spline_item(
                &mut field, params, i, height_i, falloff_i, symmetry, width, height,
            );
        } else {
            rasterize_primitive_item(
                &mut field, item_type, params, i, height_i, falloff_i, symmetry, width, height,
            );
        }
    }

    for v in field.iter_mut() {
        *v = match mode {
            // Background high, shapes low -- multiply downstream to carve.
            "valley" => (1.0 - *v).clamp(0.0, 1.0),
            // ridge / mask: coverage passes straight through.
            _ => v.clamp(0.0, 1.0),
        };
    }

    if let Some(m) = mask {
        for (idx, v) in field.iter_mut().enumerate() {
            let mx = (idx % width as usize) as u32;
            let my = (idx / width as usize) as u32;
            let mw = m.width();
            let mh = m.height();
            let smx = (mx as f32 * mw as f32 / width as f32) as u32;
            let smy = (my as f32 * mh as f32 / height as f32) as u32;
            let mv = m.get(smx.min(mw - 1), smy.min(mh - 1)).unwrap_or(1.0);
            *v *= mv;
        }
    }

    Heightmap::frbar_data(width, height, field).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeExecutor, NodeType};

    #[test]
    fn layout_generator_ellipse_peak_at_centre() {
        let executor = crate::CpuExecutor;
        let mut params = HashMap::new();
        params.insert("item_count".to_string(), ParamValue::UInt(1));
        params.insert(
            "type_0".to_string(),
            ParamValue::String("ellipse".to_string()),
        );
        params.insert("x_0".to_string(), ParamValue::Float(0.5));
        params.insert("y_0".to_string(), ParamValue::Float(0.5));
        params.insert("rx_0".to_string(), ParamValue::Float(0.3));
        params.insert("ry_0".to_string(), ParamValue::Float(0.3));
        params.insert("angle_0".to_string(), ParamValue::Float(0.0));
        params.insert("height_0".to_string(), ParamValue::Float(0.8));
        params.insert("falloff_0".to_string(), ParamValue::Float(0.5));
        let result = executor
            .execute(&NodeType::Layout, &params, &HashMap::new(), 32, 32, 32, 32)
            .unwrap();
        let PortValue::Heightmap(out) = result.get("output").unwrap() else {
            panic!("expected heightmap");
        };
        let centre = out.get(16, 16).unwrap();
        let edge = out.get(0, 0).unwrap();
        assert!(centre > 0.5, "centre should be high, got {centre}");
        assert!(edge < 0.01, "corner should be near zero, got {edge}");
    }
}
