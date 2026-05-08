//! Spring/Recoil SMF codec implementation.
//!
//! Handles writing .smf (map), .smt (tiles), and mapinfo.lua for
//! Spring engine and its forks (Recoil/BAR).

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};

use bar_data::{generate_minimap_dxt1, write_smt, ColorBuffer, SmfMap};

use super::codec::{ExportCodec, ExportPlan, WrittenFiles};
use super::config::TargetConfig;
use super::dimensions::{DimensionRule, DimensionSet};
use super::layers::{LayerSet, LayerStatus};
use super::validation::{Severity, ValidationError};
use crate::export::resample_to_u8;
use crate::recipe::MapSettings;

/// Spring/Recoil SMF format codec.
pub struct SpringSmfCodec;

impl SpringSmfCodec {
    /// Build the default target config for Spring/Recoil (BAR).
    pub fn default_config() -> TargetConfig {
        use super::dimensions::DimensionConstraint;
        use super::layers::{LayerFormat, LayerRequirement};
        use super::packaging::{ArchiveFormat, FileMapping, PackagingConfig};

        TargetConfig {
            id: "spring-smf".to_string(),
            name: "Spring/Recoil (BAR)".to_string(),
            schema_version: 1,
            version: "1.0.0".to_string(),
            codec: "spring-smf".to_string(),
            codec_params: super::config::CodecParams {
                square_size: 8,
                texels_per_square: 8,
                tile_size: 32,
                min_height: -200.0,
                max_height: 800.0,
            },
            dimension_constraint: DimensionConstraint {
                multiple_of: 128,
                min: 128,
                max: 32768,
            },
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
                    format: LayerFormat::Dxt1,
                    resolution: DimensionRule::map_squares(), // texels handled by SMT tiling
                    status: LayerStatus::Optional,
                },
                LayerRequirement {
                    name: "normalmap".to_string(),
                    format: LayerFormat::Rgb8,
                    resolution: DimensionRule::height_samples(),
                    status: LayerStatus::Optional,
                },
            ],
            packaging: PackagingConfig {
                archive_format: ArchiveFormat::SevenZip,
                extension: ".sd7".to_string(),
                layout: vec![
                    FileMapping {
                        source: "metadata".to_string(),
                        dest: "mapinfo.lua".to_string(),
                    },
                    FileMapping {
                        source: "smf".to_string(),
                        dest: "maps/{name}.smf".to_string(),
                    },
                    FileMapping {
                        source: "smt".to_string(),
                        dest: "maps/{name}.smt".to_string(),
                    },
                ],
            },
            metadata_template: None,
        }
    }
}

impl ExportCodec for SpringSmfCodec {
    fn id(&self) -> &str {
        "spring-smf"
    }

    fn description(&self) -> &str {
        "Spring/Recoil engine SMF map format (used by Beyond All Reason)"
    }

