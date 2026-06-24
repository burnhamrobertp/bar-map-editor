use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::get_string;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let mut outputs: HashMap<String, PortValue> = HashMap::new();

    // For non-paintable kinds (normalmap, grassmap, specular, files),
    // FC is a pure pass-through: forward input to same-named output
    // verbatim.
    for port_name in ["normalmap", "grassmap", "specular"] {
        if let Some(value) = ctx.inputs.get(port_name) {
            outputs.insert(port_name.to_string(), value.clone());
        }
    }

    // Paintable kinds (heightmap, texture, metalmap, typemap) consult a
    // per-kind layer asset; when the asset_path is empty, FC is
    // pass-through for that kind too. When present, the layer's bytes
    // are composited on top of the input.
    composite_heightmap_layer(
        ctx.inputs,
        &mut outputs,
        ctx.params,
        "heightmap",
        "heightmap_layer_asset_path",
    );
    composite_heightmap_layer(
        ctx.inputs,
        &mut outputs,
        ctx.params,
        "metalmap",
        "metalmap_layer_asset_path",
    );
    composite_heightmap_layer(
        ctx.inputs,
        &mut outputs,
        ctx.params,
        "typemap",
        "typemap_layer_asset_path",
    );
    composite_color_layer(
        ctx.inputs,
        &mut outputs,
        ctx.params,
        "texture",
        "color_layer_asset_path",
    );

    Ok(outputs)
}

/// Apply a sculpt delta buffer onto a heightmap in place.
/// `pixels` is a flat u8 array at `src_w x src_h`: 128 = no change,
/// 0 = max subtract, 255 = max add. `scale` controls the maximum magnitude
/// of the applied delta (e.g. 0.5 = max +/-50% shift). If `pixels` is empty
/// or wrong length the heightmap is left unchanged.
fn apply_sculpt_delta(hm: &mut Heightmap, pixels: &[u8], src_w: u32, src_h: u32, scale: f32) {
    let out_w = hm.width();
    let out_h = hm.height();
    if pixels.len() != (src_w as usize) * (src_h as usize) {
        return;
    }
    for oy in 0..out_h {
        for ox in 0..out_w {
            let sx = ox as f32 * (src_w as f32 - 1.0) / (out_w as f32 - 1.0).max(1.0);
            let sy = oy as f32 * (src_h as f32 - 1.0) / (out_h as f32 - 1.0).max(1.0);
            let x0 = sx as u32;
            let y0 = sy as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let fx = sx - sx.floor();
            let fy = sy - sy.floor();
            let v00 = pixels[(y0 as usize) * (src_w as usize) + x0 as usize] as f32;
            let v10 = pixels[(y0 as usize) * (src_w as usize) + x1 as usize] as f32;
            let v01 = pixels[(y1 as usize) * (src_w as usize) + x0 as usize] as f32;
            let v11 = pixels[(y1 as usize) * (src_w as usize) + x1 as usize] as f32;
            let v = v00 * (1.0 - fx) * (1.0 - fy)
                + v10 * fx * (1.0 - fy)
                + v01 * (1.0 - fx) * fy
                + v11 * fx * fy;
            // Map [0,255] -> [-1,1], multiply by scale, add to input
            let delta = (v - 128.0) / 128.0 * scale;
            let cur = hm.get(ox, oy).unwrap_or(0.0);
            let _ = hm.set(ox, oy, (cur + delta).clamp(0.0, 1.0));
        }
    }
}

/// FinalComposition heightmap-kind composite: read the layer asset at
/// `path_param` and apply it on top of the input value, writing the
/// result to the same-named output port. Falls through to pure
/// pass-through if the layer asset path is unset or empty.
///
/// Semantics depend on the layer's `AssetKind`:
/// - `GrayscaleU8` -- delta encoding (128 = neutral, 0 = max negative,
///   255 = max positive). Applied via `apply_sculpt_delta` with a
///   fixed scale of `0.5` for now (matches the legacy `Sculpt` node).
///   Used for the heightmap layer.
/// - `GrayscaleU8` with sentinel byte 0xFF -- "untouched" overlay.
///   Painted pixels (byte < 0xFF) overwrite the input value;
///   untouched pixels pass the input through. Used for metalmap /
///   typemap layers where the meaning of the value is quantised
///   (terrain-type ID, metal density) and "blend" doesn't make sense.
///   For these kinds the resolution must match the input; if it
///   doesn't we degrade to pass-through.
/// - `GrayscaleF32` -- not used for layers (PaintedHeightmap nodes
///   use F32, FC layers use U8); accepting it here would let it
///   silently misbehave.
fn composite_heightmap_layer(
    inputs: &HashMap<String, PortValue>,
    outputs: &mut HashMap<String, PortValue>,
    params: &HashMap<String, ParamValue>,
    port_name: &str,
    path_param: &str,
) {
    let Some(input_value) = inputs.get(port_name) else {
        return;
    };
    let PortValue::Heightmap(input_hm) = input_value else {
        // Wrong port kind on input; pass it through unchanged.
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    };
    let asset_path = get_string(params, path_param, "");
    if asset_path.is_empty() {
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    }
    let path = std::path::Path::new(asset_path);
    let Ok((header, data)) = bar_project::read_asset_file(path) else {
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    };
    let mut composited = input_hm.clone();
    let src_w = header.width.max(1);
    let src_h = header.height.max(1);
    match (header.kind, port_name) {
        // Heightmap layer: delta encoding.
        (bar_project::AssetKind::GrayscaleU8, "heightmap") => {
            apply_sculpt_delta(&mut composited, &data, src_w, src_h, 0.5);
        }
        // Metalmap / typemap layer: sentinel-overlay (byte 0xFF = untouched).
        (bar_project::AssetKind::GrayscaleU8, "metalmap")
        | (bar_project::AssetKind::GrayscaleU8, "typemap") => {
            apply_sentinel_overlay(&mut composited, &data, src_w, src_h);
        }
        _ => {
            // Unsupported (kind, port) -- pass through.
        }
    }
    outputs.insert(port_name.to_string(), PortValue::Heightmap(composited));
}

