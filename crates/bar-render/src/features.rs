// SPDX-License-Identifier: GPL-2.0-or-later
//! Feature renderer -- placeholder boxes (M1) and real S3O models (M3).
//!
//! Unknown feature types render as solid orange unit boxes. Known types with
//! loaded S3O models render their actual geometry.
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
}

// ── FeatureRenderer ───────────────────────────────────────────────────────────

/// Renders all map features, either as placeholder unit boxes or as real S3O
/// models when `load_mesh` has been called for that feature type.
///
/// Shares the camera uniform bind group (group 0) with `TerrainRenderer`.
/// All geometry is drawn in a single render pass (LoadOp::Load on color + depth).
pub struct FeatureRenderer {
    pipeline: wgpu::RenderPipeline,
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
            placeholder_vb,
            placeholder_ib,
            placeholder_instances: None,
            placeholder_count: 0,
            meshes: HashMap::new(),
        }
    }

    /// Upload a real S3O model for a named feature type.
    /// If a model already exists for this name it is replaced.
    /// Models with no geometry (e.g. hierarchical root pieces not yet supported)
    /// are silently skipped; those features render as placeholder boxes.
    pub fn load_mesh(&mut self, device: &wgpu::Device, name: &str, mesh: &bar_data::S3oMesh) {
        if mesh.vertices.is_empty() || mesh.indices.is_empty() {
            return;
        }
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("feature_vb_{name}")),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("feature_ib_{name}")),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.meshes.insert(
            name.to_lowercase(),
            FeatureMesh {
                vertex_buffer: vb,
                index_buffer: ib,
                num_indices: mesh.indices.len() as u32,
                instance_buffer: None,
                instance_count: 0,
            },
        );
    }

    /// True if a real S3O model has been loaded for this feature type.
    pub fn has_model(&self, feature_type: &str) -> bool {
        self.meshes.contains_key(&feature_type.to_lowercase())
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
    /// No-ops if there are no instances to draw.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        camera_bg: &wgpu::BindGroup,
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

        // Draw real S3O models.
        for mesh in self.meshes.values() {
            let Some(ref inst_buf) = mesh.instance_buffer else {
                continue;
            };
            if mesh.instance_count == 0 {
                continue;
            }
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, inst_buf.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_indices, 0, 0..mesh.instance_count);
        }

        // Draw placeholder boxes for unknown feature types.
        if let Some(ref inst_buf) = self.placeholder_instances {
            if self.placeholder_count > 0 {
                pass.set_vertex_buffer(0, self.placeholder_vb.slice(..));
                pass.set_vertex_buffer(1, inst_buf.slice(..));
                pass.set_index_buffer(self.placeholder_ib.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..36, 0, 0..self.placeholder_count);
            }
        }
    }
}
