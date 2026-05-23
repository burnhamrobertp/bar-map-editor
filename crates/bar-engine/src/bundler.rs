//! Bundler evaluation: post-graph orchestration for packaging.
//!
//! After the graph is evaluated, bundler nodes collect their inputs
//! (from connected Output nodes) and invoke the codec + packager pipeline.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use bar_graph::{FileRef, GraphEngine, NodeId, NodeOutputs, NodeType, ParamValue, PortValue};

use crate::recipe::Recipe;
use crate::targets::{
    create_packager, validate_bundle_path, ArchiveFormat, ExportPlan, LayerSet, Severity,
    TargetRegistry,
};

/// Result from executing a single bundler node.
#[derive(Debug)]
pub struct BundlerResult {
    /// The node ID of the bundler that was executed.
    pub node_id: NodeId,
    /// Label of the bundler node.
    pub label: String,
    /// Output path of the produced artifact.
    pub output_path: PathBuf,
    /// Number of files written by the codec.
    pub files_written: usize,
    /// Spring-internal map name: `mapinfo.lua` `name` + " " + `version` (if
    /// set). This is what goes in the startscript `MapName=` field so the
    /// engine can find the archive by its mapinfo identity, not its filename.
    pub map_internal_name: String,
}

/// Discover all Bundler nodes in the graph.
pub fn find_bundler_nodes(graph: &GraphEngine) -> Vec<NodeId> {
    graph
        .nodes()
        .iter()
        .filter(|(_, node)| node.node_type == NodeType::FinalComposition)
        .map(|(&id, _)| id)
        .collect()
}

/// Execute all bundler nodes (or a specific one by label).
///
/// `project_dir` is the absolute path to the `.barproj` directory, passed
/// through to the codec so it can fast-path compiled assets (e.g. skip
/// re-encoding the SMT when a current compiled copy exists).
pub fn execute_bundlers(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    recipe: &Recipe,
    output_dir: &Path,
    filter_label: Option<&str>,
    project_dir: Option<&Path>,
) -> Result<Vec<BundlerResult>> {
    execute_bundlers_with_format(
        graph,
        outputs,
        recipe,
        output_dir,
        filter_label,
        project_dir,
        None,
    )
}

/// Like `execute_bundlers`, but lets the caller override the archive
/// format. Used by the "Test in BAR" flow to ship a `.sdd` directory
/// (no 7-Zip compression) for iteration -- a typical Onyx-sized map
/// takes ~10s to 7z-compress; writing the same content as a directory
/// is well under a second. The engine accepts both `.sd7` and `.sdd`
/// from the `maps/` directory transparently.
pub fn execute_bundlers_with_format(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    recipe: &Recipe,
    output_dir: &Path,
    filter_label: Option<&str>,
    project_dir: Option<&Path>,
    archive_format_override: Option<ArchiveFormat>,
) -> Result<Vec<BundlerResult>> {
    let bundler_ids = find_bundler_nodes(graph);
    let mut results = Vec::new();

    for bundler_id in bundler_ids {
        let node = graph.get_node(bundler_id).unwrap();

        // Filter by label if specified
        if let Some(label) = filter_label {
            if node.label != label {
                continue;
            }
        }

        let result = execute_single_bundler(
            graph,
            outputs,
            bundler_id,
            recipe,
            output_dir,
            project_dir,
            archive_format_override,
        )
        .with_context(|| format!("Failed to execute bundler '{}'", node.label))?;

        results.push(result);
    }

    Ok(results)
}

