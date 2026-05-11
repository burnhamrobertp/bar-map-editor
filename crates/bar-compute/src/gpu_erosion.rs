//! GPU-accelerated erosion pipelines (hydraulic + thermal).
//!
//! Hydraulic erosion uses a virtual-pipe shallow-water flow model (Mei et al.
//! 2007) dispatched in 4 serial passes per iteration. This replaces the old
//! particle-based shader which suffered from data races on the heightmap.
//!
//! Thermal erosion uses ping-pong buffering. Each cell computes both its loss
//! to lower neighbours and its gain from higher neighbours, preserving mass.

use bar_data::Heightmap;
use bytemuck::{Pod, Zeroable};
use thiserror::Error;
use tracing::info;
use wgpu::util::DeviceExt;

use crate::device::GpuContext;
use crate::erosion::{FlowErosionParams, HydraulicErosionParams, ThermalErosionParams};

#[derive(Error, Debug)]
pub enum GpuErosionError {
    #[error("GPU buffer mapping failed")]
    BufferMapping,

    #[error("heightmap too large for GPU buffer limits ({size} bytes exceeds {limit} bytes)")]
    BufferTooLarge { size: u64, limit: u64 },

    #[error("compute pipeline error: {0}")]
    Pipeline(String),
}

/// Uniform buffer layout for hydraulic erosion shader.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuHydraulicParams {
    width: u32,
    height: u32,
    num_droplets: u32,
    seed: u32,
    inertia: f32,
    capacity_factor: f32,
    min_capacity: f32,
    deposition_rate: f32,
    erosion_rate: f32,
    evaporation_rate: f32,
    gravity: f32,
    max_lifetime: u32,
    erosion_radius: u32,
    _padding: u32,
}

/// Uniform buffer layout for thermal erosion shader.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuThermalParams {
    width: u32,
    height: u32,
    talus_angle: f32,
    erosion_rate: f32,
}

/// Uniform buffer layout for the virtual-pipe hydraulic flow shader.
/// Field order must match the FlowParams struct in erosion_hydraulic_flow.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuFlowParams {
    width: u32,
    height: u32,
    dt: f32,
    pipe_length: f32,
    gravity: f32,
    rain_rate: f32,
    evaporation_rate: f32,
    sediment_capacity: f32,
    erosion_rate: f32,
    deposition_rate: f32,
    min_tilt: f32,
    _padding: u32,
}

/// GPU-based erosion compute pipelines.
pub struct GpuErosionPipeline {
    hydraulic_pipeline: wgpu::ComputePipeline,
    hydraulic_layout: wgpu::BindGroupLayout,
    thermal_pipeline: wgpu::ComputePipeline,
    thermal_layout: wgpu::BindGroupLayout,
    // Virtual-pipe hydraulic flow — 4 passes per iteration
    flow_flux_pipeline: wgpu::ComputePipeline,
    flow_water_vel_pipeline: wgpu::ComputePipeline,
    flow_erosion_pipeline: wgpu::ComputePipeline,
    flow_apply_pipeline: wgpu::ComputePipeline,
    flow_layout: wgpu::BindGroupLayout,
}

