//! Export orchestration: evaluate a graph and write output files.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};

use bar_data::{compress_image_dxt1, write_smt, ColorBuffer, Heightmap, SmfMap};
use bar_graph::{
    evaluate_graph, get_grassmap_output, get_heightmap_output, get_metalmap_output,
    get_normalmap_output, get_texture_output, get_typemap_output, GraphEngine, NodeExecutor,
};

use crate::recipe::MapSettings;

/// Export the graph output as an SMF (Spring Map Format) file.
/// Now collects heightmap, metalmap, and typemap from the graph.
pub fn export_smf(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    let heightmap = get_heightmap_output(graph, &results)
        .context("No heightmap output node found — add a Heightmap Output node to the graph")?;

    let metalmap = get_metalmap_output(graph, &results);
    let typemap = get_typemap_output(graph, &results);

    write_smf_full(
        &heightmap,
        metalmap.as_ref(),
        typemap.as_ref(),
        width,
        height,
        path,
    )?;

    tracing::debug!("Exported SMF to {}", path.display());
    Ok(())
}

/// Write a heightmap (with optional metalmap/typemap) as an SMF file.
pub fn write_smf_full(
    heightmap: &Heightmap,
    metalmap: Option<&Heightmap>,
    typemap: Option<&Heightmap>,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    // In Spring SMF, map_x/map_y = number of heightmap samples - 1
    let map_x = (width - 1) as i32;
    let map_y = (height - 1) as i32;

    let mut smf = SmfMap::new(map_x, map_y).context("Failed to create SMF map")?;
    smf.heightmap = heightmap.clone();

    let (mm_w, mm_h) = smf.header.metalmap_size();
    let mm_size = (mm_w as usize) * (mm_h as usize);

    // Write metalmap if available (resample to metalmap resolution)
    if let Some(metal) = metalmap {
        smf.metalmap = resample_to_u8(metal, mm_w, mm_h);
    }

    // Write typemap if available
    if let Some(tmap) = typemap {
        let resampled = resample_to_u8(tmap, mm_w, mm_h);
        smf.typemap = resampled;
    }

    // Ensure correct sizes
    smf.metalmap.resize(mm_size, 0);
    smf.typemap.resize(mm_size, 0);

    let file =
        File::create(path).with_context(|| format!("Failed to create file: {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    smf.write(&mut writer)
        .with_context(|| format!("Failed to write SMF: {}", path.display()))?;

    Ok(())
}

/// Export the graph's heightmap output as a 16-bit grayscale PNG.
pub fn export_heightmap_png(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    let heightmap = get_heightmap_output(graph, &results)
        .context("No heightmap output node found — add a Heightmap Output node to the graph")?;

    write_heightmap_png(&heightmap, path)?;

    tracing::debug!("Exported heightmap PNG to {}", path.display());
    Ok(())
}

/// Write a heightmap as a 16-bit grayscale PNG.
pub fn write_heightmap_png(heightmap: &Heightmap, path: &Path) -> Result<()> {
    let (width, height) = (heightmap.width(), heightmap.height());
    let data = heightmap.data();

    let pixels: Vec<u16> = data
        .iter()
        .map(|&v| (v.clamp(0.0, 1.0) * 65535.0) as u16)
        .collect();

    let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(width, height, pixels)
        .context("Failed to create image buffer")?;

    img.save(path)
        .with_context(|| format!("Failed to save PNG: {}", path.display()))?;

    Ok(())
}

/// Export the graph's color texture as an SMT (Spring Map Tiles) file.
pub fn export_smt(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<(Vec<i32>, u32)> {
    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    let texture = get_texture_output(graph, &results)
        .context("No texture output found — add an Auto Texture → Bundler chain")?;

    let file =
        File::create(path).with_context(|| format!("Failed to create file: {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    let result = write_smt(&mut writer, &texture)
        .with_context(|| format!("Failed to write SMT: {}", path.display()))?;

    tracing::debug!("Exported SMT to {}", path.display());
    Ok(result)
}

/// Export the color texture as an RGBA PNG (diffuse preview).
pub fn export_texture_png(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    let texture = get_texture_output(graph, &results)
        .context("No texture output found — add an Auto Texture → Bundler chain")?;

    write_color_png(&texture, path)?;

    tracing::debug!("Exported texture PNG to {}", path.display());
    Ok(())
}

/// Export the normal map as an RGB PNG.
pub fn export_normalmap_png(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    let normalmap = get_normalmap_output(graph, &results).context(
        "No normal map output — connect a NormalMap node to the Bundler's normalmap port",
    )?;

    write_color_png(&normalmap, path)?;

    tracing::debug!("Exported normal map PNG to {}", path.display());
    Ok(())
}

/// Export the grass map as a grayscale PNG.
pub fn export_grassmap_png(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<()> {
    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    let grassmap = get_grassmap_output(graph, &results)
        .context("No grass map output — connect a GrassMap node to the Bundler's grassmap port")?;

    write_heightmap_png(&grassmap, path)?;

    tracing::debug!("Exported grass map PNG to {}", path.display());
    Ok(())
}

/// Write a ColorBuffer as an RGBA PNG.
pub fn write_color_png(color: &ColorBuffer, path: &Path) -> Result<()> {
    let rgba8 = color.to_rgba8();
    let img = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(
        color.width(),
        color.height(),
        rgba8,
    )
    .context("Failed to create RGBA image buffer")?;

    img.save(path)
        .with_context(|| format!("Failed to save PNG: {}", path.display()))?;
    Ok(())
}

/// Generate a 1024×1024 DXT1 minimap from a color texture.
pub fn generate_minimap_dxt1(texture: &ColorBuffer) -> Vec<u8> {
    let minimap = texture.resize(1024, 1024);
    let rgba8 = minimap.to_rgba8();
    compress_image_dxt1(&rgba8, 1024, 1024)
}

/// Full SD7 export: writes .smf + .smt + all available layers + mapinfo.lua into a directory.
/// Includes normal map, grass map, and specular data if those output nodes exist.
pub fn export_sd7_directory(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    width: u32,
    height: u32,
    output_dir: &Path,
    map_name: &str,
    settings: &MapSettings,
) -> Result<()> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "Failed to create output directory: {}",
            output_dir.display()
        )
    })?;

    // BAR expects SMF/SMT inside maps/ subdirectory
    let maps_dir = output_dir.join("maps");
    fs::create_dir_all(&maps_dir)?;

    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    let heightmap = get_heightmap_output(graph, &results)
        .context("No heightmap output — add a Heightmap Output node")?;

    let metalmap = get_metalmap_output(graph, &results);
    let typemap = get_typemap_output(graph, &results);
    let texture = get_texture_output(graph, &results);
    let normalmap = get_normalmap_output(graph, &results);
    let grassmap = get_grassmap_output(graph, &results);

    // Write SMF into maps/ subdirectory
    let smf_path = maps_dir.join(format!("{}.smf", map_name));
    write_smf_full(
        &heightmap,
        metalmap.as_ref(),
        typemap.as_ref(),
        width,
        height,
        &smf_path,
    )?;
    tracing::debug!("Wrote SMF: {}", smf_path.display());

    // Write SMT into maps/ subdirectory (if we have a texture)
    if let Some(ref tex) = texture {
        let smt_path = maps_dir.join(format!("{}.smt", map_name));
        let file = File::create(&smt_path)?;
        let mut writer = BufWriter::new(file);
        write_smt(&mut writer, tex)?;
        tracing::debug!("Wrote SMT: {}", smt_path.display());

        // Write diffuse preview PNG (at root for debugging)
        let tex_png_path = output_dir.join(format!("{}_diffuse.png", map_name));
        write_color_png(tex, &tex_png_path)?;
        tracing::debug!("Wrote diffuse preview: {}", tex_png_path.display());
    }

    // Write heightmap PNG (at root for debugging)
    let hm_png_path = output_dir.join(format!("{}_heightmap.png", map_name));
    write_heightmap_png(&heightmap, &hm_png_path)?;

    // Write normal map (if present)
    if let Some(ref nmap) = normalmap {
        let nm_path = output_dir.join(format!("{}_normals.png", map_name));
        write_color_png(nmap, &nm_path)?;
        tracing::debug!("Wrote normal map: {}", nm_path.display());
    }

    // Write grass map (if present)
    if let Some(ref gmap) = grassmap {
        let gm_path = output_dir.join(format!("{}_grass.png", map_name));
        write_heightmap_png(gmap, &gm_path)?;
        tracing::debug!("Wrote grass map: {}", gm_path.display());
    }

    // Write mapinfo.lua at root of archive
    let map_x = width - 1;
    let map_y = height - 1;
    let mapinfo = generate_mapinfo(map_name, map_x, map_y, settings);
    let mapinfo_path = output_dir.join("mapinfo.lua");
    fs::write(&mapinfo_path, mapinfo)?;
    tracing::debug!("Wrote mapinfo: {}", mapinfo_path.display());

    tracing::info!("SD7 directory export complete: {}", output_dir.display());
    Ok(())
}

/// Generate a mapinfo.lua using configurable settings. Legacy
/// export-path emitter (the codec at `targets/spring_smf.rs` is the
/// modern path that emits only user-explicit fields). This one
/// resolves Option overrides up front for compatibility with the
/// older formatting block.
fn generate_mapinfo(name: &str, map_x: u32, map_y: u32, settings: &MapSettings) -> String {
    let rs = settings.resolved();
    let deformable_str = if rs.deformable { "false" } else { "true" };
    let atm = &rs.atmosphere;
    let lit = &rs.lighting;
    let wat = &rs.water;

    // World-space dimensions (elmos) = squares × squareSize
    let world_x = map_x * 8;
    let world_y = map_y * 8;

    // Team start positions (in world units / elmos)
    let teams = if settings.start_positions.is_empty() {
        format!(
            "    teams = {{\n        [0] = {{ startPos = {{ x = {}, z = {} }} }},\n        [1] = {{ startPos = {{ x = {}, z = {} }} }},\n    }}",
            world_x / 4, world_y / 4, world_x * 3 / 4, world_y * 3 / 4
        )
    } else {
        let mut s = "    teams = {\n".to_string();
        for (i, pos) in settings.start_positions.iter().enumerate() {
            s.push_str(&format!(
                "        [{}] = {{ startPos = {{ x = {}, z = {} }} }},\n",
                i, pos[0], pos[1]
            ));
        }
        s.push_str("    }");
        s
    };

    // DNTS / detail texture section
    let detail_section = if settings.detail_textures.is_empty() {
        "resources = {\n        detailTex = \"\",\n    }".to_string()
    } else {
        let mut s = "resources = {\n        detailTex = \"\",\n    },\n\n    custom = {\n        dnts = {\n".to_string();
        for (i, dt) in settings.detail_textures.iter().enumerate() {
            s.push_str(&format!(
                "            [{}] = {{ file = \"{}\", scale = {} }},\n",
                i, dt.path, dt.scale
            ));
        }
        s.push_str("        },\n    }");
        s
    };

    // Splat texture scales from detail textures
    let splat_scales = if settings.detail_textures.is_empty() {
        "0.02, 0.02, 0.02, 0.02".to_string()
    } else {
        settings
            .detail_textures
            .iter()
            .take(4)
            .map(|dt| format!("{}", dt.scale))
            .chain(std::iter::repeat("0.02".to_string()))
            .take(4)
            .collect::<Vec<_>>()
            .join(", ")
    };

    format!(
        r#"local mapinfo = {{
    name        = "{name}",
    shortname   = "{name}",
    description = "Generated by BAR - Map Editor",
    author      = "BAR - Map Editor",
    version     = "1.0",
    mapfile     = "maps/{name}.smf",
    modtype     = 3,

    maphardness    = {hardness},
    notDeformable  = {deformable},
    gravity        = {gravity},
    tidalStrength  = {tidal},
    maxMetal       = {max_metal},
    extractorRadius = {extractor_radius},
    voidWater      = false,
    voidGround     = false,

    smf = {{
        minheight = {min_height},
        maxheight = {max_height},
        smtFileName0 = "maps/{name}.smt",
    }},

    {detail_section},

    splats = {{
        texScales = {{ {splat_scales} }},
        texMults  = {{ 1.0, 1.0, 1.0, 1.0 }},
    }},

    atmosphere = {{
        minWind    = {atm_min_wind},
        maxWind    = {atm_max_wind},
        fogStart   = {atm_fog_start},
        fogEnd     = {atm_fog_end},
        fogColor   = {{ {fog_r}, {fog_g}, {fog_b} }},
    }},

    lighting = {{
        sunDir              = {{ {sun_x}, {sun_y}, {sun_z} }},
        groundAmbientColor  = {{ {amb_r}, {amb_g}, {amb_b} }},
        groundDiffuseColor  = {{ {dif_r}, {dif_g}, {dif_b} }},
        groundSpecularColor = {{ {spc_r}, {spc_g}, {spc_b} }},
        specularExponent    = {spec_exp},
    }},

    water = {{
        damage    = {water_damage},
        absorb    = {{ {abs_r}, {abs_g}, {abs_b} }},
        baseColor = {{ {wbc_r}, {wbc_g}, {wbc_b} }},
        minColor  = {{ {wmc_r}, {wmc_g}, {wmc_b} }},
    }},

{teams},
}}

return mapinfo
"#,
        hardness = rs.map_hardness,
        deformable = deformable_str,
        gravity = rs.gravity,
        tidal = rs.tidal_strength,
        max_metal = rs.max_metal,
        extractor_radius = rs.extractor_radius,
        min_height = rs.min_height,
        max_height = rs.max_height,
        atm_min_wind = atm.min_wind,
        atm_max_wind = atm.max_wind,
        atm_fog_start = atm.fog_start,
        atm_fog_end = atm.fog_end,
        fog_r = atm.fog_color[0],
        fog_g = atm.fog_color[1],
        fog_b = atm.fog_color[2],
        sun_x = lit.sun_dir[0],
        sun_y = lit.sun_dir[1],
        sun_z = lit.sun_dir[2],
        amb_r = lit.ground_ambient[0],
        amb_g = lit.ground_ambient[1],
        amb_b = lit.ground_ambient[2],
        dif_r = lit.ground_diffuse[0],
        dif_g = lit.ground_diffuse[1],
        dif_b = lit.ground_diffuse[2],
        spc_r = lit.ground_specular[0],
        spc_g = lit.ground_specular[1],
        spc_b = lit.ground_specular[2],
        spec_exp = lit.spec_exponent,
        // Water mode forces damage to zero -- the BAR engine reads
        // any positive `mapinfo.water.damage` as lava, so storing
        // a stale lava value while the user is in water mode would
        // turn the exported map into lava on Test-in-BAR. Lava mode
        // emits the stored value as-is.
        water_damage = if wat.is_lava { wat.damage } else { 0.0 },
        // (`wat.is_lava` is a bool here, sourced from
        // ResolvedWater, which defaults to false when the recipe
        // hasn't expressed a preference. The
        // forced-zero-in-water-mode rule still applies because
        // this template emits `damage` unconditionally; the
        // structured emitter in `targets/spring_smf.rs` is the
        // path where unset damage stays unset.)
        abs_r = wat.absorb[0],
        abs_g = wat.absorb[1],
        abs_b = wat.absorb[2],
        wbc_r = wat.base_color[0],
        wbc_g = wat.base_color[1],
        wbc_b = wat.base_color[2],
        wmc_r = wat.min_color[0],
        wmc_g = wat.min_color[1],
        wmc_b = wat.min_color[2],
    )
}

