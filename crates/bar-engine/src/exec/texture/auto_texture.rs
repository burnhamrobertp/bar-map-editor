use std::collections::HashMap;

use bar_data::{ColorBuffer, Heightmap};
use bar_graph::{EvalError, ParamValue, PortValue};

use crate::exec::shared::{get_float, get_input_heightmap, get_optional_heightmap, get_string};
use crate::exec::texture::shared::{
    apply_color_modulation, compute_local_ao, micro_fbm, parse_hex_color_srgb, resize_color_to_tex,
};
use crate::exec::ExecCtx;

pub fn exec(ctx: &ExecCtx) -> Result<HashMap<String, PortValue>, EvalError> {
    let input = get_input_heightmap(ctx.inputs, "input")?;
    let slope = get_optional_heightmap(ctx.inputs, "slope");
    let ctrl = get_optional_heightmap(ctx.inputs, "control");
    let mask = get_optional_heightmap(ctx.inputs, "mask");
    let color = generate_auto_texture(&input, slope.as_ref(), ctx.params);
    // Neutral = transparent black so masked regions don't paint opaque gray
    // over downstream composite layers.
    let color = apply_color_modulation([0.0, 0.0, 0.0, 0.0], color, ctrl.as_ref(), mask.as_ref());
    let color = resize_color_to_tex(color, ctx.tex_w, ctx.tex_h);

    Ok(HashMap::from([(
        "output".to_string(),
        PortValue::Color(color),
    )]))
}

/// Biome gradient: list of `(rgb, height)` stops sorted by height.
/// Heights are normalised to `[0, 1]`. Each biome owns both its
/// palette AND its thresholds -- a "snow" stop in mountainous sits
/// at 0.85, in temperate at 0.95, and is absent entirely in tropical.
type BiomeGradient = &'static [([f32; 3], f32)];

const BIOME_TEMPERATE: BiomeGradient = &[
    ([0.05, 0.10, 0.30], 0.00), // deep water
    ([0.10, 0.25, 0.50], 0.15), // shallow water
    ([0.76, 0.70, 0.50], 0.20), // sand/beach
    ([0.30, 0.55, 0.15], 0.30), // lowland grass
    ([0.20, 0.45, 0.10], 0.50), // forest
    ([0.40, 0.35, 0.25], 0.65), // dirt
    ([0.45, 0.42, 0.38], 0.75), // rock
    ([0.55, 0.52, 0.48], 0.85), // light rock
    ([0.90, 0.92, 0.95], 0.95), // snow
    ([1.00, 1.00, 1.00], 1.00), // peak snow
];

const BIOME_GRASSLAND: BiomeGradient = &[
    ([0.20, 0.30, 0.20], 0.00), // muddy water
    ([0.30, 0.45, 0.25], 0.10), // marsh
    ([0.78, 0.72, 0.50], 0.15), // sand
    ([0.45, 0.65, 0.20], 0.25), // bright grass
    ([0.55, 0.60, 0.25], 0.55), // dry grass
    ([0.55, 0.45, 0.30], 0.80), // dirt
    ([0.70, 0.60, 0.45], 1.00), // tan rock -- no snow
];

const BIOME_MOUNTAINOUS: BiomeGradient = &[
    ([0.04, 0.08, 0.20], 0.00), // dark water
    ([0.10, 0.30, 0.45], 0.10), // alpine lake
    ([0.45, 0.42, 0.40], 0.15), // scree
    ([0.35, 0.45, 0.20], 0.25), // sparse grass
    ([0.20, 0.35, 0.10], 0.40), // forest
    ([0.40, 0.32, 0.25], 0.50), // dirt
    ([0.45, 0.42, 0.38], 0.60), // rock
    ([0.60, 0.58, 0.55], 0.75), // light rock
    ([0.92, 0.94, 0.96], 0.85), // snow line -- early
    ([1.00, 1.00, 1.00], 1.00), // peak snow
];

const BIOME_TROPICAL: BiomeGradient = &[
    ([0.05, 0.40, 0.55], 0.00), // deep tropical water
    ([0.30, 0.70, 0.75], 0.15), // shallow turquoise
    ([0.95, 0.92, 0.80], 0.20), // white sand
    ([0.20, 0.55, 0.15], 0.30), // jungle
    ([0.10, 0.40, 0.10], 0.55), // dense jungle
    ([0.55, 0.30, 0.20], 0.75), // red dirt
    ([0.70, 0.45, 0.30], 1.00), // red rock -- no snow
];

