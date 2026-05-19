//! Map grass widget: instanced rendering of animated grass blades.
//!
//! Visible in-game via BAR's `map_grass_gl4` LuaUI widget
//! (`bar-game/luaui/Widgets/map_grass_gl4.lua`). The widget reads
//! `mapinfo.custom.grassConfig`, samples a per-map distribution mask
//! to pick patch positions, instances grass-blade quads at those
//! positions, and animates them via a wind-perturbation noise
//! texture.
//!
//! This module exposes three things:
//! - `MapGrassWidget`: per-map config (`from_settings`).
//! - `generate_instances`: CPU scan of the distribution mask
//!   producing per-blade transforms.
//! - `MapGrassPipeline`: GPU resources (pipeline, blade mesh,
//!   instance buffer, blade-colour texture, bind group). Owned by
//!   `TerrainRenderer`; render integration in
//!   `renderer.rs::render_internal`.
//!
//! Shader half: `shaders/widgets/map_grass_vs.wgsl` plus
//! `shaders/widgets/map_grass_fs.wgsl`. Separate files because a
//! widget with its own render pipeline needs both vertex and
//! fragment entry points; the runtime concats them at pipeline-
//! build time into a single WGSL module per stage.

use bar_project::MapSettings;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Resolved configuration for the grass widget. `enabled = false`
/// when the map has no `mapinfo.custom.grassConfig` block or it
/// lacks the required `grassDistTGA` distribution mask -- the
/// renderer skips the grass pass entirely in that case.
#[derive(Debug, Clone, PartialEq)]
pub struct MapGrassWidget {
    /// True iff the recipe has a grass-distribution path AND a blade
    /// colour texture. Both are required for the widget to render
    /// anything visible. Mirrors the BAR widget's own
    /// "early-out if `grassDistTGA` is empty" gate
    /// (`map_grass_gl4.lua:117`).
    pub enabled: bool,
    /// Distribution mask filename (relative to the map archive).
    /// The widget reads this at load time; non-zero texels seed
    /// grass-blade instances at the corresponding world positions.
    pub dist_tga: String,
    /// Blade-color texture filename. Sampled by the fragment shader.
    pub blade_color_tex: String,
    /// Maximum blade size for a distribution-mask byte of 254.
    /// Linearly interpolated against `min_size` based on the byte
    /// value (per the widget's `byteToSize` helper).
    pub max_size: f32,
    pub min_size: f32,
    /// Patch grid resolution in elmos. Spacing between candidate
    /// blade positions before jitter.
    pub patch_resolution: u32,
    /// Per-patch random XZ offset (fraction of `patch_resolution`).
    pub patch_placement_jitter: f32,
    /// `grassShaderParams.MAPCOLORFACTOR` -- multiplicative blend
    /// strength between blade colour and terrain albedo.
    pub map_color_factor: f32,
    /// `grassShaderParams.MAPCOLORBASE` -- additional albedo blend
    /// at the blade base (creates a smooth transition where the
    /// blade meets the terrain).
    pub map_color_base: f32,
}

impl Default for MapGrassWidget {
    fn default() -> Self {
        Self {
            enabled: false,
            dist_tga: String::new(),
            blade_color_tex: String::new(),
            max_size: 1.7,
            min_size: 0.4,
            patch_resolution: 32,
            patch_placement_jitter: 0.66,
            map_color_factor: 0.6,
            map_color_base: 1.0,
        }
    }
}

impl MapGrassWidget {
    /// Build from a recipe's `MapSettings.custom_grass` block. The
    /// `enabled` flag follows the BAR widget's own gate -- a grass
    /// configuration with no distribution-mask path produces a
    /// disabled widget (renderer never spawns the grass pass).
    pub fn from_settings(ms: &MapSettings) -> Self {
        let g = &ms.custom_grass;
        let enabled = !g.dist_tga.is_empty() && !g.blade_color_tex.is_empty();
        Self {
            enabled,
            dist_tga: g.dist_tga.clone(),
            blade_color_tex: g.blade_color_tex.clone(),
            max_size: g.max_size,
            min_size: g.min_size,
            patch_resolution: g.patch_resolution,
            patch_placement_jitter: g.patch_placement_jitter,
            map_color_factor: g.map_color_factor,
            map_color_base: g.map_color_base,
        }
    }
}

