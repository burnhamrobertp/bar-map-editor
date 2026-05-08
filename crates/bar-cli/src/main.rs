//! BAR - Map Editor CLI — headless graph evaluation and map export.
//!
//! Processes recipe JSON files through the engine without requiring a GUI,
//! enabling scripted pipelines, CI testing, and batch map generation.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use bar_engine::recipe::{Recipe, RECIPE_SCHEMA_VERSION};
use bar_engine::CpuExecutor;

mod macro_preview;

#[derive(Parser)]
#[command(
    name = "bar-cli",
    about = "BAR - Map Editor CLI -- headless terrain generation",
    version,
    long_about = "Process BAR - Map Editor recipe files to generate terrain maps.\n\n\
                  Recipes define a node graph (noise generators, filters, combiners)\n\
                  that evaluates to produce heightmaps and map files."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate a recipe and export output files.
    Run {
        /// Path to the recipe JSON file.
        recipe: PathBuf,

        /// Output directory (defaults to current directory).
        #[arg(short, long, default_value = ".")]
        output: PathBuf,

        /// Override output width (pixels).
        #[arg(long)]
        width: Option<u32>,

        /// Override output height (pixels).
        #[arg(long)]
        height: Option<u32>,

        /// Export target ID (e.g., "spring-smf"). Uses codec-based export.
        /// This is a shortcut that creates a temporary bundler at runtime.
        #[arg(long)]
        target: Option<String>,

        /// Export a specific bundler node by label. If omitted and the graph
        /// contains bundler nodes, all bundlers are executed.
        #[arg(long)]
        bundler: Option<String>,
    },

    /// Validate a recipe file without executing it.
    Validate {
        /// Path to the recipe JSON file.
        recipe: PathBuf,
    },

    /// Print information about a recipe (nodes, connections, eval order).
    Info {
        /// Path to the recipe JSON file.
        recipe: PathBuf,
    },

    /// Generate a sample recipe and print it to stdout.
    New {
        /// Output file to write (prints to stdout if omitted).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// List available export targets.
    Targets,

    /// Import a .sd7 archive as a BAR - Map Editor project.
    Import {
        /// Path to the .sd7 archive to import.
        sd7: PathBuf,

        /// Output directory for the project files (defaults to a subdirectory
        /// named after the map, next to the .sd7 file).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Render a 3D preview of a project / SD7 to a PNG. Headless: uses a
    /// standalone wgpu device, no window. Mirrors the GUI's preview output
    /// so visual changes can be tested without launching the editor.
    Preview {
        /// Path to a project (.barproj) or map archive (.sd7). For SD7 we
        /// import internally to a temp directory and render the result.
        input: PathBuf,

        /// Output PNG path.
        #[arg(short, long, default_value = "preview.png")]
        output: PathBuf,

        /// Output image width in pixels.
        #[arg(long, default_value_t = 1024)]
        width: u32,

        /// Output image height in pixels.
        #[arg(long, default_value_t = 768)]
        height: u32,

        /// Camera azimuth in degrees (rotation around Y axis).
        #[arg(long, default_value_t = 45.0)]
        azimuth: f32,

        /// Camera elevation in degrees (above horizontal).
        #[arg(long, default_value_t = 22.5)]
        elevation: f32,

        /// Camera distance from the mesh origin.
        #[arg(long, default_value_t = 1.6)]
        distance: f32,

        /// Mesh LOD cap (max grid size). Smaller = chunkier preview. The
        /// GUI uses ~96 for the low-res pass and viewport-derived (192–512)
        /// for the high-res pass; pass `--mesh-lod 96` to mimic low-res.
        #[arg(long, default_value_t = 384)]
        mesh_lod: u32,
    },

    /// Render a macro to a heightmap PNG for value-iteration. Takes a
    /// macro name (e.g. `mountain-range`) or a path to a macro JSON,
    /// optional knob overrides via `--knob name=value`, and writes
    /// `heightmap.png` (grayscale) to the output directory.
    PreviewMacro {
        /// Macro name (e.g. `plains`) or path to a macro JSON file.
        macro_arg: String,

        /// Override a macro_param value. Repeat: `--knob ridge_density=3.0
        /// --knob smoothing=2.5`. Names must match `macro_params[].name`
        /// in the macro JSON; values are parsed against the param's `kind`.
        #[arg(long = "knob")]
        knobs: Vec<String>,

        /// Override the random seed for every UInt-typed `seed` param
        /// across the macro's inner nodes. Without this, the macro
        /// keeps its baked-in seeds — useful for reproducible runs.
        #[arg(long)]
        seed: Option<u32>,

        /// Heightmap width.
        #[arg(long, default_value_t = 512)]
        width: u32,

        /// Heightmap height.
        #[arg(long, default_value_t = 512)]
        height: u32,

        /// Output directory.
        #[arg(short, long, default_value = "macro-preview")]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            recipe,
            output,
            width,
            height,
            target,
            bundler,
        } => cmd_run(
            &recipe,
            &output,
            width,
            height,
            target.as_deref(),
            bundler.as_deref(),
        ),
        Commands::Validate { recipe } => cmd_validate(&recipe),
        Commands::Info { recipe } => cmd_info(&recipe),
        Commands::New { output } => cmd_new(output.as_deref()),
        Commands::Targets => cmd_targets(),
        Commands::Import { sd7, output } => cmd_import(&sd7, output.as_deref()),
        Commands::Preview {
            input,
            output,
            width,
            height,
            azimuth,
            elevation,
            distance,
            mesh_lod,
        } => cmd_preview(
            &input, &output, width, height, azimuth, elevation, distance, mesh_lod,
        ),
        Commands::PreviewMacro {
            macro_arg,
            knobs,
            seed,
            width,
            height,
            out,
        } => macro_preview::run(&macro_arg, &knobs, seed, width, height, &out),
    }
}