    fn validate(
        &self,
        config: &TargetConfig,
        plan: &ExportPlan,
        layers: &LayerSet,
    ) -> Result<Vec<ValidationError>> {
        let mut errors = Vec::new();
        let (sq_x, sq_y) = plan.dimensions.map_squares;

        // Check dimension constraints
        if let Err(msg) = config.dimension_constraint.check(sq_x) {
            errors.push(ValidationError {
                severity: Severity::Error,
                component: "dimensions".to_string(),
                message: format!("map_x: {}", msg),
            });
        }
        if let Err(msg) = config.dimension_constraint.check(sq_y) {
            errors.push(ValidationError {
                severity: Severity::Error,
                component: "dimensions".to_string(),
                message: format!("map_y: {}", msg),
            });
        }

        // Check required layers
        for layer in &config.layers {
            if layer.status == LayerStatus::Required && !layers.has_layer(&layer.name) {
                errors.push(ValidationError {
                    severity: Severity::Error,
                    component: layer.name.clone(),
                    message: "required layer is missing".to_string(),
                });
            }
        }

        // Warn about missing optional but recommended layers
        if !layers.has_layer("texture") {
            errors.push(ValidationError {
                severity: Severity::Warning,
                component: "texture".to_string(),
                message: "no texture layer — map will have no diffuse color".to_string(),
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
        // In Spring: mapx = heightmap_width - 1
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
        config: &TargetConfig,
        plan: &ExportPlan,
        layers: &LayerSet,
        output_dir: &Path,
    ) -> Result<WrittenFiles> {
        let mut written = WrittenFiles::default();
        let map_name = &plan.map_name;
        let (sq_x, sq_y) = plan.dimensions.map_squares;

        // Create maps/ subdirectory
        let maps_dir = output_dir.join("maps");
        fs::create_dir_all(&maps_dir)?;

        // Write SMF
        if let Some(ref heightmap) = layers.heightmap {
            let smf_path = maps_dir.join(format!("{}.smf", map_name));
            self.write_smf(
                config,
                heightmap,
                layers,
                map_name,
                sq_x,
                sq_y,
                &plan.settings,
                &smf_path,
            )?;
            written.files.push(format!("maps/{}.smf", map_name));
            tracing::info!("Wrote SMF: {}", smf_path.display());
        }

        // Write SMT (texture tiles) — generate height-based fallback if none provided
        let smt_path = maps_dir.join(format!("{}.smt", map_name));
        if let Some(ref texture) = layers.texture {
            let file = File::create(&smt_path)?;
            let mut writer = BufWriter::new(file);
            write_smt(&mut writer, texture)?;
        } else if let Some(ref heightmap) = layers.heightmap {
            // Generate a basic height-tinted fallback texture so BAR can load the map
            let tex_w = sq_x * config.codec_params.texels_per_square;
            let tex_h = sq_y * config.codec_params.texels_per_square;
            let fallback = generate_fallback_texture(heightmap, tex_w, tex_h);
            let file = File::create(&smt_path)?;
            let mut writer = BufWriter::new(file);
            write_smt(&mut writer, &fallback)?;
        }
        written.files.push(format!("maps/{}.smt", map_name));
        tracing::info!("Wrote SMT: {}", smt_path.display());

        // Write metadata (mapinfo.lua)
        let mapinfo = self.generate_mapinfo(map_name, sq_x, sq_y, plan);
        let mapinfo_path = output_dir.join("mapinfo.lua");
        fs::write(&mapinfo_path, &mapinfo)?;
        written.files.push("mapinfo.lua".to_string());
        tracing::info!("Wrote mapinfo: {}", mapinfo_path.display());

        // Write optional PNG layers for preview/debugging
        if let Some(ref normalmap) = layers.normalmap {
            let path = output_dir.join(format!("{}_normals.png", map_name));
            write_color_png(normalmap, &path)?;
            written.files.push(format!("{}_normals.png", map_name));
        }

        if let Some(ref grassmap) = layers.grassmap {
            let path = output_dir.join(format!("{}_grass.png", map_name));
            write_heightmap_png(grassmap, &path)?;
            written.files.push(format!("{}_grass.png", map_name));
        }

        Ok(written)
    }
}

impl SpringSmfCodec {
    /// Write the .smf binary file.
    #[allow(clippy::too_many_arguments)]
    fn write_smf(
        &self,
        _config: &TargetConfig,
        heightmap: &bar_data::Heightmap,
        layers: &LayerSet,
        map_name: &str,
        map_x: u32,
        map_y: u32,
        settings: &MapSettings,
        path: &Path,
    ) -> Result<()> {
        let mut smf =
            SmfMap::new(map_x as i32, map_y as i32).context("Failed to create SMF map")?;
        smf.heightmap = heightmap.clone();
        smf.smt_filename = format!("maps/{}.smt", map_name);
        smf.header.min_height = settings.min_height;
        smf.header.max_height = settings.max_height;

        let (mm_w, mm_h) = smf.header.metalmap_size();
        let mm_size = (mm_w as usize) * (mm_h as usize);

        if let Some(ref metal) = layers.metalmap {
            smf.metalmap = resample_to_u8(metal, mm_w, mm_h);
        }
        if let Some(ref tmap) = layers.typemap {
            smf.typemap = resample_to_u8(tmap, mm_w, mm_h);
        }

        smf.metalmap.resize(mm_size, 0);
        smf.typemap.resize(mm_size, 0);

        // Generate minimap: 1024×1024 top-down view, DXT1 compressed with 9 mip levels
        let minimap_rgba = if let Some(ref texture) = layers.texture {
            texture.resize(1024, 1024).to_rgba8()
        } else {
            let minimap_tex = generate_fallback_texture(heightmap, 1024, 1024);
            minimap_tex.to_rgba8()
        };
        smf.minimap_dxt1 = generate_minimap_dxt1(&minimap_rgba);

        let file = File::create(path)
            .with_context(|| format!("Failed to create file: {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        smf.write(&mut writer)
            .with_context(|| format!("Failed to write SMF: {}", path.display()))?;

        Ok(())
    }

    /// Generate mapinfo.lua content.
    ///
    /// Per `docs/bar-map-format.md`, almost every mapinfo field has an
    /// engine-side default. We only emit:
    ///
    /// - The strict floor (`name`, `mapfile`, `modtype`, `smf.smtFileName0`).
    /// - Identity fields the user actually set (None / empty → omitted).
    /// - Physics fields when they differ from the engine default — the
    ///   editor exposes these knobs and users tend to twiddle them, so
    ///   a per-field check rather than a section-level skip.
    /// - `atmosphere` / `lighting` / `water` sections only when at least
    ///   one field deviates from the engine default. All-default sections
    ///   are omitted entirely so the engine just falls back.
    /// - `teams[]` always — even if the user accepts SPAWN_FIXED, the
    ///   floor of two corner spawns must be present.
    /// - DNTs / splats only when the user supplied detail textures.
    fn generate_mapinfo(&self, name: &str, map_x: u32, map_y: u32, plan: &ExportPlan) -> String {
        let settings = &plan.settings;
        let defaults = MapSettings::default();
        let esc = |s: &str| -> String {
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        };
        let f = |x: f32| -> String {
            // Lua doesn't care about trailing zeros; trim them so the
            // generated file matches what hand-written mapinfos look like.
            let s = format!("{:.4}", x);
            let trimmed = s.trim_end_matches('0').trim_end_matches('.');
            if trimmed.is_empty() {
                "0".to_string()
            } else {
                trimmed.to_string()
            }
        };

        let mut out = String::with_capacity(2048);
        out.push_str("local mapinfo = {\n");

        // ── Identity / floor ──────────────────────────────────────
        out.push_str(&format!("    name        = \"{}\",\n", esc(name)));
        if let Some(s) = plan.shortname.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("    shortname   = \"{}\",\n", esc(s)));
        }
        if !plan.description.is_empty() {
            out.push_str(&format!(
                "    description = \"{}\",\n",
                esc(&plan.description)
            ));
        }
        if let Some(s) = plan.author.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("    author      = \"{}\",\n", esc(s)));
        }
        if let Some(s) = plan.version.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("    version     = \"{}\",\n", esc(s)));
        }
        out.push_str(&format!("    mapfile     = \"maps/{}.smf\",\n", name));
        out.push_str("    modtype     = 3,\n");

