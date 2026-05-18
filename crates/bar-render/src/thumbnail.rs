//! Offscreen renderer for S3O thumbnails shown in the feature palette.
//!
//! Renders one S3O model at a fixed three-quarter-view camera with
//! simple Lambert + ambient lighting into a 128x128 Rgba8UnormSrgb
//! target, then reads the pixels back to CPU. The caller hands the
//! bytes to egui (`ctx.load_texture`) and optionally writes them to a
//! persistent on-disk cache so subsequent app launches skip the
//! render entirely.
//!
//! `THUMB_SIZE` is 128: `128 * 4 = 512` bytes/row, satisfying wgpu's
//! `COPY_BYTES_PER_ROW_ALIGNMENT = 256`. Without that alignment the
//! texture-to-buffer copy needs a padded staging layout.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bar_data::S3oVertex;
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::features::FeatureTexture;

pub const THUMB_SIZE: u32 = 128;

/// CPU resources for one feature type: mesh buffers + per-feature
/// bind group. The render target is allocated transiently per
/// `render_to_rgba` call, so there's no GPU memory held per cached
/// feature -- only the source mesh, which is small.
struct ThumbnailEntry {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    bind_group: wgpu::BindGroup,
    aabb_min: Vec3,
    aabb_max: Vec3,
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
    /// Per-feature uniform buffer (reused; renders are serialised).
    uniform_buffer: wgpu::Buffer,
    entries: HashMap<String, ThumbnailEntry>,
}

impl FeatureThumbnailRenderer {
    pub fn new(device: &wgpu::Device) -> Self {
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
            uniform_buffer,
            entries: HashMap::new(),
        }
    }

    /// True when a thumbnail entry for this feature type has been
    /// uploaded already.
    pub fn has(&self, feature_type: &str) -> bool {
        self.entries.contains_key(&feature_type.to_lowercase())
    }

    /// Upload mesh + optional diffuse for a feature type. Replaces any
    /// existing entry of the same name.
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

        // Shift Y so the visible base sits at model Y = 0 -- matches
        // the main feature renderer's anchor shift; keeps the
        // bounding-box centring sane.
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

        let diffuse_view = match tex1 {
            Some(t) => upload_diffuse(device, queue, name, t),
            None => upload_fallback_white(device, queue),
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
            },
        );
    }

    /// Render the thumbnail for this feature type, copy the pixels
    /// back to CPU, and return them as RGBA8 (THUMB_SIZE x THUMB_SIZE).
    /// Returns `None` if no mesh has been uploaded for this name yet.
    /// This call blocks until the GPU readback completes -- thumbnails
    /// are small (64KiB at THUMB_SIZE = 128) so the wait is sub-frame.
    pub fn render_to_rgba(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        feature_type: &str,
    ) -> Option<Vec<u8>> {
        let key = feature_type.to_lowercase();
        let entry = self.entries.get(&key)?;

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("feature_thumbnail_target"),
            size: wgpu::Extent3d {
                width: THUMB_SIZE,
                height: THUMB_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

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

        let bytes_per_row = THUMB_SIZE * 4;
        assert!(bytes_per_row.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("feature_thumbnail_staging"),
            size: (bytes_per_row * THUMB_SIZE) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // Frame the AABB from a three-quarter view.
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
                    view: &target_view,
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
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(THUMB_SIZE),
                },
            },
            wgpu::Extent3d {
                width: THUMB_SIZE,
                height: THUMB_SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        // Map + block until the readback completes. Uses a shared flag
        // because `map_async` invokes the callback from the wgpu
        // device poll thread.
        let mapped = Arc::new(Mutex::new(None));
        let mapped_clone = Arc::clone(&mapped);
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                *mapped_clone.lock().unwrap() = Some(result);
            });
        let _ = device.poll(wgpu::MaintainBase::Wait);
        let result = mapped.lock().unwrap().take()?;
        result.ok()?;
        let data = staging.slice(..).get_mapped_range().to_vec();
        Some(data)
    }

    /// Drop all uploaded thumbnail meshes. Called on project reset.
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

fn upload_fallback_white(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
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
