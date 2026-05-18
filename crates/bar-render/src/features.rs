// SPDX-License-Identifier: GPL-2.0-or-later
//! Feature renderer -- placeholder boxes (M1) and real S3O models (M3) with
//! per-model diffuse textures (M4).
//!
//! Each loaded S3O has its own GPU vertex/index buffer and its own diffuse
//! `Rgba8UnormSrgb` texture. Models without a usable diffuse fall back to a
//! shared 1x1 white texture so they still draw. Placeholder cubes (used for
//! unknown feature types) also bind the default white texture.
//!
//! All draws share one pipeline and one render pass (LoadOp::Load so the
//! terrain depth buffer correctly occludes features).

use std::collections::HashMap;

use bar_data::S3oVertex;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

// ── Per-instance data ─────────────────────────────────────────────────────────

/// Per-instance GPU data: column-major 4x4 model transform + RGBA tint.
/// 80 bytes total; 16-byte aligned.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct FeatureInstance {
    pub col0: [f32; 4],
    pub col1: [f32; 4],
    pub col2: [f32; 4],
    pub col3: [f32; 4],
    pub tint: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<FeatureInstance>() == 80);

// ── Public texture upload type ────────────────────────────────────────────────

/// CPU-side RGBA8 texture passed to `FeatureRenderer::load_mesh`. The caller
/// passes one for the S3O `texture1` channel (diffuse + team mask) and one
/// for the S3O `texture2` channel (glow/specular + opacity).
pub struct FeatureTexture<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

// ── Vertex buffer layout (shared by placeholder cube and S3O models) ──────────

/// Shared vertex layout: 32 bytes (position 12 + normal 12 + uv 8).
/// `S3oVertex` has this exact layout. Placeholder cube verts get dummy UVs.
fn vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<S3oVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x2,
            },
        ],
    }
}

fn instance_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<FeatureInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 48,
                shader_location: 5,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: 64,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
        ],
    }
}

// ── Placeholder cube geometry ─────────────────────────────────────────────────

/// Unit cube with bottom face at Y=0. Vertices are `S3oVertex` with dummy UVs.
fn unit_cube_verts() -> [S3oVertex; 24] {
    macro_rules! v {
        ($p:expr, $n:expr) => {
            S3oVertex {
                position: $p,
                normal: $n,
                uv: [0.0, 0.0],
            }
        };
    }
    [
        // +Y (top)
        v!([0.5, 1.0, 0.5], [0.0, 1.0, 0.0]),
        v!([-0.5, 1.0, 0.5], [0.0, 1.0, 0.0]),
        v!([-0.5, 1.0, -0.5], [0.0, 1.0, 0.0]),
        v!([0.5, 1.0, -0.5], [0.0, 1.0, 0.0]),
        // -Y (bottom)
        v!([0.5, 0.0, -0.5], [0.0, -1.0, 0.0]),
        v!([-0.5, 0.0, -0.5], [0.0, -1.0, 0.0]),
        v!([-0.5, 0.0, 0.5], [0.0, -1.0, 0.0]),
        v!([0.5, 0.0, 0.5], [0.0, -1.0, 0.0]),
        // +X
        v!([0.5, 0.0, 0.5], [1.0, 0.0, 0.0]),
        v!([0.5, 1.0, 0.5], [1.0, 0.0, 0.0]),
        v!([0.5, 1.0, -0.5], [1.0, 0.0, 0.0]),
        v!([0.5, 0.0, -0.5], [1.0, 0.0, 0.0]),
        // -X
        v!([-0.5, 0.0, -0.5], [-1.0, 0.0, 0.0]),
        v!([-0.5, 1.0, -0.5], [-1.0, 0.0, 0.0]),
        v!([-0.5, 1.0, 0.5], [-1.0, 0.0, 0.0]),
        v!([-0.5, 0.0, 0.5], [-1.0, 0.0, 0.0]),
        // +Z
        v!([-0.5, 0.0, 0.5], [0.0, 0.0, 1.0]),
        v!([-0.5, 1.0, 0.5], [0.0, 0.0, 1.0]),
        v!([0.5, 1.0, 0.5], [0.0, 0.0, 1.0]),
        v!([0.5, 0.0, 0.5], [0.0, 0.0, 1.0]),
        // -Z
        v!([0.5, 0.0, -0.5], [0.0, 0.0, -1.0]),
        v!([0.5, 1.0, -0.5], [0.0, 0.0, -1.0]),
        v!([-0.5, 1.0, -0.5], [0.0, 0.0, -1.0]),
        v!([-0.5, 0.0, -0.5], [0.0, 0.0, -1.0]),
    ]
}