        // ── Physics ───────────────────────────────────────────────
        // Only emit fields that differ from the engine default. Each
        // line is a free-standing emit — one extracted helper per type
        // would be more code than it's worth.
        let mut physics = String::new();
        if settings.map_hardness != defaults.map_hardness {
            physics.push_str(&format!(
                "    maphardness    = {},\n",
                settings.map_hardness
            ));
        }
        if !settings.deformable {
            physics.push_str("    notDeformable  = true,\n");
        }
        if settings.gravity != defaults.gravity {
            physics.push_str(&format!("    gravity        = {},\n", f(settings.gravity)));
        }
        if settings.tidal_strength != defaults.tidal_strength {
            physics.push_str(&format!(
                "    tidalStrength  = {},\n",
                f(settings.tidal_strength)
            ));
        }
        if settings.max_metal != defaults.max_metal {
            physics.push_str(&format!(
                "    maxMetal       = {},\n",
                f(settings.max_metal)
            ));
        }
        if settings.extractor_radius != defaults.extractor_radius {
            physics.push_str(&format!(
                "    extractorRadius = {},\n",
                f(settings.extractor_radius)
            ));
        }
        if settings.void_water {
            physics.push_str("    voidWater      = true,\n");
        }
        if settings.void_ground {
            physics.push_str("    voidGround     = true,\n");
        }
        if !physics.is_empty() {
            out.push('\n');
            out.push_str(&physics);
        }

        // ── smf block ─────────────────────────────────────────────
        // smtFileName0 is the only required field; min/max heights are
        // overrides that mirror what we baked into the SMF binary, kept
        // here so the mapinfo is self-describing.
        out.push_str("\n    smf = {\n");
        out.push_str(&format!(
            "        minheight = {},\n",
            f(settings.min_height)
        ));
        out.push_str(&format!(
            "        maxheight = {},\n",
            f(settings.max_height)
        ));
        out.push_str(&format!("        smtFileName0 = \"maps/{}.smt\",\n", name));
        out.push_str("    },\n");