/// Execute a single bundler node.
fn execute_single_bundler(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    bundler_id: NodeId,
    recipe: &Recipe,
    output_dir: &Path,
    project_dir: Option<&Path>,
    archive_format_override: Option<ArchiveFormat>,
) -> Result<BundlerResult> {
    let width = recipe.output.width;
    let height = recipe.output.height;
    let settings = &recipe.output.map_settings;
    let node = graph.get_node(bundler_id).unwrap();
    let params = &node.params;

    // bar-editor only emits the BAR map format: spring-smf packed
    // as a 7z (.sd7) by default; the "Test in BAR" path overrides
    // to `Directory` so iteration skips 7z compression.
    let target_id = "spring-smf".to_string();
    let archive_format = archive_format_override.unwrap_or(ArchiveFormat::SevenZip);

    // Map identity convention: `<recipe.name>_<recipe.version>` with
    // spaces -> underscores, lowercased. Matches BAR's archive
    // naming (e.g. `onyx_cauldron_2.2.2.sd7`) so the produced
    // artifact's filename mirrors its mapinfo MapName -- avoids the
    // confusion of "what is final_composition.sd7?" when the recipe
    // says the map is called "Onyx Cauldron". A `map_name` param
    // explicitly set on the bundler node still overrides this for
    // edge cases.
    // For the canonical `.sd7` distribution artifact the slug is
    // `<name>_<version>` so downloads stay sortable side by side. The
    // `.sdd` fast-iteration artifact (Test-in-BAR) drops the version
    // so each test overwrites the previous instead of piling up
    // versioned directories in `<install>/maps/`. The mapinfo's
    // `name`/`version` -- which the engine uses to resolve the
    // archive at script time -- stays the same in both cases.
    let include_version = archive_format != ArchiveFormat::Directory;
    let map_name = match params.get("map_name") {
        Some(ParamValue::String(s)) if !s.is_empty() => s.clone(),
        _ => default_map_name_from_recipe(recipe, &node.label, include_version),
    };
    // mapinfo.lua's `name = "..."` field (and thus the engine's
    // archive identifier) must be the human-readable recipe name,
    // not the filesystem slug. Fall back to the slug only when the
    // recipe has no name set yet.
    let display_name = {
        let trimmed = recipe.name.trim();
        if trimmed.is_empty() {
            map_name.clone()
        } else {
            trimmed.to_string()
        }
    };

    let mut output_path_template = match params.get("output_path") {
        Some(ParamValue::String(s)) => s.clone(),
        _ => "{name}.sd7".to_string(),
    };
    // When the host forces a directory output (Test-in-BAR fast
    // path), rewrite the extension so the result lands in
    // `{name}.sdd/` -- the engine accepts both `.sd7` and `.sdd`
    // from `maps/`, and using a different extension keeps the
    // fast-iteration artifact from masking the canonical compiled
    // SD7 on disk.
    if archive_format_override == Some(ArchiveFormat::Directory) {
        if let Some(stem) = output_path_template.strip_suffix(".sd7") {
            output_path_template = format!("{stem}.sdd");
        }
    }

    // Collect layers from bundler's connected inputs
    let layers = collect_bundler_layers(graph, outputs, bundler_id);

    // Collect file references
    let file_refs = collect_bundler_files(graph, outputs, bundler_id);

    // Validate file reference paths
    for file_ref in &file_refs {
        validate_bundle_path(&file_ref.bundle_path)?;
    }

    // Resolve target and codec
    let registry = TargetRegistry::new();
    let config = registry
        .get_target(&target_id)
        .ok_or_else(|| {
            anyhow::anyhow!("Unknown target '{}' in bundler '{}'", target_id, node.label)
        })?
        .clone();

    let codec = registry
        .get_codec(&config.codec)
        .ok_or_else(|| anyhow::anyhow!("Unknown codec: {}", config.codec))?;

    // Compute dimensions
    let dims = codec.compute_dimensions(&config, width, height);

    // Create export plan
    let plan = ExportPlan {
        map_name: map_name.clone(),
        display_name: display_name.clone(),
        shortname: recipe.shortname.clone(),
        description: recipe.description.clone(),
        author: recipe.author.clone(),
        version: recipe.version.clone(),
        tip: recipe.tip.clone(),
        depend: recipe.depend.clone(),
        dimensions: dims,
        settings: settings.clone(),
        features: recipe.features.clone(),
        project_dir: project_dir.map(|p| p.to_path_buf()),
    };

    // Validate
    let errors = codec.validate(&config, &plan, &layers)?;
    let has_errors = errors.iter().any(|e| e.severity == Severity::Error);
    for err in &errors {
        match err.severity {
            Severity::Error => tracing::error!("[{}] {}", node.label, err),
            Severity::Warning => tracing::warn!("[{}] {}", node.label, err),
        }
    }
    if has_errors {
        anyhow::bail!(
            "Bundler '{}' validation failed — fix errors above",
            node.label
        );
    }

    // Create staging directory for codec output
    let staging_dir = output_dir.join(format!(".bundler_staging_{}", bundler_id.0));
    std::fs::create_dir_all(&staging_dir)?;

    // Write via codec
    let written = codec.write(&config, &plan, &layers, &staging_dir)?;

    // Copy file references into staging.
    // mapinfo.lua is special: if the codec already generated one, merge
    // the original's unknown keys into it rather than overwriting.
    //
    // Two BME-internal artifacts must never reach the bundle:
    //  * `_bme_smf_minimap.png` -- the SMF-embedded minimap sidecar
    //    the editor extracts for preview rendering. The engine reads
    //    the minimap from the SMF binary directly; the sidecar exists
    //    only to feed BME's renderer between import and re-compile.
    //  * `grassmap.png` -- BME materialises the SMF's MEH_Vegetation
    //    distribution mask into a PNG so the editor's grass widget
    //    can sample it without re-parsing the SMF binary every frame.
    //    Maps that ship an explicit `grassDistTGA` already have their
    //    own dist mask under `maps/`; the materialised PNG would
    //    shadow it. Skip both.
    let is_bme_internal_artifact = |bundle_path: &str| -> bool {
        let trimmed = bundle_path.trim_start_matches("./");
        trimmed.eq_ignore_ascii_case(bar_project::SMF_MINIMAP_SIDE_CAR)
            || trimmed.eq_ignore_ascii_case("grassmap.png")
    };
    for file_ref in &file_refs {
        if is_bme_internal_artifact(&file_ref.bundle_path) {
            tracing::debug!(
                "[{}] Skipping BME-internal artifact from bundle: {}",
                node.label,
                file_ref.bundle_path
            );
            continue;
        }
        let src = Path::new(&file_ref.path);
        let dest = staging_dir.join(&file_ref.bundle_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if src.exists() {
            let is_mapinfo = file_ref
                .bundle_path
                .trim_start_matches("./")
                .eq_ignore_ascii_case("mapinfo.lua");
            if is_mapinfo && dest.exists() {
                match (std::fs::read_to_string(src), std::fs::read_to_string(&dest)) {
                    (Ok(original), Ok(generated)) => {
                        let merged =
                            crate::targets::spring_smf::merge_mapinfo_lua(&generated, &original);
                        std::fs::write(&dest, merged).with_context(|| {
                            format!("Failed to write merged mapinfo.lua: {}", dest.display())
                        })?;
                    }
                    _ => {
                        tracing::warn!(
                            "[{}] Could not merge mapinfo.lua; keeping editor-generated version",
                            node.label
                        );
                    }
                }
            } else {
                std::fs::copy(src, &dest).with_context(|| {
                    format!(
                        "Failed to copy file reference: {} -> {}",
                        src.display(),
                        dest.display()
                    )
                })?;
            }
        } else {
            tracing::warn!(
                "[{}] File reference not found, skipping: {}",
                node.label,
                src.display()
            );
        }
    }

    // Resolve output path
    let resolved_output_name = output_path_template
        .replace("{project}", &map_name)
        .replace("{name}", &map_name)
        .replace("{target}", &target_id);
    let final_output_path = output_dir.join(&resolved_output_name);

    // Package. For Directory format, wipe any prior contents first so
    // stale files from previous bundles don't ghost into the new
    // archive (the DirectoryPackager copies on top without clearing,
    // which is fine for fresh outputs but wrong on repeat runs of
    // the Test-in-BAR fast path).
    if matches!(archive_format, ArchiveFormat::Directory) && final_output_path.exists() {
        let _ = std::fs::remove_dir_all(&final_output_path);
    }
    // Also remove sibling archives with the same base name but a
    // different extension (e.g. a stale `final_composition.sd7`
    // left over from before BME switched to `.sdd` output). The
    // engine matches archives by mapinfo MapName, not filename --
    // two archives in `maps/` claiming the same map produces
    // unpredictable load order. Clean them up so the *current*
    // output is unambiguously the one the engine picks.
    if let (Some(parent), Some(stem)) = (
        final_output_path.parent(),
        final_output_path.file_stem().and_then(|s| s.to_str()),
    ) {
        for ext in ["sd7", "sdz", "sdd"] {
            let sibling = parent.join(format!("{stem}.{ext}"));
            if sibling == final_output_path || !sibling.exists() {
                continue;
            }
            let result = if sibling.is_dir() {
                std::fs::remove_dir_all(&sibling)
            } else {
                std::fs::remove_file(&sibling)
            };
            match result {
                Ok(()) => tracing::debug!(
                    path = %sibling.display(),
                    "Bundler: removed stale sibling archive"
                ),
                Err(e) => tracing::warn!(
                    path = %sibling.display(),
                    err = %e,
                    "Bundler: failed to remove stale sibling archive"
                ),
            }
        }
    }
    let packager = create_packager(&archive_format);
    let layout = &config.packaging.layout;
    packager.package(&staging_dir, &final_output_path, layout)?;

    // Clean up staging
    let _ = std::fs::remove_dir_all(&staging_dir);

    tracing::debug!(
        "Bundler '{}': wrote {} to {}",
        node.label,
        packager.name(),
        final_output_path.display()
    );

    // Spring archive ID is `display_name .. " " .. version` from
    // mapinfo.lua. Some authored maps put the version inside the
    // `name` field too -- naive concatenation then produces a doubled
    // identity the engine can't resolve. The patterns we strip:
    //   "Onyx Cauldron 2.2.3"  -> base "Onyx Cauldron", version "2.2.3"
    //   "Forge v2.3"           -> base "Forge",         version "2.3"
    //   "Map_v1.0"             -> base "Map",           version "1.0"
    // Try each variant with each separator; first match wins.
    let map_internal_name = match plan.version.as_deref().filter(|v| !v.is_empty()) {
        Some(v) => {
            let mut base = plan.display_name.as_str();
            let v_lower = format!("v{v}");
            let v_upper = format!("V{v}");
            let suffix_candidates = [v.to_string(), v_lower, v_upper];
            'outer: for sfx in &suffix_candidates {
                if let Some(trimmed) = base.strip_suffix(sfx.as_str()) {
                    for sep in [' ', '_'] {
                        if let Some(stripped) = trimmed.strip_suffix(sep) {
                            base = stripped;
                            break 'outer;
                        }
                    }
                }
            }
            format!("{} {}", base, v)
        }
        None => plan.display_name.clone(),
    };

    Ok(BundlerResult {
        node_id: bundler_id,
        label: node.label.clone(),
        output_path: final_output_path,
        files_written: written.files.len(),
        map_internal_name,
    })
}

