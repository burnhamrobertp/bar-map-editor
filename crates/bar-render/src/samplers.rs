//! Shared sampler constructors.
//!
//! Centralises the choice of "what does a normal texture-filtered
//! sampler look like" so the entire renderer agrees on a single
//! filtering / anisotropy story. BAR's engine applies anisotropy
//! globally via `Springsettings.cfg::MaxTexAniso` in
//! `Bitmap.cpp:1746`; every world-space texture loaded by the engine
//! gets the same level. BME mirrors that by routing every eligible
//! sampler through this helper so we can't accidentally ship a
//! pipeline where one terrain texture looks crisp at oblique angles
//! and another, next to it, looks smeared.
//!
//! "Eligible" = anisotropy requires linear filtering on min, mag, and
//! mip per the wgpu / OpenGL spec. Samplers that have to be Nearest
//! anywhere (depth comparison, water reflection / refraction lookups
//! with nearest mip, full-screen post-passes, shadow PCF) keep their
//! bespoke configurations.

/// Standard world-space filtered sampler: linear min/mag/mip with
/// 16x anisotropy. `anisotropy_clamp: 16` is the wgpu/Vulkan/D3D12
/// portable maximum -- wgpu silently caps to the device's actual
/// max-supported level, so no device-query plumbing is needed.
///
/// Use this for every sampler bound against a textured world-space
/// asset (terrain albedo, splat detail textures, feature model
/// textures, grass blades + grass shading texture, water caustics,
/// etc.). Don't use it for samplers that bind 1x1 placeholders
/// without mipmaps, or for samplers where mipmap-Nearest is the
/// deliberate behaviour.
pub(crate) fn make_filtered_sampler(
    device: &wgpu::Device,
    label: &str,
    address_mode: wgpu::AddressMode,
) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: address_mode,
        address_mode_v: address_mode,
        address_mode_w: address_mode,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        anisotropy_clamp: 16,
        ..Default::default()
    })
}