        // ── DNTs / splats (only with custom detail textures) ──────
        if !settings.detail_textures.is_empty() {
            out.push_str("\n    resources = {\n        detailTex = \"\",\n    },\n");
            out.push_str("\n    custom = {\n        dnts = {\n");
            for (i, dt) in settings.detail_textures.iter().enumerate() {
                out.push_str(&format!(
                    "            [{}] = {{ file = \"{}\", scale = {} }},\n",
                    i,
                    esc(&dt.path),
                    f(dt.scale),
                ));
            }
            out.push_str("        },\n    },\n");

            let splat_scales: Vec<String> = settings
                .detail_textures
                .iter()
                .take(4)
                .map(|dt| f(dt.scale))
                .chain(std::iter::repeat("0.02".to_string()))
                .take(4)
                .collect();
            out.push_str(&format!(
                "\n    splats = {{\n        texScales = {{ {} }},\n        texMults  = {{ 1, 1, 1, 1 }},\n    }},\n",
                splat_scales.join(", ")
            ));
        }

        // ── Atmosphere / lighting / water (skip if all-default) ───
        let atm = &settings.atmosphere;
        if atm != &defaults.atmosphere {
            out.push_str(&format!(
                "\n    atmosphere = {{\n        minWind    = {},\n        maxWind    = {},\n        fogStart   = {},\n        fogEnd     = {},\n        fogColor   = {{ {}, {}, {} }},\n    }},\n",
                f(atm.min_wind),
                f(atm.max_wind),
                f(atm.fog_start),
                f(atm.fog_end),
                f(atm.fog_color[0]),
                f(atm.fog_color[1]),
                f(atm.fog_color[2]),
            ));
        }

        let lit = &settings.lighting;
        if lit != &defaults.lighting {
            out.push_str(&format!(
                "\n    lighting = {{\n        sunDir              = {{ {}, {}, {} }},\n        groundAmbientColor  = {{ {}, {}, {} }},\n        groundDiffuseColor  = {{ {}, {}, {} }},\n        groundSpecularColor = {{ {}, {}, {} }},\n        specularExponent    = {},\n    }},\n",
                f(lit.sun_dir[0]), f(lit.sun_dir[1]), f(lit.sun_dir[2]),
                f(lit.ground_ambient[0]), f(lit.ground_ambient[1]), f(lit.ground_ambient[2]),
                f(lit.ground_diffuse[0]), f(lit.ground_diffuse[1]), f(lit.ground_diffuse[2]),
                f(lit.ground_specular[0]), f(lit.ground_specular[1]), f(lit.ground_specular[2]),
                f(lit.spec_exponent),
            ));
        }

        let wat = &settings.water;
        if wat != &defaults.water {
            out.push_str(&format!(
                "\n    water = {{\n        damage    = {},\n        absorb    = {{ {}, {}, {} }},\n        baseColor = {{ {}, {}, {} }},\n        minColor  = {{ {}, {}, {} }},\n    }},\n",
                f(wat.damage),
                f(wat.absorb[0]), f(wat.absorb[1]), f(wat.absorb[2]),
                f(wat.base_color[0]), f(wat.base_color[1]), f(wat.base_color[2]),
                f(wat.min_color[0]), f(wat.min_color[1]), f(wat.min_color[2]),
            ));
        }

        // ── Teams (always emit) ───────────────────────────────────
        // World-space dimensions (elmos) = squares × squareSize. The
        // default two-corner layout keeps SPAWN_FIXED maps spawnable
        // even when the user hasn't explicitly placed startpoints.
        let world_x = map_x * 8;
        let world_y = map_y * 8;
        out.push_str("\n    teams = {\n");
        if settings.start_positions.is_empty() {
            out.push_str(&format!(
                "        [0] = {{ startPos = {{ x = {}, z = {} }} }},\n",
                world_x / 4,
                world_y / 4,
            ));
            out.push_str(&format!(
                "        [1] = {{ startPos = {{ x = {}, z = {} }} }},\n",
                world_x * 3 / 4,
                world_y * 3 / 4,
            ));
        } else {
            for (i, pos) in settings.start_positions.iter().enumerate() {
                out.push_str(&format!(
                    "        [{}] = {{ startPos = {{ x = {}, z = {} }} }},\n",
                    i, pos[0], pos[1],
                ));
            }
        }
        out.push_str("    },\n");

        out.push_str("}\n\nreturn mapinfo\n");
        out
    }
}

