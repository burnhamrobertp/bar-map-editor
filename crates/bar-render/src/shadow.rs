// SPDX-License-Identifier: GPL-2.0-or-later
//! Shadow mapping for terrain and features.
//!
//! Single directional light. One `Depth32Float` texture rendered from the
//! sun's POV by the caster pipelines (terrain heightmap displacement + feature
//! instances), then sampled by the terrain and feature fragment shaders to
//! compute a per-fragment `shadow_coeff` in `[0, 1]`.
//!
//! Light camera: orthographic, axis-aligned to the sun direction, sized to
//! contain the map's bounding sphere. Recomputed each frame because the user
//! can scrub the sun direction at any time and because the map dimensions
//! change on resize.
//!
//! Group layouts produced here are stable -- the same `caster_bgl` is
//! attached to both terrain and feature caster pipelines, and the same
//! `receiver_bgl` is attached to both terrain (main pass) and feature
//! receiver pipelines.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

/// Shadow map resolution. 4096x4096 (64MB Depth32Float) -- the bounding-sphere
/// frustum spans the whole map, so per-texel coverage is `map_diagonal /
/// SHADOW_RES`. At 2048 a typical 16-km map gave ~8m per shadow texel, which
/// engulfed small tree/crystal features in PCF softness and made their
/// shadows read as detached blobs. 4096 halves that to ~4m per texel. A
/// cascade / camera-focused frustum is the proper fix; bumping resolution
/// is the cheap intermediate.
const SHADOW_RES: u32 = 4096;

/// Padding factor applied to the orthographic bounds: the light camera fits
/// the map's bounding sphere multiplied by this factor, so features placed
/// slightly outside the heightmap (or tall trees) are not clipped.
const FRUSTUM_PAD: f32 = 1.15;

/// Uniform buffer payload shared by caster (light_view_proj) and receivers
/// (light_view_proj + sun_dir for lighting consistency).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct ShadowUniform {
    pub light_view_proj: [[f32; 4]; 4],
    pub sun_dir: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<ShadowUniform>() == 80);

/// Owns the shadow depth texture, the light_view_proj uniform buffer, and the
/// bind groups for casters and receivers. Created once at renderer init;
/// `update_light` recomputes the uniform without recreating GPU resources.
pub struct ShadowMap {
    // Kept alive for the lifetime of the receiver bind group; not read directly.
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    // Kept alive for the lifetime of the receiver bind group.
    #[allow(dead_code)]
    sampler: wgpu::Sampler,

    /// Bind group layout for caster pipelines (terrain + feature shadow VS):
    /// binding 0 = shadow uniform.
    pub caster_bgl: wgpu::BindGroupLayout,
    caster_bg: wgpu::BindGroup,

    /// Bind group layout for receivers (terrain + feature main FS):
    /// binding 0 = shadow uniform, binding 1 = shadow texture, binding 2 = sampler.
    pub receiver_bgl: wgpu::BindGroupLayout,
    receiver_bg: wgpu::BindGroup,

    uniform_buffer: wgpu::Buffer,
}

impl ShadowMap {
    /// Format used for the depth attachment. Exposed so caster pipelines can
    /// declare their depth-stencil state consistently.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    /// Render-target size for both axes.
    pub const RESOLUTION: u32 = SHADOW_RES;

    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow_depth"),
            size: wgpu::Extent3d {
                width: SHADOW_RES,
                height: SHADOW_RES,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Comparison sampler. `textureSampleCompare` in WGSL does the depth
        // compare per fetched texel, then the sampler's Linear filtering
        // bilinearly weights the four neighboring comparison results. The
        // upshot is hardware 2x2 PCF: softer than single-tap but tight to
        // the silhouette edge, with no manual kernel needed. `LessEqual`
        // means "receiver depth <= stored caster depth => lit".
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow_uniform"),
            size: std::mem::size_of::<ShadowUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Caster: just the uniform (binding 0). Both VS and FS stages can read
        // it; binding visibility is VERTEX | FRAGMENT.
        let caster_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_caster_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let caster_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_caster_bg"),
            layout: &caster_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Receiver: uniform + texture + sampler. Same layout for both terrain
        // and feature main passes.
        let receiver_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_receiver_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    // Comparison sampler; pairs with `textureSampleCompare` in
                    // the receiver shaders. Hardware does bilinear PCF over
                    // the 2x2 neighborhood of the sampled UV.
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let receiver_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_receiver_bg"),
            layout: &receiver_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            texture,
            view,
            sampler,
            caster_bgl,
            caster_bg,
            receiver_bgl,
            receiver_bg,
            uniform_buffer,
        }
    }

    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub fn caster_bind_group(&self) -> &wgpu::BindGroup {
        &self.caster_bg
    }

    pub fn receiver_bind_group(&self) -> &wgpu::BindGroup {
        &self.receiver_bg
    }

    /// Recompute the light view-projection from the current sun direction and
    /// scene bounds, then upload it to the uniform buffer.
    ///
    /// - `sun_dir`: the *un*-normalised sun direction from `SmfLighting`. Must
    ///   point *towards* the sun (matching the SMF convention).
    /// - `(x_extent, z_extent, height_scale)`: the render-space half-spans of
    ///   the terrain mesh and the y-axis scaling for the heightmap.
    pub fn update_light(
        &self,
        queue: &wgpu::Queue,
        sun_dir: [f32; 3],
        x_extent: f32,
        z_extent: f32,
        height_scale: f32,
    ) {
        let sun = Vec3::from(sun_dir).normalize_or(Vec3::Y);

        // Bounding sphere of the terrain AABB in render space.
        // AABB: x in [-x_extent, +x_extent], y in [0, height_scale],
        //       z in [-z_extent, +z_extent].
        let center = Vec3::new(0.0, height_scale * 0.5, 0.0);
        let radius =
            (x_extent * x_extent + z_extent * z_extent + (height_scale * 0.5).powi(2)).sqrt();
        let r = radius * FRUSTUM_PAD;

        // Position the light far enough along the sun direction that the
        // whole sphere is in front of it (positive z in light space).
        let light_eye = center + sun * (r * 2.0);
        let up = if sun.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let view = Mat4::look_at_rh(light_eye, center, up);
        let proj = Mat4::orthographic_rh(-r, r, -r, r, 0.0, r * 4.0);
        let light_vp = proj * view;

        let uniform = ShadowUniform {
            light_view_proj: light_vp.to_cols_array_2d(),
            sun_dir: [sun.x, sun.y, sun.z, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
    }
}
