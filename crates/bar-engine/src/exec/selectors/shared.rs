//! Selector kernels shared within the family.

use bar_data::Heightmap;

/// Compute slope map: each pixel is the maximum gradient magnitude at that point.
/// Output range [0, 1] where 0 = flat, 1 = very steep.
pub(crate) fn compute_slope_map(input: &Heightmap) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let c = input.get(x, y).unwrap_or(0.0);
            let r = input.get((x + 1).min(w - 1), y).unwrap_or(c);
            let l = input.get(x.saturating_sub(1), y).unwrap_or(c);
            let d = input.get(x, (y + 1).min(h - 1)).unwrap_or(c);
            let u = input.get(x, y.saturating_sub(1)).unwrap_or(c);

            let dx = (r - l) * 0.5;
            let dy = (d - u) * 0.5;
            // Scale to reasonable range (slopes are typically small values)
            let slope = (dx * dx + dy * dy).sqrt() * 4.0;
            data[(y as usize) * (w as usize) + (x as usize)] = slope.clamp(0.0, 1.0);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Height band selection. Returns 1.0 for values in [low, high], fading to 0.0
/// within `falloff` distance. `smooth` applies a smoothstep to the falloff ramp
/// (WM "Falloff type") instead of the default linear ramp.
pub(crate) fn compute_height_select(
    input: &Heightmap,
    low: f32,
    high: f32,
    falloff: f32,
    smooth: bool,
) -> Heightmap {
    let w = input.width();
    let h = input.height();
    let ramp = |dist: f32| -> f32 {
        if falloff <= 0.0 {
            return 0.0;
        }
        let t = (1.0 - dist / falloff).max(0.0);
        if smooth {
            t * t * (3.0 - 2.0 * t)
        } else {
            t
        }
    };
    let data: Vec<f32> = input
        .data()
        .iter()
        .map(|&v| {
            if v >= low && v <= high {
                1.0
            } else if v < low {
                ramp(low - v)
            } else {
                ramp(v - high)
            }
        })
        .collect();

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Morphological dilation (expand=true) or erosion (expand=false) via
/// a separable max/min filter. O(w*h*r) rather than O(w*h*r^2).
pub(crate) fn apply_morphology(input: &Heightmap, radius: f32, expand: bool) -> Heightmap {
    let w = input.width() as usize;
    let h = input.height() as usize;
    let r = radius.round() as usize;
    let identity = if expand { 0.0f32 } else { 1.0f32 };

    // Horizontal pass.
    let mut temp = vec![identity; w * h];
    let data_in = input.data();
    for py in 0..h {
        for px in 0..w {
            let lo = px.saturating_sub(r);
            let hi = (px + r).min(w - 1);
            let mut acc = data_in[py * w + lo];
            for kx in lo..=hi {
                let v = data_in[py * w + kx];
                acc = if expand { acc.max(v) } else { acc.min(v) };
            }
            temp[py * w + px] = acc;
        }
    }

    // Vertical pass.
    let mut out = vec![identity; w * h];
    for py in 0..h {
        for px in 0..w {
            let lo = py.saturating_sub(r);
            let hi = (py + r).min(h - 1);
            let mut acc = temp[lo * w + px];
            for ky in lo..=hi {
                let v = temp[ky * w + px];
                acc = if expand { acc.max(v) } else { acc.min(v) };
            }
            out[py * w + px] = acc;
        }
    }

    Heightmap::frbar_data(w as u32, h as u32, out).unwrap()
}
