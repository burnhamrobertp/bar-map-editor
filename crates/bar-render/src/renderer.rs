use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::terrain::{generate_terrain_mesh, generate_terrain_mesh_lod, TerrainVertex};
use bar_data::{ColorBuffer, Heightmap};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    /// Inverse view-projection — used by the skybox vertex shader to
    /// convert clip-space NDC into a world-space view direction.
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 3],
    has_texture: u32,
    height_scale: f32,
    water_r: f32,
    water_g: f32,
    water_b: f32,
    water_y: f32,
    /// Elapsed seconds since the renderer started, drives wave animation.
    time: f32,
    /// 0.0 = low-pass (fast); 1.0 = high-pass (sky reflections + fog +
    /// skybox background).
    quality: f32,
    /// 1.0 = the water plane should be rendered with `discard` so the
    /// reflection-pass output doesn't include it. Set during the planar
    /// reflection pass; otherwise 0.0.
    skip_water: f32,
    /// Output framebuffer size in pixels — used to convert
    /// `@builtin(position)` to a [0,1] screen UV when sampling the
    /// planar-reflection texture in the water shader.
    screen_w: f32,
    screen_h: f32,
    _pad: [f32; 2],
    // ── SMF ground shading (Recoil-port) ───────────────────────────────
    // Sourced from `MapSettings.lighting` and `MapSettings.water`. All
    // vec3 slots use the `[f32; 4]` layout so std140 alignment is
    // explicit; the trailing component is zero-pad unless documented.
    /// `sun_dir.xyz`, plus `ground_specular_exponent` in the .w slot.
    sun_dir_exp: [f32; 4],
    /// `groundAmbientColor` (rgb), pad in .a.
    ground_ambient: [f32; 4],
    /// `groundDiffuseColor` (rgb), pad in .a.
    ground_diffuse: [f32; 4],
    /// `groundSpecularColor` (rgb), pad in .a.
    ground_specular: [f32; 4],
    /// `waterAbsorbColor` (rgb), pad in .a.
    water_absorb: [f32; 4],
    /// `waterBaseColor` (rgb), pad in .a.
    water_base_color: [f32; 4],
    /// `waterMinColor` (rgb), pad in .a.
    water_min_color: [f32; 4],
    /// Brush cursor in render-space: xy = world XZ, z = radius in
    /// world units, w = 1.0 when active / 0.0 when no ring should
    /// render. Drawn as a translucent ring on the terrain so the
    /// user can see where the brush will stamp.
    brush_cursor: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<CameraUniform>() == 320);

/// Everything the renderer needs to draw one frame. Passed to
/// [`TerrainRenderer::render`]; `None` means "show nothing", `Some`
/// fully describes the scene.
///
/// The renderer caches uploaded GPU resources (mesh + albedo) keyed
/// by [`PreviewFrame::revision`]. Same revision across calls = no
/// re-upload. Different revision = re-upload from the new frame.
/// Camera and time changes don't trigger re-upload.
///
/// Lifetime is the heightmap / texture borrow — usually the eval
/// result the consumer is currently holding. The renderer copies
/// what it needs into GPU resources during upload.
pub struct PreviewFrame<'a> {
    /// Identity of the frame data. Bumping this signals to the
    /// renderer that mesh + texture must be re-uploaded.
    pub revision: u64,
    pub heightmap: &'a Heightmap,
    pub texture: Option<&'a ColorBuffer>,
    pub height_scale: f32,
    pub x_extent: f32,
    pub z_extent: f32,
    /// Render-space Y of the water plane. Negative → no water.
    pub water_y: f32,
    pub water_color: [f32; 3],
    /// Mesh LOD cap. Smaller = chunkier preview.
    pub max_grid_size: u32,
    /// True for the high-pass render (sky reflections + fog).
    pub quality_high: bool,
    /// Animation time for water waves.
    pub time: f32,
    /// SMF lighting + water-absorption inputs sourced from
    /// `MapSettings.lighting` / `MapSettings.water`. Drives the
    /// Recoil-faithful ground shading path in `smf_ground.wgsl`.
    pub smf_lighting: SmfLighting,
}