fn cmd_run(
    recipe_path: &Path,
    output_dir: &Path,
    width: Option<u32>,
    height: Option<u32>,
    target: Option<&str>,
    bundler_filter: Option<&str>,
) -> Result<()> {
    let recipe = Recipe::load(recipe_path)
        .with_context(|| format!("Failed to load recipe: {}", recipe_path.display()))?;

    let w = width.unwrap_or(recipe.output.width);
    let h = height.unwrap_or(recipe.output.height);

    tracing::info!(
        "Running recipe '{}' ({}x{}) from {}",
        recipe.name,
        w,
        h,
        recipe_path.display()
    );

    // Build graph from recipe
    let graph = recipe
        .build_graph()
        .context("Failed to build graph from recipe")?;

    // Ensure output directory exists
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Cannot create output directory: {}", output_dir.display()))?;

    let executor = CpuExecutor;
    let start = Instant::now();

    // If --target is specified, use the codec-based export path (backward compat shortcut)
    if let Some(target_id) = target {
        let written = bar_engine::export_with_target(
            &graph,
            &executor,
            &recipe,
            output_dir,
            &sanitize_filename(&recipe.name),
            target_id,
        )
        .with_context(|| format!("Export with target '{}' failed", target_id))?;

        let elapsed = start.elapsed();
        println!(
            "  ✓ Target '{}': {} files written",
            target_id,
            written.files.len()
        );
        for f in &written.files {
            println!("    - {}", f);
        }
        println!(
            "\nDone in {:.2}s — exported to {}",
            elapsed.as_secs_f64(),
            output_dir.display()
        );
        return Ok(());
    }

    // Check if graph has bundler nodes
    let bundler_nodes = bar_engine::find_bundler_nodes(&graph);
    if !bundler_nodes.is_empty() || bundler_filter.is_some() {
        // Evaluate graph first
        let results = bar_graph::evaluate_graph(&graph, &executor, w, h)
            .context("Failed to evaluate graph")?;

        // Execute bundlers
        let bundler_results = bar_engine::execute_bundlers(
            &graph,
            &results,
            &recipe,
            output_dir,
            bundler_filter,
            None,
        )
        .context("Bundler execution failed")?;

        let elapsed = start.elapsed();
        if bundler_results.is_empty() {
            if let Some(label) = bundler_filter {
                anyhow::bail!("No bundler node found with label '{}'", label);
            }
        }
        for result in &bundler_results {
            println!(
                "  ✓ Bundler '{}': {} files → {}",
                result.label,
                result.files_written,
                result.output_path.display()
            );
        }
        println!(
            "\nDone in {:.2}s — {} bundler(s) exported",
            elapsed.as_secs_f64(),
            bundler_results.len()
        );
        return Ok(());
    }

    // No --target and no bundler nodes — tell the user what to do
    anyhow::bail!(
        "No export method specified. Use --target <ID> (e.g., --target spring-smf) \
         or add Bundler nodes to your recipe graph.\n\
         Run 'om targets' to list available export targets."
    );
}

