// FBM Noise Compute Shader
// Supports multiple fractal variants: FBM (Perlin/Simplex), Ridged, Billow
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
    _padding2: f32,
    _padding3: f32,
}

@group(0) @binding(0) var<uniform> params: NoiseParams;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

// Permutation-based hash for noise
fn hash2(p: vec2<f32>) -> f32 {
    let k = vec2<f32>(0.3183099, 0.3678794);
    var pp = p * k + k.yx;
    pp = fract(pp);
    let dot_val = dot(pp, pp + 16.0);
    return fract(dot_val * dot_val * (pp.x + pp.y)) * 2.0 - 1.0;
}

// 2D gradient noise (Perlin-style)
fn gradient_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);

    // Cubic Hermite interpolation
    let u = f * f * (3.0 - 2.0 * f);

    let n00 = hash2(i + vec2<f32>(0.0, 0.0));
    let n10 = hash2(i + vec2<f32>(1.0, 0.0));
    let n01 = hash2(i + vec2<f32>(0.0, 1.0));
    let n11 = hash2(i + vec2<f32>(1.0, 1.0));

    let mix_x0 = mix(n00, n10, u.x);
    let mix_x1 = mix(n01, n11, u.x);
    return mix(mix_x0, mix_x1, u.y);
}

// Standard FBM — fractal Brownian motion
fn fbm(p: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 1.0;
    var frequency = params.frequency;
    var max_amplitude = 0.0;
    var pos = p;

    let seed_offset = vec2<f32>(f32(params.seed) * 12.9898, f32(params.seed) * 78.233);
    pos = pos + seed_offset;

    for (var i = 0u; i < params.octaves; i = i + 1u) {
        value = value + gradient_noise(pos * frequency) * amplitude;
        max_amplitude = max_amplitude + amplitude;
        amplitude = amplitude * params.persistence;
        frequency = frequency * params.lacunarity;
    }

    // Normalize to [0, 1]
    return (value / max_amplitude + 1.0) * 0.5;
}

// Ridged multi-fractal — produces sharp ridges, mountain-like terrain
fn ridged(p: vec2<f32>) -> f32 {
    let offset = 1.0;
    let gain = 2.0;

    var frequency = params.frequency;
    var amplitude = 1.0;
    var value = 0.0;
    var weight = 1.0;
    var pos = p;

    let seed_offset = vec2<f32>(f32(params.seed) * 12.9898, f32(params.seed) * 78.233);
    pos = pos + seed_offset;

    for (var i = 0u; i < params.octaves; i = i + 1u) {
        var signal = gradient_noise(pos * frequency);
        // Invert absolute value to create ridges
        signal = offset - abs(signal);
        signal = signal * signal;
        signal = signal * weight;
        value = value + signal * amplitude;
        weight = clamp(signal * gain, 0.0, 1.0);
        frequency = frequency * params.lacunarity;
        amplitude = amplitude * params.persistence;
    }

    return clamp(value * 0.5, 0.0, 1.0);
}

// Billow noise — absolute-value FBM producing puffy, rolling shapes
fn billow(p: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 1.0;
    var frequency = params.frequency;
    var max_amplitude = 0.0;
    var pos = p;

    let seed_offset = vec2<f32>(f32(params.seed) * 12.9898, f32(params.seed) * 78.233);
    pos = pos + seed_offset;

    for (var i = 0u; i < params.octaves; i = i + 1u) {
        // Absolute value creates puffy shapes instead of smooth FBM
        value = value + abs(gradient_noise(pos * frequency)) * amplitude;
        max_amplitude = max_amplitude + amplitude;
        amplitude = amplitude * params.persistence;
        frequency = frequency * params.lacunarity;
    }

    return clamp(value / max_amplitude, 0.0, 1.0);
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

    output[idx] = clamp(value, 0.0, 1.0);
}
