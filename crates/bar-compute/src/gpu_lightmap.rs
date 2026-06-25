//! GPU lightmap bake pipeline (horizon AO + soft sun shadow).
//!
//! Mirrors `gpu_noise.rs`: a `#[repr(C)] Pod` uniform that matches the WGSL
//! struct in field order + std140 padding, a `new()` that builds the pipeline
//! from `shaders/lightmap.wgsl`, and a `bake()` that uploads the heightfield to
//! a read-only storage buffer, dispatches a 16x16 grid, and reads back an RGBA
//! storage buffer into a `ColorBuffer`.

use bar_data::{ColorBuffer, Heightmap};
use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use tracing::info;
use wgpu::util::DeviceExt;

use crate::device::GpuContext;
use crate::lightmap::LightmapParams;

#[derive(Error, Debug)]
pub enum GpuLightmapError {
    #[error("GPU buffer mapping failed")]
    BufferMapping,

    #[error("lightmap pipeline error: {0}")]
    Pipeline(String),
}

/// Uniform layout for the lightmap shader. Must match the WGSL `LightmapParams`
/// struct exactly (field order + std140 16-byte alignment). `sun_dir` is a
/// padded vec4; its `w` lane is unused.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuLightmapParams {
    width: u32,
    height: u32,
    num_directions: u32,
    max_steps: u32,
    ao_strength: f32,
    ao_radius: f32,
    sun_softness: f32,
    _pad0: f32,
    sun_dir: [f32; 4],
}

impl From<&LightmapParams> for GpuLightmapParams {
    fn from(p: &LightmapParams) -> Self {
        Self {
            width: p.width,
            height: p.height,
            num_directions: p.num_directions,
            max_steps: p.max_steps,
            ao_strength: p.ao_strength,
            ao_radius: p.ao_radius,
            sun_softness: p.sun_softness,
            _pad0: 0.0,
            sun_dir: [p.sun_dir[0], p.sun_dir[1], p.sun_dir[2], 0.0],
        }
    }
}

/// GPU-based lightmap bake pipeline.
pub struct GpuLightmapPipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl GpuLightmapPipeline {
    /// Create the lightmap compute pipeline.
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lightmap"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/lightmap.wgsl").into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lightmap_bind_group_layout"),
            entries: &[
                // Uniform params
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Input heightfield (read-only storage)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Output RGBA (read-write storage)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lightmap_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("lightmap_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }

    /// Bake a lightmap on the GPU. Output channels match the CPU path:
    /// R = AO, G = sun visibility, B = AO*sun, A = 1.0.
    pub fn bake(
        &self,
        ctx: &GpuContext,
        heightmap: &Heightmap,
        params: &LightmapParams,
    ) -> Result<ColorBuffer, GpuLightmapError> {
        let w = params.width;
        let h = params.height;
        let num_pixels = (w as usize) * (h as usize);
        let output_size = (num_pixels * 4 * 4) as u64; // RGBA f32 per pixel

        let gpu_params: GpuLightmapParams = params.into();

        let params_buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lightmap_params"),
                contents: bytemuck::bytes_of(&gpu_params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let input_buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lightmap_input"),
                contents: bytemuck::cast_slice(heightmap.data()),
                usage: wgpu::BufferUsages::STORAGE,
            });

        let output_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lightmap_output"),
            size: output_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lightmap_staging"),
            size: output_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lightmap_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: input_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lightmap_encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("lightmap_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let groups_x = w.div_ceil(16);
            let groups_y = h.div_ceil(16);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, output_size);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        ctx.device.poll(wgpu::Maintain::Wait);

        receiver
            .recv()
            .map_err(|_| GpuLightmapError::BufferMapping)?
            .map_err(|_| GpuLightmapError::BufferMapping)?;

        let data = buffer_slice.get_mapped_range();
        let rgba: Vec<f32> = bytemuck::cast_slice(&data)[..num_pixels * 4].to_vec();
        drop(data);
        staging_buffer.unmap();

        info!("GPU lightmap baked: {}x{}", w, h);

        ColorBuffer::frbar_data(w, h, rgba).map_err(|e| GpuLightmapError::Pipeline(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lightmap::bake_lightmap_cpu;

    fn try_gpu_context() -> Option<GpuContext> {
        pollster::block_on(GpuContext::new_standalone()).ok()
    }

    fn central_spike(w: u32, h: u32) -> Heightmap {
        let mut data = vec![0.0f32; (w * h) as usize];
        data[((h / 2) * w + w / 2) as usize] = 1.0;
        Heightmap::frbar_data(w, h, data).unwrap()
    }

    #[test]
    fn gpu_matches_cpu_within_tolerance() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuLightmapPipeline::new(&ctx.device);
        let hm = central_spike(64, 64);
        let params = LightmapParams {
            width: 64,
            height: 64,
            ao_radius: 0.3,
            sun_dir: [0.6, 0.5, 0.62],
            sun_softness: 0.2,
            ..Default::default()
        };

        let gpu = pipeline.bake(&ctx, &hm, &params).unwrap();
        let cpu = bake_lightmap_cpu(&hm, &params);

        assert_eq!(gpu.width(), cpu.width());
        assert_eq!(gpu.height(), cpu.height());

        let mut max_diff = 0.0f32;
        for (g, c) in gpu.data().iter().zip(cpu.data().iter()) {
            max_diff = max_diff.max((g - c).abs());
        }
        assert!(
            max_diff < 2e-3,
            "GPU lightmap diverges from CPU by {max_diff}"
        );
    }

    #[test]
    fn gpu_output_in_unit_range() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuLightmapPipeline::new(&ctx.device);
        let hm = central_spike(48, 48);
        let params = LightmapParams {
            width: 48,
            height: 48,
            ..Default::default()
        };

        let cb = pipeline.bake(&ctx, &hm, &params).unwrap();
        for &v in cb.data() {
            assert!((0.0..=1.0).contains(&v), "GPU value out of range: {v}");
        }
    }
}