fn cmd_validate(recipe_path: &Path) -> Result<()> {
    let recipe = Recipe::load(recipe_path)
        .with_context(|| format!("Failed to load recipe: {}", recipe_path.display()))?;

    // Also verify graph builds successfully
    let graph = recipe.build_graph().context("Graph construction failed")?;
    let order = graph
        .topological_sort()
        .context("Topological sort failed")?;

    println!("✓ Recipe '{}' is valid", recipe.name);
    println!("  Nodes:       {}", recipe.nodes.len());
    println!("  Connections: {}", recipe.connections.len());
    println!("  Eval order:  {} steps", order.len());
    println!(
        "  Output:      {}x{}",
        recipe.output.width, recipe.output.height
    );

    Ok(())
}

fn cmd_info(recipe_path: &Path) -> Result<()> {
    let recipe = Recipe::load(recipe_path)
        .with_context(|| format!("Failed to load recipe: {}", recipe_path.display()))?;

    println!("Recipe: {}", recipe.name);
    if !recipe.description.is_empty() {
        println!("  {}", recipe.description);
    }
    println!();

    println!("Nodes ({}):", recipe.nodes.len());
    for node in &recipe.nodes {
        let label = if node.label.is_empty() {
            &node.key
        } else {
            &node.label
        };
        println!("  [{}] {} ({:?})", node.key, label, node.node_type);
        if !node.params.is_empty() {
            for (k, v) in &node.params {
                println!("    {} = {:?}", k, v);
            }
        }
    }
    println!();

    println!("Connections ({}):", recipe.connections.len());
    for conn in &recipe.connections {
        println!("  {} → {}", conn.from, conn.to);
    }
    println!();

    // Build graph and show eval order
    let graph = recipe.build_graph().context("Graph construction failed")?;
    let order = graph
        .topological_sort()
        .context("Topological sort failed")?;
    println!("Evaluation order:");
    for (i, node_id) in order.iter().enumerate() {
        let node = graph.get_node(*node_id).unwrap();
        println!("  {}. {} ({:?})", i + 1, node.label, node.node_type);
    }
    println!();

    println!("Output: {}x{}", recipe.output.width, recipe.output.height);

    Ok(())
}

fn cmd_new(output: Option<&std::path::Path>) -> Result<()> {
    let recipe = Recipe::sample();
    let json = recipe
        .to_json()
        .context("Failed to serialize sample recipe")?;

    if let Some(path) = output {
        std::fs::write(path, &json).with_context(|| format!("Cannot write {}", path.display()))?;
        println!("Sample recipe written to {}", path.display());
    } else {
        println!("{json}");
    }

    Ok(())
}

/// Sanitize a string for use as a filename.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

