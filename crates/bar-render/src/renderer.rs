use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::terrain::{
    generate_flat_grid, generate_terrain_skirts_and_cap, generate_water_plane, TerrainVertex,
};
use bar_data::{ColorBuffer, Heightmap};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 3],
    has_texture: u32,
    height_scale: f32,
    water_r: f32,
    water_g: f32,
    water_b: f32,
    water_y: f32,
    time: f32,
    quality: f32,
    skip_water: f32,
    screen_w: f32,
    screen_h: f32,
    x_extent: f32,
    z_extent: f32,
    sun_dir_exp: [f32; 4],
    ground_ambient: [f32; 4],
    ground_diffuse: [f32; 4],
    ground_specular: [f32; 4],
    water_absorb: [f32; 4],
    water_base_color: [f32; 4],
    water_min_color: [f32; 4],
    brush_cursor: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<CameraUniform>() == 320);

/// Per-frame parameters passed to [`TerrainRenderer::render`].
///
/// Heightmap and texture data are pushed via dedicated `update_*` methods on
/// `TerrainRenderer`; this struct carries only the per-frame uniform inputs
/// that can change without a geometry or texture re-upload.
pub struct PreviewFrame {
    pub height_scale: f32,
    pub x_extent: f32,
    pub z_extent: f32,
    /// Render-space Y of the water plane. Negative => no water.
    pub water_y: f32,
    pub water_color: [f32; 3],
    /// True for the high-pass render (sky reflections + fog).
    pub quality_high: bool,
    pub time: f32,
    pub smf_lighting: SmfLighting,
}

/// Engine-faithful SMF shading inputs.
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
    sky_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    // ── Group 1: albedo + metalmap + typemap ────────────────────────────────
    texture_bind_group_layout: wgpu::BindGroupLayout,
    texture_bind_group: wgpu::BindGroup,
    albedo_texture: wgpu::Texture,
    albedo_sampler: wgpu::Sampler,
    metalmap_texture: wgpu::Texture,
    typemap_texture: wgpu::Texture,
    has_albedo: bool,
    // ── Group 2: planar reflection ──────────────────────────────────────────
    reflection_bind_group_layout: wgpu::BindGroupLayout,
    reflection_sampler: wgpu::Sampler,
    reflection_bind_group: wgpu::BindGroup,
    reflection_bind_group_dummy: wgpu::BindGroup,
    // ── Group 3: water normal map ───────────────────────────────────────────
    water_normal_bind_group_layout: wgpu::BindGroupLayout,
    water_normal_bind_group: wgpu::BindGroup,
    water_normal_texture: wgpu::Texture,
    water_normal_sampler: wgpu::Sampler,
    // ── Group 4: heightmap ──────────────────────────────────────────────────
    heightmap_bind_group_layout: wgpu::BindGroupLayout,
    heightmap_bind_group: wgpu::BindGroup,
    heightmap_texture: wgpu::Texture,
    heightmap_sampler: wgpu::Sampler,
    // ── Geometry ────────────────────────────────────────────────────────────
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    num_indices: u32,
    /// Grid resolution used when building the flat terrain mesh.
    grid_n: u32,
    // ── Output targets ──────────────────────────────────────────────────────
    depth_texture: Option<wgpu::TextureView>,
    depth_format: wgpu::TextureFormat,
    output_texture: Option<wgpu::Texture>,
    output_view: Option<wgpu::TextureView>,
    reflection_texture: Option<wgpu::Texture>,
    reflection_view: Option<wgpu::TextureView>,
    reflection_depth_view: Option<wgpu::TextureView>,
    pub width: u32,
    pub height: u32,
    // ── Cached per-frame state ──────────────────────────────────────────────
    height_scale: f32,
    water_y: f32,
    water_color: [f32; 3],
    smf_lighting: SmfLighting,
    brush_cursor: Option<(f32, f32, f32)>,
    x_extent: f32,
    z_extent: f32,
    time: f32,
    quality_high: bool,
}

fn make_water_normal_map(size: u32) -> Vec<u8> {
    use std::f32::consts::TAU;
    let n = size as usize;
    let mut data = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let u = x as f32 / n as f32;
            let v = y as f32 / n as f32;
            let nx = 0.25 * (TAU * u).sin()
                   + 0.20 * (TAU * (2.0 * u + v)).sin()
                   + 0.10 * (TAU * (3.0 * u - 2.0 * v)).sin();
            let ny = 0.25 * (TAU * v).cos()
                   + 0.20 * (TAU * (u + 2.0 * v)).cos()
                   + 0.10 * (TAU * (2.0 * u + 3.0 * v)).cos();
            let nz = (1.0 - nx * nx - ny * ny).max(0.1_f32).sqrt();
            let to_u8 = |f: f32| ((f * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
            data.push(to_u8(nx));
            data.push(to_u8(ny));
            data.push(to_u8(nz));
            data.push(255u8);
        }
    }
    data
}

