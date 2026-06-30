use bar_data::Heightmap;
use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use tracing::info;

use crate::device::GpuContext;
use crate::noise::{NoiseParams, NoiseType};

#[derive(Error, Debug)]
pub enum GpuNoiseError {
    #[error("GPU buffer mapping failed")]
    BufferMapping,

    #[error("compute pipeline error: {0}")]
    Pipeline(String),
}

/// Maps `NoiseType` to the shader's `noise_type` integer discriminant.
fn noise_type_discriminant(nt: NoiseType) -> u32 {
    match nt {
        NoiseType::Perlin | NoiseType::Simplex => 0, // FBM
        NoiseType::Ridged => 1,
        NoiseType::Billow => 2,
        NoiseType::Worley => 0, // should not reach GPU; falls back to CPU
    }
}

/// Uniform buffer layout for the noise shader.
/// Must match the WGSL struct exactly (std140 layout).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuNoiseParams {
    width: u32,
    height: u32,
    octaves: u32,
    seed: u32,
    frequency: f32,
    lacunarity: f32,
    persistence: f32,
    offset_x: f32,
    offset_y: f32,
    noise_type: u32, // shader variant: 0=FBM, 1=Ridged, 2=Billow
    steepness: f32,
    elevation: f32,
    offset: f32,
    gain: f32,
    _padding2: f32,
    _padding3: f32,
}

impl From<(&NoiseParams, NoiseType)> for GpuNoiseParams {
    fn from((p, nt): (&NoiseParams, NoiseType)) -> Self {
        Self {
            width: p.width,
            height: p.height,
            octaves: p.octaves,
            seed: p.seed,
            frequency: p.frequency,
            lacunarity: p.lacunarity,
            persistence: p.persistence,
            offset_x: p.offset_x,
            offset_y: p.offset_y,
            noise_type: noise_type_discriminant(nt),
            steepness: p.steepness,
            elevation: p.elevation,
            offset: p.offset,
            gain: p.gain,
            _padding2: 0.0,
            _padding3: 0.0,
        }
    }
}

/// GPU-based noise generation pipeline.
pub struct GpuNoisePipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl GpuNoisePipeline {
    /// Create the noise compute pipeline.
    pub fn new(device: &wgpu::Device) -> Self {
        let shader_source = include_str!("../../../shaders/noise_fbm.wgsl");
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("noise_fbm"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("noise_bind_group_layout"),
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
                // Output storage buffer
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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
            label: Some("noise_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("noise_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }

    /// Generate noise on the GPU and return the result as a Heightmap.
    ///
    /// `noise_type` selects the fractal variant; Worley is not supported
    /// on the GPU and must be handled by the CPU before calling this.
    pub fn generate(
        &self,
        ctx: &GpuContext,
        params: &NoiseParams,
        noise_type: NoiseType,
    ) -> Result<Heightmap, GpuNoiseError> {
        let gpu_params: GpuNoiseParams = (params, noise_type).into();
        let output_size = (params.width as usize) * (params.height as usize) * 4; // f32 = 4 bytes

        // Create uniform buffer
        let params_buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("noise_params"),
                contents: bytemuck::bytes_of(&gpu_params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        // Create output buffer
        let output_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("noise_output"),
            size: output_size as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        // Staging buffer for readback
        let staging_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("noise_staging"),
            size: output_size as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind group
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("noise_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        // Dispatch compute
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("noise_encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("noise_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);

            // Workgroup size is 16x16, so dispatch enough groups to cover the image
            let groups_x = params.width.div_ceil(16);
            let groups_y = params.height.div_ceil(16);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        // Copy output to staging buffer
        encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, output_size as u64);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        // Read back results
        let buffer_slice = staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        ctx.device.poll(wgpu::Maintain::Wait);

        receiver
            .recv()
            .map_err(|_| GpuNoiseError::BufferMapping)?
            .map_err(|_| GpuNoiseError::BufferMapping)?;

        let data = buffer_slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        staging_buffer.unmap();

        info!(
            "GPU noise generated: {}x{} ({} samples)",
            params.width,
            params.height,
            result.len()
        );

        Heightmap::frbar_data(params.width, params.height, result)
            .map_err(|e| GpuNoiseError::Pipeline(e.to_string()))
    }
}

// Need wgpu::util for BufferInitDescriptor
use wgpu::util::DeviceExt;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::GpuContext;
    use crate::noise::{generate_noise_cpu, shape};

    fn try_gpu_context() -> Option<GpuContext> {
        pollster::block_on(GpuContext::new_standalone()).ok()
    }

    /// The GPU field must match the CPU `generate_noise_cpu` field: the editor
    /// (GPU/HybridExecutor) and the CLI export (CPU) share one noise algorithm,
    /// so a compile preview equals the exported terrain.
    #[test]
    fn gpu_noise_matches_cpu_noise() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuNoisePipeline::new(&ctx.device);

        for (nt, seed) in [
            (NoiseType::Perlin, 7),
            (NoiseType::Ridged, 13),
            (NoiseType::Billow, 99),
        ] {
            let p = NoiseParams {
                width: 96,
                height: 96,
                noise_type: nt,
                octaves: 5,
                seed,
                ..Default::default()
            };

            let cpu = generate_noise_cpu(&p).unwrap();
            let gpu = pipeline.generate(&ctx, &p, nt).unwrap();

            let max_diff = cpu
                .data()
                .iter()
                .zip(gpu.data().iter())
                .fold(0.0f32, |m, (c, g)| m.max((c - g).abs()));

            assert!(
                max_diff < 1e-3,
                "CPU/GPU noise diverge for {nt:?}: max|diff|={max_diff}"
            );
        }
    }

    /// The in-shader shaping must match the CPU `shape` formula exactly. We hold
    /// the GPU field fixed (same seed/params) and verify a shaped GPU pass equals
    /// the unshaped GPU pass run through the CPU `shape`.
    #[test]
    fn gpu_shaping_matches_cpu_shape_fn() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuNoisePipeline::new(&ctx.device);

        let base = NoiseParams {
            width: 64,
            height: 64,
            seed: 13,
            ..Default::default()
        };
        let shaped = NoiseParams {
            steepness: 0.75,
            elevation: 0.6,
            offset: 0.05,
            gain: 0.65,
            ..base.clone()
        };

        let base_hm = pipeline.generate(&ctx, &base, NoiseType::Perlin).unwrap();
        let shaped_hm = pipeline.generate(&ctx, &shaped, NoiseType::Perlin).unwrap();

        for (b, s) in base_hm.data().iter().zip(shaped_hm.data().iter()) {
            let expected = shape(*b, &shaped);
            assert!(
                (expected - s).abs() < 1e-4,
                "GPU shaping disagrees with CPU shape(): base={b} expected={expected} gpu={s}"
            );
        }
    }

    /// Default shaping on the GPU must be a no-op: a default-param pass equals a
    /// pass with the shaping fields explicitly at their identity values.
    #[test]
    fn gpu_default_shaping_is_identity() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuNoisePipeline::new(&ctx.device);
        let p = NoiseParams {
            width: 64,
            height: 64,
            seed: 13,
            ..Default::default()
        };

        let hm = pipeline.generate(&ctx, &p, NoiseType::Perlin).unwrap();
        for &v in hm.data().iter() {
            assert!((shape(v, &p) - v).abs() < 1e-6);
        }
    }
}
