// Lightmap bake compute shader.
//
// Horizon-based ambient occlusion + soft sun shadow from a heightfield.
// Must match the CPU path in crates/bar-compute/src/lightmap.rs.
//
// Output channels (per texel, RGBA): R = AO, G = sun visibility,
// B = AO*sun, A = 1.0.
//
// Heights are normalized [0,1]; XY is pixel units. A normalized unit of height
// maps to max(width,height) pixels of rise (`scale`) so angles are meaningful.
//
// std140 layout: keep field order in sync with GpuLightmapParams (Rust).
// vec3 sun_dir is padded to a vec4 (sun_dir.w is unused).

struct LightmapParams {
    width: u32,
    height: u32,
    num_directions: u32,
    max_steps: u32,
    ao_strength: f32,
    ao_radius: f32,
    sun_softness: f32,
    _pad0: f32,
    sun_dir: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: LightmapParams;
@group(0) @binding(1) var<storage, read> heights: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<f32>;

const TAU: f32 = 6.28318530718;

fn clampi(v: i32, lo: i32, hi: i32) -> i32 {
    return max(lo, min(hi, v));
}

// World height (pixel units) at integer texel, clamped to edges.
fn height_at(x: i32, y: i32, w: i32, h: i32, scale: f32) -> f32 {
    let cx = clampi(x, 0, w - 1);
    let cy = clampi(y, 0, h - 1);
    let idx = cy * w + cx;
    return heights[idx] * scale;
}

// Bilinearly sampled world height at fractional (px, py).
fn sample_height(px: f32, py: f32, w: i32, h: i32, scale: f32) -> f32 {
    let x0 = i32(floor(px));
    let y0 = i32(floor(py));
    let fx = px - f32(x0);
    let fy = py - f32(y0);

    let h00 = height_at(x0, y0, w, h, scale);
    let h10 = height_at(x0 + 1, y0, w, h, scale);
    let h01 = height_at(x0, y0 + 1, w, h, scale);
    let h11 = height_at(x0 + 1, y0 + 1, w, h, scale);

    let top = h00 * (1.0 - fx) + h10 * fx;
    let bot = h01 * (1.0 - fx) + h11 * fx;
    return top * (1.0 - fy) + bot * fy;
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if (x >= params.width || y >= params.height) {
        return;
    }

    let w = i32(params.width);
    let h = i32(params.height);
    let max_dim = f32(max(params.width, params.height));
    let scale = max_dim;

    let px = f32(x);
    let py = f32(y);
    let origin_h = height_at(i32(x), i32(y), w, h, scale);

    // ----- Horizon-based ambient occlusion -----
    let steps = max(params.max_steps, 1u);
    let dirs = max(params.num_directions, 1u);
    let radius_px = max(params.ao_radius * max_dim, 1.0);
    let step_len = max(radius_px / f32(steps), 1.0);

    var horizon_sum = 0.0;
    for (var d = 0u; d < dirs; d = d + 1u) {
        let az = (f32(d) / f32(dirs)) * TAU;
        let dx = cos(az);
        let dy = sin(az);

        var max_sin = 0.0;
        for (var s = 1u; s <= steps; s = s + 1u) {
            let dist = f32(s) * step_len;
            let sh = sample_height(px + dx * dist, py + dy * dist, w, h, scale);
            let dh = sh - origin_h;
            if (dh > 0.0) {
                let sin_ang = dh / sqrt(dh * dh + dist * dist);
                max_sin = max(max_sin, sin_ang);
            }
        }
        horizon_sum = horizon_sum + max_sin;
    }
    let avg_horizon = horizon_sum / f32(dirs);
    let ao = clamp(1.0 - avg_horizon * params.ao_strength, 0.0, 1.0);

    // ----- Sun visibility (soft shadow) -----
    var sun = 1.0;
    let sxy = sqrt(params.sun_dir.x * params.sun_dir.x + params.sun_dir.y * params.sun_dir.y);
    if (sxy >= 1e-4) {
        let dir_x = params.sun_dir.x / sxy;
        let dir_y = params.sun_dir.y / sxy;
        let rise_per_px = params.sun_dir.z / sxy;

        let reach_px = max(params.ao_radius * max_dim, 1.0);
        let sun_step = max(reach_px / f32(steps), 1.0);
        let soft = clamp(params.sun_softness, 0.0, 1.0);

        var min_clear = 1.0;
        var blocked = false;
        for (var s = 1u; s <= steps; s = s + 1u) {
            let dist = f32(s) * sun_step;
            let terrain = sample_height(px + dir_x * dist, py + dir_y * dist, w, h, scale);
            let ray = origin_h + rise_per_px * dist;
            if (terrain >= ray) {
                blocked = true;
            }
            let gap = ray - terrain;
            let band = max(soft * dist, 1e-3);
            let clear = clamp(gap / band, 0.0, 1.0);
            min_clear = min(min_clear, clear);
        }
        if (blocked) {
            sun = 0.0;
        } else {
            sun = min_clear;
        }
    }

    let idx = (y * params.width + x) * 4u;
    result[idx] = ao;
    result[idx + 1u] = sun;
    result[idx + 2u] = ao * sun;
    result[idx + 3u] = 1.0;
}