/// FinalComposition color-kind composite: alpha-mask overlay. Painted
/// pixels (alpha > 0) replace the input pixel; alpha == 0 passes the
/// input through. Layer asset is `RgbaU8`. Falls through to
/// pass-through if the layer is unset, can't be read, or the kind tag
/// is wrong.
fn composite_color_layer(
    inputs: &HashMap<String, PortValue>,
    outputs: &mut HashMap<String, PortValue>,
    params: &HashMap<String, ParamValue>,
    port_name: &str,
    path_param: &str,
) {
    let Some(input_value) = inputs.get(port_name) else {
        return;
    };
    let PortValue::Color(input_cb) = input_value else {
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    };
    let asset_path = get_string(params, path_param, "");
    if asset_path.is_empty() {
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    }
    let path = std::path::Path::new(asset_path);
    let Ok((header, data)) = bar_project::read_asset_file(path) else {
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    };
    if !matches!(header.kind, bar_project::AssetKind::RgbaU8) {
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    }
    let expected = (header.width as usize) * (header.height as usize) * 4;
    if data.len() != expected {
        outputs.insert(port_name.to_string(), input_value.clone());
        return;
    }
    let mut composited = input_cb.clone();
    let layer_w = header.width as f32;
    let layer_h = header.height as f32;
    let out_w = composited.width();
    let out_h = composited.height();
    for oy in 0..out_h {
        for ox in 0..out_w {
            // Map output pixel to nearest layer pixel (no bilinear yet;
            // alpha-mask paint doesn't benefit much from interpolation).
            let lx = ((ox as f32 / out_w.max(1) as f32) * layer_w) as u32;
            let ly = ((oy as f32 / out_h.max(1) as f32) * layer_h) as u32;
            let lx = lx.min(header.width.saturating_sub(1));
            let ly = ly.min(header.height.saturating_sub(1));
            let idx = ((ly * header.width + lx) * 4) as usize;
            let a = data[idx + 3];
            if a == 0 {
                continue;
            }
            let r = data[idx] as f32 / 255.0;
            let g = data[idx + 1] as f32 / 255.0;
            let b = data[idx + 2] as f32 / 255.0;
            let af = a as f32 / 255.0;
            let base = composited.get(ox, oy).unwrap_or([0.0; 4]);
            let new = [
                base[0] * (1.0 - af) + r * af,
                base[1] * (1.0 - af) + g * af,
                base[2] * (1.0 - af) + b * af,
                1.0,
            ];
            composited.set(ox, oy, new);
        }
    }
    outputs.insert(port_name.to_string(), PortValue::Color(composited));
}

/// Apply a sentinel-overlay layer to a heightmap in place. Each byte
/// in `pixels` is either `0xFF` (untouched -- input passes through) or
/// `0..=0xFE` (painted -- byte value / 254 replaces input). Used for
/// quantised kinds (metalmap, typemap) where blending doesn't make
/// sense and a per-pixel "did the user paint here" mask is needed.
fn apply_sentinel_overlay(hm: &mut Heightmap, pixels: &[u8], src_w: u32, src_h: u32) {
    if pixels.len() != (src_w as usize) * (src_h as usize) {
        return;
    }
    let out_w = hm.width();
    let out_h = hm.height();
    for oy in 0..out_h {
        for ox in 0..out_w {
            // Nearest-neighbour sample of the layer (preserves the
            // quantised semantics; bilinear would alias terrain-type
            // IDs at boundaries).
            let lx = ((ox as f32 / out_w.max(1) as f32) * src_w as f32) as u32;
            let ly = ((oy as f32 / out_h.max(1) as f32) * src_h as f32) as u32;
            let lx = lx.min(src_w.saturating_sub(1));
            let ly = ly.min(src_h.saturating_sub(1));
            let byte = pixels[(ly * src_w + lx) as usize];
            if byte == 0xFF {
                continue;
            }
            let v = byte as f32 / 254.0;
            let _ = hm.set(ox, oy, v);
        }
    }
}
