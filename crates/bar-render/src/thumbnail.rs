//! Offscreen renderer for S3O thumbnails shown in the feature palette.
//!
//! Each thumbnail is a single S3O model rendered with a fixed three-
//! quarter-view camera + simple Lambert lighting into a 96x96
//! Rgba8UnormSrgb texture. The texture is kept alive inside the
//! renderer so the egui `TextureId` registered against it stays valid
//! across frames.
//!
//! This is a deliberately small pipeline: it doesn't share the main
//! viewport's CameraUniform, shadow map, or splat resources. Trade-off
//! is one extra pipeline / shader; benefit is the thumbnails don't
//! drag the full viewport rendering state into the palette path.
//!
//! Caller flow (bar-app):
//! ```text
//! tn.load_mesh(...) ;            // when an S3O lands from the model loader
//! tn.render(name) -> Some(view); // produces / refreshes the offscreen texture
//! // register `view` as an egui TextureId; cache on app.feature_thumb_cache
//! ```
//!
//! Re-rendering a thumbnail re-uses the existing offscreen texture, so
//! the egui side keeps the same `TextureId` and doesn't need to be
//! re-registered.

use std::collections::HashMap;

use bar_data::S3oVertex;
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::features::FeatureTexture;

const THUMB_SIZE: u32 = 96;

/// GPU resources for one thumbnail entry: source mesh + its own
/// offscreen render target. The offscreen texture is the resource the
/// caller registers with egui.
struct ThumbnailEntry {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    bind_group: wgpu::BindGroup,
    aabb_min: Vec3,
    aabb_max: Vec3,
    /// Backing texture for the offscreen target. Held so the texture
    /// view stays valid for as long as the entry lives. Not accessed
    /// directly after construction.
    #[allow(dead_code)]
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    /// Set once after the first render so subsequent renders reuse the
    /// existing target instead of dirtying it again every frame.
    rendered: bool,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    sun_dir: [f32; 3],
    _pad: f32,
}

pub struct FeatureThumbnailRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Fallback white texture used when an S3O has no usable diffuse.
    /// Held so the underlying texture (which the per-entry bind group
    /// references) stays alive.
    #[allow(dead_code)]
    default_diffuse_view: wgpu::TextureView,
    /// Per-feature uniform buffer (reused; one for the whole renderer
    /// since we render thumbnails serially).
    uniform_buffer: wgpu::Buffer,
    entries: HashMap<String, ThumbnailEntry>,
}