/// Export using the codec-based target system.
///
/// This evaluates the graph, builds a LayerSet, validates against the target,
/// and calls the codec to write all output files.
///
/// `target_id_or_path` can be either a built-in target ID (e.g., "spring-smf")
/// or a path to a custom target TOML file.
pub fn export_with_target(
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    recipe: &crate::recipe::Recipe,
    output_dir: &Path,
    map_name: &str,
    target_id_or_path: &str,
) -> Result<crate::targets::WrittenFiles> {
    let width = recipe.output.width;
    let height = recipe.output.height;
    let settings = &recipe.output.map_settings;
    use crate::targets::{load_target_config, ExportPlan, LayerSet, Severity, TargetRegistry};

    let registry = TargetRegistry::new();

    // Try as built-in target ID first, then as a file path
    let config = if let Some(builtin) = registry.get_target(target_id_or_path) {
        builtin.clone()
    } else {
        let path = Path::new(target_id_or_path);
        if path.exists() {
            load_target_config(path)?
        } else {
            anyhow::bail!(
                "Unknown target '{}' — not a built-in ID or a valid file path",
                target_id_or_path
            );
        }
    };

    let codec = registry
        .get_codec(&config.codec)
        .ok_or_else(|| anyhow::anyhow!("Unknown codec: {}", config.codec))?;

    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "Failed to create output directory: {}",
            output_dir.display()
        )
    })?;

    // Evaluate graph
    let results = evaluate_graph(
        graph,
        executor,
        width,
        height,
        (width - 1) * 8,
        (height - 1) * 8,
    )
    .context("Failed to evaluate graph")?;

    // Build layer set
    let layers = LayerSet {
        heightmap: get_heightmap_output(graph, &results),
        metalmap: get_metalmap_output(graph, &results),
        typemap: get_typemap_output(graph, &results),
        texture: get_texture_output(graph, &results),
        normalmap: get_normalmap_output(graph, &results),
        grassmap: get_grassmap_output(graph, &results),
        specular: None,
    };

    // Compute dimensions
    let dims = codec.compute_dimensions(&config, width, height);

    // Create export plan
    let display_name = {
        let trimmed = recipe.name.trim();
        if trimmed.is_empty() {
            map_name.to_string()
        } else {
            trimmed.to_string()
        }
    };
    let plan = ExportPlan {
        map_name: map_name.to_string(),
        display_name,
        shortname: recipe.shortname.clone(),
        description: recipe.description.clone(),
        author: recipe.author.clone(),
        version: recipe.version.clone(),
        tip: recipe.tip.clone(),
        depend: recipe.depend.clone(),
        dimensions: dims,
        settings: settings.clone(),
        features: recipe.features.clone(),
        project_dir: None,
    };

    // Validate
    let errors = codec.validate(&config, &plan, &layers)?;
    let has_errors = errors.iter().any(|e| e.severity == Severity::Error);
    for err in &errors {
        match err.severity {
            Severity::Error => tracing::error!("{}", err),
            Severity::Warning => tracing::warn!("{}", err),
        }
    }
    if has_errors {
        anyhow::bail!("Export validation failed — fix errors above before exporting");
    }

    // Write via codec
    let written = codec.write(&config, &plan, &layers, output_dir)?;

    // Also write debug PNGs (heightmap preview)
    if let Some(ref hm) = layers.heightmap {
        let hm_png_path = output_dir.join(format!("{}_heightmap.png", map_name));
        write_heightmap_png(hm, &hm_png_path)?;
    }
    if let Some(ref tex) = layers.texture {
        let tex_png_path = output_dir.join(format!("{}_diffuse.png", map_name));
        write_color_png(tex, &tex_png_path)?;
    }

    tracing::info!(
        "Export complete via target '{}': {} files written",
        target_id_or_path,
        written.files.len()
    );
    Ok(written)
}

