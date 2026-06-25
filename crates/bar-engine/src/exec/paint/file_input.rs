use std::collections::HashMap;

use bar_data::Heightmap;
use bar_graph::{EvalError, PortValue};

use crate::exec::shared::get_string;
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let path = get_string(ctx.params, "path", "");
    let hm = load_file_input(path, ctx.hm_w, ctx.hm_h)?;

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Heightmap(hm),
    )]))
}

/// Load an image file as a heightmap. Supports PNG/TIFF 8/16-bit grayscale + RGB.
pub(crate) fn load_file_input(path: &str, width: u32, height: u32) -> Result<Heightmap, EvalError> {
    if path.is_empty() {
        // No file specified -- return flat heightmap at 0.5
        let data = vec![0.5f32; (width as usize) * (height as usize)];
        return Heightmap::frbar_data(width, height, data)
            .map_err(|e| EvalError::Compute(e.to_string()));
    }

    let img = image::open(path)
        .map_err(|e| EvalError::Compute(format!("Failed to load image '{}': {}", path, e)))?;

    let gray = img.to_luma16();
    let (iw, ih) = gray.dimensions();

    // If dimensions match, use directly; otherwise resample
    let data: Vec<f32> = if iw == width && ih == height {
        gray.pixels().map(|p| p.0[0] as f32 / 65535.0).collect()
    } else {
        // Bilinear resample to target dimensions
        let mut resampled = Vec::with_capacity((width as usize) * (height as usize));
        for y in 0..height {
            for x in 0..width {
                let sx = x as f32 * (iw as f32 - 1.0) / (width as f32 - 1.0).max(1.0);
                let sy = y as f32 * (ih as f32 - 1.0) / (height as f32 - 1.0).max(1.0);
                let x0 = (sx as u32).min(iw - 1);
                let y0 = (sy as u32).min(ih - 1);
                let x1 = (x0 + 1).min(iw - 1);
                let y1 = (y0 + 1).min(ih - 1);
                let fx = sx - sx.floor();
                let fy = sy - sy.floor();
                let v00 = gray.get_pixel(x0, y0).0[0] as f32;
                let v10 = gray.get_pixel(x1, y0).0[0] as f32;
                let v01 = gray.get_pixel(x0, y1).0[0] as f32;
                let v11 = gray.get_pixel(x1, y1).0[0] as f32;
                let v = v00 * (1.0 - fx) * (1.0 - fy)
                    + v10 * fx * (1.0 - fy)
                    + v01 * (1.0 - fx) * fy
                    + v11 * fx * fy;
                resampled.push(v / 65535.0);
            }
        }
        resampled
    };

    Heightmap::frbar_data(width, height, data).map_err(|e| EvalError::Compute(e.to_string()))
}
