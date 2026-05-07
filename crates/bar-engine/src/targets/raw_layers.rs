//! Raw layers codec: exports terrain data as individual image files.
//!
//! No binary format — just PNGs/TIFFs for each layer. Useful for
//! generic pipelines, debugging, and non-Spring engines.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::codec::{ExportCodec, ExportPlan, WrittenFiles};
use super::config::TargetConfig;
use super::dimensions::{DimensionConstraint, DimensionRule, DimensionSet};
use super::layers::{LayerFormat, LayerRequirement, LayerSet, LayerStatus};
use super::packaging::{ArchiveFormat, PackagingConfig};
use super::validation::{Severity, ValidationError};

/// Raw layers codec — writes each layer as an individual image file.
pub struct RawLayersCodec;

impl RawLayersCodec {
    /// Build the default target config for raw layer export.
    pub fn default_config() -> TargetConfig {
        TargetConfig {
            id: "raw-layers".to_string(),
            name: "Raw Image Layers (PNG)".to_string(),
            schema_version: 1,
            version: "1.0.0".to_string(),
            codec: "raw-layers".to_string(),
            codec_params: Default::default(),
            dimension_constraint: DimensionConstraint::none(),
            layers: vec![
                LayerRequirement {
                    name: "heightmap".to_string(),
                    format: LayerFormat::U16,
                    resolution: DimensionRule::height_samples(),
                    status: LayerStatus::Required,
                },
                LayerRequirement {
                    name: "metalmap".to_string(),
                    format: LayerFormat::U8,
                    resolution: DimensionRule::half_map_squares(),
                    status: LayerStatus::Optional,
                },
                LayerRequirement {
                    name: "typemap".to_string(),
                    format: LayerFormat::U8,
                    resolution: DimensionRule::half_map_squares(),
                    status: LayerStatus::Optional,
                },
                LayerRequirement {
                    name: "grassmap".to_string(),
                    format: LayerFormat::U8,
                    resolution: DimensionRule::map_squares(),
                    status: LayerStatus::Optional,
                },
                LayerRequirement {
                    name: "texture".to_string(),
                    format: LayerFormat::Rgba8,
                    resolution: DimensionRule::map_squares(),
                    status: LayerStatus::Optional,
                },
                LayerRequirement {
                    name: "normalmap".to_string(),
                    format: LayerFormat::Rgb8,
                    resolution: DimensionRule::height_samples(),
                    status: LayerStatus::Optional,
                },
                LayerRequirement {
                    name: "specular".to_string(),
                    format: LayerFormat::Rgb8,
                    resolution: DimensionRule::height_samples(),
                    status: LayerStatus::Optional,
                },
            ],
            packaging: PackagingConfig {
                archive_format: ArchiveFormat::Directory,
                extension: String::new(),
                layout: Vec::new(),
            },
            metadata_template: None,
        }
    }
}

impl ExportCodec for RawLayersCodec {
    fn id(&self) -> &str {
        "raw-layers"
    }

    fn description(&self) -> &str {
        "Export all layers as individual image files (PNG)"
    }

    fn validate(
        &self,
        _config: &TargetConfig,
        _plan: &ExportPlan,
        layers: &LayerSet,
    ) -> Result<Vec<ValidationError>> {
        let mut errors = Vec::new();

        if !layers.has_layer("heightmap") {
            errors.push(ValidationError {
                severity: Severity::Error,
                component: "heightmap".to_string(),
                message: "required heightmap layer is missing".to_string(),
            });
        }

        Ok(errors)
    }

    fn compute_dimensions(
        &self,
        config: &TargetConfig,
        heightmap_width: u32,
        heightmap_height: u32,
    ) -> DimensionSet {
        let sq_x = heightmap_width - 1;
        let sq_y = heightmap_height - 1;

        let layer_dimensions = config
            .layers
            .iter()
            .map(|layer| {
                let (w, h) = layer.resolution.resolve(sq_x, sq_y);
                (layer.name.clone(), w, h)
            })
            .collect();

        DimensionSet {
            map_squares: (sq_x, sq_y),
            layer_dimensions,
        }
    }

