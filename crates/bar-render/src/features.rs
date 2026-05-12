// SPDX-License-Identifier: GPL-2.0-or-later
//! Feature placeholder renderer.
//!
//! Renders placed map features (trees, rocks, geo-thermal vents, etc.) as
//! solid orange unit boxes at correct world positions. No game assets or
//! model data are required -- this is the M1 placeholder pass.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Per-vertex data for the placeholder cube mesh.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct FeatureVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

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

fn vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<FeatureVertex>() as wgpu::BufferAddress,
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

/// Unit cube with bottom face at Y=0, top face at Y=1.
/// Each face has 4 unique vertices with face normals (24 verts total).
fn unit_cube_verts() -> [FeatureVertex; 24] {
    macro_rules! v {
        ([$px:literal, $py:literal, $pz:literal], [$nx:literal, $ny:literal, $nz:literal]) => {
            FeatureVertex {
                position: [$px, $py, $pz],
                normal: [$nx, $ny, $nz],
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

/// Renders all map features as placeholder unit boxes.
///
/// Shares the camera uniform bind group (group 0) with `TerrainRenderer`.
/// Draws in a separate render pass after terrain using `LoadOp::Load` so
/// terrain depth correctly occludes features.
pub struct FeatureRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: Option<wgpu::Buffer>,
    pub(crate) instance_count: u32,
}

impl FeatureRenderer {
    pub fn new(
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        camera_bgl: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("feature_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/features.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("feature_pipeline_layout"),
            bind_group_layouts: &[camera_bgl],
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
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let verts = unit_cube_verts();
        let inds = unit_cube_indices();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("feature_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("feature_ib"),
            contents: bytemuck::cast_slice(&inds),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            instance_buffer: None,
            instance_count: 0,
        }
    }

    /// Rebuild the GPU instance buffer from the given slice.
    /// Pass an empty slice to hide all features.
    pub fn update_instances(&mut self, device: &wgpu::Device, instances: &[FeatureInstance]) {
        if instances.is_empty() {
            self.instance_buffer = None;
            self.instance_count = 0;
            return;
        }
        self.instance_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("feature_instance_buffer"),
                contents: bytemuck::cast_slice(instances),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        );
        self.instance_count = instances.len() as u32;
    }

    /// Record a feature render pass into `encoder` using `LoadOp::Load` on
    /// both color and depth so prior terrain geometry occludes features.
    /// No-ops when no instances have been uploaded.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        camera_bg: &wgpu::BindGroup,
    ) {
        let Some(ref inst_buf) = self.instance_buffer else {
            return;
        };
        if self.instance_count == 0 {
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
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, inst_buf.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..36, 0, 0..self.instance_count);
    }
}
