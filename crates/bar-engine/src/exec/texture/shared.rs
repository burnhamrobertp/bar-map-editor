//! Texture kernels shared within the family.

use bar_data::{ColorBuffer, Heightmap};

/// Apply optional control and mask to a Color output.
/// Blends each pixel from `neutral` toward `effect` by `control * mask`.
/// Returns `effect` unchanged when both are None; otherwise mutates in place.
pub(crate) fn apply_color_modulation(
    neutral: [f32; 4],
    mut effect: ColorBuffer,
    control: Option<&Heightmap>,
    mask: Option<&Heightmap>,
) -> ColorBuffer {
    if control.is_none() && mask.is_none() {
        return effect;
    }
    if let Some(c) = control {
        debug_assert_eq!(
            effect.width(),
            c.width(),
            "apply_color_modulation: control width"
        );
        debug_assert_eq!(
            effect.height(),
            c.height(),
            "apply_color_modulation: control height"
        );
    }
    if let Some(m) = mask {
        debug_assert_eq!(
            effect.width(),
            m.width(),
            "apply_color_modulation: mask width"
        );
        debug_assert_eq!(
            effect.height(),
            m.height(),
            "apply_color_modulation: mask height"
        );
    }
    let ctrl_d = control.map(Heightmap::data);
    let mask_d = mask.map(Heightmap::data);
    for (i, pixel) in effect.data_mut().chunks_exact_mut(4).enumerate() {
        let cv = ctrl_d.map_or(1.0, |d| d[i].clamp(0.0, 1.0));
        let mv = mask_d.map_or(1.0, |d| d[i].clamp(0.0, 1.0));
        let t = cv * mv;
        for ch in 0..4 {
            pixel[ch] = neutral[ch] + (pixel[ch] - neutral[ch]) * t;
        }
    }
    effect
}

/// Resize a ColorBuffer to (tw, th) only when dimensions differ.
/// Bridge nodes (heightmap-in, color-out) generate at hm dims then call this
/// to match the working/compile texture resolution.
pub(crate) fn resize_color_to_tex(cb: ColorBuffer, tw: u32, th: u32) -> ColorBuffer {
    if cb.width() == tw && cb.height() == th {
        cb
    } else {
        cb.resize(tw, th)
    }
}

/// Parse a 6-digit `RRGGBB` hex string into an `[r, g, b]` array of
/// `f32` values in [0.0, 1.0]. Returns `None` if the string isn't six
/// valid hex digits.
pub(crate) fn parse_hex_color_srgb(s: &str) -> Option<[f32; 3]> {
    let bytes = s.as_bytes();
    if bytes.len() != 6 {
        return None;
    }
    let mut out = [0f32; 3];
    for i in 0..3 {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out[i] = ((hi << 4 | lo) as f32) / 255.0;
    }
    Some(out)
}

/// Simple ambient occlusion: compare center height to neighbors.
pub(crate) fn compute_local_ao(heightmap: &Heightmap, x: u32, y: u32) -> f32 {
    let c = heightmap.get(x, y).unwrap_or(0.5);
    let w = heightmap.width();
    let h = heightmap.height();
    let mut sum = 0.0f32;
    let mut count = 0.0f32;

    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
            let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
            let nh = heightmap.get(nx, ny).unwrap_or(c);
            // If neighbor is higher, this point is "occluded"
            sum += (c - nh).max(0.0);
            count += 1.0;
        }
    }

    // Map occlusion to brightness [0.7, 1.0]
    let occlusion = (sum / count * 5.0).clamp(0.0, 1.0);
    1.0 - occlusion * 0.3
}

fn detail_hash(ix: i32, iy: i32) -> f32 {
    let h = ix
        .wrapping_mul(374761393i32)
        .wrapping_add(iy.wrapping_mul(668265263i32));
    let h = (h ^ (h >> 13)).wrapping_mul(1274126177i32);
    let h = h ^ (h >> 16);
    (h as u32) as f32 / u32::MAX as f32
}

fn detail_value_noise(fx: f32, fy: f32) -> f32 {
    let ix = fx.floor() as i32;
    let iy = fy.floor() as i32;
    let tx = fx - ix as f32;
    let ty = fy - iy as f32;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let a = detail_hash(ix, iy) + sx * (detail_hash(ix + 1, iy) - detail_hash(ix, iy));
    let b = detail_hash(ix, iy + 1) + sx * (detail_hash(ix + 1, iy + 1) - detail_hash(ix, iy + 1));
    a + sy * (b - a)
}

/// 4-octave value-noise FBM over UV space. Returns [0, 1].
pub(crate) fn micro_fbm(ux: f32, uy: f32, base_freq: f32) -> f32 {
    let mut val = 0.0f32;
    let mut amp = 0.5f32;
    let mut freq = base_freq;
    let mut norm = 0.0f32;
    for _ in 0..4 {
        val += detail_value_noise(ux * freq, uy * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    val / norm
}
