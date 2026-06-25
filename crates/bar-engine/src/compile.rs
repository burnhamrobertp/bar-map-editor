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
    evaluate_graph_with_progress, get_heightmap_output, get_texture_output, GraphEngine,
    NodeExecutor, NodeOutputs, NodeType, ParamValue,
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
/// - `compiled/tile_index.bin`
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

    compile_from_outputs(
        project_dir,
        graph,
        &outputs,
        recipe,
        CompileDims {
            map_x,
            map_y,
            tex_w,
            tex_h,
        },
        on_progress,
    )
}

/// Dimensions required by `compile_from_outputs`.
pub struct CompileDims {
    pub map_x: u32,
    pub map_y: u32,
    pub tex_w: u32,
    pub tex_h: u32,
}

/// Write compiled output from already-evaluated graph outputs.
///
/// Used by the bundle runner to avoid double-evaluating the graph when
/// auto-recompiling before packaging.
pub fn compile_from_outputs(
    project_dir: &Path,
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    recipe: &Recipe,
    dims: CompileDims,
    on_progress: &dyn Fn(&str),
) -> Result<()> {
    let CompileDims {
        map_x,
        map_y,
        tex_w,
        tex_h,
    } = dims;
    tracing::debug!(
        map_x,
        map_y,
        tex_w,
        tex_h,
        project = %project_dir.display(),
        "Compile: writing output from evaluated graph"
    );

    let pkg = PackageDir::open(project_dir).context("Cannot open project package")?;
    let compiled_dir = pkg.compiled_dir();
    std::fs::create_dir_all(&compiled_dir)
        .with_context(|| format!("Cannot create {}", compiled_dir.display()))?;

    let smt_path = pkg.compiled_smt_path(&recipe.name);

    // Attempt direct block copy when texture comes straight from ImportedTexture
    // with no intervening color processing.
    let (tiles_x, tiles_y, tile_indices) =
        match try_direct_smt_copy(graph, &smt_path).unwrap_or(None) {
            Some(info) => {
                tracing::debug!("Compile: used direct ImportedTexture block copy for SMT");
                info
            }
            None => {
                // Full encode path: pull texture from graph outputs.
                on_progress("[95%] Encoding texture to DXT1");
                tracing::debug!(tex_w, tex_h, "Compile: encoding texture to DXT1");
                let color = find_texture_output(graph, outputs).context(
                    "No texture output found -- wire a texture node to the Bundler's texture port",
                )?;

                let mut writer = std::io::BufWriter::new(
                    std::fs::File::create(&smt_path)
                        .with_context(|| format!("Cannot create {}", smt_path.display()))?,
                );
                write_smt(&mut writer, &color).context("Failed to encode SMT")?;
                tracing::debug!(path = %smt_path.display(), "Compile: SMT written");

                // write_smt produces sequential tiles: tiles_x = tex_w/32, tiles_y = tex_h/32.
                let tx = tex_w / 32;
                let ty = tex_h / 32;
                let indices: Vec<i32> = (0..(tx * ty) as i32).collect();
                (tx, ty, indices)
            }
        };

    tracing::debug!(tiles_x, tiles_y, "Compile: writing tile index");
    on_progress("[97%] Writing tile index");
    let idx_path = pkg.compiled_tile_index_path();
    let idx_bytes: Vec<u8> = tile_indices.iter().flat_map(|&i| i.to_le_bytes()).collect();
    std::fs::write(&idx_path, &idx_bytes)
        .with_context(|| format!("Cannot write {}", idx_path.display()))?;

    on_progress("[97%] Writing heightmap");
    if let Some(hm) = get_heightmap_output(graph, outputs) {
        tracing::debug!(
            w = hm.width(),
            h = hm.height(),
            "Compile: writing heightmap.bin"
        );
        let hm_path = pkg.compiled_heightmap_path();
        write_heightmap_bin(&hm_path, &hm)?;
    } else {
        tracing::debug!(
            "Compile: no heightmap wired to FinalComposition -- heightmap.bin not written"
        );
    }

    on_progress("[99%] Writing fingerprint");
    write_fingerprint(&pkg, recipe, map_x, map_y, tiles_x, tiles_y, project_dir)?;
    tracing::info!(map_x, map_y, tiles_x, tiles_y, "Compile: complete");
    on_progress("[100%] Compile complete");
    Ok(())
}

