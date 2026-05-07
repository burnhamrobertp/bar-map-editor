use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum ComputeError {
    #[error("failed to request GPU adapter")]
    NoAdapter,

    #[error("failed to request GPU device: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),

    #[error("GPU compute not available, falling back to CPU")]
    GpuUnavailable,
}

/// Shared GPU context providing access to the wgpu device and queue.
/// Can be created standalone (CLI) or from eframe's render state (GUI).
/// wgpu Device/Queue are internally Arc-based and cheap to clone.
#[derive(Clone)]
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// Create a GPU context from existing device and queue (e.g., from eframe's RenderState).
    pub fn from_existing(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, queue }
    }

    /// Create a standalone GPU context (requests its own adapter and device).
    /// Used for CLI/headless mode.
    pub async fn new_standalone() -> Result<Self, ComputeError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(ComputeError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        info!(
            "GPU adapter: {} ({:?})",
            adapter_info.name, adapter_info.backend
        );

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("bar-compute"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits {
                        // Allow larger storage buffers for high-res heightmaps (8K = 256MB)
                        max_storage_buffer_binding_size: 512 * 1024 * 1024,
                        max_buffer_size: 512 * 1024 * 1024,
                        ..wgpu::Limits::default()
                    },
                    ..Default::default()
                },
                None,
            )
            .await?;

        Ok(Self { device, queue })
    }
}

/// Manages the wgpu device and queue for compute operations.
/// Legacy convenience wrapper — prefer GpuContext for new code.
pub struct ComputeDevice {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
}

impl ComputeDevice {
    /// Initialize the GPU compute device.
    /// Prefers high-performance discrete GPU, falls back to integrated.
    pub async fn new() -> Result<Self, ComputeError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(ComputeError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        info!(
            "GPU adapter: {} ({:?})",
            adapter_info.name, adapter_info.backend
        );

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("bar-compute"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits {
                        max_storage_buffer_binding_size: 512 * 1024 * 1024,
                        max_buffer_size: 512 * 1024 * 1024,
                        ..wgpu::Limits::default()
                    },
                    ..Default::default()
                },
                None,
            )
            .await?;

        Ok(Self {
            device,
            queue,
            adapter_info,
        })
    }

    /// Get a GpuContext referencing this device (cheap clone, Arc-based internally).
    pub fn as_context(&self) -> GpuContext {
        GpuContext {
            device: self.device.clone(),
            queue: self.queue.clone(),
        }
    }
}