fn unit_cube_indices() -> [u16; 36] {
    let mut idx = [0u16; 36];
    for face in 0..6u16 {
        let b = face * 4;
        let i = (face * 6) as usize;
        idx[i] = b;
        idx[i + 1] = b + 1;
        idx[i + 2] = b + 2;
        idx[i + 3] = b;
        idx[i + 4] = b + 2;
        idx[i + 5] = b + 3;
    }
    idx
}

// ── Named mesh storage ────────────────────────────────────────────────────────

struct FeatureMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    instance_buffer: Option<wgpu::Buffer>,
    instance_count: u32,
    /// Group-1 bind group: per-mesh diffuse texture + sampler. Always present;
    /// falls back to the renderer-wide 1x1 white texture when the model had no
    /// usable diffuse.
    texture_bind_group: wgpu::BindGroup,
    /// True when the bind group references the model's own diffuse (vs. the
    /// fallback white). Surfaced only for logging.
    #[allow(dead_code)]
    has_texture: bool,
    /// Model-space AABB after the same anchor shift applied to vertices on
    /// load (`shift_y = max(0, aabb_min.y)`). Stored so cursor picking can
    /// test the cursor ray against each instance's oriented bounding box.
    aabb_min: [f32; 3],
    aabb_max: [f32; 3],
}

// ── FeatureRenderer ───────────────────────────────────────────────────────────

/// Renders all map features, either as placeholder unit boxes or as real S3O
/// models when `load_mesh` has been called for that feature type.
///
/// Shares the camera uniform bind group (group 0) with `TerrainRenderer`. Each
/// mesh owns a group-1 bind group containing its diffuse texture + sampler.
/// All geometry is drawn in a single render pass (LoadOp::Load on color + depth).
pub struct FeatureRenderer {
    pipeline: wgpu::RenderPipeline,
    /// Depth-only caster pipeline. Same vertex / instance layout as `pipeline`
    /// so the same per-mesh buffers feed both.
    shadow_pipeline: wgpu::RenderPipeline,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    /// Linear sampler reused across all per-mesh bind groups.
    sampler: wgpu::Sampler,
    /// 1x1 white texture used by placeholders and any mesh without a diffuse.
    default_white_view: wgpu::TextureView,
    /// 1x1 (0,0,0,1) texture used as the texture2 (shading) fallback. The
    /// feature shader now interprets shading_sample.r as emissive and .g as a
    /// specular multiplier (matches engine `ModelFragProg.glsl:87-109`); a
    /// white fallback there would make every placeholder cube self-illuminate
    /// at 100% brightness. Black RGB, opaque alpha gives the correct no-op.
    default_shading_view: wgpu::TextureView,
    /// Bind group for the placeholder cube, points at `default_white_view`.
    placeholder_bind_group: wgpu::BindGroup,
    // Placeholder cube geometry
    placeholder_vb: wgpu::Buffer,
    placeholder_ib: wgpu::Buffer,
    placeholder_instances: Option<wgpu::Buffer>,
    placeholder_count: u32,
    // Real S3O meshes keyed by lowercase feature type name
    meshes: HashMap<String, FeatureMesh>,
}