/// If `bundler.texture` is directly fed by an `ImportedTexture` node, copy
/// its `.smt` file to `dest` without re-encoding. Returns the tile grid
/// dimensions and tile indices on success, `Ok(None)` when the fast path
/// does not apply.
fn try_direct_smt_copy(graph: &GraphEngine, dest: &Path) -> Result<Option<(u32, u32, Vec<i32>)>> {
    for conn in graph.connections() {
        if conn.to.port_name != "texture" {
            continue;
        }
        let Some(to_node) = graph.get_node(conn.to.node_id) else {
            continue;
        };
        if to_node.node_type != NodeType::FinalComposition {
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
        tracing::debug!(
            src = %asset_path,
            "Compile: direct block copy from ImportedTexture"
        );

        let tiles_x = match from_node.params.get("tiles_x") {
            Some(ParamValue::Int(v)) => (*v).max(0) as u32,
            Some(ParamValue::Float(v)) => (*v).max(0.0) as u32,
            _ => 0,
        };
        let tiles_y = match from_node.params.get("tiles_y") {
            Some(ParamValue::Int(v)) => (*v).max(0) as u32,
            Some(ParamValue::Float(v)) => (*v).max(0.0) as u32,
            _ => 0,
        };
        let tile_indices =
            if let Some(ParamValue::String(idx_path)) = from_node.params.get("tile_index_path") {
                if !idx_path.is_empty() {
                    match std::fs::read(idx_path) {
                        Ok(bytes) => bytes
                            .chunks_exact(4)
                            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                            .collect(),
                        Err(_) => vec![],
                    }
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

        return Ok(Some((tiles_x, tiles_y, tile_indices)));
    }
    Ok(None)
}

/// Find the texture connected to the first Bundler's `texture` port in the
/// evaluated graph outputs.
fn find_texture_output(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
) -> Option<bar_data::ColorBuffer> {
    get_texture_output(graph, outputs)
}

/// Read `compiled/heightmap.bin` written by `compile_project`. Returns `None`
/// when the file is absent, truncated, or has zero dimensions.
pub fn read_compiled_heightmap(pkg: &PackageDir) -> Option<bar_data::Heightmap> {
    let bytes = std::fs::read(pkg.compiled_heightmap_path()).ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let w = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let h = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if w == 0 || h == 0 || bytes.len() < 8 + w as usize * h as usize * 4 {
        return None;
    }
    let mut hm = bar_data::Heightmap::new(w, h).ok()?;
    for y in 0..h {
        for x in 0..w {
            let off = 8 + (y as usize * w as usize + x as usize) * 4;
            let v =
                f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            let _ = hm.set(x, y, v);
        }
    }
    Some(hm)
}

/// Write a heightmap as raw binary: 4-byte LE u32 width, 4-byte LE u32 height,
/// then width*height f32 LE samples in row-major order.
fn write_heightmap_bin(path: &Path, hm: &bar_data::Heightmap) -> Result<()> {
    let w = hm.width();
    let h = hm.height();
    let mut buf = Vec::with_capacity(8 + w as usize * h as usize * 4);
    buf.extend_from_slice(&w.to_le_bytes());
    buf.extend_from_slice(&h.to_le_bytes());
    for y in 0..h {
        for x in 0..w {
            buf.extend_from_slice(&hm.get(x, y).unwrap_or(0.0).to_le_bytes());
        }
    }
    std::fs::write(path, &buf)
        .with_context(|| format!("Cannot write heightmap to {}", path.display()))
}

/// Build and write `compiled/fingerprint.json`.
fn write_fingerprint(
    pkg: &PackageDir,
    recipe: &Recipe,
    map_x: u32,
    map_y: u32,
    tiles_x: u32,
    tiles_y: u32,
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

    let rs = recipe.output.map_settings.resolved();
    let fp = Fingerprint {
        recipe_hash,
        map_x,
        map_y,
        tiles_x,
        tiles_y,
        min_height: rs.min_height,
        max_height: rs.max_height,
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
