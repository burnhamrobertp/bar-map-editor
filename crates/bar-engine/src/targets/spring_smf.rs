//! Spring/Recoil SMF codec implementation.
//!
//! Handles writing .smf (map), .smt (tiles), and mapinfo.lua for
//! Spring engine and its forks (Recoil/BAR).

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};

use bar_data::{generate_minimap_dxt1, write_smt, ColorBuffer, SmfFeaturePlacement, SmfMap};
use bar_project::PackageDir;

use super::codec::{ExportCodec, ExportPlan, WrittenFiles};
use super::config::TargetConfig;
use super::dimensions::{DimensionRule, DimensionSet};
use super::layers::{LayerSet, LayerStatus};
use super::lua_table::{fmt_f32, LuaTable};
use super::validation::{Severity, ValidationError};
use crate::export::resample_to_u8;
use crate::recipe::{MapSettings, PlacedFeature};

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
                &plan.features,
                plan.project_dir.as_deref(),
                &smf_path,
            )?;
            written.files.push(format!("maps/{}.smf", map_name));
            tracing::debug!("Wrote SMF: {}", smf_path.display());
        }

        // Write SMT (texture tiles) — generate height-based fallback if none provided.
        // When a compiled SMT exists and is current, copy it directly instead of
        // re-encoding at working resolution (avoids both the resize and DXT1 encode).
        let tex_target_w = sq_x * config.codec_params.texels_per_square;
        let tex_target_h = sq_y * config.codec_params.texels_per_square;
        let smt_path = maps_dir.join(format!("{}.smt", map_name));

        let used_compiled = if let Some(ref proj_dir) = plan.project_dir {
            if let Ok(pkg) = PackageDir::open(proj_dir) {
                let recipe_json = std::fs::read_to_string(pkg.recipe_path()).unwrap_or_default();
                let stale = pkg.is_stale(&recipe_json, sq_x, sq_y);
                tracing::debug!(stale, sq_x, sq_y, "Codec: compiled SMT staleness check");
                if !stale {
                    let compiled_smt = pkg.compiled_smt_path(map_name);
                    if compiled_smt.exists() {
                        tracing::debug!(src = %compiled_smt.display(), dst = %smt_path.display(), "Codec: copying compiled SMT");
                        fs::copy(&compiled_smt, &smt_path).with_context(|| {
                            format!(
                                "Failed to copy compiled SMT {} -> {}",
                                compiled_smt.display(),
                                smt_path.display()
                            )
                        })?;
                        tracing::debug!(
                            "Bundle: used compiled SMT (skipped re-encode): {}",
                            compiled_smt.display()
                        );
                        true
                    } else {
                        tracing::debug!(path = %compiled_smt.display(), "Codec: compiled SMT not found on disk -- will re-encode");
                        false
                    }
                } else {
                    tracing::debug!("Codec: compiled SMT is stale -- will re-encode");
                    false
                }
            } else {
                tracing::debug!("Codec: no project dir -- compiled SMT fast-path unavailable");
                false
            }
        } else {
            false
        };

        if !used_compiled {
            if let Some(ref texture) = layers.texture {
                let file = File::create(&smt_path)?;
                let mut writer = BufWriter::new(file);
                let scaled = texture.resize(tex_target_w, tex_target_h);
                write_smt(&mut writer, &scaled)?;
            } else if let Some(ref heightmap) = layers.heightmap {
                let fallback = generate_fallback_texture(heightmap, tex_target_w, tex_target_h);
                let file = File::create(&smt_path)?;
                let mut writer = BufWriter::new(file);
                write_smt(&mut writer, &fallback)?;
            }
        }
        written.files.push(format!("maps/{}.smt", map_name));
        tracing::debug!("Wrote SMT: {}", smt_path.display());

        // Write metadata (mapinfo.lua)
        let mapinfo = self.generate_mapinfo(map_name, sq_x, sq_y, plan);
        let mapinfo_path = output_dir.join("mapinfo.lua");
        fs::write(&mapinfo_path, &mapinfo)?;
        written.files.push("mapinfo.lua".to_string());
        tracing::debug!("Wrote mapinfo: {}", mapinfo_path.display());

        // The engine reads neither a separate normals.png (normals
        // are derived from the heightmap at runtime) nor a debug
        // grassmap PNG (the grass widget samples `grassDistTGA` set
        // in mapinfo). Earlier versions of this codec wrote both as
        // "preview / debugging" artifacts; they were shipping in
        // every bundled archive untouched by anything. Drop them.

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
        features: &[PlacedFeature],
        project_dir: Option<&Path>,
        path: &Path,
    ) -> Result<()> {
        let mut smf =
            SmfMap::new(map_x as i32, map_y as i32).context("Failed to create SMF map")?;
        smf.heightmap = heightmap.clone();
        smf.smt_filename = format!("maps/{}.smt", map_name);
        let rs = settings.resolved();
        smf.header.min_height = rs.min_height;
        smf.header.max_height = rs.max_height;
        // Only SMF-native placements go into the SMF feature section.
        // FeaturePlacer-set features are written separately to
        // `mapconfig/featureplacer/set.lua` (no length cap, gadget
        // spawns at runtime). Conflating the two truncates long FP
        // names at the engine reader's 31-char limit -- see
        // `bar_project::recipe::FeatureSource` for the full
        // explanation.
        //
        // Belt-and-suspenders: ALSO skip any feature whose name
        // would overflow the engine's 31-char reader buffer (see
        // `SMFMapFile.h:62` -- `char featureTypes[16384][32]`, read
        // up to K-1 bytes per name). This catches features that
        // came from older recipes (pre-source-tracking) where the
        // serde default tagged FP entries as `Smf`. Without this
        // guard, exporting such a recipe writes names that get
        // truncated by the engine reader and split into garbled
        // pieces, eventually triggering a `LoadFeatureDefsFromMap`
        // access violation when something downstream indexes by
        // the bogus type. 31 = K-1.
        use bar_project::recipe::FeatureSource;
        const ENGINE_FEATURE_NAME_LIMIT: usize = 31;
        smf.features = features
            .iter()
            .filter(|f| matches!(f.source, FeatureSource::Smf))
            .filter(|f| {
                if f.feature_type.len() <= ENGINE_FEATURE_NAME_LIMIT {
                    true
                } else {
                    tracing::warn!(
                        feature_type = %f.feature_type,
                        len = f.feature_type.len(),
                        "SMF export: skipping feature with name > 31 chars (engine reader would truncate)"
                    );
                    false
                }
            })
            .map(|f| SmfFeaturePlacement {
                feature_type: f.feature_type.clone(),
                x: f.x,
                y: f.y,
                z: f.z,
                // PlacedFeature.angle is in Spring heading units
                // (full circle = 65536); the SMF binary expects
                // `MapFeatureStruct.rotation` in radians. Convert at
                // the export boundary.
                angle: f.angle * std::f32::consts::PI / 32768.0,
                taken_damage: f.taken_damage,
            })
            .collect();

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

        // Generate minimap: 1024x1024 top-down view, DXT1 compressed with 9 mip levels.
        // User-chosen minimap_override file (from `MapSettings.minimap`,
        // resolved against `<project>/passthrough/`) wins over the
        // terrain-derived fallback so authored maps preserve their
        // hand-drawn minimap art instead of getting auto-regenerated
        // from the texture layer on every bundle.
        let override_rgba = settings
            .minimap
            .as_deref()
            .filter(|s| !s.is_empty())
            .zip(project_dir)
            .and_then(|(name, dir)| load_minimap_override(dir, name));
        let minimap_rgba = if let Some(rgba) = override_rgba {
            rgba
        } else if let Some(ref texture) = layers.texture {
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
    pub fn generate_mapinfo(
        &self,
        name: &str,
        map_x: u32,
        map_y: u32,
        plan: &ExportPlan,
    ) -> String {
        let settings = &plan.settings;
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
        // `name` is the engine-visible map identity (used to build the
        // archive ID at script lookup time), distinct from the
        // filesystem slug `name` parameter passed in for `mapfile`.
        out.push_str(&format!(
            "    name        = \"{}\",\n",
            esc(&plan.display_name)
        ));
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
        if let Some(s) = plan.tip.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("    tip         = \"{}\",\n", esc(s)));
        }
        out.push_str(&format!("    mapfile     = \"maps/{}.smf\",\n", name));
        out.push_str("    modtype     = 3,\n");
        if !plan.depend.is_empty() {
            out.push_str("    depend      = {\n");
            for dep in &plan.depend {
                out.push_str(&format!("        \"{}\",\n", esc(dep)));
            }
            out.push_str("    },\n");
        }

        // ── Physics (top-level scalars) ───────────────────────────
        // Bare `key = value` lines, no surrounding sub-table.
        // Booleans use `opt_bool` so an explicit-false from the
        // source round-trips verbatim (`voidWater = false` survives
        // a re-bundle instead of collapsing to absent).
        // `deformable` is stored on the recipe as the positive form
        // but the engine spells it `notDeformable` (inverse flag),
        // so we invert at emit time.
        let physics = {
            let mut t = LuaTable::new(4);
            t.opt("maphardness", settings.map_hardness)
                .opt_bool("notDeformable", settings.deformable.map(|v| !v))
                .opt_f32("gravity", settings.gravity)
                .opt_f32("tidalStrength", settings.tidal_strength)
                .opt_f32("maxMetal", settings.max_metal)
                .opt_f32("extractorRadius", settings.extractor_radius)
                .opt_bool("voidWater", settings.void_water)
                .opt_bool("voidGround", settings.void_ground)
                .opt_bool("autoShowMetal", settings.auto_show_metal);
            t.finish_bare()
        };
        if let Some(block) = physics {
            out.push('\n');
            out.push_str(&block);
        }

        // ── smf block ─────────────────────────────────────────────
        // smtFileName0 is the only required field. min/max heights
        // emit only when set on the recipe -- the SMF binary already
        // carries them in its header, so omitting from mapinfo lets
        // the engine read them from there.
        out.push_str("\n    smf = {\n");
        if let Some(v) = settings.min_height {
            out.push_str(&format!("        minheight = {},\n", f(v)));
        }
        if let Some(v) = settings.max_height {
            out.push_str(&format!("        maxheight = {},\n", f(v)));
        }
        out.push_str(&format!("        smtFileName0 = \"maps/{}.smt\",\n", name));
        out.push_str("    },\n");

        // ── resources = { ... } ───────────────────────────────────
        // Every texture filename the source mapinfo declared rounds
        // trips back through this block. Empty strings act as the
        // unset sentinel for these path fields (matches the engine's
        // "absent ≡ no texture" convention), so we don't emit them.
        let res = &settings.resources;
        // `splatDetailTex` is a presence flag, not a sampled texture: the SSMF
        // shader enables the detail-normal splat path only when it is set, but
        // never reads the file. So if the map declares splat detail normals but
        // left the flag empty, emit the community placeholder -- otherwise the
        // normals silently render nothing in-game.
        let has_splat_normals = !res.splat_detail_normal_tex_1.is_empty()
            || !res.splat_detail_normal_tex_2.is_empty()
            || !res.splat_detail_normal_tex_3.is_empty()
            || !res.splat_detail_normal_tex_4.is_empty();
        let splat_detail_flag = if !res.splat_detail_tex.is_empty() {
            res.splat_detail_tex.clone()
        } else if has_splat_normals {
            bar_project::SPLAT_DETAIL_FLAG_PLACEHOLDER.to_string()
        } else {
            String::new()
        };
        let res_block = {
            let mut t = LuaTable::new(8);
            t.opt_str("detailTex", Some(res.detail_tex.as_str()))
                .opt_str("splatDistrTex", Some(res.splat_distr_tex.as_str()))
                .opt_str(
                    "splatDetailNormalTex1",
                    Some(res.splat_detail_normal_tex_1.as_str()),
                )
                .opt_str(
                    "splatDetailNormalTex2",
                    Some(res.splat_detail_normal_tex_2.as_str()),
                )
                .opt_str(
                    "splatDetailNormalTex3",
                    Some(res.splat_detail_normal_tex_3.as_str()),
                )
                .opt_str(
                    "splatDetailNormalTex4",
                    Some(res.splat_detail_normal_tex_4.as_str()),
                )
                .opt_str("skyReflectModTex", Some(res.sky_reflect_mod_tex.as_str()))
                .opt_str("specularTex", Some(res.specular_tex.as_str()))
                .opt_str("grassShadingTex", Some(res.grass_shading_tex.as_str()))
                .opt_str("lightEmissionTex", Some(res.light_emission_tex.as_str()))
                .opt_str("detailNormalTex", Some(res.detail_normal_tex.as_str()))
                .opt_str("splatDetailTex", Some(splat_detail_flag.as_str()));
            if res.splat_detail_normal_diffuse_alpha {
                t.opt_bool("splatDetailNormalDiffuseAlpha", Some(true));
            }
            t.finish_block(4, "resources")
        };
        if let Some(block) = res_block {
            out.push('\n');
            out.push_str(&block);
        }

        // ── custom = { ... } block ────────────────────────────────
        // Collects every sub-table BME emits under `custom`. Only
        // emitted if at least one piece is non-default, so empty
        // recipes don't ship a vacuous `custom = {}`.
        let mut custom_parts: Vec<String> = Vec::new();

        if !settings.detail_textures.is_empty() {
            let mut s = String::from("        dnts = {\n");
            for (i, dt) in settings.detail_textures.iter().enumerate() {
                s.push_str(&format!(
                    "            [{}] = {{ file = \"{}\", scale = {} }},\n",
                    i,
                    esc(&dt.path),
                    f(dt.scale),
                ));
            }
            s.push_str("        },\n");
            custom_parts.push(s);
        }

        // grassConfig drives BAR's map_grass_gl4 widget. Emits ONLY
        // fields the user actually set (Some); never synthesises
        // defaults the source mapinfo didn't have. The widget falls
        // back to its own hard-coded defaults for anything we omit
        // (`map_grass_gl4.lua:87-110`), so we don't have to mirror
        // them here -- and BME drift never bakes stale values into
        // the bundled archive. `is_enabled()` gates the whole block:
        // a configuration with no distribution mask path is the BAR
        // widget's "do nothing" signal, so we don't emit a vacuous
        // `grassConfig = {}` that the widget would still treat as
        // "render zero blades".
        // custom.fog sub-block: only present when the source mapinfo
        // had one. Round-trips the height-fog volume the engine's
        // widget reads (`color`, `height`, `fogatten`).
        let cf = &settings.custom_fog;
        if cf.enabled {
            let mut t = LuaTable::new(12);
            t.opt_vec3("color", Some(cf.color))
                .opt_f32("height", Some(cf.height_elmos))
                .opt_f32("fogatten", Some(cf.atten));
            if let Some(block) = t.finish_block(8, "fog") {
                custom_parts.push(block);
            }
        }

        // custom.clouds sub-block: volumetric-cloud widget config.
        let cc = &settings.custom_clouds;
        let clouds_block = {
            let mut t = LuaTable::new(12);
            t.opt_f32("speed", cc.speed)
                .opt_vec3("color", cc.color)
                .opt_f32("height", cc.height)
                .opt_f32("bottom", cc.bottom)
                .opt_f32("fade_alt", cc.fade_alt)
                .opt_f32("scale", cc.scale)
                .opt_f32("opacity", cc.opacity)
                .opt_bool("clamp_to_map", cc.clamp_to_map)
                .opt_f32("sun_penetration", cc.sun_penetration);
            t.finish_block(8, "clouds")
        };
        if let Some(block) = clouds_block {
            custom_parts.push(block);
        }

        let g = &settings.custom_grass;
        if g.is_enabled() {
            // Inner shader-params sub-table -- only fields the user
            // set get emitted, and the wrapper itself only emits when
            // at least one field is present.
            let mut sp = LuaTable::new(16);
            sp.opt_f32("MAPCOLORFACTOR", g.map_color_factor)
                .opt_f32("MAPCOLORBASE", g.map_color_base)
                .opt_f32("ALPHATHRESHOLD", g.alpha_threshold)
                .opt_f32("SHADOWFACTOR", g.shadow_factor)
                .opt_f32("GRASSBRIGHTNESS", g.grass_brightness)
                .opt_f32("FADESTART", g.fade_start)
                .opt_f32("FADEEND", g.fade_end)
                .opt_f32("WINDSTRENGTH", g.wind_strength)
                .opt_f32("WINDSCALE", g.wind_scale)
                .opt_f32("WINDSAMPLESCALE", g.wind_sample_scale);
            let shader_block = sp.finish_block(12, "grassShaderParams");

            let mut t = LuaTable::new(12);
            t.opt_str("grassDistTGA", g.dist_tga.as_deref())
                .opt_str("grassBladeColorTex", g.blade_color_tex.as_deref())
                .opt_f32("grassMaxSize", g.max_size)
                .opt_f32("grassMinSize", g.min_size)
                .opt("patchResolution", g.patch_resolution)
                .opt_f32("patchPlacementJitter", g.patch_placement_jitter);
            if let Some(block) = shader_block {
                t.child(&block);
            }
            t.opt_f32("grassWindMult", g.grass_wind_mult);
            if let Some(block) = t.finish_block(8, "grassConfig") {
                custom_parts.push(block);
            }
        }

        if !custom_parts.is_empty() {
            out.push_str("\n    custom = {\n");
            for part in &custom_parts {
                out.push_str(part);
            }
            out.push_str("    },\n");
        }

        // ── sound = { preset, passfilter } ────────────────────────
        // Top-level (not under `custom`). Preset name + passfilter
        // gains; reverb sub-block is not modelled (unused on real
        // maps -- most leave it as a comment block).
        let snd = &settings.sound;
        let sound_block = {
            let mut t = LuaTable::new(8);
            t.opt_str("preset", snd.preset.as_deref());
            let pf_block = {
                let mut pf = LuaTable::new(12);
                pf.opt_f32("gainlf", snd.passfilter_gainlf)
                    .opt_f32("gainhf", snd.passfilter_gainhf);
                pf.finish_block(8, "passfilter")
            };
            if let Some(b) = pf_block {
                t.child(&b);
            }
            t.finish_block(4, "sound")
        };
        if let Some(block) = sound_block {
            out.push('\n');
            out.push_str(&block);
        }

        // ── splats = { texScales = {...}, texMults = {...} } ──────
        // Sibling of `resources` at the mapinfo root. Recipe carries
        // these as `Option<[f32; 4]>` so a source that didn't define
        // splats stays None and the block is omitted; a source that
        // did defines exact channel values that round-trip back.
        let splats_block = {
            let mut t = LuaTable::new(8);
            t.opt_vec4("texScales", res.splat_tex_scales)
                .opt_vec4("texMults", res.splat_tex_mults);
            t.finish_block(4, "splats")
        };
        if let Some(block) = splats_block {
            out.push('\n');
            out.push_str(&block);
        }

        // ── Atmosphere / lighting / water ─────────────────────────
        // Each block is built with the `LuaTable` chain helper. One
        // line per modelled field; `None` values are skipped at the
        // builder, so the bundled mapinfo only carries values the
        // recipe actually set. Adding a new mapinfo key here is one
        // builder call -- no per-field `if let Some / push_str` copy.
        let atm = &settings.atmosphere;
        let atm_block = {
            let mut t = LuaTable::new(8);
            t.opt_f32("minWind", atm.min_wind)
                .opt_f32("maxWind", atm.max_wind)
                .opt_f32("fogStart", atm.fog_start)
                .opt_f32("fogEnd", atm.fog_end)
                .opt_vec3("fogColor", atm.fog_color)
                .opt_vec3("sunColor", atm.sun_color)
                .opt_vec3("skyColor", atm.sky_color)
                .opt_vec3("skyDir", atm.sky_dir)
                .opt_f32("cloudDensity", atm.cloud_density)
                .opt_vec3("cloudColor", atm.cloud_color)
                .opt_str("skyBox", atm.skybox.as_deref());
            t.finish_block(4, "atmosphere")
        };
        if let Some(block) = atm_block {
            out.push('\n');
            out.push_str(&block);
        }

        let lit = &settings.lighting;
        let lit_block = {
            let mut t = LuaTable::new(8);
            // Engine `sunDir` is a `float4(x, y, z, intensity)`. When
            // intensity is `Some`, emit the 4-element form so the
            // shader can pick it up; otherwise emit a bare vec3.
            match (lit.sun_dir, lit.sun_intensity) {
                (Some(d), Some(i)) => {
                    t.child(&format!(
                        "        sunDir = {{ {}, {}, {}, {} }},\n",
                        fmt_f32(d[0]),
                        fmt_f32(d[1]),
                        fmt_f32(d[2]),
                        fmt_f32(i)
                    ));
                }
                (Some(d), None) => {
                    t.opt_vec3("sunDir", Some(d));
                }
                (None, Some(i)) => {
                    // Intensity set without explicit direction: emit
                    // engine-default direction with the explicit `w`.
                    t.child(&format!(
                        "        sunDir = {{ 0, 1, 2, {} }},\n",
                        fmt_f32(i)
                    ));
                }
                (None, None) => {}
            }
            t.opt_vec3("groundAmbientColor", lit.ground_ambient)
                .opt_vec3("groundDiffuseColor", lit.ground_diffuse)
                .opt_vec3("groundSpecularColor", lit.ground_specular)
                .opt_f32("specularExponent", lit.spec_exponent)
                .opt_f32("groundShadowDensity", lit.ground_shadow_density)
                .opt_f32("unitShadowDensity", lit.unit_shadow_density)
                .opt_vec3("unitAmbientColor", lit.unit_ambient)
                .opt_vec3("unitDiffuseColor", lit.unit_diffuse)
                .opt_vec3("unitSpecularColor", lit.unit_specular);
            t.finish_block(4, "lighting")
        };
        if let Some(block) = lit_block {
            out.push('\n');
            out.push_str(&block);
        }

        let wat = &settings.water;
        let lav = &settings.lava;
        // Water mode forces damage to zero so a stale value doesn't
        // accidentally trip the engine's water-damage fallback in
        // `bar-game/modules/lava.lua` (one of its lava triggers).
        // Lava mode exports the lava-side damage value so the engine
        // charges the right amount even if the lava gadget fails to
        // load. `fluid_mode == None` means "user expressed no
        // preference" -- leave the stored water damage alone so an
        // empty recipe doesn't drag `water = { damage = 0 }` into
        // mapinfo.
        let exported_water_damage = match settings.fluid_mode {
            Some(bar_project::recipe::FluidMode::Water) => Some(0.0),
            Some(bar_project::recipe::FluidMode::Lava) => lav.damage.or(wat.damage),
            None => wat.damage,
        };
        let wat_block = {
            let mut t = LuaTable::new(8);
            t.opt_f32("damage", exported_water_damage)
                .opt_vec3("absorb", wat.absorb)
                .opt_vec3("baseColor", wat.base_color)
                .opt_vec3("minColor", wat.min_color)
                .opt_vec3("surfaceColor", wat.surface_color)
                .opt_f32("surfaceAlpha", wat.surface_alpha)
                .opt_vec3("diffuseColor", wat.diffuse_color)
                .opt_vec3("specularColor", wat.specular_color)
                .opt_f32("ambientFactor", wat.ambient_factor)
                .opt_f32("diffuseFactor", wat.diffuse_factor)
                .opt_f32("specularFactor", wat.specular_factor)
                .opt_f32("specularPower", wat.specular_power)
                .opt_f32("fresnelMin", wat.fresnel_min)
                .opt_f32("fresnelMax", wat.fresnel_max)
                .opt_f32("fresnelPower", wat.fresnel_power)
                .opt_f32("reflectionDistortion", wat.reflection_distortion)
                .opt_f32("perlinAmplitude", wat.perlin_amplitude)
                .opt_f32("blurBase", wat.blur_base)
                .opt_f32("blurExponent", wat.blur_exponent)
                .opt_f32("causticsResolution", wat.caustics_resolution)
                .opt_f32("causticsStrength", wat.caustics_strength)
                .opt_f32("waveOffsetFactor", wat.wave_offset_factor)
                .opt_f32("waveFoamDistortion", wat.wave_foam_distortion)
                .opt_f32("waveFoamIntensity", wat.wave_foam_intensity)
                .opt_f32("waveLength", wat.wave_length)
                .opt_bool("forceRendering", wat.force_rendering)
                .opt_bool("hasWaterPlane", wat.has_water_plane)
                .opt("numTiles", wat.num_tiles)
                .opt_f32("perlinStartFreq", wat.perlin_start_freq)
                .opt_f32("perlinLacunarity", wat.perlin_lacunarity)
                .opt_vec3("planeColor", wat.plane_color)
                .opt_f32("repeatX", wat.repeat_x)
                .opt_f32("repeatY", wat.repeat_y)
                .opt_bool("shoreWaves", wat.shore_waves)
                .opt_str("normalTexture", wat.normal_texture.as_deref());
            t.finish_block(4, "water")
        };
        if let Some(block) = wat_block {
            out.push('\n');
            out.push_str(&block);
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

        // ── replace = { ... } ─────────────────────────────────────
        // Engine-meaningful but almost always empty on real maps.
        // Round-trip preserves the source's explicit (empty) `{}` so
        // a re-bundled archive matches byte-for-byte.
        if settings.replace.declared {
            out.push_str("\n    replace = {\n");
            for (k, v) in &settings.replace.entries {
                out.push_str(&format!("        [\"{}\"] = \"{}\",\n", esc(k), esc(v)));
            }
            out.push_str("    },\n");
        }

        // ── terrainTypes = { [N] = { ... } } ──────────────────────
        // Per-terrain-index movement / hardness / track-receiving
        // descriptors. Round-trips the author's authored entries.
        if !settings.terrain_types.is_empty() {
            out.push_str("\n    terrainTypes = {\n");
            for entry in &settings.terrain_types {
                out.push_str(&format!("        [{}] = {{\n", entry.index));
                if let Some(name) = entry.name.as_deref() {
                    out.push_str(&format!("            name = \"{}\",\n", esc(name)));
                }
                if let Some(h) = entry.hardness {
                    out.push_str(&format!("            hardness = {},\n", fmt_f32(h)));
                }
                if let Some(rt) = entry.receive_tracks {
                    out.push_str(&format!("            receiveTracks = {},\n", rt));
                }
                if !entry.move_speeds.is_empty() {
                    out.push_str("            moveSpeeds = {\n");
                    for (k, v) in &entry.move_speeds {
                        out.push_str(&format!("                {} = {},\n", k, fmt_f32(*v)));
                    }
                    out.push_str("            },\n");
                }
                out.push_str("        },\n");
            }
            out.push_str("    },\n");
        }

        out.push_str("}\n\n");
        // ── Runtime normalisation: lowerkeys + mapconfig merge ────
        // BAR-shipped mapinfos all end with a `lowerkeys(mapinfo)`
        // call that recursively lowercases every table key. Without
        // it, Lua widgets (case-sensitive table reads) can't find
        // sub-tables they expect at lowercase paths -- the
        // `map_grass_gl4` widget reads `mapinfo.custom.grassconfig`
        // (lowercase) and gets `nil` if the bundled mapinfo has
        // `grassConfig` (camelCase). Emit the helper + call so
        // bundled maps behave like authored ones.
        //
        // The do/end block following lowerkeys is the standard BAR
        // `mapconfig/mapinfo/*.lua` overlay loader: any per-game
        // map-option file in that path can mutate the mapinfo table
        // at load time (e.g. Onyx Cauldron's `0_apply_options.lua`
        // adjusts heightmap range based on a "Dry" lobby option).
        // Boilerplate-identical across BAR maps; carrying it keeps
        // those overlays working on re-bundled archives.
        out.push_str(LOWERKEYS_AND_MERGE_BOILERPLATE);
        out.push_str("\nreturn mapinfo\n");
        out
    }
}