/// Per-blade instance data uploaded to the grass pipeline's instance
/// buffer. Matches BAR widget's `instancePosRotSize` vertex attribute
/// layout (`map_grass_gl4.vert.glsl:24`), but in BME's render space
/// (not engine elmos) so the vertex shader can multiply directly
/// against `camera.view_proj` without a conversion step.
///
/// Render-space units: `world_x` and `world_z` cover the playable
/// area as `[-x_extent, +x_extent]` / `[-z_extent, +z_extent]`
/// (typically `[-1, +1]`). `size` is the elmo-space blade-size
/// multiplier scaled by the renderer's elmo-to-render conversion
/// so the shader can multiply the static mesh's elmo-unit positions
/// by it and land in render space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GrassInstance {
    /// Render-space X position.
    pub world_x: f32,
    /// Random Y rotation in radians.
    pub rotation: f32,
    /// Render-space Z position.
    pub world_z: f32,
    /// Render-space size scalar (already converted from elmos via
    /// the renderer's elmo-to-render factor). The shader multiplies
    /// the mesh's elmo-unit positions by this.
    pub size: f32,
}

/// Static blade mesh vertex layout. Two crossed quads in object
/// space (origin at blade base). The vertex shader scales by
/// instance.size and rotates around Y, then anchors to the
/// heightmap at the instance's world XZ.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BladeVertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

/// Blade mesh vertex positions in **elmo units**, matching BAR's
/// `grassPatches.lua` blade-patch layout (height ~17 elmos, width
/// ~2 elmos). The vertex shader multiplies these by the
/// per-instance `size` (which is the elmo-space mapinfo multiplier
/// pre-converted to render units by `generate_instances`); a
/// `size = 2` instance therefore renders a ~34-elmo-tall blade in
/// render space, matching the engine's visible scale.
const BLADE_MESH_HEIGHT_ELMOS: f32 = 17.0;
const BLADE_MESH_HALF_WIDTH_ELMOS: f32 = 1.0;

/// 8 vertices, 12 indices (4 triangles). Each blade renders both
/// faces of two crossed quads; back-face culling is disabled on
/// the pipeline so we don't need to duplicate indices for the
/// reverse winding.
const BLADE_VERTICES: &[BladeVertex] = &[
    // Quad 1: axis along X.
    BladeVertex {
        pos: [-BLADE_MESH_HALF_WIDTH_ELMOS, 0.0, 0.0],
        uv: [0.0, 1.0],
    },
    BladeVertex {
        pos: [BLADE_MESH_HALF_WIDTH_ELMOS, 0.0, 0.0],
        uv: [1.0, 1.0],
    },
    BladeVertex {
        pos: [BLADE_MESH_HALF_WIDTH_ELMOS, BLADE_MESH_HEIGHT_ELMOS, 0.0],
        uv: [1.0, 0.0],
    },
    BladeVertex {
        pos: [-BLADE_MESH_HALF_WIDTH_ELMOS, BLADE_MESH_HEIGHT_ELMOS, 0.0],
        uv: [0.0, 0.0],
    },
    // Quad 2: axis along Z.
    BladeVertex {
        pos: [0.0, 0.0, -BLADE_MESH_HALF_WIDTH_ELMOS],
        uv: [0.0, 1.0],
    },
    BladeVertex {
        pos: [0.0, 0.0, BLADE_MESH_HALF_WIDTH_ELMOS],
        uv: [1.0, 1.0],
    },
    BladeVertex {
        pos: [0.0, BLADE_MESH_HEIGHT_ELMOS, BLADE_MESH_HALF_WIDTH_ELMOS],
        uv: [1.0, 0.0],
    },
    BladeVertex {
        pos: [0.0, BLADE_MESH_HEIGHT_ELMOS, -BLADE_MESH_HALF_WIDTH_ELMOS],
        uv: [0.0, 0.0],
    },
];

const BLADE_INDICES: &[u16] = &[
    0, 1, 2, 2, 3, 0, // quad 1
    4, 5, 6, 6, 7, 4, // quad 2
];