/// Engine-faithful SMF shading inputs. The values mirror the engine's
/// `groundAmbientColor` / `groundDiffuseColor` / `groundSpecularColor`
/// / `groundSpecularExponent` / `lightDir` and the `waterAbsorbColor`
/// / `waterBaseColor` / `waterMinColor` block. Defaults match
/// `bar-project::recipe::LightingSettings::default()` and
/// `WaterSettings::default()` so an unconfigured map renders cleanly.
#[derive(Clone, Copy, Debug)]
pub struct SmfLighting {
    pub sun_dir: [f32; 3],
    pub ground_ambient: [f32; 3],
    pub ground_diffuse: [f32; 3],
    pub ground_specular: [f32; 3],
    pub specular_exponent: f32,
    pub water_absorb: [f32; 3],
    pub water_base: [f32; 3],
    pub water_min: [f32; 3],
}

impl Default for SmfLighting {
    fn default() -> Self {
        Self {
            sun_dir: [0.0, 1.0, 2.0],
            ground_ambient: [0.5, 0.5, 0.5],
            ground_diffuse: [0.5, 0.5, 0.5],
            ground_specular: [0.1, 0.1, 0.1],
            specular_exponent: 10.0,
            water_absorb: [0.0, 0.0, 0.0],
            water_base: [0.0, 0.0, 0.0],
            water_min: [0.0, 0.0, 0.0],
        }
    }
}

impl SmfLighting {
    /// Pack into the GPU-aligned `[f32; 4]` slots used by
    /// `CameraUniform`. Sun direction is normalised here so the
    /// shader can skip the renormalise step every fragment.
    fn to_uniform_slots(&self) -> SmfUniformSlots {
        let s = self.sun_dir;
        let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt().max(1e-4);
        let s = [s[0] / len, s[1] / len, s[2] / len];
        SmfUniformSlots {
            sun_dir_exp: [s[0], s[1], s[2], self.specular_exponent.max(1.0)],
            ground_ambient: [self.ground_ambient[0], self.ground_ambient[1], self.ground_ambient[2], 0.0],
            ground_diffuse: [self.ground_diffuse[0], self.ground_diffuse[1], self.ground_diffuse[2], 0.0],
            ground_specular: [self.ground_specular[0], self.ground_specular[1], self.ground_specular[2], 0.0],
            water_absorb: [self.water_absorb[0], self.water_absorb[1], self.water_absorb[2], 0.0],
            water_base_color: [self.water_base[0], self.water_base[1], self.water_base[2], 0.0],
            water_min_color: [self.water_min[0], self.water_min[1], self.water_min[2], 0.0],
        }
    }
}

struct SmfUniformSlots {
    sun_dir_exp: [f32; 4],
    ground_ambient: [f32; 4],
    ground_diffuse: [f32; 4],
    ground_specular: [f32; 4],
    water_absorb: [f32; 4],
    water_base_color: [f32; 4],
    water_min_color: [f32; 4],
}

/// The terrain rendering pipeline.
pub struct TerrainRenderer {
    render_pipeline: wgpu::RenderPipeline,
    /// Skybox pipeline — drawn after terrain in the same render pass at
    /// the far plane. Active only when `quality_high` is true.
    sky_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    texture_bind_group: wgpu::BindGroup,
    albedo_texture: wgpu::Texture,
    albedo_sampler: wgpu::Sampler,
    has_texture: bool,
    height_scale: f32,
    water_y: f32,
    water_color: [f32; 3],
    /// SMF lighting cached from the most recent frame so the per-frame
    /// camera-uniform write (and the reflection pre-pass write) both
    /// have the same values.
    smf_lighting: SmfLighting,
    /// Brush cursor in render-space — `(world_x, world_z, radius)`,
    /// where radius is in the same world units as the mesh extents.
    /// `None` means no ring is drawn. Set per-frame from the GUI's
    /// pick result; the shader paints a translucent ring at the
    /// cursor's surface footprint so the user can see where the
    /// brush will stamp before they click.
    brush_cursor: Option<(f32, f32, f32)>,
    x_extent: f32,
    z_extent: f32,
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    depth_texture: Option<wgpu::TextureView>,
    depth_format: wgpu::TextureFormat,
    output_texture: Option<wgpu::Texture>,
    output_view: Option<wgpu::TextureView>,
    /// Reflection-pass colour target — receives the scene rendered with
    /// the camera mirrored through the water plane. Sampled by the water
    /// shader as the actual planar reflection.
    reflection_texture: Option<wgpu::Texture>,
    reflection_view: Option<wgpu::TextureView>,
    /// Reflection-pass depth target. Separate from the main one because
    /// the two passes have completely different geometry visibility.
    reflection_depth_view: Option<wgpu::TextureView>,
    /// Bind group containing the reflection texture + sampler — bound to
    /// `@group(2)` in the water shader. Two variants: the "real" one
    /// points at the planar-reflection texture; the "dummy" one points
    /// at a 1×1 default and is bound during the reflection pre-pass
    /// itself (a texture cannot be both a render target and a sample
    /// source within a single render pass).
    reflection_bind_group_layout: wgpu::BindGroupLayout,
    reflection_sampler: wgpu::Sampler,
    reflection_bind_group: wgpu::BindGroup,
    reflection_bind_group_dummy: wgpu::BindGroup,
    water_normal_bind_group_layout: wgpu::BindGroupLayout,
    water_normal_bind_group: wgpu::BindGroup,
    water_normal_texture: wgpu::Texture,
    water_normal_sampler: wgpu::Sampler,
    pub width: u32,
    pub height: u32,
    /// Elapsed seconds, fed each frame for water wave animation.
    time: f32,
    /// True for the high-pass render (full sky reflection + fog); false for
    /// low-pass (cheap, static, no atmospherics).
    quality_high: bool,
    /// Revision of the most-recently-uploaded mesh/texture. The
    /// renderer re-uploads from the next frame only when this
    /// differs from the new frame's revision. `None` means "no frame
    /// is currently held" — the next render() will clear the
    /// viewport and the next non-empty frame will force an upload.
    last_uploaded_revision: Option<u64>,
}