impl FeatureRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_bgl: &wgpu::BindGroupLayout,
        shadow_caster_bgl: &wgpu::BindGroupLayout,
        shadow_receiver_bgl: &wgpu::BindGroupLayout,
    ) -> Self {
        // Concatenate the shared SMF ground-shade helper so features use the
        // exact same Lambert + Blinn-Phong + SMF intensity-mult math as the
        // terrain. This keeps lit-side / shadow-side contrast identical
        // between ground and features.
        let smf_ground = include_str!("../../../shaders/recoil/smf_ground.wgsl");
        let features_main = include_str!("../../../shaders/features.wgsl");
        let combined = format!("{smf_ground}\n{features_main}");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("feature_shader"),
            source: wgpu::ShaderSource::Wgsl(combined.into()),
        });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("feature_shadow_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/shadow_feature.wgsl").into(),
            ),
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("feature_texture_bgl"),
                entries: &[
                    // texture1: diffuse rgb + team-mask alpha
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
                    // texture2: shading rgb + opacity alpha
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Shared filtering sampler for both textures.
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("feature_pipeline_layout"),
            bind_group_layouts: &[
                camera_bgl,                 // group 0: camera + lighting
                &texture_bind_group_layout, // group 1: tex1 + tex2 + sampler
                shadow_receiver_bgl,        // group 2: shadow tex + sampler + light_vp
            ],
            push_constant_ranges: &[],
        });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("feature_shadow_pipeline_layout"),
                bind_group_layouts: &[
                    shadow_caster_bgl,          // group 0: light view-proj uniform
                    &texture_bind_group_layout, // group 1: tex1 + tex2 + sampler (for cutout)
                ],
                push_constant_ranges: &[],
            });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("feature_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_feature"),
                buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_feature"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    // Real opacity now comes from texture2.a; enable standard
                    // alpha blending so semi-transparent crystals composite
                    // correctly. Tree leaf cutouts also rely on the shader's
                    // sub-threshold discard to skip near-zero alpha texels.
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

        // Depth-only caster pipeline for the shadow pass. Same vertex /
        // instance layout as the main pipeline; renders to the shadow map's
        // depth attachment with no color target.
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("feature_shadow_pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_shadow_feature"),
                buffers: &[vertex_buffer_layout(), instance_buffer_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shadow_shader,
                entry_point: Some("fs_shadow_feature"),
                targets: &[],
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
                format: crate::shadow::ShadowMap::FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                // Small bias to reduce shadow acne on near-grazing surfaces.
                // Small slope-scale bias is enough for acne avoidance on
                // sloped feature faces; larger values eroded small crystals
                // entirely (their full depth extent in light space is
                // smaller than the bias).
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 0.5,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // 1x1 white texture used as default / placeholder diffuse.
        let default_white = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("feature_default_white"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &default_white,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let default_white_view = default_white.create_view(&wgpu::TextureViewDescriptor::default());

        // 1x1 (0,0,0,255) texture used as the texture2 fallback. See the
        // `default_shading_view` field doc for why a black RGB / opaque alpha
        // pixel is the correct no-op when emissive / spec-mult channels are
        // sampled.
        let default_shading = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("feature_default_shading"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &default_shading,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0u8, 0, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let default_shading_view =
            default_shading.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("feature_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let placeholder_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("feature_placeholder_bg"),
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&default_white_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    // Black RGB, opaque alpha -- emissive / spec-mult no-op.
                    resource: wgpu::BindingResource::TextureView(&default_shading_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let verts = unit_cube_verts();
        let inds = unit_cube_indices();
        let placeholder_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("feature_placeholder_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let placeholder_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("feature_placeholder_ib"),
            contents: bytemuck::cast_slice(&inds),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            pipeline,
            shadow_pipeline,
            texture_bind_group_layout,
            sampler,
            default_white_view,
            default_shading_view,
            placeholder_bind_group,
            placeholder_vb,
            placeholder_ib,
            placeholder_instances: None,
            placeholder_count: 0,
            meshes: HashMap::new(),
        }
    }

    /// Upload one CPU-side rgba8 texture and return its view, or `None` for
    /// missing/invalid input (caller substitutes the default-white view).
    fn upload_one(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
        slot: &str,
        tex: Option<&FeatureTexture>,
    ) -> Option<wgpu::TextureView> {
        let t = tex?;
        if t.width == 0 || t.height == 0 || t.rgba.is_empty() {
            tracing::warn!(name, slot, "feature texture empty; using fallback white");
            return None;
        }
        let expected = (t.width as usize) * (t.height as usize) * 4;
        if t.rgba.len() != expected {
            tracing::warn!(
                name,
                slot,
                got = t.rgba.len(),
                expected,
                "feature texture rgba length mismatch; using fallback white"
            );
            return None;
        }
        let gpu_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("feature_{slot}_{name}")),
            size: wgpu::Extent3d {
                width: t.width,
                height: t.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            t.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(t.width * 4),
                rows_per_image: Some(t.height),
            },
            wgpu::Extent3d {
                width: t.width,
                height: t.height,
                depth_or_array_layers: 1,
            },
        );
        // The view holds an internal Arc to the texture, so dropping `gpu_tex`
        // here is safe -- the bind group keeps both alive transitively.
        Some(gpu_tex.create_view(&wgpu::TextureViewDescriptor::default()))
    }

    /// Build a bind group for one feature mesh from its tex1 and tex2 inputs.
    /// Missing/invalid textures fall back to renderer-wide defaults:
    ///   tex1 -> 1x1 white (no team-colour shift, fully visible diffuse)
    ///   tex2 -> 1x1 (0,0,0,1) (emissive 0, spec-mult 0, fully opaque)
    fn build_texture_bind_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
        tex1: Option<&FeatureTexture>,
        tex2: Option<&FeatureTexture>,
    ) -> (wgpu::BindGroup, bool, bool) {
        let view1 = self.upload_one(device, queue, name, "tex1", tex1);
        let view2 = self.upload_one(device, queue, name, "tex2", tex2);
        let has_tex1 = view1.is_some();
        let has_tex2 = view2.is_some();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("feature_bg_{name}")),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        view1.as_ref().unwrap_or(&self.default_white_view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        view2.as_ref().unwrap_or(&self.default_shading_view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        (bind_group, has_tex1, has_tex2)
    }

    /// Upload a real S3O model and its two diffuse textures for a named
    /// feature type. If a model already exists for this name it is replaced.
    /// Models with no geometry (e.g. hierarchical root pieces not yet supported)
    /// are silently skipped; those features render as placeholder boxes.
    pub fn load_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
        mesh: &bar_data::S3oMesh,
        tex1: Option<&FeatureTexture>,
        tex2: Option<&FeatureTexture>,
    ) {
        if mesh.vertices.is_empty() || mesh.indices.is_empty() {
            return;
        }

        // Many BAR S3O models put their anchor / piece-tree origin a few
        // elmos below the lowest visible vertex, so when we translate the
        // instance to `ry = ground_height` the visible body floats. This
        // is most obvious as a gap between a thin feature's base and where
        // its shadow starts on the ground.
        //
        // Shift all vertex Y values by -max(0, aabb_min.y) so the lowest
        // visible vertex sits at model Y = 0. We do NOT shift up if
        // `aabb_min.y < 0` -- models that intentionally extend below the
        // anchor (tree roots, rocks meant to half-bury) should stay that way.
        let shift_y = mesh.aabb_min[1].max(0.0);
        let vertices: Vec<bar_data::S3oVertex> = if shift_y > 0.0 {
            mesh.vertices
                .iter()
                .map(|v| bar_data::S3oVertex {
                    position: [v.position[0], v.position[1] - shift_y, v.position[2]],
                    normal: v.normal,
                    uv: v.uv,
                })
                .collect()
        } else {
            mesh.vertices.clone()
        };
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("feature_vb_{name}")),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("feature_ib_{name}")),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let (bg, has_tex1, _has_tex2) =
            self.build_texture_bind_group(device, queue, name, tex1, tex2);
        self.meshes.insert(
            name.to_lowercase(),
            FeatureMesh {
                vertex_buffer: vb,
                index_buffer: ib,
                num_indices: mesh.indices.len() as u32,
                instance_buffer: None,
                instance_count: 0,
                texture_bind_group: bg,
                has_texture: has_tex1,
                aabb_min: [
                    mesh.aabb_min[0],
                    mesh.aabb_min[1] - shift_y,
                    mesh.aabb_min[2],
                ],
                aabb_max: [
                    mesh.aabb_max[0],
                    mesh.aabb_max[1] - shift_y,
                    mesh.aabb_max[2],
                ],
            },
        );
    }

    /// True if a real S3O model has been loaded for this feature type.
    pub fn has_model(&self, feature_type: &str) -> bool {
        self.meshes.contains_key(&feature_type.to_lowercase())
    }

    /// Model-space AABB (after the anchor-shift applied at load time) for a
    /// loaded feature type. Returns `None` if no model has been loaded for
    /// this name.
    pub fn mesh_aabb(&self, feature_type: &str) -> Option<(glam::Vec3, glam::Vec3)> {
        let m = self.meshes.get(&feature_type.to_lowercase())?;
        Some((glam::Vec3::from(m.aabb_min), glam::Vec3::from(m.aabb_max)))
    }

    /// Names of all feature types with loaded S3O models.
    pub fn loaded_model_names(&self) -> impl Iterator<Item = &str> {
        self.meshes.keys().map(|s| s.as_str())
    }

    /// Upload grouped instance data.
    ///
    /// - `groups`: instances for feature types that have a loaded S3O model,
    ///   keyed by lowercase feature type name.
    /// - `unknowns`: instances for feature types with no loaded model;
    ///   rendered with the placeholder cube.
    pub fn update_instances_grouped(
        &mut self,
        device: &wgpu::Device,
        groups: &HashMap<String, Vec<FeatureInstance>>,
        unknowns: &[FeatureInstance],
    ) {
        // Placeholder instances.
        if unknowns.is_empty() {
            self.placeholder_instances = None;
            self.placeholder_count = 0;
        } else {
            self.placeholder_instances = Some(device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("feature_placeholder_inst"),
                    contents: bytemuck::cast_slice(unknowns),
                    usage: wgpu::BufferUsages::VERTEX,
                },
            ));
            self.placeholder_count = unknowns.len() as u32;
        }

        // Per-model instances.
        for (name, mesh) in &mut self.meshes {
            let instances = groups.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
            if instances.is_empty() {
                mesh.instance_buffer = None;
                mesh.instance_count = 0;
            } else {
                mesh.instance_buffer = Some(device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("feature_inst_{name}")),
                        contents: bytemuck::cast_slice(instances),
                        usage: wgpu::BufferUsages::VERTEX,
                    },
                ));
                mesh.instance_count = instances.len() as u32;
            }
        }
    }

    /// Total number of feature instances (across all meshes + placeholders).
    pub fn total_instance_count(&self) -> u32 {
        let model_total: u32 = self.meshes.values().map(|m| m.instance_count).sum();
        model_total + self.placeholder_count
    }

    /// Record all feature draw calls into `encoder`, sharing `output_view` and
    /// `depth_view` with the terrain pass (LoadOp::Load keeps terrain depth).
    /// `shadow_bg` is the shadow receiver bind group built by `ShadowMap`.
    /// No-ops if there are no instances to draw.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        camera_bg: &wgpu::BindGroup,
        shadow_bg: &wgpu::BindGroup,
    ) {
        if self.total_instance_count() == 0 {
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("feature_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera_bg, &[]);
        pass.set_bind_group(2, shadow_bg, &[]);

        // Draw real S3O models.
        for mesh in self.meshes.values() {
            let Some(ref inst_buf) = mesh.instance_buffer else {
                continue;
            };
            if mesh.instance_count == 0 {
                continue;
            }
            pass.set_bind_group(1, &mesh.texture_bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, inst_buf.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_indices, 0, 0..mesh.instance_count);
        }

        // Draw placeholder boxes for unknown feature types.
        if let Some(ref inst_buf) = self.placeholder_instances {
            if self.placeholder_count > 0 {
                pass.set_bind_group(1, &self.placeholder_bind_group, &[]);
                pass.set_vertex_buffer(0, self.placeholder_vb.slice(..));
                pass.set_vertex_buffer(1, inst_buf.slice(..));
                pass.set_index_buffer(self.placeholder_ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..36, 0, 0..self.placeholder_count);
            }
        }
    }

    /// Record the depth-only shadow casting draws into an existing render pass
    /// that has the shadow map bound as the depth attachment. Caller provides
    /// the shadow caster bind group (group 0 = light_view_proj uniform).
    pub fn draw_shadow(&self, pass: &mut wgpu::RenderPass, shadow_caster_bg: &wgpu::BindGroup) {
        if self.total_instance_count() == 0 {
            return;
        }
        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_bind_group(0, shadow_caster_bg, &[]);
        for mesh in self.meshes.values() {
            let Some(ref inst_buf) = mesh.instance_buffer else {
                continue;
            };
            if mesh.instance_count == 0 {
                continue;
            }
            pass.set_bind_group(1, &mesh.texture_bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, inst_buf.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_indices, 0, 0..mesh.instance_count);
        }
        // Placeholder boxes also cast shadows so unknown features have visible
        // contact with the ground.
        if let Some(ref inst_buf) = self.placeholder_instances {
            if self.placeholder_count > 0 {
                pass.set_bind_group(1, &self.placeholder_bind_group, &[]);
                pass.set_vertex_buffer(0, self.placeholder_vb.slice(..));
                pass.set_vertex_buffer(1, inst_buf.slice(..));
                pass.set_index_buffer(self.placeholder_ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..36, 0, 0..self.placeholder_count);
            }
        }
    }
}