/// Wind sway amplitude in world elmos. Hardcoded -- the widget
/// exposes `WINDSTRENGTH` per map but every BAR map uses the
/// default `0.1`.
const WIND_STRENGTH: f32 = 0.1;
/// Distance (elmos) past which blades fully fade out. Linear ramp
/// starts at 0.65 of this value.
const FADE_END: f32 = 8000.0;

/// GPU-resident grass renderer. Owned by `TerrainRenderer`; lives
/// across map switches (the pipeline is static, only the instance
/// buffer + blade-colour texture get replaced when a new map's
/// `mapinfo.custom.grassConfig` lands).
pub struct MapGrassPipeline {
    pipeline: wgpu::RenderPipeline,
    blade_bgl: wgpu::BindGroupLayout,
    /// Static blade mesh (8 vertices, 12 indices). Allocated once
    /// at construction; the instance buffer is what changes per
    /// map.
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    /// Per-pipeline tuning constants (wind strength, blend factors,
    /// fade distance).
    params_buffer: wgpu::Buffer,
    /// Default 1x1 blade-color texture. Retained for the lifetime
    /// of the pipeline so `clear_blade_color` can reset to it
    /// cheaply, and so it stays alive even if `blade_color_texture`
    /// is swapped out mid-frame.
    #[allow(dead_code)]
    blade_color_default: wgpu::Texture,
    blade_color_texture: wgpu::Texture,
    blade_color_sampler: wgpu::Sampler,
    /// Bind group for the grass-specific resources (blade tex,
    /// heightmap, params uniform). Rebuilt whenever the blade
    /// texture OR heightmap texture changes.
    bind_group: Option<wgpu::BindGroup>,
    /// Per-map instance buffer. None until the distribution mask
    /// has been processed via `update_instances`.
    instance_buffer: Option<wgpu::Buffer>,
    instance_count: u32,
    /// Cached widget config; carries the per-map shader-blend
    /// factors that get re-packed into `params_buffer` after any
    /// recipe change.
    widget: MapGrassWidget,
}