impl TerrainRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, output_format: wgpu::TextureFormat) -> Self {
        // We assemble the WGSL module from two files: a vendored Recoil
        // shader port (modern sky) and the OM-specific terrain shader.
        // WGSL has no preprocessor #include; concatenating the strings
        // before compilation gives the same effect. The sky port lives
        // in `shaders/recoil/` because it carries upstream's GPL terms;
        // see docs/licensing.md.
        let modern_sky_source = include_str!("../../../shaders/recoil/modern_sky.wgsl");
        let smf_ground_source = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let smf_water_source = include_str!("../../../shaders/recoil/smf_water.wgsl");
        let terrain_source = include_str!("../../../shaders/terrain.wgsl");
        let shader_source = format!(
            "{modern_sky_source}\n{smf_ground_source}\n{smf_water_source}\n{terrain_source}"
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // Group 0: camera uniform
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("camera_bind_group_layout"),
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

        // Group 1: albedo texture + sampler
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Group 2: planar-reflection texture + sampler. Sampled by the
        // water shader via screen UV to show actual reflected geometry
        // (mountains, sky) instead of a procedural fake.
        let reflection_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("reflection_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Group 3: water normal-map texture + sampler.  Used by `smf_water.wgsl`
        // to compute per-fragment surface normals for the BumpWater port.
        let water_normal_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("water_normal_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain_pipeline_layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &texture_bind_group_layout,
                &reflection_bind_group_layout,
                &water_normal_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });

        let depth_format = wgpu::TextureFormat::Depth32Float;

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TerrainVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    // Standard alpha blending so the water plane can be
                    // rendered semi-transparent over the underwater terrain
                    // beneath it. Opaque terrain (alpha=1.0) is unaffected
                    // by this blend mode and behaves as if REPLACE were set.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Skybox pipeline — fullscreen triangle at depth 1.0, no vertex
        // buffer. Reuses the camera bind group; doesn't need the texture
        // bind group, but the pipeline_layout includes both so we just
        // pass through whatever's bound.
        let sky_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky_pipeline_layout"),
                bind_group_layouts: &[&camera_bind_group_layout],
                push_constant_ranges: &[],
            });
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky_pipeline"),
            layout: Some(&sky_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            // Depth: LessEqual so sky (z=1.0) draws only where nothing
            // closer has been drawn. Don't write depth — we don't want
            // the sky to occlude anything that comes after.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let smf = SmfLighting::default().to_uniform_slots();
        let camera_uniform = CameraUniform {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            inv_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            camera_pos: [0.0, 0.0, 0.0],
            has_texture: 0,
            height_scale: 0.3,
            water_r: 0.2,
            water_g: 0.4,
            water_b: 0.7,
            water_y: -1.0,
            time: 0.0,
            quality: 1.0,
            skip_water: 0.0,
            screen_w: 512.0,
            screen_h: 512.0,
            _pad: [0.0; 2],
            sun_dir_exp: smf.sun_dir_exp,
            ground_ambient: smf.ground_ambient,
            ground_diffuse: smf.ground_diffuse,
            ground_specular: smf.ground_specular,
            water_absorb: smf.water_absorb,
            water_base_color: smf.water_base_color,
            water_min_color: smf.water_min_color,
            // No active brush cursor at construction time.
            brush_cursor: [0.0, 0.0, 0.0, 0.0],
        };

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera_buffer"),
            contents: bytemuck::bytes_of(&camera_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // Create sampler (shared across all albedo textures)
        let albedo_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("albedo_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Default 1×1 white texture used until a real albedo is uploaded
        let white: [u8; 4] = [255, 255, 255, 255];
        let albedo_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("albedo_default"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &white,
        );

        let albedo_view = albedo_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_bind_group"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&albedo_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&albedo_sampler),
                },
            ],
        });

        // Reflection sampler — linear filtering so the planar reflection
        // doesn't pixelate when stretched across the water surface.
        let reflection_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("reflection_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Default 1×1 sky-colour reflection until a real reflection pass
        // has rendered. This way the very first frame still has a sensible
        // value to sample (procedural-sky-ish blue) instead of garbage.
        let sky_default: [u8; 4] = [180, 200, 230, 255];
        let reflection_default_tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("reflection_default"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &sky_default,
        );
        let reflection_default_view =
            reflection_default_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let make_reflection_bg = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("reflection_bind_group"),
                layout: &reflection_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&reflection_sampler),
                    },
                ],
            })
        };
        let reflection_bind_group = make_reflection_bg(&reflection_default_view);
        let reflection_bind_group_dummy = make_reflection_bg(&reflection_default_view);

        // Group 3: water normal map stub — 1×1 flat-normal texel [127,127,255,255]
        // decodes to vec3(0,0,1) (perfectly flat). Replaced when a real normal-map
        // asset lands; the stub exercises the full bind-group path immediately.
        let water_normal_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water_normal_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let flat_normal: [u8; 4] = [127, 127, 255, 255];
        let water_normal_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("water_normal_stub"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &flat_normal,
        );
        let water_normal_view =
            water_normal_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let water_normal_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water_normal_bind_group"),
            layout: &water_normal_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&water_normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&water_normal_sampler),
                },
            ],
        });

        Self {
            render_pipeline,
            sky_pipeline,
            camera_buffer,
            camera_bind_group,
            texture_bind_group_layout,
            texture_bind_group,
            albedo_texture,
            albedo_sampler,
            has_texture: false,
            height_scale: 0.3,
            water_y: -1.0,
            water_color: [0.2, 0.4, 0.7],
            smf_lighting: SmfLighting::default(),
            brush_cursor: None,
            x_extent: 0.5,
            z_extent: 0.5,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            depth_texture: None,
            depth_format,
            output_texture: None,
            output_view: None,
            reflection_texture: None,
            reflection_view: None,
            reflection_depth_view: None,
            reflection_bind_group_layout,
            reflection_sampler,
            reflection_bind_group,
            reflection_bind_group_dummy,
            water_normal_bind_group_layout,
            water_normal_bind_group,
            water_normal_texture,
            water_normal_sampler,
            width: 512,
            height: 512,
            time: 0.0,
            quality_high: true,
            last_uploaded_revision: None,
        }
    }

    /// Set the elapsed-time uniform used for animated water waves.
    /// Internal: callers should pass `time` through a `PreviewFrame`
    /// instead of mutating renderer state directly.
    fn set_time(&mut self, seconds: f32) {
        self.time = seconds % (std::f32::consts::TAU * 60.0);
    }

    /// Toggle high-pass rendering (sky reflections + atmospheric fog).
    /// Internal — drive via `PreviewFrame::quality_high`.
    fn set_quality_high(&mut self, enabled: bool) {
        self.quality_high = enabled;
    }

    /// Set (or clear) the brush cursor visualisation. `Some((wx, wz,
    /// radius))` draws a translucent ring on the terrain at world
    /// position `(wx, wz)` with `radius` in render-space units.
    /// `None` removes the ring. Driven by the GUI's pick-on-hover
    /// path each frame the inspector is in Sculpt mode.
    pub fn set_brush_cursor(&mut self, cursor: Option<(f32, f32, f32)>) {
        self.brush_cursor = cursor;
    }

    /// Pack `brush_cursor` into the uniform-friendly `[f32; 4]` slot.
    /// The .w component is the active flag (1.0 = render, 0.0 =
    /// skip), so the shader only has to branch on a single value.
    fn brush_cursor_uniform(&self) -> [f32; 4] {
        match self.brush_cursor {
            Some((x, z, r)) => [x, z, r, 1.0],
            None => [0.0, 0.0, 0.0, 0.0],
        }
    }

    /// Upload a new albedo texture from a `ColorBuffer`. Internal —
    /// callers drive textures through `PreviewFrame::texture`.
    fn update_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, color_buf: &ColorBuffer) {
        let rgba = color_buf.to_rgba8();
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("albedo_texture"),
                size: wgpu::Extent3d {
                    width: color_buf.width(),
                    height: color_buf.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &rgba,
        );

        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_bind_group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.albedo_sampler),
                },
            ],
        });

        self.albedo_texture = tex;
        self.has_texture = true;
    }

    /// Update the terrain mesh from a heightmap. Internal — callers
    /// drive geometry through `PreviewFrame`.
    fn update_mesh(&mut self, device: &wgpu::Device, heightmap: &Heightmap, height_scale: f32, x_extent: f32, z_extent: f32, water_y: f32, water_color: [f32; 3]) {
        self.height_scale = height_scale;
        self.water_y = water_y;
        self.water_color = water_color;
        self.x_extent = x_extent;
        self.z_extent = z_extent;
        let (vertices, indices) = generate_terrain_mesh(heightmap, height_scale, x_extent, z_extent, water_y);
        self.num_indices = indices.len() as u32;

        self.vertex_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain_vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        );

        self.index_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain_indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
        );
    }

    /// Update terrain mesh with LOD (decimated to max_grid_size for
    /// interactive preview). Internal — callers drive geometry
    /// through `PreviewFrame`.
    fn update_mesh_lod(
        &mut self,
        device: &wgpu::Device,
        heightmap: &Heightmap,
        height_scale: f32,
        max_grid_size: u32,
        x_extent: f32,
        z_extent: f32,
        water_y: f32,
        water_color: [f32; 3],
    ) {
        self.height_scale = height_scale;
        self.water_y = water_y;
        self.water_color = water_color;
        self.x_extent = x_extent;
        self.z_extent = z_extent;
        let (vertices, indices) = generate_terrain_mesh_lod(heightmap, height_scale, max_grid_size, x_extent, z_extent, water_y);
        self.num_indices = indices.len() as u32;

        self.vertex_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain_vertices_lod"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        );

        self.index_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain_indices_lod"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
        );
    }

    /// Resize the off-screen render target.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;

        // Create output texture (TEXTURE_BINDING needed for egui integration)
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.output_view = Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
        self.output_texture = Some(texture);

        // Create depth texture
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_texture =
            Some(depth_texture.create_view(&wgpu::TextureViewDescriptor::default()));

        // Reflection colour texture — same size as the main output, sampled
        // by the water shader at full resolution. Linear-space format
        // (Rgba8Unorm, not Srgb) so we can both render to it and sample
        // from it without a colour-space round-trip.
        let reflection_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reflection_color"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let reflection_view = reflection_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Reflection depth target — separate from main since the two passes
        // have different visible geometry.
        let reflection_depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("reflection_depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.reflection_depth_view = Some(
            reflection_depth.create_view(&wgpu::TextureViewDescriptor::default()),
        );

        // Re-bind the "real" reflection bind group at the new texture.
        // The dummy bind group still points at the 1×1 default created
        // in `new()` — it's only used by the reflection pre-pass and
        // doesn't need rebinding on resize.
        self.reflection_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reflection_bind_group"),
            layout: &self.reflection_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&reflection_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.reflection_sampler),
                },
            ],
        });
        self.reflection_view = Some(reflection_view);
        self.reflection_texture = Some(reflection_tex);
    }

    /// Render `frame` to the off-screen texture. `None` clears the
    /// viewport to a neutral background.
    ///
    /// This is the **only** public entry point for changing what's
    /// drawn. There's no separate `update_*` / `clear_*` mutation
    /// path that can leave the renderer in a stale state — the
    /// frame fully describes the scene each call.
    ///
    /// Re-uploads of mesh/texture happen only when the frame's
    /// `revision` changes, so camera-only or animation-only
    /// re-renders are cheap (just a re-render of the cached
    /// resources).
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        frame: Option<&PreviewFrame>,
    ) {
        match frame {
            None => {
                // Drop GPU mesh + reset water/texture flags so the
                // empty pass below has nothing left to "leak through".
                self.clear_mesh();
                self.render_empty(device, queue);
            }
            Some(f) => {
                self.sync_to_frame(device, queue, f);
                self.render_internal(device, queue, camera);
            }
        }
    }

    /// Refresh GPU state from a frame. Re-uploads mesh + texture
    /// only when `frame.revision` differs from the last uploaded
    /// revision; otherwise just refreshes cheap params (water, time,
    /// quality).
    fn sync_to_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        f: &PreviewFrame,
    ) {
        let needs_reupload = self.last_uploaded_revision != Some(f.revision);
        if needs_reupload {
            self.update_mesh_lod(
                device,
                f.heightmap,
                f.height_scale,
                f.max_grid_size,
                f.x_extent,
                f.z_extent,
                f.water_y,
                f.water_color,
            );
            if let Some(tex) = f.texture {
                self.update_texture(device, queue, tex);
            } else {
                // Frame says no texture: drop the flag so the shader
                // takes the procedural-color path.
                self.has_texture = false;
            }
            self.last_uploaded_revision = Some(f.revision);
        }
        // Cheap-to-refresh params get reapplied every call so they
        // can change without bumping revision (animation, camera,
        // quality toggle).
        self.height_scale = f.height_scale;
        self.water_y = f.water_y;
        self.water_color = f.water_color;
        self.smf_lighting = f.smf_lighting;
        self.x_extent = f.x_extent;
        self.z_extent = f.z_extent;
        self.set_quality_high(f.quality_high);
        self.set_time(f.time);
    }

    /// Emit a single clear pass to the output texture so the egui
    /// viewport doesn't keep displaying the previous frame's pixels.
    fn render_empty(&self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let (Some(output_view), Some(depth_view)) =
            (self.output_view.as_ref(), self.depth_texture.as_ref())
        else {
            return;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear_output_encoder"),
        });
        {
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_output_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.07,
                            g: 0.07,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Internal: actually run the multi-pass render pipeline using
    /// the GPU state currently held. Public callers go through
    /// [`Self::render`] which sets that state from a frame first.
    fn render_internal(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
    ) {
        let Some(ref output_view) = self.output_view else {
            return;
        };
        let Some(ref depth_view) = self.depth_texture else {
            return;
        };
        let Some(ref vertex_buffer) = self.vertex_buffer else {
            return;
        };
        let Some(ref index_buffer) = self.index_buffer else {
            return;
        };

        let aspect = self.width as f32 / self.height.max(1) as f32;
        let view_proj = camera.view_projection(aspect);
        let cam_pos = camera.position();

        // ── Pass 1: planar reflection ───────────────────────────────────────
        // Render the scene with the camera mirrored through the water plane
        // y = water_y. The result is sampled by the water shader in pass 2,
        // giving real reflection of mountains + sky (matching how the BAR
        // website's water looks). Skipped when there's no water.
        if self.quality_high && self.water_y >= 0.0 {
            if let (Some(ref reflection_view), Some(ref reflection_depth_view)) =
                (&self.reflection_view, &self.reflection_depth_view)
            {
                // Build a reflected view matrix by mirroring the camera
                // through the water plane y = water_y. The reflected
                // camera position is below the water (or above if the
                // camera is below); the look-at target is also mirrored;
                // the up vector flips so the orientation is consistent.
                // This is the approach Three.js's `Reflector` uses and
                // is more robust than `view_proj * M_reflect`, which can
                // run into degenerate winding/depth issues.
                let wy = self.water_y;
                let cam_pos_refl =
                    glam::Vec3::new(cam_pos.x, 2.0 * wy - cam_pos.y, cam_pos.z);
                let target_refl = glam::Vec3::new(
                    camera.target.x,
                    2.0 * wy - camera.target.y,
                    camera.target.z,
                );
                let up_refl = glam::Vec3::new(0.0, -1.0, 0.0);
                let view_refl = Mat4::look_at_rh(cam_pos_refl, target_refl, up_refl);
                let projection = camera.projection_matrix(aspect);
                let view_proj_refl = projection * view_refl;

                let smf = self.smf_lighting.to_uniform_slots();
                let refl_uniform = CameraUniform {
                    view_proj: view_proj_refl.to_cols_array_2d(),
                    inv_view_proj: view_proj_refl.inverse().to_cols_array_2d(),
                    camera_pos: [cam_pos_refl.x, cam_pos_refl.y, cam_pos_refl.z],
                    has_texture: self.has_texture as u32,
                    height_scale: self.height_scale,
                    water_r: self.water_color[0],
                    water_g: self.water_color[1],
                    water_b: self.water_color[2],
                    water_y: self.water_y,
                    time: self.time,
                    quality: 0.0, // skip recursive sky reflection in this pass
                    skip_water: 1.0,
                    screen_w: self.width as f32,
                    screen_h: self.height as f32,
                    _pad: [0.0; 2],
                    sun_dir_exp: smf.sun_dir_exp,
                    ground_ambient: smf.ground_ambient,
                    ground_diffuse: smf.ground_diffuse,
                    ground_specular: smf.ground_specular,
                    water_absorb: smf.water_absorb,
                    water_base_color: smf.water_base_color,
                    water_min_color: smf.water_min_color,
                    // No brush cursor in the reflection pass — the
                    // ring should be visible on the main surface,
                    // not duplicated in the water reflection.
                    brush_cursor: [0.0, 0.0, 0.0, 0.0],
                };
                queue.write_buffer(
                    &self.camera_buffer,
                    0,
                    bytemuck::bytes_of(&refl_uniform),
                );

                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("reflection_encoder"),
                });
                {
                    let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("reflection_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: reflection_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.55,
                                    g: 0.7,
                                    b: 0.85,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(
                            wgpu::RenderPassDepthStencilAttachment {
                                view: reflection_depth_view,
                                depth_ops: Some(wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(1.0),
                                    store: wgpu::StoreOp::Store,
                                }),
                                stencil_ops: None,
                            },
                        ),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    rp.set_pipeline(&self.render_pipeline);
                    rp.set_bind_group(0, &self.camera_bind_group, &[]);
                    rp.set_bind_group(1, &self.texture_bind_group, &[]);
                    // Use the dummy reflection bind group: the actual
                    // reflection texture is the colour target for this
                    // pass, so it can't simultaneously be sampled. The
                    // shader's water branch discards in this pass anyway
                    // (skip_water = 1.0), so the binding goes unused.
                    rp.set_bind_group(2, &self.reflection_bind_group_dummy, &[]);
                    rp.set_bind_group(3, &self.water_normal_bind_group, &[]);
                    rp.set_vertex_buffer(0, vertex_buffer.slice(..));
                    rp.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..self.num_indices, 0, 0..1);

                    // Sky in the reflection — terrain peaks are reflected,
                    // and the sky background fills everywhere terrain didn't.
                    rp.set_pipeline(&self.sky_pipeline);
                    rp.set_bind_group(0, &self.camera_bind_group, &[]);
                    rp.draw(0..3, 0..1);
                }
                queue.submit(std::iter::once(enc.finish()));
            }
        }

        // ── Pass 2: main render ─────────────────────────────────────────────
        let smf_main = self.smf_lighting.to_uniform_slots();
        let camera_uniform = CameraUniform {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_pos: [cam_pos.x, cam_pos.y, cam_pos.z],
            has_texture: self.has_texture as u32,
            height_scale: self.height_scale,
            water_r: self.water_color[0],
            water_g: self.water_color[1],
            water_b: self.water_color[2],
            water_y: self.water_y,
            time: self.time,
            quality: if self.quality_high { 1.0 } else { 0.0 },
            skip_water: 0.0,
            screen_w: self.width as f32,
            screen_h: self.height as f32,
            _pad: [0.0; 2],
            sun_dir_exp: smf_main.sun_dir_exp,
            ground_ambient: smf_main.ground_ambient,
            ground_diffuse: smf_main.ground_diffuse,
            ground_specular: smf_main.ground_specular,
            water_absorb: smf_main.water_absorb,
            water_base_color: smf_main.water_base_color,
            water_min_color: smf_main.water_min_color,
            brush_cursor: self.brush_cursor_uniform(),
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("terrain_render_encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.15,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.set_bind_group(1, &self.texture_bind_group, &[]);
            render_pass.set_bind_group(2, &self.reflection_bind_group, &[]);
            render_pass.set_bind_group(3, &self.water_normal_bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.num_indices, 0, 0..1);

            // Skybox pass — only on high-pass renders. Fills any pixels the
            // terrain didn't cover with the procedural sky. Drawn LAST so
            // depth-test occlusion (LessEqual at z=1.0) keeps terrain in
            // front; LessEqual rather than Less so the far-plane sky still
            // passes against the cleared depth buffer (which is also 1.0).
            if self.quality_high {
                render_pass.set_pipeline(&self.sky_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            }
        }

        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Check if we have a mesh to render.
    pub fn has_mesh(&self) -> bool {
        self.vertex_buffer.is_some()
    }

    /// Drop any uploaded mesh so the next render leaves the viewport
    /// empty. Called when the active preview target has nothing
    /// usefully wired into it — without this, `update_mesh_lod` is
    /// only called on success, and a stale mesh from a prior
    /// successful preview would keep showing as if it were the
    /// current output.
    /// Drop the GPU mesh + reset water/texture flags so a subsequent
    /// render emits an empty viewport rather than the previous
    /// frame's pixels. Internal — public callers go through
    /// `render(.., None)`.
    fn clear_mesh(&mut self) {
        self.vertex_buffer = None;
        self.index_buffer = None;
        self.num_indices = 0;
        // Disable water and texture sampling so partial leftover
        // state can't bleed through any subsequent render.
        self.water_y = -1.0;
        self.has_texture = false;
        self.last_uploaded_revision = None;
    }

    /// Geometry parameters from the most recent `update_mesh` /
    /// `update_mesh_lod`. Used by ray-cast picking so the picking math
    /// uses the same world-space layout the GPU mesh was built with.
    /// Returns `(height_scale, x_extent, z_extent)`.
    pub fn mesh_extents(&self) -> (f32, f32, f32) {
        (self.height_scale, self.x_extent, self.z_extent)
    }

    /// Get the output texture view for external use (e.g., egui display).
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.output_view.as_ref()
    }

    /// Copy the rendered output texture back to a CPU buffer of RGBA8 bytes.
    /// Used by the headless CLI preview command to write a PNG. Returns
    /// `None` if no render has happened yet (no output texture).
    ///
    /// The function is synchronous: it submits a copy command, polls the
    /// device until it completes, then maps the staging buffer.
    pub fn read_pixels(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<Vec<u8>> {
        let texture = self.output_texture.as_ref()?;
        let w = self.width;
        let h = self.height;
        // wgpu requires buffer copy bytes_per_row to be aligned to 256.
        let bytes_per_pixel = 4u32;
        let unpadded_bpr = w * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bpr = ((unpadded_bpr + align - 1) / align) * align;
        let buffer_size = (padded_bpr * h) as wgpu::BufferAddress;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain_readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("terrain_readback_encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &staging,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv().ok()?.ok()?;

        let raw = slice.get_mapped_range();
        // Strip per-row padding: copy `unpadded_bpr` bytes from each row.
        let mut out = Vec::with_capacity((unpadded_bpr * h) as usize);
        for row in 0..h {
            let start = (row * padded_bpr) as usize;
            let end = start + unpadded_bpr as usize;
            out.extend_from_slice(&raw[start..end]);
        }
        drop(raw);
        staging.unmap();
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    /// Parse-time validation for the assembled terrain shader. Catches
    /// regressions in the WGSL — including ports of new Recoil shaders
    /// — before they reach the GPU. This runs in `cargo test` (no
    /// device required) so CI catches mistakes that would otherwise
    /// only show up on first launch.
    #[test]
    fn terrain_shader_wgsl_parses() {
        let modern_sky = include_str!("../../../shaders/recoil/modern_sky.wgsl");
        let smf_ground = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let smf_water = include_str!("../../../shaders/recoil/smf_water.wgsl");
        let terrain = include_str!("../../../shaders/terrain.wgsl");
        let combined = format!("{modern_sky}\n{smf_ground}\n{smf_water}\n{terrain}");
        let module = naga::front::wgsl::parse_str(&combined);
        assert!(
            module.is_ok(),
            "terrain shader failed to parse: {:?}",
            module.err()
        );
    }

    /// Minimap port (Recoil) is not yet wired into a pipeline, but the
    /// WGSL still has to parse so a future hook-up doesn't break on
    /// syntax. Pin the parse result here so any drift is caught.
    #[test]
    fn minimap_shader_wgsl_parses() {
        let minimap = include_str!("../../../shaders/recoil/minimap.wgsl");
        let module = naga::front::wgsl::parse_str(minimap);
        assert!(
            module.is_ok(),
            "minimap shader failed to parse: {:?}",
            module.err()
        );
    }
}