/// Helper: write a ColorBuffer as PNG.
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

/// Helper: write a Heightmap as grayscale PNG.
fn write_heightmap_png(hm: &bar_data::Heightmap, path: &Path) -> Result<()> {
    let data: Vec<u8> = (0..hm.width() * hm.height())
        .map(|i| {
            let x = i % hm.width();
            let y = i / hm.width();
            (hm.get(x, y).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0) as u8
        })
        .collect();
    image::save_buffer(path, &data, hm.width(), hm.height(), image::ColorType::L8)
        .with_context(|| format!("Failed to write PNG: {}", path.display()))?;
    Ok(())
}

/// Generate a height-based fallback texture when no diffuse texture layer is provided.
/// Low areas are dark green, mid areas are brown/tan, high areas are gray/white.
fn generate_fallback_texture(
    heightmap: &bar_data::Heightmap,
    tex_w: u32,
    tex_h: u32,
) -> ColorBuffer {
    let hm_w = heightmap.width();
    let hm_h = heightmap.height();

    let mut data = vec![0.0f32; (tex_w as usize) * (tex_h as usize) * 4];

    for ty in 0..tex_h {
        for tx in 0..tex_w {
            // Map texture pixel to heightmap coordinate
            let hx = (tx as f32 / tex_w as f32 * (hm_w - 1) as f32) as u32;
            let hy = (ty as f32 / tex_h as f32 * (hm_h - 1) as f32) as u32;
            let h = heightmap
                .get(hx.min(hm_w - 1), hy.min(hm_h - 1))
                .unwrap_or(0.0);

            // Color ramp: dark green → tan → gray → white
            let (r, g, b) = if h < 0.25 {
                (0.16f32, 0.31f32, 0.12f32)
            } else if h < 0.5 {
                let t = (h - 0.25) * 4.0;
                (0.16 + t * 0.39, 0.31 + t * 0.16, 0.12 + t * 0.12)
            } else if h < 0.75 {
                let t = (h - 0.5) * 4.0;
                (0.55 + t * 0.12, 0.47 - t * 0.08, 0.24 + t * 0.20)
            } else {
                let t = (h - 0.75) * 4.0;
                let v = (0.67 + t * 0.23).min(0.90);
                (v, v, v)
            };

            let idx = ((ty * tex_w + tx) as usize) * 4;
            data[idx] = r;
            data[idx + 1] = g;
            data[idx + 2] = b;
            data[idx + 3] = 1.0;
        }
    }

    ColorBuffer::frbar_data(tex_w, tex_h, data).expect("fallback texture dimensions valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::layers::LayerSet;

    #[test]
    fn test_default_config_is_valid() {
        let config = SpringSmfCodec::default_config();
        assert_eq!(config.id, "spring-smf");
        assert_eq!(config.codec, "spring-smf");
        assert_eq!(config.codec_params.square_size, 8);
        assert!(config.dimension_constraint.check(4096).is_ok());
        assert!(config.dimension_constraint.check(100).is_err());
    }

    #[test]
    fn test_compute_dimensions() {
        let codec = SpringSmfCodec;
        let config = SpringSmfCodec::default_config();
        let dims = codec.compute_dimensions(&config, 4097, 4097);

        assert_eq!(dims.map_squares, (4096, 4096));
        assert_eq!(dims.get("heightmap"), Some((4097, 4097)));
        assert_eq!(dims.get("metalmap"), Some((2048, 2048)));
        assert_eq!(dims.get("typemap"), Some((2048, 2048)));
        assert_eq!(dims.get("grassmap"), Some((4096, 4096)));
    }

    #[test]
    fn test_validate_missing_heightmap() {
        let codec = SpringSmfCodec;
        let config = SpringSmfCodec::default_config();
        let dims = codec.compute_dimensions(&config, 4097, 4097);
        let plan = ExportPlan {
            map_name: "test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            dimensions: dims,
            settings: MapSettings::default(),
        };
        let layers = LayerSet::default(); // no layers

        let errors = codec.validate(&config, &plan, &layers).unwrap();
        assert!(errors
            .iter()
            .any(|e| e.component == "heightmap" && e.severity == Severity::Error));
    }

    fn make_plan(settings: MapSettings) -> ExportPlan {
        let codec = SpringSmfCodec;
        let config = SpringSmfCodec::default_config();
        ExportPlan {
            map_name: "test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            dimensions: codec.compute_dimensions(&config, 257, 257),
            settings,
        }
    }

    #[test]
    fn mapinfo_skips_atmosphere_lighting_water_when_default() {
        let codec = SpringSmfCodec;
        let plan = make_plan(MapSettings::default());
        let lua = codec.generate_mapinfo("test", 32, 32, &plan);
        assert!(
            !lua.contains("atmosphere"),
            "default atmosphere should be omitted; got:\n{lua}"
        );
        assert!(
            !lua.contains("lighting"),
            "default lighting should be omitted; got:\n{lua}"
        );
        // The mapinfo block has no top-level "water" — just make sure the
        // empty section header isn't there.
        assert!(
            !lua.contains("water = {"),
            "default water should be omitted; got:\n{lua}"
        );
    }

    #[test]
    fn mapinfo_emits_atmosphere_when_modified() {
        let codec = SpringSmfCodec;
        let mut s = MapSettings::default();
        s.atmosphere.fog_color = [0.1, 0.2, 0.3];
        let lua = codec.generate_mapinfo("test", 32, 32, &make_plan(s));
        assert!(
            lua.contains("atmosphere"),
            "modified atmosphere should be emitted; got:\n{lua}"
        );
        assert!(
            lua.contains("fogColor"),
            "fogColor should appear in atmosphere block; got:\n{lua}"
        );
    }

    #[test]
    fn mapinfo_omits_void_flags_when_false() {
        let codec = SpringSmfCodec;
        let plan = make_plan(MapSettings::default());
        let lua = codec.generate_mapinfo("test", 32, 32, &plan);
        assert!(
            !lua.contains("voidWater"),
            "voidWater=false should be omitted; got:\n{lua}"
        );
        assert!(
            !lua.contains("voidGround"),
            "voidGround=false should be omitted; got:\n{lua}"
        );
    }

    #[test]
    fn mapinfo_emits_void_flags_when_true() {
        let codec = SpringSmfCodec;
        let mut s = MapSettings::default();
        s.void_water = true;
        s.void_ground = true;
        let lua = codec.generate_mapinfo("test", 32, 32, &make_plan(s));
        assert!(lua.contains("voidWater"));
        assert!(lua.contains("voidGround"));
    }

    #[test]
    fn mapinfo_always_emits_required_fields() {
        let codec = SpringSmfCodec;
        let plan = make_plan(MapSettings::default());
        let lua = codec.generate_mapinfo("kolmog", 32, 32, &plan);
        // Engine-required floor: name, mapfile, smtFileName0, teams.
        assert!(lua.contains("name        = \"kolmog\""));
        assert!(lua.contains("mapfile     = \"maps/kolmog.smf\""));
        assert!(lua.contains("smtFileName0 = \"maps/kolmog.smt\""));
        assert!(lua.contains("teams = {"));
        assert!(lua.contains("startPos"));
    }

    #[test]
    fn mapinfo_skips_unset_identity_fields() {
        let codec = SpringSmfCodec;
        let plan = make_plan(MapSettings::default());
        let lua = codec.generate_mapinfo("test", 32, 32, &plan);
        // shortname/author/version are None and description is empty in
        // make_plan, so none of those keys should appear.
        assert!(!lua.contains("shortname"));
        assert!(!lua.contains("author"));
        assert!(!lua.contains("version"));
        assert!(!lua.contains("description"));
    }

    #[test]
    fn mapinfo_emits_set_identity_fields() {
        let codec = SpringSmfCodec;
        let mut plan = make_plan(MapSettings::default());
        plan.shortname = Some("kolm".to_string());
        plan.description = "A nice map".to_string();
        plan.author = Some("rb".to_string());
        plan.version = Some("2".to_string());
        let lua = codec.generate_mapinfo("test", 32, 32, &plan);
        assert!(lua.contains("shortname   = \"kolm\""));
        assert!(lua.contains("description = \"A nice map\""));
        assert!(lua.contains("author      = \"rb\""));
        assert!(lua.contains("version     = \"2\""));
    }

    #[test]
    fn mapinfo_escapes_lua_strings() {
        let codec = SpringSmfCodec;
        let mut plan = make_plan(MapSettings::default());
        plan.description = "He said \"hi\" \\ done".to_string();
        let lua = codec.generate_mapinfo("test", 32, 32, &plan);
        assert!(
            lua.contains(r#"description = "He said \"hi\" \\ done""#),
            "expected lua-escaped description; got:\n{lua}"
        );
    }
}