const BIOME_DESERT: BiomeGradient = &[
    ([0.78, 0.72, 0.55], 0.00), // dry lakebed (no water)
    ([0.85, 0.75, 0.55], 0.20), // tan sand
    ([0.90, 0.78, 0.50], 0.40), // golden sand
    ([0.70, 0.45, 0.30], 0.60), // red dirt
    ([0.60, 0.35, 0.25], 0.75), // red rock
    ([0.45, 0.25, 0.20], 0.90), // dark red rock
    ([0.85, 0.75, 0.60], 1.00), // pale rock crown
];

const BIOME_TUNDRA: BiomeGradient = &[
    ([0.10, 0.15, 0.25], 0.00), // dark cold water
    ([0.30, 0.30, 0.30], 0.15), // frozen mud
    ([0.40, 0.45, 0.30], 0.25), // sparse moss
    ([0.45, 0.50, 0.40], 0.40), // grey-green tundra
    ([0.60, 0.62, 0.65], 0.55), // frost rock
    ([0.85, 0.88, 0.92], 0.70), // snow takes over early
    ([0.95, 0.97, 1.00], 1.00), // ice
];

const BIOME_LUNAR: BiomeGradient = &[
    ([0.10, 0.10, 0.10], 0.00), // crater shadow
    ([0.30, 0.30, 0.30], 0.30), // regolith
    ([0.55, 0.55, 0.55], 0.60), // light regolith
    ([0.75, 0.75, 0.75], 0.85), // highland
    ([0.90, 0.90, 0.90], 1.00), // peak
];

/// Resolve a biome name (from the AutoTexture `biome` param) to its
/// gradient table. Falls back to temperate for unknown values.
fn biome_gradient(name: &str) -> BiomeGradient {
    match name {
        "grassland" => BIOME_GRASSLAND,
        "mountainous" => BIOME_MOUNTAINOUS,
        "tropical" => BIOME_TROPICAL,
        "desert" => BIOME_DESERT,
        "tundra" => BIOME_TUNDRA,
        "lunar" => BIOME_LUNAR,
        _ => BIOME_TEMPERATE,
    }
}

fn sample_gradient(stops: &[([f32; 3], f32)], t: f32) -> [f32; 3] {
    if t <= stops[0].1 {
        return stops[0].0;
    }
    for i in 1..stops.len() {
        if t <= stops[i].1 {
            let frac = (t - stops[i - 1].1) / (stops[i].1 - stops[i - 1].1);
            let a = stops[i - 1].0;
            let b = stops[i].0;
            return [
                a[0] + (b[0] - a[0]) * frac,
                a[1] + (b[1] - a[1]) * frac,
                a[2] + (b[2] - a[2]) * frac,
            ];
        }
    }
    stops.last().unwrap().0
}

/// Gradient `t` at which a biome's land begins (the beach/scree stop just above
/// its water bands). `desert`/`lunar` have no water, so their land starts at 0.
fn biome_shoreline(biome: &str) -> f32 {
    match biome {
        "desert" | "lunar" => 0.0,
        "temperate" | "tropical" => 0.20,
        _ => 0.15, // grassland, mountainous, tundra
    }
}

/// Remap a normalized height so the biome's shoreline (`shore`) lands on the
/// actual waterline (`sea`, the normalized position of world height 0). Below
/// `sea` compresses into the gradient's water bands `[0, shore]`; above stretches
/// across the land bands `[shore, 1]`. `sea <= 0` => no water (all land).
fn remap_for_sea_level(h: f32, sea: f32, shore: f32) -> f32 {
    if sea <= 0.0 {
        return shore + h * (1.0 - shore);
    }
    if sea >= 1.0 {
        return (h / sea) * shore;
    }
    if h <= sea {
        (h / sea) * shore
    } else {
        shore + ((h - sea) / (1.0 - sea)) * (1.0 - shore)
    }
}

