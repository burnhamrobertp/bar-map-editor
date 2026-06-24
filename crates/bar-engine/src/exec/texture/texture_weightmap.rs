use std::collections::HashMap;

use bar_data::ColorBuffer;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{get_float, get_string, get_uint};

/// Sample `tex` at output pixel `(ox, oy)` using nearest-neighbour scaling
/// to the output dimensions `(ow, oh)`.
fn sample_color_nn(tex: &ColorBuffer, ox: u32, oy: u32, ow: u32, oh: u32) -> [f32; 4] {
    let sx = ((ox as f32 / ow as f32) * tex.width() as f32) as u32;
    let sy = ((oy as f32 / oh as f32) * tex.height() as f32) as u32;
    tex.get(sx.min(tex.width() - 1), sy.min(tex.height() - 1))
        .unwrap_or([0.0; 4])
}

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let mut outputs: HashMap<String, PortValue> = HashMap::new();

    let priority_type = get_string(ctx.params, "priority_type", "weighted_blend");
    let layer_count = get_uint(ctx.params, "layer_count", 2).clamp(2, 8) as usize;

    struct Layer {
        tex: ColorBuffer,
        priority: f32,
        exclusion: f32,
    }
    let mut layers: Vec<Layer> = Vec::new();
    for i in 0..layer_count {
        let Some(PortValue::Color(tex)) = ctx.inputs.get(&format!("texture_{i}")) else {
            continue;
        };
        let priority = get_float(ctx.params, &format!("priority_{i}"), (7 - i) as f32);
        let exclusion = get_float(ctx.params, &format!("exclusion_{i}"), 0.0).clamp(0.0, 1.0);
        layers.push(Layer {
            tex: tex.clone(),
            priority,
            exclusion,
        });
    }

    if layers.is_empty() {
        let out = ColorBuffer::new(ctx.tex_w, ctx.tex_h).unwrap();
        outputs.insert("output".to_string(), PortValue::Color(out));
    } else {
        let w = layers[0].tex.width();
        let h = layers[0].tex.height();
        let mut out = ColorBuffer::new(w, h).unwrap();

        match priority_type {
            "priority" => {
                // Sort highest priority first.
                layers.sort_by(|a, b| {
                    b.priority
                        .partial_cmp(&a.priority)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for y in 0..h {
                    for x in 0..w {
                        let mut remaining = 1.0f32;
                        let mut r = 0.0f32;
                        let mut g = 0.0f32;
                        let mut b_out = 0.0f32;
                        for layer in &layers {
                            if remaining <= 0.001 {
                                break;
                            }
                            let raw_w =
                                sample_color_nn(&layer.tex, x, y, w, h)[3].clamp(0.0, 1.0);
                            let contribution = (raw_w * remaining).clamp(0.0, remaining);
                            let col = sample_color_nn(&layer.tex, x, y, w, h);
                            r += col[0] * contribution;
                            g += col[1] * contribution;
                            b_out += col[2] * contribution;
                            remaining -= contribution * layer.exclusion;
                            remaining = remaining.max(0.0);
                        }
                        out.set(x, y, [r, g, b_out, 1.0]);
                    }
                }
            }
            _ => {
                // weighted_blend: normalize all weights at each pixel.
                for y in 0..h {
                    for x in 0..w {
                        let weights: Vec<f32> = layers
                            .iter()
                            .map(|l| sample_color_nn(&l.tex, x, y, w, h)[3].clamp(0.0, 1.0))
                            .collect();
                        let total: f32 = weights.iter().sum();
                        if total < 0.0001 {
                            out.set(x, y, [0.0, 0.0, 0.0, 0.0]);
                            continue;
                        }
                        let (mut r, mut g, mut b_out) = (0.0f32, 0.0f32, 0.0f32);
                        for (layer, &wt) in layers.iter().zip(weights.iter()) {
                            let col = sample_color_nn(&layer.tex, x, y, w, h);
                            let norm = wt / total;
                            r += col[0] * norm;
                            g += col[1] * norm;
                            b_out += col[2] * norm;
                        }
                        out.set(x, y, [r, g, b_out, 1.0]);
                    }
                }
            }
        }
        outputs.insert("output".to_string(), PortValue::Color(out));
    }

    Ok(outputs)
}