    fn write(
        &self,
        _config: &TargetConfig,
        plan: &ExportPlan,
        layers: &LayerSet,
        output_dir: &Path,
    ) -> Result<WrittenFiles> {
        fs::create_dir_all(output_dir)?;
        let mut written = WrittenFiles::default();
        let name = &plan.map_name;

        // Heightmap as 16-bit PNG
        if let Some(ref hm) = layers.heightmap {
            let path = output_dir.join(format!("{}_heightmap.png", name));
            write_heightmap_16bit(hm, &path)?;
            written.files.push(format!("{}_heightmap.png", name));
        }

        // Metalmap as 8-bit grayscale
        if let Some(ref mm) = layers.metalmap {
            let path = output_dir.join(format!("{}_metalmap.png", name));
            write_heightmap_8bit(mm, &path)?;
            written.files.push(format!("{}_metalmap.png", name));
        }

        // Typemap as 8-bit grayscale
        if let Some(ref tm) = layers.typemap {
            let path = output_dir.join(format!("{}_typemap.png", name));
            write_heightmap_8bit(tm, &path)?;
            written.files.push(format!("{}_typemap.png", name));
        }

        // Grassmap as 8-bit grayscale
        if let Some(ref gm) = layers.grassmap {
            let path = output_dir.join(format!("{}_grassmap.png", name));
            write_heightmap_8bit(gm, &path)?;
            written.files.push(format!("{}_grassmap.png", name));
        }

        // Texture as RGBA PNG
        if let Some(ref tex) = layers.texture {
            let path = output_dir.join(format!("{}_texture.png", name));
            write_color_png(tex, &path)?;
            written.files.push(format!("{}_texture.png", name));
        }

        // Normal map as RGB PNG
        if let Some(ref nm) = layers.normalmap {
            let path = output_dir.join(format!("{}_normalmap.png", name));
            write_color_png(nm, &path)?;
            written.files.push(format!("{}_normalmap.png", name));
        }

        // Specular as RGB PNG
        if let Some(ref sp) = layers.specular {
            let path = output_dir.join(format!("{}_specular.png", name));
            write_color_png(sp, &path)?;
            written.files.push(format!("{}_specular.png", name));
        }

        Ok(written)
    }
}

/// Write a heightmap as 16-bit grayscale PNG.
fn write_heightmap_16bit(hm: &bar_data::Heightmap, path: &Path) -> Result<()> {
    let w = hm.width();
    let h = hm.height();
    let data: Vec<u16> = (0..w * h)
        .map(|i| {
            let x = i % w;
            let y = i / w;
            let v = hm.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0);
            (v * 65535.0) as u16
        })
        .collect();

    let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_be_bytes()).collect();
    image::save_buffer(path, &bytes, w, h, image::ColorType::L16)
        .with_context(|| format!("Failed to write 16-bit PNG: {}", path.display()))?;
    Ok(())
}

/// Write a heightmap as 8-bit grayscale PNG.
fn write_heightmap_8bit(hm: &bar_data::Heightmap, path: &Path) -> Result<()> {
    let w = hm.width();
    let h = hm.height();
    let data: Vec<u8> = (0..w * h)
        .map(|i| {
            let x = i % w;
            let y = i / w;
            (hm.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0) as u8
        })
        .collect();

    image::save_buffer(path, &data, w, h, image::ColorType::L8)
        .with_context(|| format!("Failed to write PNG: {}", path.display()))?;
    Ok(())
}

/// Write a ColorBuffer as RGBA PNG.
fn write_color_png(buffer: &bar_data::ColorBuffer, path: &Path) -> Result<()> {
    let rgba = buffer.to_rgba8();
    image::save_buffer(
        path,
        &rgba,
        buffer.width(),
        buffer.height(),
        image::ColorType::Rgba8,
    )
    .with_context(|| format!("Failed to write PNG: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_layers_default_config() {
        let config = RawLayersCodec::default_config();
        assert_eq!(config.id, "raw-layers");
        assert_eq!(config.codec, "raw-layers");
        assert_eq!(config.layers.len(), 7);
    }

    #[test]
    fn test_raw_layers_dimensions() {
        let codec = RawLayersCodec;
        let config = RawLayersCodec::default_config();
        let dims = codec.compute_dimensions(&config, 1025, 1025);
        assert_eq!(dims.map_squares, (1024, 1024));
        assert_eq!(dims.get("heightmap"), Some((1025, 1025)));
        assert_eq!(dims.get("metalmap"), Some((512, 512)));
    }
}
