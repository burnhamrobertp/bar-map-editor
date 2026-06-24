//! Filter kernels shared within the family.

/// Bilinear sample with clamp-to-edge.
pub(crate) fn bilinear_sample(data: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x0 = (x.floor() as i32).clamp(0, w as i32 - 1) as usize;
    let y0 = (y.floor() as i32).clamp(0, h as i32 - 1) as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = (x - x.floor()).clamp(0.0, 1.0);
    let fy = (y - y.floor()).clamp(0.0, 1.0);
    let v00 = data[y0 * w + x0];
    let v10 = data[y0 * w + x1];
    let v01 = data[y1 * w + x0];
    let v11 = data[y1 * w + x1];
    let v0 = v00 + (v10 - v00) * fx;
    let v1 = v01 + (v11 - v01) * fx;
    v0 + (v1 - v0) * fy
}