impl MapGrassPipeline {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_bgl: &wgpu::BindGroupLayout,
        output_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map_grass_vs"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../shaders/widgets/map_grass_vs.wgsl").into(),
            ),
        });
        let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("map_grass_fs"),
            source: wgpu::ShaderSource::Wgsl({
                // Concat the VS source again so the FS module sees
                // the shared `blade_color_tex` / `blade_color_sam`
                // bindings declared in the VS file. WGSL modules
                // are single-translation-units; the simplest way to
                // share bindings between two entry points in
                // wgpu is to compile them as one module each, both
                // including the binding declarations.
                let vs_src = include_str!("../../../../shaders/widgets/map_grass_vs.wgsl");
                let fs_src = include_str!("../../../../shaders/widgets/map_grass_fs.wgsl");
                format!("{vs_src}\n{fs_src}").into()
            }),
        });

        let blade_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map_grass_bgl"),
            entries: &[
                // blade color texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // heightmap (non-filterable, R32Float).
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // grass params uniform.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map_grass_pipeline_layout"),
            bind_group_layouts: &[camera_bgl, &blade_bgl],
            push_constant_ranges: &[],
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_grass_vertices"),
            contents: bytemuck::cast_slice(BLADE_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_grass_indices"),
            contents: bytemuck::cast_slice(BLADE_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map_grass_params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let default_params = [WIND_STRENGTH, 0.6_f32, 1.0_f32, FADE_END];
        queue.write_buffer(&params_buffer, 0, bytemuck::bytes_of(&default_params));

        // 1x1 white default blade-colour texture. Inert until the
        // map upload replaces it -- with `enabled = false` on the
        // widget config the renderer doesn't draw anyway.
        let default_pixel: [u8; 4] = [255, 255, 255, 255];
        let blade_color_default = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_blade_color_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
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
            &default_pixel,
        );
        // The active texture starts as a clone of the default.
        let blade_color_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_blade_color"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
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
            &default_pixel,
        );
        let blade_color_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("map_grass_blade_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map_grass_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vs,
                entry_point: Some("vs_grass"),
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<BladeVertex>() as wgpu::BufferAddress,
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
                                format: wgpu::VertexFormat::Float32x2,
                            },
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GrassInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        }],
                    },
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // Disable culling so both sides of each quad render.
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                // Read existing depth (so terrain occludes far
                // blades), don't write to it -- writing would make
                // alpha-tested blades cast hard depth shadows across
                // the foliage edges where the alpha is sub-threshold.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fs,
                entry_point: Some("fs_grass"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            blade_bgl,
            vertex_buffer,
            index_buffer,
            params_buffer,
            blade_color_default,
            blade_color_texture,
            blade_color_sampler,
            bind_group: None,
            instance_buffer: None,
            instance_count: 0,
            widget: MapGrassWidget::default(),
        }
    }

    /// Update the per-map widget config + repack the params uniform.
    /// Returns the new `enabled` state so callers can branch on
    /// whether to even bother loading assets.
    ///
    /// `elmo_to_render` converts the elmo-space `FADE_END` constant
    /// into render units that match `world_pos` in the shader. The
    /// caller (`TerrainRenderer::sync_grass_assets`) computes this
    /// from the same `height_scale / height_range_elmos` factor it
    /// uses for instance sizes; without it the fade ramp would
    /// kick in way past the rendered viewport.
    pub fn set_config(
        &mut self,
        queue: &wgpu::Queue,
        widget: MapGrassWidget,
        elmo_to_render: f32,
    ) -> bool {
        let params = [
            WIND_STRENGTH,
            widget.map_color_factor,
            widget.map_color_base,
            FADE_END * elmo_to_render.max(1e-6),
        ];
        queue.write_buffer(&self.params_buffer, 0, bytemuck::bytes_of(&params));
        let enabled = widget.enabled;
        self.widget = widget;
        if !enabled {
            // Drop any stale instance buffer so a previous map's
            // grass doesn't ghost into the next one.
            self.instance_buffer = None;
            self.instance_count = 0;
        }
        enabled
    }

    /// Replace the blade-colour texture with a freshly-decoded RGBA
    /// asset. The bind group must be rebuilt afterwards via
    /// `rebuild_bind_group`.
    pub fn update_blade_color(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        self.blade_color_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_blade_color"),
                size: wgpu::Extent3d {
                    width,
                    height,
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
            rgba,
        );
    }

    /// Reset the blade-colour texture to the inert white default
    /// (e.g. on map switch when no grass widget is configured).
    pub fn clear_blade_color(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let default_pixel: [u8; 4] = [255, 255, 255, 255];
        self.blade_color_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("map_grass_blade_color_default"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
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
            &default_pixel,
        );
    }

    /// Upload (or replace) the instance buffer. An empty `instances`
    /// vector clears the buffer.
    pub fn update_instances(&mut self, device: &wgpu::Device, instances: &[GrassInstance]) {
        if instances.is_empty() {
            self.instance_buffer = None;
            self.instance_count = 0;
            return;
        }
        self.instance_buffer = Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("map_grass_instances"),
                contents: bytemuck::cast_slice(instances),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        );
        self.instance_count = instances.len() as u32;
    }

    /// Rebuild the grass bind group against the current blade-colour
    /// + heightmap views. Called every time either texture changes.
    pub fn rebuild_bind_group(
        &mut self,
        device: &wgpu::Device,
        heightmap_view: &wgpu::TextureView,
    ) {
        let blade_view = self
            .blade_color_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map_grass_bind_group"),
            layout: &self.blade_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&blade_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.blade_color_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(heightmap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.params_buffer.as_entire_binding(),
                },
            ],
        }));
    }

    /// Whether the renderer should issue the draw call this frame.
    /// True only when: config is enabled, AND we have instances,
    /// AND the bind group has been built (heightmap available).
    pub fn ready_to_draw(&self) -> bool {
        self.widget.enabled
            && self.instance_count > 0
            && self.instance_buffer.is_some()
            && self.bind_group.is_some()
    }

    /// Emit the grass draw inside an already-active render pass.
    /// Caller is responsible for setting the camera bind group
    /// (group 0); we bind the grass bind group at group 1 here.
    pub fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        if !self.ready_to_draw() {
            return;
        }
        let bg = match &self.bind_group {
            Some(bg) => bg,
            None => return,
        };
        let inst = match &self.instance_buffer {
            Some(b) => b,
            None => return,
        };
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(1, bg, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, inst.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..BLADE_INDICES.len() as u32, 0, 0..self.instance_count);
    }

    /// Read-only access to the cached widget config (used by
    /// callers that need to know `widget.enabled` after a
    /// `set_config` call without holding their own copy).
    pub fn widget(&self) -> &MapGrassWidget {
        &self.widget
    }
}

