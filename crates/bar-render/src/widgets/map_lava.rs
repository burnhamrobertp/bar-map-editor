//! Lava widget: port of bar-game's `luarules/gadgets/map_lava.lua`
//! plus `shaders/GLSL/lava/lava.frag.glsl`. In BAR the gadget calls
//! `Spring.SetDrawWater(false)` and renders its own plane; in BME
//! the water plane geometry is reused and the fragment shader
//! branches into `shade_map_lava` (in
//! `shaders/widgets/map_lava.wgsl`) whenever the lava flag is set
//! on `water_params.fresnel.w`.
//!
//! This module owns the three texture uploads. There is no
//! per-frame state and no editable config yet -- the shader hard-
//! codes BAR's defaults from `bar-game/modules/lava.lua`. When the
//! editor grows lava-config UI, add a uniform here and thread it
//! through the same bind group as the water params.
//!
//! Texture sources are CC0 originals from
//! `bar-game/luaui/images/lava/`:
//! - `lava2_diffuseemit.dds` (RGB = base colour, A = emission /
//!   heat mask).
//! - `lava2_normalheight.dds` (RGB = tangent normal, A = parallax
//!   height; we ignore A since parallax is out of scope).
//! - `lavadistortion.dds` (noise field for the heat-haze warp).
//!
//! Bundled in `assets/widgets/lava/` and loaded via
//! `include_bytes!` so the textures are always available regardless
//! of whether the user has a BAR install detected.

/// Decode the three bundled lava DDS files and upload them as
/// `Rgba8Unorm` 2D textures. Returns the textures themselves so the
/// caller can stash them on `TerrainRenderer` and re-create views as
/// needed at bind-group rebuild time.
pub fn upload_bundled_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::Texture, wgpu::Texture, wgpu::Texture) {
    const DIFFUSE_EMIT_DDS: &[u8] =
        include_bytes!("../../../../assets/widgets/lava/lava2_diffuseemit.dds");
    const NORMAL_HEIGHT_DDS: &[u8] =
        include_bytes!("../../../../assets/widgets/lava/lava2_normalheight.dds");
    const DISTORTION_DDS: &[u8] =
        include_bytes!("../../../../assets/widgets/lava/lavadistortion.dds");

    let diffuse_emit = decode_and_upload(device, queue, "lava_diffuse_emit", DIFFUSE_EMIT_DDS);
    let normal_height = decode_and_upload(device, queue, "lava_normal_height", NORMAL_HEIGHT_DDS);
    let distortion = decode_and_upload(device, queue, "lava_distortion", DISTORTION_DDS);
    (diffuse_emit, normal_height, distortion)
}

fn decode_and_upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    dds_bytes: &[u8],
) -> wgpu::Texture {
    // Bundled assets always decode; if they don't, the build is
    // broken in a way that's worth panicking on rather than silently
    // falling back to a black 1x1.
    let (rgba, w, h) = bar_data::load_dds_2d_bytes(dds_bytes)
        .unwrap_or_else(|e| panic!("bundled lava texture '{label}' failed to decode: {e}"));
    use wgpu::util::DeviceExt;
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &rgba,
    )
}