/// Collect layer data from a bundler's connected input ports.
fn collect_bundler_layers(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    bundler_id: NodeId,
) -> LayerSet {
    let mut layers = LayerSet {
        heightmap: None,
        metalmap: None,
        typemap: None,
        texture: None,
        normalmap: None,
        grassmap: None,
        specular: None,
    };

    // For each connection to this bundler, check what port it connects to
    for conn in graph.connections() {
        if conn.to.node_id != bundler_id {
            continue;
        }

        // Get the value from the upstream node's output
        let value = outputs
            .get(&conn.from.node_id)
            .and_then(|node_outputs| node_outputs.get(&conn.from.port_name));

        let Some(value) = value else { continue };

        match conn.to.port_name.as_str() {
            "heightmap" => {
                if let PortValue::Heightmap(hm) = value {
                    layers.heightmap = Some(hm.clone());
                }
            }
            "metalmap" => {
                if let PortValue::Heightmap(hm) = value {
                    layers.metalmap = Some(hm.clone());
                }
            }
            "typemap" => {
                if let PortValue::Heightmap(hm) = value {
                    layers.typemap = Some(hm.clone());
                }
            }
            "grassmap" => {
                if let PortValue::Heightmap(hm) = value {
                    layers.grassmap = Some(hm.clone());
                }
            }
            "specular" => {
                if let PortValue::Heightmap(hm) = value {
                    // SpecularMap produces a Heightmap but LayerSet expects ColorBuffer.
                    // Convert grayscale heightmap to RGBA (grayscale → all channels).
                    let w = hm.width();
                    let h = hm.height();
                    let mut cb = bar_data::ColorBuffer::new(w, h).unwrap();
                    for y in 0..h {
                        for x in 0..w {
                            let v = hm.get(x, y).unwrap_or(0.0);
                            cb.set(x, y, [v, v, v, 1.0]);
                        }
                    }
                    layers.specular = Some(cb);
                }
            }
            "texture" => {
                if let PortValue::Color(cb) = value {
                    layers.texture = Some(cb.clone());
                }
            }
            "normalmap" => {
                if let PortValue::Color(cb) = value {
                    layers.normalmap = Some(cb.clone());
                }
            }
            _ => {}
        }
    }

    layers
}

