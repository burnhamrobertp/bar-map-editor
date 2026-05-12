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
        .filter(|(_, node)| node.node_type == NodeType::Bundler)
        .map(|(&id, _)| id)
        .collect()
}

/// Execute all bundler nodes (or a specific one by label).
pub fn execute_bundlers(
    graph: &GraphEngine,
    outputs: &NodeOutputs,
    recipe: &Recipe,
    output_dir: &Path,
    filter_label: Option<&str>,
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

        let result = execute_single_bundler(graph, outputs, bundler_id, recipe, output_dir)
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
) -> Result<BundlerResult> {
    let width = recipe.output.width;
    let height = recipe.output.height;
    let settings = &recipe.output.map_settings;
    let node = graph.get_node(bundler_id).unwrap();
    let params = &node.params;

    // bar-editor only emits the BAR map format: spring-smf packed
    // as a 7z (.sd7). No params here vary that — `target` /
    // `archive_format` were dropped from Bundler's defaults along
    // with the matching properties UI.
    let target_id = "spring-smf".to_string();
    let archive_format = ArchiveFormat::SevenZip;

    let map_name = match params.get("map_name") {
        Some(ParamValue::String(s)) => s.clone(),
        _ => node.label.to_lowercase().replace(' ', "_"),
    };

    let output_path_template = match params.get("output_path") {
        Some(ParamValue::String(s)) => s.clone(),
        _ => "{name}.sd7".to_string(),
    };

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
        shortname: recipe.shortname.clone(),
        description: recipe.description.clone(),
        author: recipe.author.clone(),
        version: recipe.version.clone(),
        dimensions: dims,
        settings: settings.clone(),
        features: recipe.features.clone(),
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
    for file_ref in &file_refs {
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

    // Package
    let packager = create_packager(&archive_format);
    let layout = &config.packaging.layout;
    packager.package(&staging_dir, &final_output_path, layout)?;

    // Clean up staging
    let _ = std::fs::remove_dir_all(&staging_dir);

    tracing::info!(
        "Bundler '{}': wrote {} to {}",
        node.label,
        packager.name(),
        final_output_path.display()
    );

    let map_internal_name = match plan.version.as_deref().filter(|v| !v.is_empty()) {
        Some(v) => format!("{} {}", plan.map_name, v),
        None => plan.map_name.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{GraphEngine, Node, NodeId, NodeType};

    #[test]
    fn test_find_bundler_nodes() {
        let mut graph = GraphEngine::new();
        let perlin = Node::new(NodeId(0), NodeType::PerlinNoise, "Perlin");
        graph.add_node(perlin);
        let bundler = Node::new(NodeId(0), NodeType::Bundler, "BAR Export");
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
