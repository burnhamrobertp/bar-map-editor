// SPDX-License-Identifier: GPL-3.0-or-later
//
// Fragment half of the BAR `map_grass_gl4` widget port. Concatenated
// with `map_grass_vs.wgsl` at pipeline-build time -- bindings,
// structs, and the `camera` uniform are shared between the two
// files.
//
// Behaviour mirrors `map_grass_gl4.frag.glsl` with the LOS / night-
// factor / fog terms dropped (gameplay state, not in BME's static
// preview).

struct FsIn {
    @location(0) uv: vec2<f32>,
    @location(1) world_xz: vec2<f32>,
    @location(2) fade: f32,
}

@fragment
fn fs_grass(in: FsIn) -> @location(0) vec4<f32> {
    var color = textureSample(blade_color_tex, blade_color_sam, in.uv);
    // Engine `ALPHATHRESHOLD` cull (`map_grass_gl4.frag.glsl:69`).
    // Without this, alpha-test artefacts ride the blade silhouette
    // and the blades read as soft fuzz instead of crisp shapes.
    if color.a < 0.01 {
        discard;
    }
    color.a = color.a * in.fade;
    return color;
}