/// Standard BAR mapinfo postlude: `lowerkeys` helper, the call that
/// lowercases the mapinfo table, and the `mapconfig/mapinfo/*.lua`
/// merge loop. Copied verbatim from authored BAR maps so re-bundled
/// archives expose the same key casing + mapconfig-overlay behaviour
/// the engine and widgets expect.
const LOWERKEYS_AND_MERGE_BOILERPLATE: &str = r#"local function lowerkeys(ta)
    local fix = {}
    for i, v in pairs(ta) do
        if (type(i) == "string") then
            if (i ~= i:lower()) then
                fix[#fix + 1] = i
            end
        end
        if (type(v) == "table") then
            lowerkeys(v)
        end
    end
    for i = 1, #fix do
        local idx = fix[i]
        ta[idx:lower()] = ta[idx]
        ta[idx] = nil
    end
end

lowerkeys(mapinfo)

local function tmerge(t1, t2)
    for i, v in pairs(t2) do
        if (type(v) == "table") then
            t1[i] = t1[i] or {}
            tmerge(t1[i], v)
        else
            t1[i] = v
        end
    end
end

getfenv()["mapinfo"] = mapinfo
local files = VFS.DirList("mapconfig/mapinfo/", "*.lua")
table.sort(files)
for i = 1, #files do
    local newcfg = VFS.Include(files[i])
    if newcfg then
        lowerkeys(newcfg)
        tmerge(mapinfo, newcfg)
    end
end
getfenv()["mapinfo"] = nil
"#;

// ── mapinfo.lua merge ────────────────────────────────────────────────────────

/// Byte offset of the opening `{` in `local mapinfo = { ... }`.
fn find_mapinfo_open(lua: &str) -> Option<usize> {
    let header = lua.find("local mapinfo")?;
    lua[header..].find('{').map(|r| header + r)
}

/// Byte offset of the closing `}` of the mapinfo table.
fn find_mapinfo_close(lua: &str) -> usize {
    let open = match find_mapinfo_open(lua) {
        Some(p) => p,
        None => return lua.len(),
    };
    let bytes = lua.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut str_char = b'"';
    let mut i = open;
    while i < bytes.len() {
        if in_str {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == str_char {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => {
                in_str = true;
                str_char = bytes[i];
                i += 1;
            }
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    lua.len()
}

/// Extract `(key, raw_block)` pairs for each top-level assignment in the
/// mapinfo table. `raw_block` is the verbatim text from the key name up to
/// and including the trailing comma (or up to the closing `}` for the last
/// entry), trimmed of surrounding whitespace.
fn mapinfo_top_level_entries(lua: &str) -> Vec<(String, String)> {
    let table_open = match find_mapinfo_open(lua) {
        Some(p) => p,
        None => return Vec::new(),
    };
    // Work inside the outer `{`.
    let chars: Vec<char> = lua[table_open + 1..].chars().collect();
    let n = chars.len();
    let mut i = 0usize;
    let mut depth: i32 = 0; // 0 = top-level inside mapinfo table
    let mut entries: Vec<(String, String)> = Vec::new();

    while i < n {
        // String literal — skip contents to avoid false `{` / `}` matches.
        if (chars[i] == '"' || chars[i] == '\'') && depth == 0 {
            let q = chars[i];
            i += 1;
            while i < n {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                let done = chars[i] == q;
                i += 1;
                if done {
                    break;
                }
            }
            continue;
        }
        // Lua comment — skip to EOL at any depth.
        if chars[i] == '-' && i + 1 < n && chars[i + 1] == '-' {
            i += 2;
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match chars[i] {
            '{' => {
                depth += 1;
                i += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                i += 1;
            }
            '}' => break, // closing brace of mapinfo table
            c if depth == 0 && (c.is_alphabetic() || c == '_') => {
                // Start of a top-level key identifier.
                let entry_start = i;
                while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let key: String = chars[entry_start..i].iter().collect();
                // Skip horizontal whitespace.
                while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                    i += 1;
                }
                // Must be `=` (not `==`).
                if i < n && chars[i] == '=' && (i + 1 >= n || chars[i + 1] != '=') {
                    i += 1; // skip `=`
                            // Capture value until `,` or `}` at depth 0.
                    let mut val_depth: i32 = 0;
                    let mut val_in_str = false;
                    let mut val_str_char = '"';
                    loop {
                        if i >= n {
                            break;
                        }
                        if val_in_str {
                            if chars[i] == '\\' {
                                i += 2;
                                continue;
                            }
                            if chars[i] == val_str_char {
                                val_in_str = false;
                            }
                            i += 1;
                            continue;
                        }
                        match chars[i] {
                            '"' | '\'' => {
                                val_in_str = true;
                                val_str_char = chars[i];
                                i += 1;
                            }
                            '-' if i + 1 < n && chars[i + 1] == '-' => {
                                i += 2;
                                while i < n && chars[i] != '\n' {
                                    i += 1;
                                }
                            }
                            '{' => {
                                val_depth += 1;
                                i += 1;
                            }
                            '}' if val_depth > 0 => {
                                val_depth -= 1;
                                i += 1;
                            }
                            '}' if val_depth == 0 => {
                                // Last entry; no trailing comma.
                                let raw: String = chars[entry_start..i].iter().collect();
                                entries.push((key, raw.trim().to_string()));
                                break;
                            }
                            ',' if val_depth == 0 => {
                                let raw: String = chars[entry_start..=i].iter().collect();
                                entries.push((key, raw.trim().to_string()));
                                i += 1;
                                break;
                            }
                            _ => {
                                i += 1;
                            }
                        }
                    }
                }
                // If not followed by `=`, just skip past whatever it was.
            }
            _ => {
                i += 1;
            }
        }
    }

    entries
}

/// Merge two `mapinfo.lua` strings. `generated` is authoritative for all
/// fields the editor manages (name, mapfile, smf block, teams, etc.).
/// `original` provides any top-level keys absent from `generated` — e.g.
/// `depend`, `replace`, or custom game-mod entries — which are appended to
/// the output verbatim. If `generated` doesn't contain a mapinfo table,
/// `original` is returned unchanged.
pub fn merge_mapinfo_lua(generated: &str, original: &str) -> String {
    if !generated.contains("local mapinfo") {
        return original.to_string();
    }

    let gen_keys: std::collections::HashSet<String> = mapinfo_top_level_entries(generated)
        .into_iter()
        .map(|(k, _)| k)
        .collect();

    let extras: Vec<String> = mapinfo_top_level_entries(original)
        .into_iter()
        .filter(|(k, _)| !gen_keys.contains(k))
        .map(|(_, raw)| raw)
        .collect();

    if extras.is_empty() {
        return generated.to_string();
    }

    let close = find_mapinfo_close(generated);
    let mut result = generated[..close].to_string();
    if !result.ends_with('\n') {
        result.push('\n');
    }
    for extra in &extras {
        result.push_str("    ");
        result.push_str(extra);
        result.push('\n');
    }
    result.push_str(&generated[close..]);
    result
}

/// Generate a height-based fallback texture when no diffuse texture layer is provided.
/// Low areas are dark green, mid areas are brown/tan, high areas are gray/white.
/// Resolve and decode a user-supplied minimap image to a flat 1024x1024
/// RGBA buffer in the layout the DXT1 minimap encoder expects. Returns
/// `None` if the file can't be found (in either `passthrough/` or the
/// project root) or fails to decode -- the caller falls back to the
/// terrain-derived auto-minimap in that case.
fn load_minimap_override(project_dir: &Path, filename: &str) -> Option<Vec<u8>> {
    let path = bar_project::find_file_in_dir(&project_dir.join("passthrough"), filename)
        .or_else(|| bar_project::find_file_in_dir(project_dir, filename))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let (rgba, w, h) = if ext == "dds" {
        bar_data::load_dds_2d(&path).ok()?
    } else {
        let bytes = std::fs::read(&path).ok()?;
        let fmt = image::ImageFormat::from_extension(&ext)?;
        let img = image::load_from_memory_with_format(&bytes, fmt).ok()?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        (rgba.into_raw(), w, h)
    };
    if w == 1024 && h == 1024 {
        return Some(rgba);
    }
    let buf = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, rgba)?;
    let resized = image::imageops::resize(&buf, 1024, 1024, image::imageops::FilterType::Triangle);
    Some(resized.into_raw())
}

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
            display_name: "test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: Vec::new(),
            dimensions: dims,
            settings: MapSettings::default(),
            features: Vec::new(),
            project_dir: None,
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
            display_name: "test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            tip: None,
            depend: Vec::new(),
            dimensions: codec.compute_dimensions(&config, 257, 257),
            settings,
            features: Vec::new(),
            project_dir: None,
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
        s.atmosphere.fog_color = Some([0.1, 0.2, 0.3]);
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
        let s = MapSettings {
            void_water: Some(true),
            void_ground: Some(true),
            ..MapSettings::default()
        };
        let lua = codec.generate_mapinfo("test", 32, 32, &make_plan(s));
        assert!(lua.contains("voidWater"));
        assert!(lua.contains("voidGround"));
    }

    #[test]
    fn mapinfo_always_emits_required_fields() {
        let codec = SpringSmfCodec;
        let plan = make_plan(MapSettings::default());
        let lua = codec.generate_mapinfo("kolmog", 32, 32, &plan);
        // Engine-required floor: name (from display_name), mapfile +
        // smtFileName0 (from the filesystem slug), teams.
        // make_plan sets display_name="test", so the engine-visible
        // identity is "test" while the on-disk slug is "kolmog".
        assert!(lua.contains("name        = \"test\""));
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
    fn merge_preserves_unknown_keys_from_original() {
        let generated = "local mapinfo = {\n    name = \"foo\",\n    mapfile = \"maps/foo.smf\",\n}\n\nreturn mapinfo\n";
        let original = "local mapinfo = {\n    name = \"old\",\n    depend = { \"BAR\" },\n    replace = {},\n}\nreturn mapinfo\n";
        let merged = merge_mapinfo_lua(generated, original);
        // Editor-managed fields from generated win.
        assert!(
            merged.contains("name = \"foo\""),
            "generated name should win"
        );
        assert!(
            !merged.contains("name = \"old\""),
            "original name must be dropped"
        );
        // Unknown fields from original are preserved.
        assert!(merged.contains("depend"), "depend should be carried over");
        assert!(merged.contains("replace"), "replace should be carried over");
    }

    #[test]
    fn merge_empty_original_returns_generated() {
        let generated = "local mapinfo = {\n    name = \"x\",\n}\nreturn mapinfo\n";
        let merged = merge_mapinfo_lua(generated, "");
        assert_eq!(merged, generated);
    }

    #[test]
    fn merge_empty_generated_returns_original() {
        let original = "local mapinfo = {\n    name = \"y\",\n}\nreturn mapinfo\n";
        let merged = merge_mapinfo_lua("", original);
        assert_eq!(merged, original);
    }

    #[test]
    fn merge_no_extras_returns_generated_unchanged() {
        let generated = "local mapinfo = {\n    name = \"foo\",\n    mapfile = \"maps/foo.smf\",\n}\n\nreturn mapinfo\n";
        let original = "local mapinfo = {\n    name = \"bar\",\n    mapfile = \"maps/bar.smf\",\n}\nreturn mapinfo\n";
        let merged = merge_mapinfo_lua(generated, original);
        assert_eq!(
            merged, generated,
            "no extras means generated is returned as-is"
        );
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

    // ── Robust round-trip tests ─────────────────────────────────────
    //
    // These tests exercise the mapinfo emitter as a structural
    // round-trip rather than asserting one field at a time. The point
    // is to detect drift between emitter and parser WITHOUT needing
    // hand-maintained per-field assertions:
    //
    // * `default_recipe_emits_only_the_required_floor` enumerates the
    //   small set of keys the engine demands; any other top-level key
    //   in a default-recipe emission is a regression (the emitter
    //   synthesised a value that wasn't there).
    //
    // * `fully_populated_settings_round_trip_through_emit_and_parse`
    //   sets every modelled `Option<T>` to a distinct value, emits,
    //   re-parses, and `assert_eq!`s the parsed result against the
    //   original via the derived `PartialEq`. Adding a new Option
    //   field automatically gets tested as long as the helper below
    //   sets it; if you add a parsable-but-not-emitted field (or
    //   vice-versa) this test fails.
    //
    // Maintenance shape: the only thing that has to be updated when
    // adding a new mapinfo key is `fully_populated_settings()` -- one
    // line per field. All other tests pick it up automatically.

    /// Reference fixture: a `MapSettings` with every modelled
    /// `Option<T>` field set to a distinct, recognisable value. The
    /// round-trip test compares this to its emit-then-parse result, so
    /// every field this fixture knows about gets exercised end-to-end.
    fn fully_populated_settings() -> MapSettings {
        use crate::recipe::{
            AtmosphereSettings, CustomGrassSettings, LightingSettings, WaterSettings,
        };
        MapSettings {
            min_height: Some(-50.0),
            max_height: Some(800.0),
            map_hardness: Some(150),
            gravity: Some(80.0),
            detail_textures: Vec::new(),
            deformable: Some(false),
            void_water: Some(true),
            void_ground: Some(true),
            tidal_strength: Some(18.0),
            max_metal: Some(3.6),
            extractor_radius: Some(90.0),
            auto_show_metal: Some(true),
            replace: crate::recipe::ReplaceTable {
                declared: true,
                entries: Vec::new(),
            },
            terrain_types: vec![crate::recipe::TerrainTypeEntry {
                index: 0,
                name: Some("Ground".to_string()),
                hardness: Some(1.0),
                receive_tracks: Some(true),
                move_speeds: vec![
                    ("tank".to_string(), 1.0),
                    ("kbot".to_string(), 1.0),
                    ("hover".to_string(), 1.0),
                    ("ship".to_string(), 1.0),
                ],
            }],
            atmosphere: AtmosphereSettings {
                min_wind: Some(1.0),
                max_wind: Some(29.0),
                fog_start: Some(0.5),
                fog_end: Some(1.0),
                fog_color: Some([0.11, 0.13, 0.15]),
                sun_color: Some([1.0, 0.92, 0.78]),
                sky_color: Some([0.43, 0.58, 0.64]),
                sky_dir: Some([0.0, 0.5, -1.0]),
                cloud_density: Some(0.3),
                cloud_color: Some([0.9, 0.9, 0.9]),
                skybox: Some("cleardesert.dds".to_string()),
            },
            lighting: LightingSettings {
                sun_dir: Some([-0.64, 0.66, -0.57]),
                sun_intensity: Some(0.75),
                ground_ambient: Some([0.56, 0.55, 0.55]),
                ground_diffuse: Some([0.75, 0.75, 0.8]),
                ground_specular: Some([0.5, 0.5, 0.5]),
                spec_exponent: Some(100.0),
                ground_shadow_density: Some(0.75),
                unit_shadow_density: Some(0.9),
                unit_ambient: Some([0.57, 0.57, 0.57]),
                unit_diffuse: Some([1.0, 0.98, 0.92]),
                unit_specular: Some([0.8, 0.6, 0.6]),
            },
            water: WaterSettings {
                damage: Some(0.5),
                absorb: Some([0.011, 0.011, 0.015]),
                base_color: Some([0.5, 0.68, 0.68]),
                min_color: Some([0.022, 0.0035, 0.035]),
                surface_color: Some([0.5, 0.6, 0.65]),
                surface_alpha: Some(0.6),
                diffuse_color: Some([1.0, 1.0, 1.0]),
                specular_color: Some([0.65, 0.65, 0.7]),
                ambient_factor: Some(0.3),
                diffuse_factor: Some(1.0),
                specular_factor: Some(0.8),
                specular_power: Some(20.0),
                fresnel_min: Some(0.2),
                fresnel_max: Some(1.4),
                fresnel_power: Some(8.0),
                reflection_distortion: Some(1.2),
                perlin_amplitude: Some(0.9),
                blur_base: Some(2.0),
                blur_exponent: Some(1.5),
                caustics_resolution: Some(75.0),
                caustics_strength: Some(0.08),
                wave_offset_factor: Some(0.0),
                wave_foam_distortion: Some(0.05),
                wave_foam_intensity: Some(0.5),
                wave_length: Some(0.15),
                force_rendering: Some(false),
                has_water_plane: Some(false),
                num_tiles: Some(4),
                perlin_start_freq: Some(8.0),
                perlin_lacunarity: Some(3.0),
                plane_color: Some([0.13, 0.22, 0.25]),
                repeat_x: Some(0.0),
                repeat_y: Some(0.0),
                shore_waves: Some(true),
                normal_texture: Some("maps/waterbump.dds".to_string()),
            },
            lava: Default::default(),
            fluid_mode: Some(bar_project::recipe::FluidMode::Lava),
            custom_fog: Default::default(),
            custom_grass: CustomGrassSettings {
                dist_tga: Some("maps/grass.tga".to_string()),
                blade_color_tex: Some("maps/blades.dds".to_string()),
                max_size: Some(2.0),
                min_size: Some(0.4),
                patch_resolution: Some(32),
                patch_placement_jitter: Some(0.6),
                map_color_factor: Some(0.2),
                map_color_base: Some(0.6),
                alpha_threshold: Some(0.02),
                shadow_factor: Some(0.25),
                grass_brightness: Some(1.1),
                fade_start: Some(5000.0),
                fade_end: Some(8000.0),
                wind_strength: Some(0.1),
                wind_scale: Some(0.33),
                wind_sample_scale: Some(0.001),
                grass_wind_mult: Some(4.5),
            },
            custom_clouds: crate::recipe::CustomCloudsSettings {
                speed: Some(0.05),
                color: Some([0.9, 0.9, 0.9]),
                height: Some(1100.0),
                bottom: Some(300.0),
                fade_alt: Some(400.0),
                scale: Some(750.0),
                opacity: Some(0.35),
                clamp_to_map: Some(false),
                sun_penetration: Some(15.0),
            },
            sound: crate::recipe::SoundSettings {
                preset: Some("default".to_string()),
                passfilter_gainlf: Some(1.0),
                passfilter_gainhf: Some(1.0),
            },
            resources: Default::default(),
            minimap: None,
            start_positions: Vec::new(),
        }
    }

    /// Emit + re-parse + return. The parser side is the canonical
    /// reader BME uses on import (`apply_mapinfo_overrides`), so any
    /// emit/parse drift surfaces here as a structural difference.
    fn roundtrip(settings: MapSettings) -> MapSettings {
        let codec = SpringSmfCodec;
        let plan = make_plan(settings);
        let lua = codec.generate_mapinfo("test", 32, 32, &plan);
        let mut out = MapSettings::default();
        bar_project::apply_mapinfo_overrides(&lua, &mut out);
        out
    }

    #[test]
    fn default_recipe_emits_only_the_required_floor() {
        // A `MapSettings::default()` has every Option as `None`. The
        // engine still requires a minimum set of keys (the "floor")
        // for the archive to be loadable. Anything emitted beyond
        // that is BME synthesising a value the recipe didn't set --
        // exactly the regression the Option refactor exists to
        // prevent. This test will catch a regression to the old
        // "emit defaults" pattern without enumerating every Option
        // field; adding a new Option doesn't require touching it.
        let codec = SpringSmfCodec;
        let lua = codec.generate_mapinfo("test", 32, 32, &make_plan(MapSettings::default()));
        let entries = mapinfo_top_level_entries(&lua);
        // Engine-required: `name` + `mapfile` to find the archive,
        // `modtype` to identify it as a map, `smf` for the SMT
        // reference, `teams` for spawn positions. Everything else is
        // optional per `bar-recoil/rts/Map/MapInfo.cpp`.
        let floor: std::collections::HashSet<&str> = ["name", "mapfile", "modtype", "smf", "teams"]
            .iter()
            .copied()
            .collect();
        for (key, raw) in &entries {
            assert!(
                floor.contains(key.as_str()),
                "Default-recipe mapinfo has unexpected key `{key}`:\n  raw:\n    {raw}\n\
                 Full output:\n{lua}\n\n\
                 This key was emitted despite no recipe field setting it. Likely\n\
                 a regression: a new field was added to the emitter without an\n\
                 Option gate, or a field's gate became unconditional."
            );
        }
        // Sanity: the floor itself is present (no accidental drop).
        let present: std::collections::HashSet<&str> =
            entries.iter().map(|(k, _)| k.as_str()).collect();
        for f in &floor {
            assert!(
                present.contains(f),
                "Engine-required key `{f}` missing from default emission:\n{lua}"
            );
        }
    }

    #[test]
    fn fully_populated_settings_round_trip_through_emit_and_parse() {
        // Property: for every Option field BME models, emit-then-parse
        // recovers the same value. Implementation: derived `PartialEq`
        // does the work. Adding a new Option field is automatically
        // covered once `fully_populated_settings` sets it; if the
        // emitter or parser don't agree on a field, the assert_eq
        // diff pinpoints exactly which one.
        let input = fully_populated_settings();
        let output = roundtrip(input.clone());

        assert_eq!(
            input.atmosphere, output.atmosphere,
            "atmosphere fields drift between emitter and parser"
        );
        assert_eq!(
            input.lighting, output.lighting,
            "lighting fields drift between emitter and parser"
        );
        assert_eq!(
            input.water, output.water,
            "water fields drift between emitter and parser"
        );
        assert_eq!(
            input.custom_grass, output.custom_grass,
            "custom_grass fields drift between emitter and parser"
        );

        // Top-level physics scalars. PartialEq across the whole
        // MapSettings would be ideal but `custom_fog` / `resources` /
        // `detail_textures` / `start_positions` aren't part of this
        // emit path yet (separate subsystems), so we compare just
        // what the emitter writes today. Adding emit support for one
        // of those subsystems means moving its field into this
        // assertion block.
        assert_eq!(input.min_height, output.min_height);
        assert_eq!(input.max_height, output.max_height);
        assert_eq!(input.map_hardness, output.map_hardness);
        assert_eq!(input.gravity, output.gravity);
        assert_eq!(input.tidal_strength, output.tidal_strength);
        assert_eq!(input.max_metal, output.max_metal);
        assert_eq!(input.extractor_radius, output.extractor_radius);
        assert_eq!(input.deformable, output.deformable);
        assert_eq!(input.void_water, output.void_water);
        assert_eq!(input.void_ground, output.void_ground);
    }

    #[test]
    fn onyx_like_grass_config_emits_under_custom() {
        // Onyx Cauldron's source mapinfo carries:
        //
        //     custom = {
        //         grassConfig = {
        //             grassDistTGA      = "...",
        //             grassBladeColorTex = "...",
        //             grassMaxSize       = 2.0,
        //             grassShaderParams  = {
        //                 MAPCOLORFACTOR = 0.2,
        //                 MAPCOLORBASE   = 0.6,
        //             },
        //         },
        //     }
        //
        // The widget needs the `custom.grassConfig` block at runtime
        // -- without it, BAR's `map_grass_gl4` LuaUI widget reads no
        // distribution mask and renders zero blades, even when the
        // mask asset is otherwise present in the archive. This test
        // pins the emit shape so a regression that drops the wrapper
        // (or the grassConfig sub-block) fails immediately rather
        // than as a "no grass in-game" surprise.
        use crate::recipe::CustomGrassSettings;
        let s = MapSettings {
            custom_grass: CustomGrassSettings {
                dist_tga: Some("maps/Onyx Cauldron 2.0_grassDist.tga".to_string()),
                blade_color_tex: Some("maps/grass_field_mixed.dds.cached.dds".to_string()),
                max_size: Some(2.0),
                map_color_factor: Some(0.2),
                map_color_base: Some(0.6),
                ..Default::default()
            },
            ..MapSettings::default()
        };
        let codec = SpringSmfCodec;
        let lua = codec.generate_mapinfo("onyx", 32, 32, &make_plan(s));

        assert!(
            lua.contains("custom = {"),
            "Onyx-like grass settings must produce a `custom = {{ ... }}` wrapper.\nGot:\n{lua}"
        );
        assert!(
            lua.contains("grassConfig"),
            "grassConfig sub-block missing.\nGot:\n{lua}"
        );
        assert!(
            lua.contains("grassDistTGA"),
            "grassDistTGA missing inside grassConfig.\nGot:\n{lua}"
        );
        assert!(
            lua.contains("grassBladeColorTex"),
            "grassBladeColorTex missing inside grassConfig.\nGot:\n{lua}"
        );
        assert!(
            lua.contains("grassShaderParams"),
            "grassShaderParams sub-block missing.\nGot:\n{lua}"
        );
        assert!(
            lua.contains("MAPCOLORFACTOR"),
            "MAPCOLORFACTOR shader param missing.\nGot:\n{lua}"
        );

        // Round-trip via the parser confirms structure is correct
        // enough that the parsing-side recovers every value.
        let mut parsed = MapSettings::default();
        bar_project::apply_mapinfo_overrides(&lua, &mut parsed);
        assert_eq!(
            parsed.custom_grass.dist_tga.as_deref(),
            Some("maps/Onyx Cauldron 2.0_grassDist.tga")
        );
        assert_eq!(
            parsed.custom_grass.blade_color_tex.as_deref(),
            Some("maps/grass_field_mixed.dds.cached.dds")
        );
        assert_eq!(parsed.custom_grass.max_size, Some(2.0));
        assert_eq!(parsed.custom_grass.map_color_factor, Some(0.2));
        assert_eq!(parsed.custom_grass.map_color_base, Some(0.6));
    }

    #[test]
    fn default_recipe_round_trips_to_all_none() {
        // Companion to the previous: if the emitter never synthesises
        // a value, the parser never picks one up, and the round-trip
        // of `MapSettings::default()` is itself a `MapSettings::default()`
        // (modulo the floor fields the parser doesn't touch). A regression
        // where some field's emitter synthesises a default would show
        // up as a `Some(_)` in `output` where `input` had `None`.
        let input = MapSettings::default();
        let output = roundtrip(input.clone());

        assert_eq!(output.atmosphere, input.atmosphere);
        assert_eq!(output.lighting, input.lighting);
        assert_eq!(output.water, input.water);
        assert_eq!(output.custom_grass, input.custom_grass);
        assert_eq!(output.gravity, None);
        assert_eq!(output.tidal_strength, None);
        assert_eq!(output.max_metal, None);
        assert_eq!(output.extractor_radius, None);
        assert_eq!(output.map_hardness, None);
        assert_eq!(output.void_water, None);
        assert_eq!(output.void_ground, None);
        assert_eq!(output.deformable, None);
    }
}