impl GpuErosionPipeline {
    /// Create both erosion compute pipelines.
    pub fn new(device: &wgpu::Device) -> Self {
        let hydraulic_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("erosion_hydraulic"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/erosion_hydraulic.wgsl").into(),
            ),
        });

        let thermal_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("erosion_thermal"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/erosion_thermal.wgsl").into(),
            ),
        });

        // Hydraulic: uniform params + read_write heightmap storage
        let hydraulic_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hydraulic_bind_group_layout"),
            entries: &[
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

        let hydraulic_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("hydraulic_pipeline_layout"),
                bind_group_layouts: &[&hydraulic_layout],
                push_constant_ranges: &[],
            });

        let hydraulic_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("hydraulic_pipeline"),
            layout: Some(&hydraulic_pipeline_layout),
            module: &hydraulic_shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        // Thermal: uniform params + read_write input + read_write output (ping-pong)
        let thermal_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("thermal_bind_group_layout"),
            entries: &[
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

        let thermal_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("thermal_pipeline_layout"),
                bind_group_layouts: &[&thermal_layout],
                push_constant_ranges: &[],
            });

        let thermal_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("thermal_pipeline"),
            layout: Some(&thermal_pipeline_layout),
            module: &thermal_shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Self {
            hydraulic_pipeline,
            hydraulic_layout,
            thermal_pipeline,
            thermal_layout,
            flow_flux_pipeline: Self::make_flow_pipeline(device, "pass_flux"),
            flow_water_vel_pipeline: Self::make_flow_pipeline(device, "pass_water_vel"),
            flow_erosion_pipeline: Self::make_flow_pipeline(device, "pass_erosion"),
            flow_apply_pipeline: Self::make_flow_pipeline(device, "pass_apply"),
            flow_layout: Self::make_flow_layout(device),
        }
    }

    /// Create the shared 8-binding layout for all 4 flow passes.
    fn make_flow_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        let storage = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("flow_bind_group_layout"),
            entries: &[
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
                storage(1), // terrain
                storage(2), // water
                storage(3), // sediment
                storage(4), // flux
                storage(5), // velocity
                storage(6), // scratch
                storage(7), // sediment_out
            ],
        })
    }

    /// Compile one entry point from the hydraulic flow shader.
    fn make_flow_pipeline(device: &wgpu::Device, entry_point: &str) -> wgpu::ComputePipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(entry_point),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../shaders/erosion_hydraulic_flow.wgsl").into(),
            ),
        });
        let layout = Self::make_flow_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("flow_pipeline_layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry_point),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            cache: None,
        })
    }

    /// Check if a heightmap buffer fits within device limits.
    fn check_buffer_size(ctx: &GpuContext, num_floats: usize) -> Result<(), GpuErosionError> {
        let size = (num_floats * 4) as u64;
        let limit = ctx.device.limits().max_storage_buffer_binding_size as u64;
        if size > limit {
            return Err(GpuErosionError::BufferTooLarge { size, limit });
        }
        Ok(())
    }

    /// Run hydraulic erosion on the GPU.
    ///
    /// NOTE: Results are nondeterministic due to concurrent droplet access.
    /// Use for interactive preview and large maps where speed matters.
    pub fn hydraulic_erode(
        &self,
        ctx: &GpuContext,
        heightmap: &Heightmap,
        params: &HydraulicErosionParams,
    ) -> Result<Heightmap, GpuErosionError> {
        let w = heightmap.width();
        let h = heightmap.height();
        let num_pixels = (w as usize) * (h as usize);
        Self::check_buffer_size(ctx, num_pixels)?;

        let gpu_params = GpuHydraulicParams {
            width: w,
            height: h,
            num_droplets: params.num_droplets,
            seed: params.seed,
            inertia: params.inertia,
            capacity_factor: params.capacity_factor,
            min_capacity: params.min_capacity,
            deposition_rate: params.deposition_rate,
            erosion_rate: params.erosion_rate,
            evaporation_rate: params.evaporation_rate,
            gravity: params.gravity,
            max_lifetime: params.max_lifetime,
            erosion_radius: params.erosion_radius,
            _padding: 0,
        };

        let buffer_size = (num_pixels * 4) as u64;

        // Upload heightmap data as read_write storage
        let heightmap_buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("hydraulic_heightmap"),
                contents: bytemuck::cast_slice(heightmap.data()),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });

        let params_buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("hydraulic_params"),
                contents: bytemuck::bytes_of(&gpu_params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let staging_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hydraulic_staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hydraulic_bind_group"),
            layout: &self.hydraulic_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: heightmap_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("hydraulic_encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("hydraulic_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.hydraulic_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            // One thread per droplet, workgroup size = 64
            let groups = params.num_droplets.div_ceil(64);
            pass.dispatch_workgroups(groups, 1, 1);
        }

        encoder.copy_buffer_to_buffer(&heightmap_buffer, 0, &staging_buffer, 0, buffer_size);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        // Read back
        let data = self.readback_buffer(ctx, &staging_buffer, num_pixels)?;

        // Clamp to [0, 1] (GPU shader doesn't guarantee this)
        let clamped: Vec<f32> = data.iter().map(|v| v.clamp(0.0, 1.0)).collect();

        info!(
            "GPU hydraulic erosion complete: {}x{}, {} droplets",
            w, h, params.num_droplets
        );

        Heightmap::frbar_data(w, h, clamped).map_err(|e| GpuErosionError::Pipeline(e.to_string()))
    }

    /// Run thermal erosion on the GPU with ping-pong buffering.
    ///
    /// Results are deterministic and should closely match CPU output.
    pub fn thermal_erode(
        &self,
        ctx: &GpuContext,
        heightmap: &Heightmap,
        params: &ThermalErosionParams,
    ) -> Result<Heightmap, GpuErosionError> {
        let w = heightmap.width();
        let h = heightmap.height();
        let num_pixels = (w as usize) * (h as usize);
        Self::check_buffer_size(ctx, num_pixels)?;

        let gpu_params = GpuThermalParams {
            width: w,
            height: h,
            talus_angle: params.talus_angle,
            erosion_rate: params.erosion_rate,
        };

        let buffer_size = (num_pixels * 4) as u64;

        // Create two storage buffers for ping-pong
        let buffer_a = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("thermal_buffer_a"),
                contents: bytemuck::cast_slice(heightmap.data()),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });

        let buffer_b = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("thermal_buffer_b"),
            size: buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let params_buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("thermal_params"),
                contents: bytemuck::bytes_of(&gpu_params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let staging_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("thermal_staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create two bind groups for ping-pong (A→B and B→A)
        let bind_group_ab = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("thermal_bind_ab"),
            layout: &self.thermal_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer_a.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer_b.as_entire_binding(),
                },
            ],
        });

        let bind_group_ba = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("thermal_bind_ba"),
            layout: &self.thermal_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer_b.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer_a.as_entire_binding(),
                },
            ],
        });

        // Dispatch all iterations in a single encoder
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("thermal_encoder"),
            });

        let groups_x = w.div_ceil(16);
        let groups_y = h.div_ceil(16);

        for i in 0..params.iterations {
            let bind_group = if i % 2 == 0 {
                &bind_group_ab
            } else {
                &bind_group_ba
            };

            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("thermal_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.thermal_pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        // Result is in buffer_a if iterations is even, buffer_b if odd
        let result_buffer = if params.iterations.is_multiple_of(2) {
            &buffer_a
        } else {
            &buffer_b
        };

        encoder.copy_buffer_to_buffer(result_buffer, 0, &staging_buffer, 0, buffer_size);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        let data = self.readback_buffer(ctx, &staging_buffer, num_pixels)?;

        // Clamp to [0, 1]
        let clamped: Vec<f32> = data.iter().map(|v| v.clamp(0.0, 1.0)).collect();

        info!(
            "GPU thermal erosion complete: {}x{}, {} iterations",
            w, h, params.iterations
        );

        Heightmap::frbar_data(w, h, clamped).map_err(|e| GpuErosionError::Pipeline(e.to_string()))
    }

    /// Run hydraulic erosion using the virtual-pipe shallow-water model.
    ///
    /// 4 compute passes are dispatched per iteration inside a single command
    /// encoder; wgpu guarantees sequential execution so inter-pass barriers are
    /// implicit.  After each iteration's `pass_apply`, the sediment ping-pong
    /// buffer is copied on the GPU (no CPU readback required).
    pub fn hydraulic_flow_erode(
        &self,
        ctx: &GpuContext,
        heightmap: &Heightmap,
        params: &FlowErosionParams,
    ) -> Result<Heightmap, GpuErosionError> {
        let w = heightmap.width();
        let h = heightmap.height();
        let num_pixels = w as usize * h as usize;

        // Flux buffer is the largest: 4 × f32 per cell
        Self::check_buffer_size(ctx, num_pixels * 4)?;

        let f32_size = (num_pixels * 4) as u64; // one f32 per cell
        let vec4_size = (num_pixels * 16) as u64; // vec4<f32> per cell (flux)
        let vec2_size = (num_pixels * 8) as u64; // vec2<f32> per cell (velocity)

        let gpu_params = GpuFlowParams {
            width: w,
            height: h,
            dt: params.dt,
            pipe_length: params.pipe_length,
            gravity: params.gravity,
            rain_rate: params.rain_rate,
            evaporation_rate: params.evaporation_rate,
            sediment_capacity: params.sediment_capacity,
            erosion_rate: params.erosion_rate,
            deposition_rate: params.deposition_rate,
            min_tilt: params.min_tilt,
            _padding: 0,
        };

        let params_buf = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("flow_params"),
                contents: bytemuck::bytes_of(&gpu_params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        // terrain — initialised with heightmap data; read back at the end
        let terrain_buf = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("flow_terrain"),
                contents: bytemuck::cast_slice(heightmap.data()),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });

        // water, flux, velocity, scratch — start at zero
        let make_zero = |label: &'static str, size: u64, extra: wgpu::BufferUsages| {
            ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | extra,
                mapped_at_creation: false,
            })
        };
        let water_buf = make_zero("flow_water", f32_size, wgpu::BufferUsages::empty());
        // sediment receives the ping-pong copy each iteration
        let sediment_buf = make_zero("flow_sediment", f32_size, wgpu::BufferUsages::COPY_DST);
        let flux_buf = make_zero("flow_flux", vec4_size, wgpu::BufferUsages::empty());
        let velocity_buf = make_zero("flow_velocity", vec2_size, wgpu::BufferUsages::empty());
        let scratch_buf = make_zero("flow_scratch", f32_size, wgpu::BufferUsages::empty());
        // sediment_out is the ping-pong write target; copied → sediment each step
        let sediment_out_buf =
            make_zero("flow_sediment_out", f32_size, wgpu::BufferUsages::COPY_SRC);

        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flow_bind_group"),
            layout: &self.flow_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: terrain_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: water_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: sediment_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: flux_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: velocity_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: scratch_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: sediment_out_buf.as_entire_binding(),
                },
            ],
        });

        let groups_x = w.div_ceil(16);
        let groups_y = h.div_ceil(16);

        let iterations = params.iterations.clamp(5, 200);

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("flow_encoder"),
            });

        for _ in 0..iterations {
            let dispatch = |encoder: &mut wgpu::CommandEncoder,
                            pipeline: &wgpu::ComputePipeline,
                            label: &'static str| {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(label),
                    timestamp_writes: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(groups_x, groups_y, 1);
            };

            dispatch(&mut encoder, &self.flow_flux_pipeline, "pass_flux");
            dispatch(
                &mut encoder,
                &self.flow_water_vel_pipeline,
                "pass_water_vel",
            );
            dispatch(&mut encoder, &self.flow_erosion_pipeline, "pass_erosion");
            dispatch(&mut encoder, &self.flow_apply_pipeline, "pass_apply");

            // Ping-pong: commit advected sediment for the next iteration
            encoder.copy_buffer_to_buffer(&sediment_out_buf, 0, &sediment_buf, 0, f32_size);
        }

        // Read back terrain to CPU
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flow_staging"),
            size: f32_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&terrain_buf, 0, &staging, 0, f32_size);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        let data = self.readback_buffer(ctx, &staging, num_pixels)?;
        let clamped: Vec<f32> = data.iter().map(|v| v.clamp(0.0, 1.0)).collect();

        info!(
            "GPU hydraulic flow erosion complete: {}×{}, {} iterations",
            w, h, iterations
        );

        Heightmap::frbar_data(w, h, clamped).map_err(|e| GpuErosionError::Pipeline(e.to_string()))
    }

    /// Read back a staging buffer into a Vec<f32>.
    fn readback_buffer(
        &self,
        ctx: &GpuContext,
        staging: &wgpu::Buffer,
        num_floats: usize,
    ) -> Result<Vec<f32>, GpuErosionError> {
        let buffer_slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        ctx.device.poll(wgpu::Maintain::Wait);

        receiver
            .recv()
            .map_err(|_| GpuErosionError::BufferMapping)?
            .map_err(|_| GpuErosionError::BufferMapping)?;

        let data = buffer_slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&data)[..num_floats].to_vec();
        drop(data);
        staging.unmap();

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::GpuContext;

    /// Try to get a GPU context. Returns None if no GPU is available.
    fn try_gpu_context() -> Option<GpuContext> {
        pollster::block_on(GpuContext::new_standalone()).ok()
    }

    fn make_cone(size: u32) -> Heightmap {
        let mut data = vec![0.0f32; (size * size) as usize];
        let center = size as f32 / 2.0;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                data[(y * size + x) as usize] = (1.0 - dist / center).max(0.0);
            }
        }
        Heightmap::frbar_data(size, size, data).unwrap()
    }

    #[test]
    fn test_gpu_hydraulic_erosion_output_range() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuErosionPipeline::new(&ctx.device);
        let input = make_cone(64);
        let params = HydraulicErosionParams {
            num_droplets: 2000,
            max_lifetime: 20,
            erosion_radius: 2,
            ..Default::default()
        };

        let result = pipeline.hydraulic_erode(&ctx, &input, &params).unwrap();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 64);

        // All values should be in [0, 1]
        for &v in result.data() {
            assert!(
                (0.0..=1.0).contains(&v),
                "GPU hydraulic erosion value out of range: {v}"
            );
        }

        // Should produce some modification to the terrain
        // (GPU hydraulic is nondeterministic, so we just verify it ran)
        let input_sum: f32 = input.data().iter().sum();
        let result_sum: f32 = result.data().iter().sum();
        assert!(
            (result_sum - input_sum).abs() > 0.001,
            "GPU erosion should modify the terrain (sums: input={input_sum}, result={result_sum})"
        );
    }

    #[test]
    fn test_gpu_thermal_erosion_output_range() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuErosionPipeline::new(&ctx.device);
        let input = make_cone(64);
        let params = ThermalErosionParams {
            iterations: 10,
            talus_angle: 0.02,
            erosion_rate: 0.5,
        };

        let result = pipeline.thermal_erode(&ctx, &input, &params).unwrap();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 64);

        // All values should be in [0, 1]
        for &v in result.data() {
            assert!(
                (0.0..=1.0).contains(&v),
                "GPU thermal erosion value out of range: {v}"
            );
        }

        // Should reduce the peak (thermal erosion smooths)
        let center_idx = 32 * 64 + 32;
        assert!(
            result.data()[center_idx] < input.data()[center_idx],
            "GPU thermal erosion should lower the peak"
        );
    }

    #[test]
    fn test_gpu_thermal_erosion_reduces_slopes() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuErosionPipeline::new(&ctx.device);
        let input = make_cone(64);
        let params = ThermalErosionParams {
            iterations: 50,
            talus_angle: 0.01,
            erosion_rate: 0.5,
        };

        let result = pipeline.thermal_erode(&ctx, &input, &params).unwrap();

        // GPU thermal erosion should reduce peak height
        // (shader removes material from steep cells)
        let center_idx = 32 * 64 + 32;
        assert!(
            result.data()[center_idx] < input.data()[center_idx],
            "GPU thermal erosion should lower the peak: before={}, after={}",
            input.data()[center_idx],
            result.data()[center_idx]
        );
    }

    #[test]
    fn test_gpu_hydraulic_flow_erosion_output_range() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        let pipeline = GpuErosionPipeline::new(&ctx.device);
        let input = make_cone(64);
        let params = FlowErosionParams {
            iterations: 20,
            ..Default::default()
        };

        let result = pipeline
            .hydraulic_flow_erode(&ctx, &input, &params)
            .unwrap();
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 64);

        for &v in result.data() {
            assert!(
                (0.0..=1.0).contains(&v),
                "GPU flow erosion value out of range: {v}"
            );
        }

        // Peak should be eroded
        let center_idx = 32 * 64 + 32;
        assert!(
            result.data()[center_idx] < input.data()[center_idx],
            "GPU flow erosion should lower the peak: before={}, after={}",
            input.data()[center_idx],
            result.data()[center_idx]
        );
    }

    #[test]
    fn test_buffer_size_check() {
        let Some(ctx) = try_gpu_context() else {
            eprintln!("Skipping GPU test: no GPU adapter available");
            return;
        };

        // Check that buffer size validation works
        // This test verifies the check_buffer_size logic
        let limit = ctx.device.limits().max_storage_buffer_binding_size;
        let within_limit = (limit as usize) / 4; // number of floats
        assert!(GpuErosionPipeline::check_buffer_size(&ctx, within_limit).is_ok());

        let over_limit = (limit as usize) / 4 + 1000;
        assert!(GpuErosionPipeline::check_buffer_size(&ctx, over_limit).is_err());
    }
}