/// Tiny deterministic hash producing `[0, 1)` floats. Matches the
/// widget's per-patch jitter pattern (deterministic-per-position so
/// blades don't dance between frames).
fn hash01(x: i32, z: i32, salt: u32) -> f32 {
    // xorshift32-ish mix. Repeatable across runs, no random-state.
    let mut h: u32 = (x as u32).wrapping_mul(0x9E3779B1);
    h ^= (z as u32).wrapping_mul(0x85EBCA77);
    h ^= salt.wrapping_mul(0xC2B2AE3D);
    h ^= h.rotate_left(13);
    h = h.wrapping_mul(0x85EBCA6B);
    h ^= h >> 13;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// Scan an 8-bit distribution mask and produce one instance per
/// non-zero texel. The mask is sized
/// `(map_width_elmos / patch_resolution) x (map_height_elmos /
/// patch_resolution)`; each texel byte controls the blade size for
/// that patch (0 = no blade, 254 = max_size).
///
/// Output is in **render space**: positions land in
/// `[-x_extent_render, +x_extent_render]` /
/// `[-z_extent_render, +z_extent_render]`, and `size` is the
/// elmo-space multiplier already scaled by `elmo_to_render` so the
/// shader can apply it directly against the static mesh's
/// elmo-unit positions.
pub fn generate_instances(
    widget: &MapGrassWidget,
    mask: &[u8],
    mask_w: u32,
    mask_h: u32,
    x_extent_render: f32,
    z_extent_render: f32,
    elmo_to_render: f32,
) -> Vec<GrassInstance> {
    if !widget.enabled || mask.is_empty() {
        return Vec::new();
    }
    let stride_x = (2.0 * x_extent_render) / mask_w.max(1) as f32;
    let stride_z = (2.0 * z_extent_render) / mask_h.max(1) as f32;
    // Jitter as a fraction of the per-texel patch stride. Engine
    // uses `patch_placement_jitter * patch_resolution_elmos`; we
    // mirror that in render space because the mask was sized so
    // that `stride_elmos == patch_resolution`.
    let jitter_amount_x = widget.patch_placement_jitter * stride_x;
    let jitter_amount_z = widget.patch_placement_jitter * stride_z;
    let size_range = (widget.max_size - widget.min_size).max(0.0);

    let mut instances = Vec::new();
    for z in 0..mask_h {
        for x in 0..mask_w {
            let byte = mask[(z * mask_w + x) as usize];
            if byte == 0 {
                continue;
            }
            // Patch centre + per-patch jitter (deterministic from
            // grid position so two loads of the same map produce
            // identical placements).
            let jitter_x = (hash01(x as i32, z as i32, 0) - 0.5) * 2.0 * jitter_amount_x;
            let jitter_z = (hash01(x as i32, z as i32, 1) - 0.5) * 2.0 * jitter_amount_z;
            let rotation = hash01(x as i32, z as i32, 2) * std::f32::consts::TAU;
            let size_elmos = widget.min_size + (byte as f32 / 254.0) * size_range;
            let world_x = -x_extent_render + (x as f32 + 0.5) * stride_x + jitter_x;
            let world_z = -z_extent_render + (z as f32 + 0.5) * stride_z + jitter_z;
            instances.push(GrassInstance {
                world_x,
                rotation,
                world_z,
                size: size_elmos * elmo_to_render,
            });
        }
    }
    instances
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_project::recipe::CustomGrassSettings;

    #[test]
    fn default_is_disabled() {
        let w = MapGrassWidget::default();
        assert!(!w.enabled);
    }

    #[test]
    fn missing_dist_tga_disables_widget() {
        // BAR's widget mirrors this: no distribution mask -> no
        // patches to spawn from, so the whole pass is dead. We
        // surface that as `enabled = false` so the renderer skips
        // the grass draw call entirely.
        let ms = MapSettings {
            custom_grass: CustomGrassSettings {
                blade_color_tex: "maps/blades.dds".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let w = MapGrassWidget::from_settings(&ms);
        assert!(!w.enabled);
    }

    #[test]
    fn missing_blade_color_tex_disables_widget() {
        let ms = MapSettings {
            custom_grass: CustomGrassSettings {
                dist_tga: "maps/dist.tga".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let w = MapGrassWidget::from_settings(&ms);
        assert!(!w.enabled);
    }

    #[test]
    fn disabled_widget_produces_no_instances() {
        let widget = MapGrassWidget::default();
        let inst = generate_instances(&widget, &[1, 1, 1, 1], 2, 2, 1.0, 1.0, 1.0);
        assert!(inst.is_empty());
    }

    #[test]
    fn non_zero_mask_seeds_one_instance_per_texel() {
        let widget = MapGrassWidget {
            enabled: true,
            dist_tga: "x".to_string(),
            blade_color_tex: "y".to_string(),
            patch_resolution: 32,
            patch_placement_jitter: 0.0, // disable jitter for predictable test
            ..MapGrassWidget::default()
        };
        // 2x2 mask: top-left and bottom-right set. Render-space
        // half-extents of 1.0 give a [-1, +1] playable area.
        let mask = [254u8, 0, 0, 127];
        let inst = generate_instances(&widget, &mask, 2, 2, 1.0, 1.0, 1.0);
        assert_eq!(inst.len(), 2);
        // First instance ~ patch centre (-0.5, -0.5), second ~ (+0.5, +0.5).
        assert!((inst[0].world_x - -0.5).abs() < 1e-3);
        assert!((inst[0].world_z - -0.5).abs() < 1e-3);
        assert!((inst[1].world_x - 0.5).abs() < 1e-3);
        // Size scales with byte value.
        assert!(inst[0].size > inst[1].size);
    }

    #[test]
    fn deterministic_jitter() {
        let widget = MapGrassWidget {
            enabled: true,
            dist_tga: "x".to_string(),
            blade_color_tex: "y".to_string(),
            patch_placement_jitter: 0.5,
            ..MapGrassWidget::default()
        };
        let mask = vec![100u8; 16];
        let a = generate_instances(&widget, &mask, 4, 4, 1.0, 1.0, 1e-3);
        let b = generate_instances(&widget, &mask, 4, 4, 1.0, 1.0, 1e-3);
        assert_eq!(a.len(), b.len());
        for (ai, bi) in a.iter().zip(b.iter()) {
            assert_eq!(ai.world_x, bi.world_x);
            assert_eq!(ai.world_z, bi.world_z);
            assert_eq!(ai.rotation, bi.rotation);
        }
    }

    #[test]
    fn size_is_pre_converted_to_render_units() {
        let widget = MapGrassWidget {
            enabled: true,
            dist_tga: "x".to_string(),
            blade_color_tex: "y".to_string(),
            max_size: 2.0,
            min_size: 2.0, // pin size so byte value doesn't matter
            patch_placement_jitter: 0.0,
            ..MapGrassWidget::default()
        };
        let mask = [254u8];
        let inst = generate_instances(&widget, &mask, 1, 1, 1.0, 1.0, 1e-3);
        // 2 elmos * 1e-3 render-per-elmo = 0.002 render units.
        assert!((inst[0].size - 0.002).abs() < 1e-6);
    }

    #[test]
    fn full_config_enables_and_round_trips() {
        let ms = MapSettings {
            custom_grass: CustomGrassSettings {
                dist_tga: "maps/dist.tga".to_string(),
                blade_color_tex: "maps/blades.dds".to_string(),
                max_size: 2.0,
                min_size: 0.5,
                patch_resolution: 16,
                patch_placement_jitter: 0.4,
                map_color_factor: 0.2,
                map_color_base: 0.6,
            },
            ..Default::default()
        };
        let w = MapGrassWidget::from_settings(&ms);
        assert!(w.enabled);
        assert_eq!(w.dist_tga, "maps/dist.tga");
        assert_eq!(w.blade_color_tex, "maps/blades.dds");
        assert!((w.max_size - 2.0).abs() < 1e-6);
        assert!((w.map_color_factor - 0.2).abs() < 1e-6);
    }
}