/// Resample a heightmap to target dimensions and convert to u8 [0-255].
pub(crate) fn resample_to_u8(hm: &Heightmap, target_w: u32, target_h: u32) -> Vec<u8> {
    let src_w = hm.width();
    let src_h = hm.height();
    let mut out = vec![0u8; (target_w as usize) * (target_h as usize)];

    for y in 0..target_h {
        for x in 0..target_w {
            let sx = (x as f32 * src_w as f32 / target_w as f32) as u32;
            let sy = (y as f32 * src_h as f32 / target_h as f32) as u32;
            let sx = sx.min(src_w - 1);
            let sy = sy.min(src_h - 1);
            let v = hm.get(sx, sy).unwrap_or(0.0);
            out[(y as usize) * (target_w as usize) + (x as usize)] =
                (v.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::Recipe;
    use crate::CpuExecutor;

    #[test]
    fn test_export_smf_from_recipe() {
        let recipe = Recipe::sample();
        let graph = recipe.build_graph().unwrap();
        let executor = CpuExecutor;

        let dir = std::env::temp_dir();
        let path = dir.join("om_test_export.smf");

        export_smf(&graph, &executor, 65, 65, &path).unwrap();

        // Verify file reads back correctly
        let mut file = File::open(&path).unwrap();
        let map = SmfMap::read(&mut file).unwrap();
        assert_eq!(map.header.heightmap_size(), (65, 65));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_export_png_from_recipe() {
        let recipe = Recipe::sample();
        let graph = recipe.build_graph().unwrap();
        let executor = CpuExecutor;

        let dir = std::env::temp_dir();
        let path = dir.join("om_test_export.png");

        export_heightmap_png(&graph, &executor, 64, 64, &path).unwrap();

        let img = image::open(&path).unwrap();
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 64);

        std::fs::remove_file(&path).ok();
    }
}