/// Collect file references connected to a bundler's `files` port.
fn collect_bundler_files(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    bundler_id: NodeId,
) -> Vec<FileRef> {
    let mut files = Vec::new();

    for conn in graph.connections() {
        if conn.to.node_id != bundler_id || conn.to.port_name != "files" {
            continue;
        }

        if let Some(node_outputs) = outputs.get(&conn.from.node_id) {
            match node_outputs.get(&conn.from.port_name) {
                Some(PortValue::File(file_ref)) => {
                    files.push(file_ref.clone());
                }
                Some(PortValue::FileList(list)) => {
                    files.extend(list.iter().cloned());
                }
                _ => {}
            }
        }
    }

    files
}

/// Re-emit only `mapinfo.lua` into an existing bundle directory
/// without re-running the graph or rewriting any heavy artifacts
/// (SMF / SMT / passthrough file copies). Used by the Test-in-BAR
/// fast-iteration path when the only change since the last
/// successful bundle was to mapinfo-affecting settings (atmosphere,
/// lighting, water, grass, physics scalars, identity, etc.).
///
/// Preserves the merge-with-original behaviour the full bundler
/// performs: if `<project_dir>/passthrough/mapinfo.lua` exists, its
/// unknown keys are merged into the freshly generated file so
/// hand-authored fields aren't lost.
///
/// `bundle_dir` must be an existing `.sdd` directory previously
/// produced by [`execute_bundlers_with_format`] with
/// [`ArchiveFormat::Directory`] -- the heavy artifacts there are
/// assumed to still be current.
pub fn regenerate_mapinfo_in_bundle(
    graph: &GraphEngine,
    recipe: &Recipe,
    bundle_dir: &Path,
    project_dir: Option<&Path>,
) -> Result<()> {
    let bundler_ids = find_bundler_nodes(graph);
    let bundler_id = bundler_ids
        .first()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("Graph has no Bundler node"))?;
    let node = graph
        .get_node(bundler_id)
        .ok_or_else(|| anyhow::anyhow!("Bundler node missing from graph"))?;
    let params = &node.params;

    // Same map_name resolution as the full bundler for the
    // Directory (Test-in-BAR) path: no version suffix.
    let map_name = match params.get("map_name") {
        Some(ParamValue::String(s)) if !s.is_empty() => s.clone(),
        _ => default_map_name_from_recipe(recipe, &node.label, false),
    };
    let display_name = {
        let trimmed = recipe.name.trim();
        if trimmed.is_empty() {
            map_name.clone()
        } else {
            trimmed.to_string()
        }
    };

    let registry = TargetRegistry::new();
    let config = registry
        .get_target("spring-smf")
        .ok_or_else(|| anyhow::anyhow!("Missing spring-smf target config"))?
        .clone();
    let dims = registry
        .get_codec(&config.codec)
        .ok_or_else(|| anyhow::anyhow!("Missing spring-smf codec"))?
        .compute_dimensions(&config, recipe.output.width, recipe.output.height);

    let plan = ExportPlan {
        map_name: map_name.clone(),
        display_name,
        shortname: recipe.shortname.clone(),
        description: recipe.description.clone(),
        author: recipe.author.clone(),
        version: recipe.version.clone(),
        tip: recipe.tip.clone(),
        depend: recipe.depend.clone(),
        dimensions: dims.clone(),
        settings: recipe.output.map_settings.clone(),
        features: recipe.features.clone(),
        project_dir: project_dir.map(|p| p.to_path_buf()),
    };

    let codec = crate::targets::SpringSmfCodec;
    let map_x = (recipe.output.width - 1) / 64;
    let map_y = (recipe.output.height - 1) / 64;
    let generated = codec.generate_mapinfo(&map_name, map_x, map_y, &plan);

    // Preserve hand-authored / unknown keys from the project's
    // passthrough mapinfo.lua, mirroring the merge the full bundler
    // performs after the codec write step.
    let final_lua = project_dir
        .map(|p| p.join("passthrough").join("mapinfo.lua"))
        .filter(|p| p.is_file())
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .map(|original| crate::targets::spring_smf::merge_mapinfo_lua(&generated, &original))
        .unwrap_or(generated);

    let dest = bundle_dir.join("mapinfo.lua");
    std::fs::write(&dest, final_lua)
        .with_context(|| format!("Failed to write {}", dest.display()))?;
    Ok(())
}