fn cmd_import(sd7_path: &Path, output_dir: Option<&Path>) -> Result<()> {
    tracing::info!("Importing {}", sd7_path.display());

    // Default output dir: <sd7_dir>/<sd7_stem>/
    let default_out = {
        let parent = sd7_path.parent().unwrap_or_else(|| Path::new("."));
        let stem = sd7_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported".to_string());
        parent.join(stem)
    };
    let out_dir = output_dir.unwrap_or(&default_out);

    let start = Instant::now();
    let project = bar_engine::import_sd7_to_project(sd7_path, out_dir)
        .with_context(|| format!("Failed to import {}", sd7_path.display()))?;

    let project_name = sanitize_filename(&project.recipe.name);
    let project_path = out_dir.join(format!("{project_name}.barproj"));
    project
        .save(&project_path)
        .with_context(|| format!("Failed to save project to {}", project_path.display()))?;

    let elapsed = start.elapsed();
    println!(
        "✓ Imported '{}' in {:.2}s",
        project.recipe.name,
        elapsed.as_secs_f64()
    );
    println!(
        "  Map size:  {}×{}",
        project.recipe.output.width, project.recipe.output.height
    );
    println!(
        "  Height:    {:.0}..{:.0} world units",
        project.recipe.output.map_settings.min_height,
        project.recipe.output.map_settings.max_height
    );
    println!("  Project:   {}", project_path.display());
    println!("  Heightmap: {}", out_dir.join("heightmap.png").display());

    Ok(())
}

fn cmd_targets() -> Result<()> {
    let registry = bar_engine::TargetRegistry::new();

    println!("Available export targets:\n");
    for id in registry.target_ids() {
        let target = registry.get_target(id).unwrap();
        println!("  {} — {}", id, target.name);
        println!("    Codec:   {}", target.codec);
        println!("    Version: {}", target.version);
        println!("    Layers:  {}", target.layers.len());
        println!();
    }

    println!("Use --target <ID> with the 'run' command to export using a target.");
    Ok(())
}

