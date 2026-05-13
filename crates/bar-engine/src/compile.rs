//! Compile step: evaluate the texture subgraph at native resolution,
//! encode to DXT1/BC1, write `compiled/<name>.smt`, and record a
//! staleness fingerprint.
//!
//! The compiled output is the authoritative native-resolution texture.
//! Bundle integration (Phase 6) copies it directly instead of re-encoding.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use bar_data::write_smt;
use bar_graph::{
    evaluate_graph_with_progress, get_texture_output, GraphEngine, NodeExecutor, NodeOutputs,
    NodeType, ParamValue,
};
use bar_project::{AssetStat, Fingerprint, PackageDir};

use bar_project::recipe::Recipe;

/// Evaluate the graph at native resolution and write the compiled output.
///
/// `project_dir` must point to the `.barproj` directory; assets inside it are
/// read by the executor (runtime paths must already be injected).
///
/// On success, writes:
/// - `compiled/<map_name>.smt`
/// - `compiled/fingerprint.json`
pub fn compile_project(
    project_dir: &Path,
    graph: &GraphEngine,
    executor: &dyn NodeExecutor,
    recipe: &Recipe,
    on_progress: &dyn Fn(&str),
) -> Result<()> {
    let map_x = recipe.output.width.saturating_sub(1);
    let map_y = recipe.output.height.saturating_sub(1);
    let tex_w = map_x * 8;
    let tex_h = map_y * 8;

    anyhow::ensure!(
        tex_w > 0 && tex_h > 0,
        "Map dimensions too small to compile (map_x={map_x}, map_y={map_y})"
    );

    on_progress("[ 0%] Evaluating graph at native resolution");
    let outputs = evaluate_graph_with_progress(
        graph,
        executor,
        map_x + 1,
        map_y + 1,
        tex_w,
        tex_h,
        on_progress,
    )
    .context("Graph evaluation failed")?;

    let pkg = PackageDir::open(project_dir).context("Cannot open project package")?;
    let compiled_dir = pkg.compiled_dir();
    std::fs::create_dir_all(&compiled_dir)
        .with_context(|| format!("Cannot create {}", compiled_dir.display()))?;

    // Safe map name for the file system.
    let safe_name: String = recipe
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let smt_path = compiled_dir.join(format!("{safe_name}.smt"));

    // Attempt direct block copy when texture comes straight from ImportedTexture
    // with no intervening color processing.
    let copied = try_direct_smt_copy(graph, &smt_path).unwrap_or(false);

    if !copied {
        // Full encode path: pull texture from graph outputs.
        on_progress("[95%] Encoding texture to DXT1");
        let color = find_texture_output(graph, &outputs).context(
            "No texture output found -- wire a texture node to the Bundler's texture port",
        )?;

        let mut writer = std::io::BufWriter::new(
            std::fs::File::create(&smt_path)
                .with_context(|| format!("Cannot create {}", smt_path.display()))?,
        );
        write_smt(&mut writer, &color).context("Failed to encode SMT")?;
    }

    on_progress("[98%] Writing fingerprint");
    write_fingerprint(&pkg, recipe, map_x, map_y, project_dir)?;
    on_progress("[100%] Compile complete");
    Ok(())
}

/// If `bundler.texture` is directly fed by an `ImportedTexture` node, copy
/// its `.smt` file to `dest` without re-encoding. Returns `Ok(true)` on
/// success, `Ok(false)` when the fast path does not apply.
fn try_direct_smt_copy(graph: &GraphEngine, dest: &Path) -> Result<bool> {
    for conn in graph.connections() {
        if conn.to.port_name != "texture" {
            continue;
        }
        let Some(to_node) = graph.get_node(conn.to.node_id) else {
            continue;
        };
        if to_node.node_type != NodeType::Bundler {
            continue;
        }
        let Some(from_node) = graph.get_node(conn.from.node_id) else {
            continue;
        };
        if from_node.node_type != NodeType::ImportedTexture {
            continue;
        }
        let Some(ParamValue::String(asset_path)) = from_node.params.get("asset_path") else {
            continue;
        };
        if asset_path.is_empty() {
            continue;
        }
        std::fs::copy(asset_path, dest).with_context(|| {
            format!(
                "Direct SMT copy failed: {} -> {}",
                asset_path,
                dest.display()
            )
        })?;
        tracing::info!(
            src = %asset_path,
            "Compile: direct block copy from ImportedTexture"
        );
        return Ok(true);
    }
    Ok(false)
}

/// Find the texture connected to the first Bundler's `texture` port in the
/// evaluated graph outputs.
fn find_texture_output(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
) -> Option<bar_data::ColorBuffer> {
    get_texture_output(graph, outputs)
}

/// Build and write `compiled/fingerprint.json`.
fn write_fingerprint(
    pkg: &PackageDir,
    recipe: &Recipe,
    map_x: u32,
    map_y: u32,
    project_dir: &Path,
) -> Result<()> {
    let recipe_json = serde_json::to_string(recipe).unwrap_or_else(|_| String::new());
    let recipe_hash = format!("{:016x}", fnv64(recipe_json.as_bytes()));

    let mut assets: HashMap<String, AssetStat> = HashMap::new();
    let assets_dir = project_dir.join("assets");
    if let Ok(entries) = std::fs::read_dir(&assets_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = std::fs::metadata(&path) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                assets.insert(
                    entry.file_name().to_string_lossy().into_owned(),
                    AssetStat {
                        size: meta.len(),
                        mtime_secs: mtime,
                    },
                );
            }
        }
    }

    let fp = Fingerprint {
        recipe_hash,
        map_x,
        map_y,
        assets,
    };
    pkg.write_fingerprint(&fp)
}

fn fnv64(data: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}