impl FeatureThumbnailRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("feature_thumbnail.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/feature_thumbnail.wgsl").into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("feature_thumbnail_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("feature_thumbnail_pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("feature_thumbnail_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
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
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("feature_thumbnail_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // 1x1 white default for models with no diffuse texture.
        let default_diffuse = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("feature_thumbnail_default_white"),
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
                texture: &default_diffuse,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8; 4],
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
        let default_diffuse_view =
            default_diffuse.create_view(&wgpu::TextureViewDescriptor::default());
        // Leak the default texture into the renderer by holding only its
        // view -- the underlying texture is kept alive by the device's
        // resource tracking via the bind groups that reference it.
        std::mem::forget(default_diffuse);

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("feature_thumbnail_uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            default_diffuse_view,
            uniform_buffer,
            entries: HashMap::new(),
        }
    }

    /// True when a thumbnail entry for this feature type has already been
    /// uploaded (mesh + texture + per-feature bind group).
    pub fn has(&self, feature_type: &str) -> bool {
        self.entries.contains_key(&feature_type.to_lowercase())
    }

    /// Upload mesh + optional diffuse texture for a feature type. Allocates
    /// the offscreen render target the first time we see this name;
    /// subsequent calls overwrite the existing entry.
    pub fn load_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        name: &str,
        mesh: &bar_data::S3oMesh,
        tex1: Option<&FeatureTexture>,
    ) {
        if mesh.vertices.is_empty() || mesh.indices.is_empty() {
            return;
        }

        // Shift Y so the visible base sits at model Y = 0 -- matches the
        // shift the main feature renderer applies. Keeps the camera-
        // framing math below simple (object centre at AABB midpoint).
        let shift_y = mesh.aabb_min[1].max(0.0);
        let vertices: Vec<S3oVertex> = if shift_y > 0.0 {
            mesh.vertices
                .iter()
                .map(|v| S3oVertex {
                    position: [v.position[0], v.position[1] - shift_y, v.position[2]],
                    normal: v.normal,
                    uv: v.uv,
                })
                .collect()
        } else {
            mesh.vertices.clone()
        };
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("thumb_vb_{name}")),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("thumb_ib_{name}")),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let diffuse_view = if let Some(t) = tex1 {
            upload_diffuse(device, queue, name, t)
        } else {
            // Bind groups need a real view; reuse the default white
            // via a freshly-created view of the same texture would
            // require holding the texture -- which we've std::mem::forget'd.
            // Workaround: create a per-entry white texture so the
            // bind group has a unique owned view.
            create_fallback_white(device, queue)
        };

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("thumb_bg_{name}")),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&diffuse_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        // Offscreen render target (kept alive in the entry so the egui
        // TextureId registered against the view stays valid).
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("thumb_target_{name}")),
            size: wgpu::Extent3d {
                width: THUMB_SIZE,
                height: THUMB_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let aabb_min = Vec3::from_array([
            mesh.aabb_min[0],
            mesh.aabb_min[1] - shift_y,
            mesh.aabb_min[2],
        ]);
        let aabb_max = Vec3::from_array([
            mesh.aabb_max[0],
            mesh.aabb_max[1] - shift_y,
            mesh.aabb_max[2],
        ]);

        self.entries.insert(
            name.to_lowercase(),
            ThumbnailEntry {
                vertex_buffer,
                index_buffer,
                num_indices: mesh.indices.len() as u32,
                bind_group,
                aabb_min,
                aabb_max,
                target,
                target_view,
                rendered: false,
            },
        );
    }

    /// Render (or re-render) the thumbnail for this feature type. Returns
    /// the offscreen texture view for the caller to register with egui.
    /// Returns `None` when no mesh has been uploaded for this name yet.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        feature_type: &str,
    ) -> Option<&wgpu::TextureView> {
        let key = feature_type.to_lowercase();
        let entry = self.entries.get_mut(&key)?;

        // Per-entry depth buffer is small; create on each render.
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("feature_thumbnail_depth"),
            size: wgpu::Extent3d {
                width: THUMB_SIZE,
                height: THUMB_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

        // Frame the AABB: look at its centre from a three-quarter view.
        let centre = (entry.aabb_min + entry.aabb_max) * 0.5;
        let extent = (entry.aabb_max - entry.aabb_min).length().max(1e-3);
        let dist = extent * 1.2;
        let azimuth = std::f32::consts::FRAC_PI_4;
        let elevation = 0.45_f32;
        let cam_offset = Vec3::new(
            dist * elevation.cos() * azimuth.cos(),
            dist * elevation.sin(),
            dist * elevation.cos() * azimuth.sin(),
        );
        let cam_pos = centre + cam_offset;
        let view = Mat4::look_at_rh(cam_pos, centre, Vec3::Y);
        let proj = Mat4::perspective_rh(0.7, 1.0, dist * 0.05, dist * 4.0);
        let view_proj = proj * view;
        let uniforms = Uniforms {
            view_proj: view_proj.to_cols_array_2d(),
            model: Mat4::IDENTITY.to_cols_array_2d(),
            sun_dir: [-0.35, 0.85, 0.4],
            _pad: 0.0,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("feature_thumbnail_encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("feature_thumbnail_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &entry.target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.10,
                            g: 0.11,
                            b: 0.13,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &entry.bind_group, &[]);
            pass.set_vertex_buffer(0, entry.vertex_buffer.slice(..));
            pass.set_index_buffer(entry.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..entry.num_indices, 0, 0..1);
        }
        queue.submit(Some(encoder.finish()));
        entry.rendered = true;
        Some(&entry.target_view)
    }

    /// True if a thumbnail has already been rendered for this feature
    /// type (used to skip re-rendering on every frame).
    pub fn is_rendered(&self, feature_type: &str) -> bool {
        self.entries
            .get(&feature_type.to_lowercase())
            .map(|e| e.rendered)
            .unwrap_or(false)
    }

    /// Drop all thumbnails. Called on project reset so a new map's
    /// catalog starts with a fresh cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn upload_diffuse(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    name: &str,
    tex: &FeatureTexture,
) -> wgpu::TextureView {
    let tex_handle = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("thumb_diffuse_{name}")),
        size: wgpu::Extent3d {
            width: tex.width,
            height: tex.height,
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
            texture: &tex_handle,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        tex.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * tex.width),
            rows_per_image: Some(tex.height),
        },
        wgpu::Extent3d {
            width: tex.width,
            height: tex.height,
            depth_or_array_layers: 1,
        },
    );
    let view = tex_handle.create_view(&wgpu::TextureViewDescriptor::default());
    std::mem::forget(tex_handle);
    view
}

fn create_fallback_white(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("thumb_fallback_white"),
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
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[255u8; 4],
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
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    std::mem::forget(tex);
    view
}
