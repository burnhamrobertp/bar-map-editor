// Gamma-encode post-process pass.
//
// BME mirrors BAR's gamma-incorrect pipeline: every shader multiplication
// runs in sRGB-perceptual space and writes raw perceptual bytes to a
// non-sRGB framebuffer. On a native sRGB display BAR's final light
// intensity is therefore byte/255 raised to the display gamma (~2.2).
//
// In BME, eframe composites our render target onto an sRGB swapchain,
// which would otherwise re-encode our perceptual bytes such that the
// display intensity ends up at V instead of V^2.2 -- a brighter, more
// saturated image that doesn't match the engine. This pass samples the
// final composited render target and writes pow(V, 2.2) into a separate
// display texture. egui samples that texture, the sRGB swapchain
// encoding cancels back to the raw perceptual byte, and the display
// gamma decodes to V^2.2, matching BAR's in-game appearance.
//
// Cross-pass intermediates (refraction / reflection textures) stay in
// perceptual space because they are sampled by other shaders that
// expect BAR's perceptual values; only the final swapchain-bound copy
// is gamma-encoded.

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_gamma(@builtin(vertex_index) idx: u32) -> VsOut {
    // Fullscreen triangle: vertex indices 0/1/2 map to clip-space
    // (-1,-1) / (3,-1) / (-1,3), which fully covers [-1,1]^2 with one
    // primitive (no vertex buffer needed).
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    var out: VsOut;
    out.clip = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    // Flip V because wgpu's texture origin is top-left while the
    // fullscreen triangle math above puts (0,0) at clip-space (-1,-1)
    // which is the bottom-left.
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

// Exponent is fed in via a uniform so it can be tuned at runtime via
// the viewport debug overlay's gamma slider. 1.0 = no correction
// (BAR's perceptual pixels straight through, visibly too bright on
// eframe's sRGB swapchain); 2.2 = full display-gamma decode (matches
// BAR exactly if the swapchain were a pure sRGB re-encode, but
// overshoots dark because egui_wgpu's compose pipeline only does
// partial gamma handling). Empirical sweet spot lives somewhere in
// between -- pick the value visually against an in-engine reference
// screenshot, then hardcode it once we converge.
// 16-byte struct (matches the four-f32 buffer write in renderer.rs).
// Padding is scalar rather than vec3 because vec3 in WGSL uniform
// memory aligns/pads to 16 bytes by itself, which would bloat the
// struct to 32 and trip wgpu's binding-size validation.
struct GammaParams {
    exponent: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};
@group(0) @binding(2) var<uniform> gamma_params: GammaParams;

@fragment
fn fs_gamma(in: VsOut) -> @location(0) vec4<f32> {
    let col = textureSample(src_tex, src_samp, in.uv).rgb;
    let encoded = pow(max(col, vec3<f32>(0.0)), vec3<f32>(gamma_params.exponent));
    return vec4<f32>(encoded, 1.0);
}
