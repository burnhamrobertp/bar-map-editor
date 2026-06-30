// FBM Noise Compute Shader
// Supports multiple fractal variants: FBM (Perlin/Simplex), Ridged, Billow.
//
// The base gradient noise (`gnoise`) is bit-for-bit identical to `gnoise` in
// bar-compute/src/noise.rs (the CPU path), so the GPU editor and the CPU
// export/CLI produce the same terrain.
//
// noise_type values:
//   0 = Standard FBM  (Perlin / Simplex)
//   1 = Ridged multi-fractal
//   2 = Billow (absolute-value FBM)
//   3 = Worley — not supported on GPU, falls back to CPU

struct NoiseParams {
    width: u32,
    height: u32,
    octaves: u32,
    seed: u32,
    frequency: f32,
    lacunarity: f32,
    persistence: f32,
    offset_x: f32,
    offset_y: f32,
    noise_type: u32,  // selects the fractal variant (see above)
    steepness: f32,   // contrast about midpoint (0.5 = no-op)
    elevation: f32,   // output bias (0.5 = no-op)
    offset: f32,      // additive offset (0.0 = no-op)
    gain: f32,        // contrast about 0.5 (0.5 = no-op)
    _padding2: f32,
    _padding3: f32,
}

@group(0) @binding(0) var<uniform> params: NoiseParams;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

// PCG-style integer lattice hash. Identical to `hash_cell` in noise.rs; PCG is
// the standard portable WGSL hash, so u32 `*` wraps like Rust's wrapping_mul.
fn hash_cell(ix: i32, iy: i32, seed: u32) -> u32 {
    var n: u32 = u32(ix) * 1597334677u + u32(iy) * 3812015801u + seed * 2654435761u;
    n = n * 747796405u + 2891336453u;
    let word = ((n >> ((n >> 28u) + 4u)) ^ n) * 277803737u;
    return (word >> 22u) ^ word;
}

// 8-direction gradient via switch -- avoids const-array runtime indexing, which
// some naga/SPIR-V backends miscompile to a fixed element. Order matches GRADS
// in noise.rs.
fn grad_at(ix: i32, iy: i32, seed: u32) -> vec2<f32> {
    let s = 0.70710677;
    switch (hash_cell(ix, iy, seed) & 7u) {
        case 0u: { return vec2<f32>(1.0, 0.0); }
        case 1u: { return vec2<f32>(-1.0, 0.0); }
        case 2u: { return vec2<f32>(0.0, 1.0); }
        case 3u: { return vec2<f32>(0.0, -1.0); }
        case 4u: { return vec2<f32>(s, s); }
        case 5u: { return vec2<f32>(-s, s); }
        case 6u: { return vec2<f32>(s, -s); }
        default: { return vec2<f32>(-s, -s); }
    }
}

fn quintic(t: f32) -> f32 {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// Unified gradient (Perlin-style) noise in ~[-1, 1]. Identical to `gnoise`
// in noise.rs.
fn gnoise(px: f32, py: f32, seed: u32) -> f32 {
    let x0 = floor(px);
    let y0 = floor(py);
    let ix = i32(x0);
    let iy = i32(y0);
    let fx = px - x0;
    let fy = py - y0;

    let g00 = grad_at(ix, iy, seed);
    let g10 = grad_at(ix + 1, iy, seed);
    let g01 = grad_at(ix, iy + 1, seed);
    let g11 = grad_at(ix + 1, iy + 1, seed);

    let d00 = g00.x * fx + g00.y * fy;
    let d10 = g10.x * (fx - 1.0) + g10.y * fy;
    let d01 = g01.x * fx + g01.y * (fy - 1.0);
    let d11 = g11.x * (fx - 1.0) + g11.y * (fy - 1.0);

    let u = quintic(fx);
    let v = quintic(fy);
    let x0m = d00 + u * (d10 - d00);
    let x1m = d01 + u * (d11 - d01);
    return (x0m + v * (x1m - x0m)) * 1.4142135;
}

// Standard FBM — fractal Brownian motion.
fn fbm(p: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 1.0;
    var frequency = params.frequency;
    var max_amplitude = 0.0;

    for (var i = 0u; i < params.octaves; i = i + 1u) {
        value = value + gnoise(p.x * frequency, p.y * frequency, params.seed) * amplitude;
        max_amplitude = max_amplitude + amplitude;
        amplitude = amplitude * params.persistence;
        frequency = frequency * params.lacunarity;
    }

    return (value / max_amplitude + 1.0) * 0.5;
}

// Ridged multi-fractal — sharp ridges, mountain-like terrain.
fn ridged(p: vec2<f32>) -> f32 {
    let offset = 1.0;
    let gain = 2.0;

    var frequency = params.frequency;
    var amplitude = 1.0;
    var value = 0.0;
    var weight = 1.0;

    for (var i = 0u; i < params.octaves; i = i + 1u) {
        var signal = offset - abs(gnoise(p.x * frequency, p.y * frequency, params.seed));
        signal = signal * signal * weight;
        value = value + signal * amplitude;
        weight = clamp(signal * gain, 0.0, 1.0);
        frequency = frequency * params.lacunarity;
        amplitude = amplitude * params.persistence;
    }

    return clamp(value * 0.5, 0.0, 1.0);
}

// Billow noise — absolute-value FBM producing puffy, rolling shapes.
fn billow(p: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 1.0;
    var frequency = params.frequency;
    var max_amplitude = 0.0;

    for (var i = 0u; i < params.octaves; i = i + 1u) {
        value = value + abs(gnoise(p.x * frequency, p.y * frequency, params.seed)) * amplitude;
        max_amplitude = max_amplitude + amplitude;
        amplitude = amplitude * params.persistence;
        frequency = frequency * params.lacunarity;
    }

    return clamp(value / max_amplitude, 0.0, 1.0);
}

// WM-style output shaping. Identity at default params
// (steepness=0.5, elevation=0.5, offset=0.0, gain=0.5).
// Must stay bit-identical to `shape` in noise.rs (CPU path).
fn shape(v: f32) -> f32 {
    let t = (params.steepness - 0.5) * 2.0;
    let smoothed = v * v * (3.0 - 2.0 * v);
    let inv = 0.5 + (v - smoothed) + (v - 0.5);
    var shaped = smoothed;
    if (t < 0.0) {
        shaped = inv;
    }
    var result = v + (shaped - v) * abs(t);

    let g = params.gain * 2.0;
    result = 0.5 + (result - 0.5) * g;

    result = result + (params.elevation - 0.5) * 2.0;
    result = result + params.offset;

    return clamp(result, 0.0, 1.0);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    let uv = vec2<f32>(
        f32(x) / f32(params.width) + params.offset_x,
        f32(y) / f32(params.height) + params.offset_y
    );

    var value: f32;
    switch params.noise_type {
        case 1u: { value = ridged(uv); }
        case 2u: { value = billow(uv); }
        default: { value = fbm(uv); }   // 0 = FBM (default for Perlin/Simplex)
    }

    output[idx] = shape(clamp(value, 0.0, 1.0));
}
