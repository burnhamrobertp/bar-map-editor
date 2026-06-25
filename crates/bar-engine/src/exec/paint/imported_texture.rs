use std::collections::HashMap;

use bar_data::ColorBuffer;
use bar_graph::{EvalError, PortValue};

use crate::exec::ExecCtx;
use crate::exec::shared::{assemble_texture_preview, get_string, get_uint};

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let asset_path = get_string(ctx.params, "asset_path", "");
    let idx_path = get_string(ctx.params, "tile_index_path", "");
    let tiles_x = get_uint(ctx.params, "tiles_x", 0);
    let tiles_y = get_uint(ctx.params, "tiles_y", 0);
    let color =
        if asset_path.is_empty() || idx_path.is_empty() || tiles_x == 0 || tiles_y == 0 {
            ColorBuffer::new(ctx.tex_w, ctx.tex_h).unwrap()
        } else {
            let tiles_result = (|| {
                let file = std::fs::File::open(asset_path).ok()?;
                bar_data::smt::read_smt(&mut std::io::BufReader::new(file)).ok()
            })();
            let idx_result = std::fs::read(idx_path).ok();
            match (tiles_result, idx_result) {
                (Some(tiles), Some(idx_bytes)) => {
                    let tile_indices: Vec<i32> = idx_bytes
                        .chunks(4)
                        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    let rgba = assemble_texture_preview(
                        &tiles,
                        &tile_indices,
                        tiles_x,
                        tiles_y,
                        ctx.tex_w,
                        ctx.tex_h,
                    );
                    let mut buf = ColorBuffer::new(ctx.tex_w, ctx.tex_h).unwrap();
                    for (i, px) in rgba.chunks(4).enumerate() {
                        let x = (i as u32) % ctx.tex_w;
                        let y = (i as u32) / ctx.tex_w;
                        buf.set(
                            x,
                            y,
                            [
                                px[0] as f32 / 255.0,
                                px[1] as f32 / 255.0,
                                px[2] as f32 / 255.0,
                                1.0,
                            ],
                        );
                    }
                    buf
                }
                _ => {
                    tracing::warn!(
                        asset_path,
                        "ImportedTexture: failed to read SMT or tile index"
                    );
                    ColorBuffer::new(ctx.tex_w, ctx.tex_h).unwrap()
                }
            }
        };

    Ok(HashMap::from([("output".to_string(), PortValue::Color(color))]))
}
