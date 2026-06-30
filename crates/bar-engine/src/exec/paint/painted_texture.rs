use std::collections::HashMap;

use bar_graph::{EvalError, PortValue};

use crate::exec::paint::shared::painted_rgb_to_color_buffer;
use crate::exec::shared::get_string;
use crate::exec::ExecCtx;

/// Source resolution for the `PaintedTexture` node's brush canvas.
/// Fixed for now; could be made a param like PaintedHeightmap.
const PAINTED_TEXTURE_RES: u32 = 256;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let path = get_string(ctx.params, "asset_path", "");
    // Imported textures can be any rectangular resolution;
    // hand-painted textures are square at PAINTED_TEXTURE_RES.
    // Read the header so we honour both.
    let (src_w, src_h, pixels) = if path.is_empty() {
        (PAINTED_TEXTURE_RES, PAINTED_TEXTURE_RES, Vec::new())
    } else {
        match bar_project::read_asset_file(std::path::Path::new(path)) {
            Ok((header, data)) => (header.width.max(1), header.height.max(1), data),
            Err(e) => {
                tracing::warn!(path, error = %e, "Failed to read texture asset");
                (PAINTED_TEXTURE_RES, PAINTED_TEXTURE_RES, Vec::new())
            }
        }
    };
    let tex = painted_rgb_to_color_buffer(pixels, src_w, src_h, ctx.tex_w, ctx.tex_h);

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Color(tex),
    )]))
}