/// Render a 3D preview headlessly and write a PNG. Mirrors the GUI's preview
/// pipeline: same height_scale formula, same TerrainRenderer, same camera
/// math. Lets visual changes be tested without launching the editor.
#[allow(clippy::too_many_arguments)]
fn cmd_preview(
    input_path: &Path,
    output_path: &Path,
    out_w: u32,
    out_h: u32,
    azimuth_deg: f32,
    elevation_deg: f32,
    distance: f32,
    mesh_lod: u32,
) -> Result<()> {
    use anyhow::Context as _;
    use bar_compute::GpuContext;
    use bar_engine::CpuExecutor;
    use bar_graph::{
        evaluate_graph, get_bundler_node_heightmap, get_bundler_node_texture,
        get_preview_heightmap, get_texture_output,
    };
    use bar_render::{Camera, TerrainRenderer};

    // Resolve input — either an .barproj or an .sd7. SD7s get imported to
    // a temp dir first; the produced project is then rendered.
    let project = load_project_for_preview(input_path)
        .with_context(|| format!("Failed to load preview input: {}", input_path.display()))?;

    let project_dir = input_path.parent().map(|p| p.to_path_buf());
    let graph = project
        .recipe
        .build_graph()
        .context("Failed to build graph from project recipe")?;

    let (w, h) = (project.recipe.output.width, project.recipe.output.height);
    let (min_h, max_h) = (
        project.recipe.output.map_settings.min_height,
        project.recipe.output.map_settings.max_height,
    );

    // Resolve any project-relative paths in the recipe so the executor can
    // read the on-disk files. Mirrors what the GUI does at apply_project.
    let mut graph = graph;
    if let Some(ref dir) = project_dir {
        resolve_relative_paths_in_graph(&mut graph, dir);
    }

    // Evaluate via the CPU executor — mirrors GUI's preview path which
    // (in headless mode) uses CpuExecutor too.
    let executor = CpuExecutor;
    let outputs = evaluate_graph(&graph, &executor, w, h)
        .map_err(|e| anyhow::anyhow!("Graph evaluation failed: {e:?}"))?;

    // Find the heightmap and (optionally) the texture wired to the first
    // bundler. Falls back to the topo-last heightmap so non-bundler graphs
    // (e.g. recipes without a Bundler node) still render.
    let bundler_id = bar_engine::find_bundler_nodes(&graph).first().copied();
    let heightmap = bundler_id
        .and_then(|id| get_bundler_node_heightmap(&graph, &outputs, id))
        .or_else(|| get_preview_heightmap(&graph, &outputs))
        .ok_or_else(|| anyhow::anyhow!("No heightmap available in this project"))?;
    let texture = bundler_id
        .and_then(|id| get_bundler_node_texture(&graph, &outputs, id))
        .or_else(|| get_texture_output(&graph, &outputs));

    // Compute the same height_scale / extent / water_y the GUI uses.
    let (height_scale, water_y, x_extent, z_extent) = {
        let pw = (w as f32 - 1.0).max(1.0);
        let ph = (h as f32 - 1.0).max(1.0);
        let pm = pw.max(ph);
        let xe = (0.5 * pw / pm).min(0.5);
        let ze = (0.5 * ph / pm).min(0.5);
        let hr = (max_h - min_h).abs().max(1.0);
        let hs = (hr / (pm * 8.0)).max(0.005);
        let wy = if min_h < 0.0 {
            (-min_h / hr) * hs
        } else {
            -1.0
        };
        (hs, wy, xe, ze)
    };

    // Diagnostic: actual data range vs the SMF header's nominal range. A
    // big gap between the two means the map's heightmap doesn't fill the
    // allocated [min_h, max_h] range — terrain will look flatter than the
    // header suggests.
    let data = heightmap.data();
    let data_min_norm = data.iter().copied().fold(f32::INFINITY, f32::min);
    let data_max_norm = data.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let data_mean_norm: f32 = data.iter().copied().sum::<f32>() / data.len() as f32;
    let header_range = max_h - min_h;
    let elmo_min = data_min_norm * header_range + min_h;
    let elmo_max = data_max_norm * header_range + min_h;
    let elmo_mean = data_mean_norm * header_range + min_h;
    let actual_range = elmo_max - elmo_min;
    let utilisation = if header_range > 0.0 {
        actual_range / header_range
    } else {
        0.0
    };
    let below_water = data
        .iter()
        .filter(|&&v| v * header_range + min_h < 0.0)
        .count();
    let below_water_pct = below_water as f32 * 100.0 / data.len() as f32;

    println!(
        "Preview params: w={w} h={h}\n\
         SMF header range:   [{min_h:.0}, {max_h:.0}] elmos (span {header_range:.0})\n\
         Actual data range:  [{elmo_min:.1}, {elmo_max:.1}] elmos (span {actual_range:.1})\n\
         Mean elevation:     {elmo_mean:.1} elmos\n\
         Range utilisation:  {:.1}% of header span used\n\
         Below sea level:    {below_water_pct:.1}% of pixels\n\
         Render scale:       height_scale={height_scale:.4} x_extent={x_extent:.4} z_extent={z_extent:.4} water_y={water_y:.4}",
        utilisation * 100.0
    );

    // Build the headless GPU context.
    let gpu =
        pollster::block_on(GpuContext::new_standalone()).context("Failed to create wgpu device")?;

    // Set up the renderer at the requested output resolution.
    let mut renderer =
        TerrainRenderer::new(&gpu.device, &gpu.queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    renderer.resize(&gpu.device, out_w, out_h);
    renderer.update_heightmap(
        &gpu.device,
        &gpu.queue,
        &heightmap,
        height_scale,
        x_extent,
        z_extent,
        water_y,
        [0.2, 0.45, 0.75],
        mesh_lod,
    );
    if let Some(ref tex) = texture {
        renderer.update_albedo(&gpu.device, &gpu.queue, tex);
    }
    let frame = bar_render::PreviewFrame {
        height_scale,
        x_extent,
        z_extent,
        water_y,
        water_color: [0.2, 0.45, 0.75],
        // CLI always uses the high-pass (full) shader -- the low-pass is for
        // the GUI's progressive refinement, not relevant headlessly.
        quality_high: true,
        time: 0.0,
        // CLI doesn't read MapSettings.lighting yet -- fall back to engine
        // defaults so the renderer still produces a sensible image.
        smf_lighting: bar_render::SmfLighting::default(),
    };

    // Camera with user-supplied angles.
    let camera = Camera {
        target: glam::Vec3::ZERO,
        distance,
        azimuth: azimuth_deg.to_radians(),
        elevation: elevation_deg.to_radians(),
        fov: std::f32::consts::FRAC_PI_4,
        near: 0.01,
        far: 1000.0,
    };
    renderer.render(&gpu.device, &gpu.queue, &camera, Some(&frame));

    let pixels = renderer
        .read_pixels(&gpu.device, &gpu.queue)
        .ok_or_else(|| anyhow::anyhow!("Renderer produced no output texture"))?;

    let img = image::RgbaImage::from_raw(out_w, out_h, pixels)
        .ok_or_else(|| anyhow::anyhow!("Failed to wrap pixels into image buffer"))?;
    img.save(output_path)
        .with_context(|| format!("Failed to write {}", output_path.display()))?;

    println!("Wrote preview to {}", output_path.display());
    Ok(())
}

/// Load a `.barproj` directly, or build a GUI-equivalent graph from an `.sd7`.
///
/// SD7 path: extract via `extract_sd7_to_work_dir`, then construct a recipe
/// with SmfImport + SmtImport + Bundler nodes wired the same way the GUI's
/// `finish_open_map` builds them. This means the CLI preview matches what
/// the user sees in the editor — same heightmap, same texture, same
/// connections — instead of going through the simplified
/// `import_sd7_to_project` path which only piped a heightmap PNG.
fn load_project_for_preview(path: &Path) -> Result<bar_engine::Project> {
    use anyhow::Context as _;
    use bar_engine::recipe::{MapSettings, OutputConfig, Recipe, RecipeConnection, RecipeNode};
    use bar_engine::{EditorLayout, Project};
    use bar_graph::{NodeType, ParamValue};
    use std::collections::HashMap as Map;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match ext.as_deref() {
        Some("sd7") => {
            let scan = bar_engine::extract_sd7_to_work_dir(path)
                .with_context(|| format!("Failed to extract {}", path.display()))?;

            let mut nodes: Vec<RecipeNode> = Vec::new();
            let mut connections: Vec<RecipeConnection> = Vec::new();

            if let Some(ref smf_abs) = scan.smf_abs {
                let mut params = Map::new();
                params.insert(
                    "path".into(),
                    ParamValue::String(smf_abs.to_string_lossy().into_owned()),
                );
                params.insert("load_metalmap".into(), ParamValue::Bool(true));
                params.insert("load_typemap".into(), ParamValue::Bool(true));
                nodes.push(RecipeNode {
                    key: "smf".into(),
                    node_type: NodeType::SmfImport,
                    label: "SMF Import".into(),
                    params,
                });
                connections.push(RecipeConnection {
                    from: "smf.heightmap".into(),
                    to: "bundler.heightmap".into(),
                });
                connections.push(RecipeConnection {
                    from: "smf.metalmap".into(),
                    to: "bundler.metalmap".into(),
                });
                connections.push(RecipeConnection {
                    from: "smf.typemap".into(),
                    to: "bundler.typemap".into(),
                });
            }

            if let Some(ref smt_abs) = scan.smt_abs {
                let mut params = Map::new();
                params.insert(
                    "path".into(),
                    ParamValue::String(smt_abs.to_string_lossy().into_owned()),
                );
                if let Some(ref smf_abs) = scan.smf_abs {
                    params.insert(
                        "smf_path".into(),
                        ParamValue::String(smf_abs.to_string_lossy().into_owned()),
                    );
                }
                if let Some((tx, ty)) = scan.tile_grid {
                    params.insert("tiles_x".into(), ParamValue::UInt(tx));
                    params.insert("tiles_y".into(), ParamValue::UInt(ty));
                }
                nodes.push(RecipeNode {
                    key: "smt".into(),
                    node_type: NodeType::SmtImport,
                    label: "SMT Import".into(),
                    params,
                });
                connections.push(RecipeConnection {
                    from: "smt.texture".into(),
                    to: "bundler.texture".into(),
                });
            }

            nodes.push(RecipeNode {
                key: "bundler".into(),
                node_type: NodeType::Bundler,
                label: "Bundler".into(),
                params: Map::new(),
            });

            let (min_height, max_height) = scan.height_range.unwrap_or((0.0, 1024.0));
            let (width, height) = scan.map_dims.unwrap_or((257, 257));

            let recipe = Recipe {
                schema_version: RECIPE_SCHEMA_VERSION,
                name: scan.map_name.clone(),
                shortname: None,
                description: format!(
                    "Imported preview: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                author: None,
                version: None,
                nodes,
                connections,
                output: OutputConfig {
                    width,
                    height,
                    map_settings: MapSettings {
                        min_height,
                        max_height,
                        ..MapSettings::default()
                    },
                },
            };

            // width / height already live on `recipe.output`; no
            // duplicate on the layout.
            let _ = (width, height);
            Ok(Project {
                recipe,
                sculpt: Default::default(),
                layout: EditorLayout {
                    node_positions: Map::new(),
                    node_sizes: Map::new(),
                    canvas_offset: (0.0, 0.0),
                    map_info_file: None,
                    groups: Vec::new(),
                    open_tabs: Vec::new(),
                    active_tab: 0,
                },
            })
        }
        _ => bar_engine::Project::load(path)
            .with_context(|| format!("Failed to load {}", path.display())),
    }
}

/// Mirror of the GUI's `bar://` resolver — replaces project-relative paths
/// with absolute paths anchored at the project's directory so executors
/// can read them.
fn resolve_relative_paths_in_graph(graph: &mut bar_graph::GraphEngine, project_dir: &Path) {
    use bar_graph::{NodeType, ParamValue};
    const PROJECT_RELATIVE_PREFIX: &str = "bar://";
    let resolve = |s: &str| -> String {
        if let Some(rest) = s.strip_prefix(PROJECT_RELATIVE_PREFIX) {
            project_dir.join(rest).to_string_lossy().into_owned()
        } else {
            s.to_string()
        }
    };
    for (_, node) in graph.nodes_mut() {
        let path_keys: &[&str] = match node.node_type {
            NodeType::SmfImport | NodeType::FileReference => &["path"],
            NodeType::SmtImport => &["path", "smf_path"],
            NodeType::PassThrough => &[],
            _ => &[],
        };
        for key in path_keys {
            if let Some(ParamValue::String(s)) = node.params.get(*key).cloned() {
                let r = resolve(&s);
                if r != s {
                    node.params
                        .insert((*key).to_string(), ParamValue::String(r));
                }
            }
        }
        if matches!(node.node_type, NodeType::PassThrough) {
            if let Some(ParamValue::String(s)) = node.params.get("files").cloned() {
                let mut changed = false;
                let new = s
                    .lines()
                    .map(|line| {
                        let mut parts = line.splitn(2, '|');
                        let abs = parts.next().unwrap_or("");
                        let bundle = parts.next().unwrap_or("");
                        let r = resolve(abs);
                        if r != abs {
                            changed = true;
                        }
                        format!("{r}|{bundle}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if changed {
                    node.params
                        .insert("files".to_string(), ParamValue::String(new));
                }
            }
        }
    }
}