/// Create a 1x1 R32Float texture with value `v`. Used for default metalmap/typemap
/// and the initial heightmap (all-zero terrain before any eval completes).
fn make_default_r32float(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, v: f32) -> wgpu::Texture {
    let data = v.to_ne_bytes();
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    )
}

impl TerrainRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, output_format: wgpu::TextureFormat) -> Self {
        // Assemble WGSL from Recoil ports + original shaders. Concatenation
        // gives the same effect as #include; WGSL has no preprocessor.
        let modern_sky_source = include_str!("../../../shaders/recoil/modern_sky.wgsl");
        let smf_ground_source = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let water_source = include_str!("../../../shaders/water.wgsl");
        let terrain_source = include_str!("../../../shaders/terrain.wgsl");
        let shader_source = format!(
            "{modern_sky_source}\n{smf_ground_source}\n{water_source}\n{terrain_source}"
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

        // Group 1: albedo (tex + sampler) + metalmap (tex) + typemap (tex) + shared material sampler
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Group 2: planar reflection
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

        // Group 3: water normal map
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

        // Group 4: heightmap for vertex displacement
        let heightmap_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("heightmap_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
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
                &heightmap_bind_group_layout,
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
            x_extent: 0.5,
            z_extent: 0.5,
            sun_dir_exp: smf.sun_dir_exp,
            ground_ambient: smf.ground_ambient,
            ground_diffuse: smf.ground_diffuse,
            ground_specular: smf.ground_specular,
            water_absorb: smf.water_absorb,
            water_base_color: smf.water_base_color,
            water_min_color: smf.water_min_color,
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

        let albedo_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("albedo_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

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
        let metalmap_texture = make_default_r32float(device, queue, "metalmap_default", 0.0);
        let typemap_texture  = make_default_r32float(device, queue, "typemap_default",  0.0);

        let texture_bind_group = {
            let av = albedo_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mv = metalmap_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let tv = typemap_texture.create_view(&wgpu::TextureViewDescriptor::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("texture_bind_group"),
                layout: &texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&av) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&albedo_sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&mv) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tv) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&albedo_sampler) },
                ],
            })
        };

        let reflection_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("reflection_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

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
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&reflection_sampler) },
                ],
            })
        };
        let reflection_bind_group = make_reflection_bg(&reflection_default_view);
        let reflection_bind_group_dummy = make_reflection_bg(&reflection_default_view);

        let water_normal_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water_normal_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let water_normal_data = make_water_normal_map(128);
        let water_normal_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("water_normal"),
                size: wgpu::Extent3d { width: 128, height: 128, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &water_normal_data,
        );
        let water_normal_view =
            water_normal_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let water_normal_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water_normal_bind_group"),
            layout: &water_normal_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&water_normal_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&water_normal_sampler) },
            ],
        });

        // Default 1x1 heightmap (zero height) until update_heightmap is called.
        let heightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("heightmap_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let heightmap_texture = make_default_r32float(device, queue, "heightmap_default", 0.0);
        let heightmap_bind_group = {
            let hv = heightmap_texture.create_view(&wgpu::TextureViewDescriptor::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("heightmap_bind_group"),
                layout: &heightmap_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hv) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&heightmap_sampler) },
                ],
            })
        };

        Self {
            render_pipeline,
            sky_pipeline,
            camera_buffer,
            camera_bind_group,
            texture_bind_group_layout,
            texture_bind_group,
            albedo_texture,
            albedo_sampler,
            metalmap_texture,
            typemap_texture,
            has_albedo: false,
            reflection_bind_group_layout,
            reflection_sampler,
            reflection_bind_group,
            reflection_bind_group_dummy,
            water_normal_bind_group_layout,
            water_normal_bind_group,
            water_normal_texture,
            water_normal_sampler,
            heightmap_bind_group_layout,
            heightmap_bind_group,
            heightmap_texture,
            heightmap_sampler,
            vertex_buffer: None,
            index_buffer: None,
            num_indices: 0,
            grid_n: 512,
            depth_texture: None,
            depth_format,
            output_texture: None,
            output_view: None,
            reflection_texture: None,
            reflection_view: None,
            reflection_depth_view: None,
            width: 512,
            height: 512,
            height_scale: 0.3,
            water_y: -1.0,
            water_color: [0.2, 0.4, 0.7],
            smf_lighting: SmfLighting::default(),
            brush_cursor: None,
            x_extent: 0.5,
            z_extent: 0.5,
            time: 0.0,
            quality_high: true,
        }
    }

    // ── Public update methods ───────────────────────────────────────────────

    /// Full heightmap replacement. Rebuilds the terrain mesh (flat grid + skirts
    /// + water plane) and uploads the heightmap texture. Called on graph re-eval
    /// or project switch. Recreates the heightmap GPU texture if dimensions changed.
    pub fn update_heightmap(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hm: &Heightmap,
        height_scale: f32,
        x_extent: f32,
        z_extent: f32,
        water_y: f32,
        water_color: [f32; 3],
        grid_n: u32,
    ) {
        self.height_scale = height_scale;
        self.x_extent = x_extent;
        self.z_extent = z_extent;
        self.water_y = water_y;
        self.water_color = water_color;
        self.grid_n = grid_n;

        // Build mesh: flat grid + skirts/cap + optional water plane.
        let (mut verts, mut idxs) = generate_flat_grid(grid_n);

        let skirt_base = verts.len() as u32;
        let (skirt_v, skirt_i) = generate_terrain_skirts_and_cap(hm, height_scale, x_extent, z_extent, grid_n);
        idxs.extend(skirt_i.iter().map(|i| i + skirt_base));
        verts.extend(skirt_v);

        let water_base = verts.len() as u32;
        let (water_v, water_i) = generate_water_plane(x_extent, z_extent, water_y);
        idxs.extend(water_i.iter().map(|i| i + water_base));
        verts.extend(water_v);

        self.num_indices = idxs.len() as u32;
        self.vertex_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain_vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        }));
        self.index_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain_indices"),
            contents: bytemuck::cast_slice(&idxs),
            usage: wgpu::BufferUsages::INDEX,
        }));

        // Upload heightmap texture.
        let hm_w = hm.width();
        let hm_h = hm.height();
        let data: Vec<f32> = (0..hm_h)
            .flat_map(|y| (0..hm_w).map(move |x| hm.get(x, y).unwrap_or(0.0)))
            .collect();

        let old_size = (self.heightmap_texture.width(), self.heightmap_texture.height());
        if old_size == (hm_w, hm_h) {
            // Same dimensions: write in place.
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.heightmap_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&data),
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(hm_w * 4),
                    rows_per_image: Some(hm_h),
                },
                wgpu::Extent3d { width: hm_w, height: hm_h, depth_or_array_layers: 1 },
            );
        } else {
            // Dimensions changed: recreate texture and bind group.
            let tex = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("heightmap_tex"),
                    size: wgpu::Extent3d { width: hm_w, height: hm_h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R32Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                bytemuck::cast_slice(&data),
            );
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            self.heightmap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("heightmap_bind_group"),
                layout: &self.heightmap_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.heightmap_sampler) },
                ],
            });
            self.heightmap_texture = tex;
        }
    }

    /// Sub-region heightmap upload. Called per brush dab to update only the
    /// dirty rectangle without rebuilding the mesh. `data` is row-major f32
    /// of length `w * h`.
    pub fn update_heightmap_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[f32],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.heightmap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(data),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Replace the albedo texture from a `ColorBuffer`. Sets the `has_albedo`
    /// flag so the shader takes the texture path instead of procedural colour.
    pub fn update_albedo(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, cb: &ColorBuffer) {
        let rgba = cb.to_rgba8();
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("albedo_texture"),
                size: wgpu::Extent3d {
                    width: cb.width(),
                    height: cb.height(),
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
        self.albedo_texture = tex;
        self.has_albedo = true;
        self.rebuild_material_bind_group(device);
    }

    /// Sub-region albedo upload. `data` is row-major RGBA8 of length `w * h * 4`.
    pub fn update_albedo_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[u8],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.albedo_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Reset albedo to the 1x1 white default. Called when an eval produces
    /// no texture so the shader falls back to procedural height colour.
    pub fn clear_albedo(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let white: [u8; 4] = [255, 255, 255, 255];
        let tex = device.create_texture_with_data(
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
        self.albedo_texture = tex;
        self.has_albedo = false;
        self.rebuild_material_bind_group(device);
    }

    /// Replace the metalmap texture from a `Heightmap` (values 0..1).
    pub fn update_metalmap(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hm: &Heightmap) {
        let data: Vec<f32> = (0..hm.height())
            .flat_map(|y| (0..hm.width()).map(move |x| hm.get(x, y).unwrap_or(0.0)))
            .collect();
        self.metalmap_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("metalmap_tex"),
                size: wgpu::Extent3d { width: hm.width(), height: hm.height(), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&data),
        );
        self.rebuild_material_bind_group(device);
    }

    /// Sub-region metalmap upload. `data` is row-major f32 of length `w * h`.
    pub fn update_metalmap_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[f32],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.metalmap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(data),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Replace the typemap texture from a `Heightmap` (values 0..1).
    pub fn update_typemap(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hm: &Heightmap) {
        let data: Vec<f32> = (0..hm.height())
            .flat_map(|y| (0..hm.width()).map(move |x| hm.get(x, y).unwrap_or(0.0)))
            .collect();
        self.typemap_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("typemap_tex"),
                size: wgpu::Extent3d { width: hm.width(), height: hm.height(), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&data),
        );
        self.rebuild_material_bind_group(device);
    }

    /// Sub-region typemap upload. `data` is row-major f32 of length `w * h`.
    pub fn update_typemap_region(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: &[f32],
    ) {
        if w == 0 || h == 0 {
            return;
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.typemap_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(data),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Rebuild the group-1 bind group after any of the three material textures change.
    fn rebuild_material_bind_group(&mut self, device: &wgpu::Device) {
        let av = self.albedo_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mv = self.metalmap_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let tv = self.typemap_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_bind_group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&av) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.albedo_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&mv) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tv) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.albedo_sampler) },
            ],
        });
    }

    // ── Camera and animation ────────────────────────────────────────────────

    fn set_time(&mut self, seconds: f32) {
        self.time = seconds % (std::f32::consts::TAU * 60.0);
    }

    fn set_quality_high(&mut self, enabled: bool) {
        self.quality_high = enabled;
    }

    pub fn set_brush_cursor(&mut self, cursor: Option<(f32, f32, f32)>) {
        self.brush_cursor = cursor;
    }

    fn brush_cursor_uniform(&self) -> [f32; 4] {
        match self.brush_cursor {
            Some((x, z, r)) => [x, z, r, 1.0],
            None => [0.0, 0.0, 0.0, 0.0],
        }
    }

    // ── Resize / render ─────────────────────────────────────────────────────

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_output"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
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

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_texture =
            Some(depth_texture.create_view(&wgpu::TextureViewDescriptor::default()));

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

        self.reflection_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reflection_bind_group"),
            layout: &self.reflection_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&reflection_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.reflection_sampler) },
            ],
        });
        self.reflection_view = Some(reflection_view);
        self.reflection_texture = Some(reflection_tex);
    }

    /// Render one frame. `None` clears the viewport; `Some(frame)` renders the
    /// scene. Heightmap and texture data flow through the `update_*` methods;
    /// `PreviewFrame` carries only per-frame uniform inputs (water, time, quality).
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        frame: Option<&PreviewFrame>,
    ) {
        match frame {
            None => {
                self.clear_mesh();
                self.render_empty(device, queue);
            }
            Some(f) => {
                self.sync_to_frame(f);
                self.render_internal(device, queue, camera);
            }
        }
    }

    /// Apply per-frame params from a `PreviewFrame`. No geometry or texture uploads.
    fn sync_to_frame(&mut self, f: &PreviewFrame) {
        self.height_scale = f.height_scale;
        self.water_y = f.water_y;
        self.water_color = f.water_color;
        self.smf_lighting = f.smf_lighting;
        self.x_extent = f.x_extent;
        self.z_extent = f.z_extent;
        self.set_quality_high(f.quality_high);
        self.set_time(f.time);
    }

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
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.07, g: 0.07, b: 0.09, a: 1.0 }),
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

    fn render_internal(&self, device: &wgpu::Device, queue: &wgpu::Queue, camera: &Camera) {
        let Some(ref output_view) = self.output_view else { return; };
        let Some(ref depth_view) = self.depth_texture else { return; };
        let Some(ref vertex_buffer) = self.vertex_buffer else { return; };
        let Some(ref index_buffer) = self.index_buffer else { return; };

        let aspect = self.width as f32 / self.height.max(1) as f32;
        let view_proj = camera.view_projection(aspect);
        let cam_pos = camera.position();

        // ── Pass 1: planar reflection ───────────────────────────────────────
        if self.quality_high && self.water_y >= 0.0 {
            if let (Some(ref reflection_view), Some(ref reflection_depth_view)) =
                (&self.reflection_view, &self.reflection_depth_view)
            {
                let wy = self.water_y;
                let cam_pos_refl = glam::Vec3::new(cam_pos.x, 2.0 * wy - cam_pos.y, cam_pos.z);
                let target_refl = glam::Vec3::new(
                    camera.target.x,
                    2.0 * wy - camera.target.y,
                    camera.target.z,
                );
                let view_refl = Mat4::look_at_rh(cam_pos_refl, target_refl, glam::Vec3::new(0.0, -1.0, 0.0));
                let view_proj_refl = camera.projection_matrix(aspect) * view_refl;

                let smf = self.smf_lighting.to_uniform_slots();
                let refl_uniform = CameraUniform {
                    view_proj: view_proj_refl.to_cols_array_2d(),
                    inv_view_proj: view_proj_refl.inverse().to_cols_array_2d(),
                    camera_pos: [cam_pos_refl.x, cam_pos_refl.y, cam_pos_refl.z],
                    has_texture: self.has_albedo as u32,
                    height_scale: self.height_scale,
                    water_r: self.water_color[0],
                    water_g: self.water_color[1],
                    water_b: self.water_color[2],
                    water_y: self.water_y,
                    time: self.time,
                    quality: 0.0,
                    skip_water: 1.0,
                    screen_w: self.width as f32,
                    screen_h: self.height as f32,
                    x_extent: self.x_extent,
                    z_extent: self.z_extent,
                    sun_dir_exp: smf.sun_dir_exp,
                    ground_ambient: smf.ground_ambient,
                    ground_diffuse: smf.ground_diffuse,
                    ground_specular: smf.ground_specular,
                    water_absorb: smf.water_absorb,
                    water_base_color: smf.water_base_color,
                    water_min_color: smf.water_min_color,
                    brush_cursor: [0.0, 0.0, 0.0, 0.0],
                };
                queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&refl_uniform));

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
                                load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.55, g: 0.7, b: 0.85, a: 1.0 }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: reflection_depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    rp.set_pipeline(&self.render_pipeline);
                    rp.set_bind_group(0, &self.camera_bind_group, &[]);
                    rp.set_bind_group(1, &self.texture_bind_group, &[]);
                    rp.set_bind_group(2, &self.reflection_bind_group_dummy, &[]);
                    rp.set_bind_group(3, &self.water_normal_bind_group, &[]);
                    rp.set_bind_group(4, &self.heightmap_bind_group, &[]);
                    rp.set_vertex_buffer(0, vertex_buffer.slice(..));
                    rp.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..self.num_indices, 0, 0..1);

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
            has_texture: self.has_albedo as u32,
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
            x_extent: self.x_extent,
            z_extent: self.z_extent,
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
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }),
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
            render_pass.set_bind_group(4, &self.heightmap_bind_group, &[]);
            render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.num_indices, 0, 0..1);

            if self.quality_high {
                render_pass.set_pipeline(&self.sky_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            }
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    // ── Accessors ───────────────────────────────────────────────────────────

    pub fn has_mesh(&self) -> bool {
        self.vertex_buffer.is_some()
    }

    fn clear_mesh(&mut self) {
        self.vertex_buffer = None;
        self.index_buffer = None;
        self.num_indices = 0;
        self.water_y = -1.0;
        self.has_albedo = false;
    }

    /// World-space geometry extents used by the CPU ray-cast picker.
    /// Returns `(height_scale, x_extent, z_extent)`.
    pub fn mesh_extents(&self) -> (f32, f32, f32) {
        (self.height_scale, self.x_extent, self.z_extent)
    }

    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.output_view.as_ref()
    }

    /// Copy the rendered output back to a CPU RGBA8 buffer. Used by the
    /// headless CLI preview command. Returns `None` if no render has occurred.
    pub fn read_pixels(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<u8>> {
        let texture = self.output_texture.as_ref()?;
        let w = self.width;
        let h = self.height;
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
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
        device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv().ok()?.ok()?;

        let raw = slice.get_mapped_range();
        let mut out = Vec::with_capacity((unpadded_bpr * h) as usize);
        for row in 0..h {
            let start = (row * padded_bpr) as usize;
            out.extend_from_slice(&raw[start..start + unpadded_bpr as usize]);
        }
        drop(raw);
        staging.unmap();
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn terrain_shader_wgsl_parses() {
        let modern_sky = include_str!("../../../shaders/recoil/modern_sky.wgsl");
        let smf_ground = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let water = include_str!("../../../shaders/water.wgsl");
        let terrain = include_str!("../../../shaders/terrain.wgsl");
        let combined = format!("{modern_sky}\n{smf_ground}\n{water}\n{terrain}");
        let module = naga::front::wgsl::parse_str(&combined);
        assert!(
            module.is_ok(),
            "terrain shader failed to parse: {:?}",
            module.err()
        );
    }

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
