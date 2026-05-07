//! GPU-accelerated filter pipelines (blur).
//!
//! Implements the same 3-pass box blur algorithm as the CPU path to ensure
//! deterministic, semantically identical results regardless of backend.

use bytemuck::{Pod, Zeroable};
use bar_data::Heightmap;
use thiserror::Error;
use tracing::info;
use wgpu::util::DeviceExt;

use crate::device::GpuContext;

#[derive(Error, Debug)]
pub enum GpuFilterError {
    #[error("GPU buffer mapping failed")]
    BufferMapping,

    #[error("heightmap too large for GPU buffer limits ({size} bytes exceeds {limit} bytes)")]
    BufferTooLarge { size: u64, limit: u64 },

    #[error("filter error: {0}")]
    Filter(String),
}

/// Uniform buffer for the blur shader.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuBlurParams {
    width: u32,
    height: u32,
    radius: u32,
    horizontal: u32,
}

/// GPU-based filter compute pipeline.
pub struct GpuFilterPipeline {
    blur_pipeline: wgpu::ComputePipeline,
    blur_layout: wgpu::BindGroupLayout,
}

impl GpuFilterPipeline {
    /// Create the filter compute pipeline.
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur_box"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/blur_box.wgsl").into(),
            ),
        });

        let blur_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("blur_bind_group_layout"),
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
                    // Input storage (read-only)
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
                    // Output storage (read-write)
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

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("blur_pipeline_layout"),
                bind_group_layouts: &[&blur_layout],
                push_constant_ranges: &[],
            });

        let blur_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("blur_pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

        Self {
            blur_pipeline,
            blur_layout,
        }
    }

    /// Apply 3-pass box blur on the GPU (matches CPU semantics exactly).
    pub fn box_blur(
        &self,
        ctx: &GpuContext,
        heightmap: &Heightmap,
        radius: u32,
    ) -> Result<Heightmap, GpuFilterError> {
        let w = heightmap.width();
        let h = heightmap.height();
        let num_pixels = (w as usize) * (h as usize);
        let buffer_size = (num_pixels * 4) as u64;
        let r = radius.clamp(1, 64);

        // Check buffer limits
        let limit = ctx.device.limits().max_storage_buffer_binding_size as u64;
        if buffer_size > limit {
            return Err(GpuFilterError::BufferTooLarge {
                size: buffer_size,
                limit,
            });
        }

        // Create two storage buffers for ping-pong
        let buffer_a =
            ctx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("blur_buffer_a"),
                    contents: bytemuck::cast_slice(heightmap.data()),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                });

        let buffer_b = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur_buffer_b"),
            size: buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur_staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let groups_x = w.div_ceil(16);
        let groups_y = h.div_ceil(16);

        let mut encoder =
            ctx.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("blur_encoder"),
                });

        // 3 passes × 2 directions = 6 dispatches
        // State tracking: which buffer is current input
        // Start: data in buffer_a
        // Pass pattern: A→B (horiz), B→A (vert), A→B (horiz), B→A (vert), A→B (horiz), B→A (vert)
        // After 6 dispatches: result in buffer_a
        for pass in 0..6u32 {
            let horizontal = if pass % 2 == 0 { 1u32 } else { 0u32 };

            let params = GpuBlurParams {
                width: w,
                height: h,
                radius: r,
                horizontal,
            };

            let params_buffer =
                ctx.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("blur_params"),
                        contents: bytemuck::bytes_of(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

            // Even passes: A→B, Odd passes: B→A
            let (input_buf, output_buf) = if pass % 2 == 0 {
                (&buffer_a, &buffer_b)
            } else {
                (&buffer_b, &buffer_a)
            };

            let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blur_bind_group"),
                layout: &self.blur_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: input_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: output_buf.as_entire_binding(),
                    },
                ],
            });

            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("blur_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.blur_pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        // After 6 passes (even count), result is in buffer_a
        encoder.copy_buffer_to_buffer(&buffer_a, 0, &staging_buffer, 0, buffer_size);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        // Read back
        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        ctx.device.poll(wgpu::Maintain::Wait);

        receiver
            .recv()
            .map_err(|_| GpuFilterError::BufferMapping)?
            .map_err(|_| GpuFilterError::BufferMapping)?;

        let data = buffer_slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data)[..num_pixels].to_vec();
        drop(data);
        staging_buffer.unmap();

        info!("GPU box blur complete: {}x{}, radius={}", w, h, r);

        Heightmap::frbar_data(w, h, result)
            .map_err(|e| GpuFilterError::Filter(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::GpuContext;

    fn try_gpu_context() -> Option<GpuContext> {
        pollster::block_on(GpuContext::new_standalone()).ok()
    }

    #[test]
    fn test_gpu_blur_smooths() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuFilterPipeline::new(&ctx.device);

        // Create a heightmap with a sharp spike
        let mut data = vec![0.0f32; 64 * 64];
        data[32 * 64 + 32] = 1.0; // single spike at center
        let input = Heightmap::frbar_data(64, 64, data).unwrap();

        let result = pipeline.box_blur(&ctx, &input, 3).unwrap();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 64);

        // After blur, the center spike should be reduced
        let center_val = result.data()[32 * 64 + 32];
        assert!(
            center_val < 1.0,
            "Blur should reduce spike: got {center_val}"
        );

        // And neighbors should have received some value
        let neighbor_val = result.data()[32 * 64 + 33];
        assert!(
            neighbor_val > 0.0,
            "Blur should spread to neighbors: got {neighbor_val}"
        );
    }

    #[test]
    fn test_gpu_blur_preserves_uniform() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuFilterPipeline::new(&ctx.device);

        // Uniform value should remain unchanged after blur
        let data = vec![0.5f32; 32 * 32];
        let input = Heightmap::frbar_data(32, 32, data).unwrap();

        let result = pipeline.box_blur(&ctx, &input, 4).unwrap();

        // All values should still be ~0.5 (edge effects may cause slight variation)
        for (i, &v) in result.data().iter().enumerate() {
            let x = i % 32;
            let y = i / 32;
            // Skip border pixels where edge clamping affects result
            if x >= 4 && x < 28 && y >= 4 && y < 28 {
                assert!(
                    (v - 0.5).abs() < 0.001,
                    "Interior pixel ({x},{y}) should be ~0.5, got {v}"
                );
            }
        }
    }

    #[test]
    fn test_gpu_blur_output_range() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuFilterPipeline::new(&ctx.device);

        // Input with values in [0, 1]
        let data: Vec<f32> = (0..64 * 64)
            .map(|i| (i as f32) / (64.0 * 64.0 - 1.0))
            .collect();
        let input = Heightmap::frbar_data(64, 64, data).unwrap();

        let result = pipeline.box_blur(&ctx, &input, 2).unwrap();

        for &v in result.data() {
            assert!(
                (0.0..=1.0).contains(&v),
                "GPU blur output value out of range: {v}"
            );
        }
    }
}