/// Generate a diffuse texture from a heightmap using elevation-banded
/// gradient mapping + slope-driven rock blending. Drives `AutoTexture`.
pub(crate) fn generate_auto_texture(
    heightmap: &Heightmap,
    slope: Option<&Heightmap>,
    params: &HashMap<String, ParamValue>,
) -> ColorBuffer {
    let w = heightmap.width();
    let h = heightmap.height();
    let mut color = ColorBuffer::new(w, h).unwrap();

    let slope_power = get_float(params, "slope_power", 0.7).max(0.01);
    let slope_blend_scale = get_float(params, "slope_blend", 1.0).clamp(0.0, 1.0);
    let ao_strength = get_float(params, "ao_strength", 1.0).clamp(0.0, 1.0);
    let detail_strength = get_float(params, "detail_strength", 0.15).clamp(0.0, 1.0);
    let rock_hex = get_string(params, "rock_color", "736B61");
    let rock_rgb = parse_hex_color_srgb(rock_hex).unwrap_or([0.45, 0.42, 0.38]);
    let biome = get_string(params, "biome", "temperate");
    let gradient = biome_gradient(biome);
    let sea_level = get_float(params, "sea_level", 0.0).clamp(0.0, 1.0);
    let shore = biome_shoreline(biome);

    for y in 0..h {
        for x in 0..w {
            let height_val = heightmap.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            let slope_val = slope
                .and_then(|s| s.get(x, y))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);

            let base_color =
                sample_gradient(gradient, remap_for_sea_level(height_val, sea_level, shore));

            let slope_blend = slope_val.powf(slope_power) * slope_blend_scale;
            let r = base_color[0] * (1.0 - slope_blend) + rock_rgb[0] * slope_blend;
            let g = base_color[1] * (1.0 - slope_blend) + rock_rgb[1] * slope_blend;
            let b = base_color[2] * (1.0 - slope_blend) + rock_rgb[2] * slope_blend;

            // Lerp AO toward 1.0 by (1 - ao_strength) so the param
            // smoothly fades the darkening rather than gating it.
            let ao_raw = compute_local_ao(heightmap, x, y);
            let ao = 1.0 - (1.0 - ao_raw) * ao_strength;

            // FBM micro-detail grain: same pattern as RockSoil/Vegetation.
            let ux = x as f32 / w as f32;
            let uy = y as f32 / h as f32;
            let detail = 1.0 + detail_strength * (micro_fbm(ux, uy, 8.0) * 2.0 - 1.0);

            color.set(
                x,
                y,
                [
                    (r * ao * detail).clamp(0.0, 1.0),
                    (g * ao * detail).clamp(0.0, 1.0),
                    (b * ao * detail).clamp(0.0, 1.0),
                    1.0,
                ],
            );
        }
    }

    color
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_data::Heightmap;
    use bar_graph::ParamValue;
    use std::collections::HashMap;

    fn ramp(n: u32) -> Heightmap {
        let mut d = vec![0.0f32; (n * n) as usize];
        for y in 0..n {
            for x in 0..n {
                d[(y * n + x) as usize] = x as f32 / (n - 1) as f32;
            }
        }
        Heightmap::frbar_data(n, n, d).unwrap()
    }

    fn params(sea: f32) -> HashMap<String, ParamValue> {
        HashMap::from([
            ("biome".to_string(), ParamValue::String("temperate".into())),
            ("sea_level".to_string(), ParamValue::Float(sea)),
            ("slope_blend".to_string(), ParamValue::Float(0.0)),
        ])
    }

    #[test]
    fn sea_level_zero_paints_no_water() {
        let tex = generate_auto_texture(&ramp(32), None, &params(0.0));
        // Lowest terrain maps to the shoreline (sand) band, not blue water.
        let c = tex.get(0, 16).unwrap();
        assert!(
            c[0] > c[2],
            "sea_level=0 lowest should be land, not blue: {c:?}"
        );
    }

    #[test]
    fn water_only_below_sea_level() {
        let tex = generate_auto_texture(&ramp(64), None, &params(0.3));
        let below = tex.get(2, 32).unwrap(); // h ~0.03 < 0.3 -> water
        let above = tex.get(50, 32).unwrap(); // h ~0.79 > 0.3 -> land
        assert!(
            below[2] > below[0],
            "below sea_level should be blue: {below:?}"
        );
        assert!(
            above[0] > above[2],
            "above sea_level should be land: {above:?}"
        );
    }
}
