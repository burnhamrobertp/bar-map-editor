//! Combiner kernels shared within the family.

use bar_data::Heightmap;

pub(crate) fn combine_heightmaps(a: &Heightmap, b: &Heightmap, op: impl Fn(f32, f32) -> f32) -> Heightmap {
    let w = a.width().min(b.width());
    let h = a.height().min(b.height());
    let mut data = vec![0.0f32; (w as usize) * (h as usize)];

    for y in 0..h {
        for x in 0..w {
            let va = a.get(x, y).unwrap_or(0.0);
            let vb = b.get(x, y).unwrap_or(0.0);
            data[(y as usize) * (w as usize) + (x as usize)] = op(va, vb);
        }
    }

    Heightmap::frbar_data(w, h, data).unwrap()
}

/// Universal combiner (WM Combiner). `mode` selects the per-pixel operation;
/// `factor` is the blend amount / Strength: each pixel becomes
/// `lerp(a, op(a,b), factor)`. `blend` makes `op == b`, so the default
/// (mode="blend", factor=0.5) is exactly the classic `blend_heightmaps`.
/// Operands and results are kept in the engine's normalised 0..1 space.
pub(crate) fn combine_mode_heightmaps(a: &Heightmap, b: &Heightmap, mode: &str, factor: f32) -> Heightmap {
    combine_heightmaps(a, b, |va, vb| {
        let op = match mode {
            "add" => (va + vb).min(1.0),
            "subtract" => (va - vb).max(0.0),
            "multiply" => va * vb,
            "divide" => {
                if vb > 1e-6 {
                    (va / vb).clamp(0.0, 1.0)
                } else {
                    va
                }
            }
            "average" => (va + vb) * 0.5,
            "screen" => 1.0 - (1.0 - va) * (1.0 - vb),
            // B drives the exponent (0.1..4): B<0.5 brightens, B>0.5 darkens.
            "power" => va.max(0.0).powf(0.1 + vb * 3.9),
            "difference" => (va - vb).abs(),
            "max" => va.max(vb),
            "min" => va.min(vb),
            // "blend" and any unknown mode: lerp toward B.
            _ => vb,
        };
        va + (op - va) * factor
    })
}