/// Slug-ify the recipe's name + version into a BAR-style archive
/// filename stem. Example: name="Onyx Cauldron", version="2.2.3"
/// -> "onyx_cauldron_2.2.3". Falls back to a sanitised version of
/// `node_label` (the bundler node's display name) when the recipe
/// hasn't been given a name yet -- keeps fresh projects from
/// writing literal `_.sd7` files.
fn default_map_name_from_recipe(
    recipe: &Recipe,
    node_label: &str,
    include_version: bool,
) -> String {
    let raw_name = recipe.name.trim();
    let base = if raw_name.is_empty() {
        node_label
    } else {
        raw_name
    };
    let version = recipe
        .version
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    // Strip the version from the name if it's already there so the
    // filename doesn't end up "onyx_cauldron_2.2.3_2.2.3".
    let base = match version {
        Some(v) => {
            let mut trimmed = base;
            for sep in [' ', '_'] {
                if let Some(without_v) = trimmed.strip_suffix(v) {
                    if let Some(without_sep) = without_v.strip_suffix(sep) {
                        trimmed = without_sep;
                        break;
                    }
                }
            }
            trimmed
        }
        None => base,
    };
    let slug = base
        .to_lowercase()
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect::<String>();
    match version {
        Some(v) if include_version => format!("{slug}_{v}"),
        _ => slug,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{GraphEngine, Node, NodeId, NodeType};

    #[test]
    fn test_find_bundler_nodes() {
        let mut graph = GraphEngine::new();
        let perlin = Node::new(NodeId(0), NodeType::PerlinNoise, "Perlin");
        graph.add_node(perlin);
        let bundler = Node::new(NodeId(0), NodeType::FinalComposition, "BAR Export");
        let bid = graph.add_node(bundler);

        let bundlers = find_bundler_nodes(&graph);
        assert_eq!(bundlers.len(), 1);
        assert_eq!(bundlers[0], bid);
    }

    #[test]
    fn test_find_no_bundler_nodes() {
        let mut graph = GraphEngine::new();
        let perlin = Node::new(NodeId(0), NodeType::PerlinNoise, "Perlin");
        graph.add_node(perlin);

        let bundlers = find_bundler_nodes(&graph);
        assert!(bundlers.is_empty());
    }

    #[test]
    fn test_validate_bundle_path_valid() {
        assert!(validate_bundle_path("maps/test.smf").is_ok());
        assert!(validate_bundle_path("mapinfo.lua").is_ok());
        assert!(validate_bundle_path("unittextures/normals.dds").is_ok());
    }

    #[test]
    fn test_validate_bundle_path_invalid() {
        assert!(validate_bundle_path("").is_err());
        assert!(validate_bundle_path("/absolute/path").is_err());
        assert!(validate_bundle_path("../escape").is_err());
        assert!(validate_bundle_path("maps/../../../etc/passwd").is_err());
    }
}
