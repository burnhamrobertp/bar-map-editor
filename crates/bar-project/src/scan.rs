//! Result type for .sd7 work-directory scanning, plus the conversion to `Project`.
//!
//! `WorkDirScan` is produced by `bar_engine::extract_sd7_to_work_dir` and
//! consumed by the GUI's import workflow.  Keeping it here (rather than in
//! bar-engine) lets the GUI depend on this lightweight type without pulling in
//! the full engine.

use std::collections::HashMap;
use std::path::PathBuf;

use bar_graph::{NodeType, ParamValue};

use crate::project::{EditorLayout, NodeSize, Position, Project};
use crate::recipe::{
    MapSettings, OutputConfig, Recipe, RecipeConnection, RecipeNode, RECIPE_SCHEMA_VERSION,
};

/// Convert a scanned SD7 work directory into a `Project` ready for `apply_project`.
///
/// Builds the canonical five-node import pipeline (SmfImport, SmtImport,
/// PassThrough, NormalMap, Bundler) from the scan result. The returned
/// `Project` has no path (it has not been saved yet) and `is_dirty` will
/// be set to `true` by the caller after installing via `apply_project`.
pub fn scan_to_project(scan: &WorkDirScan) -> Project {
    let source_x = 80.0_f32;
    let bundler_x = 700.0_f32;
    let nm_x = (source_x + 165.0 + bundler_x) / 2.0; // 472.5

    let mut nodes: Vec<RecipeNode> = Vec::new();
    let mut connections: Vec<RecipeConnection> = Vec::new();
    let mut node_positions: HashMap<String, Position> = HashMap::new();
    let mut node_sizes: HashMap<String, NodeSize> = HashMap::new();

    // Build the full passthrough file list. Include smf and smt so that an
    // unedited round-trip preserves them byte-for-byte.
    let mut passthrough_entries: Vec<(PathBuf, PathBuf)> = scan.passthrough_files.clone();
    if let (Some(abs), Some(rel)) = (scan.smf_abs.as_ref(), scan.smf_rel.as_ref()) {
        passthrough_entries.push((abs.clone(), rel.clone()));
    }
    if let (Some(abs), Some(rel)) = (scan.smt_abs.as_ref(), scan.smt_rel.as_ref()) {
        passthrough_entries.push((abs.clone(), rel.clone()));
    }

    let map_info_file: Option<String> = passthrough_entries
        .iter()
        .map(|(_, rel)| rel.to_string_lossy().replace('\\', "/"))
        .find(|p| p.eq_ignore_ascii_case("mapinfo.lua"));

    // SmfImport
    let has_smf = scan.smf_abs.is_some();
    if let Some(ref smf_abs) = scan.smf_abs {
        let mut params = HashMap::new();
        params.insert(
            "path".to_string(),
            ParamValue::String(smf_abs.to_string_lossy().to_string()),
        );
        params.insert("load_metalmap".to_string(), ParamValue::Bool(true));
        params.insert("load_typemap".to_string(), ParamValue::Bool(true));
        nodes.push(RecipeNode {
            key: "smf".to_string(),
            node_type: NodeType::SmfImport,
            label: "SMF Import".to_string(),
            params,
        });
        node_positions.insert(
            "smf".to_string(),
            Position {
                x: source_x,
                y: 130.0,
            },
        );
        node_sizes.insert(
            "smf".to_string(),
            NodeSize {
                width: 165.0,
                height: 100.0,
            },
        );
    }

    // SmtImport
    if let Some(ref smt_abs) = scan.smt_abs {
        let mut params = HashMap::new();
        params.insert(
            "path".to_string(),
            ParamValue::String(smt_abs.to_string_lossy().to_string()),
        );
        if let Some(ref smf_abs) = scan.smf_abs {
            params.insert(
                "smf_path".to_string(),
                ParamValue::String(smf_abs.to_string_lossy().to_string()),
            );
        }
        if let Some((tx, ty)) = scan.tile_grid {
            params.insert("tiles_x".to_string(), ParamValue::UInt(tx));
            params.insert("tiles_y".to_string(), ParamValue::UInt(ty));
        }
        nodes.push(RecipeNode {
            key: "smt".to_string(),
            node_type: NodeType::SmtImport,
            label: "SMT Import".to_string(),
            params,
        });
        node_positions.insert(
            "smt".to_string(),
            Position {
                x: source_x,
                y: 360.0,
            },
        );
        node_sizes.insert(
            "smt".to_string(),
            NodeSize {
                width: 165.0,
                height: 100.0,
            },
        );
    }

    // PassThrough
    let has_pass = !passthrough_entries.is_empty();
    if has_pass {
        let file_list: String = passthrough_entries
            .iter()
            .map(|(abs, rel)| {
                let bundle = rel.to_string_lossy().replace('\\', "/");
                format!("{}|{}", abs.to_string_lossy(), bundle)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut params = HashMap::new();
        params.insert("files".to_string(), ParamValue::String(file_list));
        nodes.push(RecipeNode {
            key: "pass".to_string(),
            node_type: NodeType::PassThrough,
            label: "Pass-Through".to_string(),
            params,
        });
        node_positions.insert(
            "pass".to_string(),
            Position {
                x: source_x,
                y: 570.0,
            },
        );
        node_sizes.insert(
            "pass".to_string(),
            NodeSize {
                width: 165.0,
                height: 80.0,
            },
        );
    }

    // NormalMap (only when SMF present)
    if has_smf {
        nodes.push(RecipeNode {
            key: "nm".to_string(),
            node_type: NodeType::NormalMap,
            label: "Normal Map".to_string(),
            params: HashMap::new(),
        });
        node_positions.insert("nm".to_string(), Position { x: nm_x, y: 478.0 });
        node_sizes.insert(
            "nm".to_string(),
            NodeSize {
                width: 140.0,
                height: 60.0,
            },
        );
    }

    // Bundler (always)
    {
        let mut params = HashMap::new();
        params.insert(
            "map_name".to_string(),
            ParamValue::String(scan.map_name.clone()),
        );
        nodes.push(RecipeNode {
            key: "bundler".to_string(),
            node_type: NodeType::Bundler,
            label: "BAR .sd7".to_string(),
            params,
        });
        node_positions.insert(
            "bundler".to_string(),
            Position {
                x: bundler_x,
                y: 270.0,
            },
        );
        node_sizes.insert(
            "bundler".to_string(),
            NodeSize {
                width: 165.0,
                height: 210.0,
            },
        );
    }

    // Preview (only when SMF is present -- heightmap is required to render anything)
    if has_smf {
        nodes.push(RecipeNode {
            key: "preview".to_string(),
            node_type: NodeType::Preview,
            label: "3D Preview".to_string(),
            params: HashMap::new(),
        });
        node_positions.insert(
            "preview".to_string(),
            Position {
                x: bundler_x,
                y: 540.0,
            },
        );
        node_sizes.insert(
            "preview".to_string(),
            NodeSize {
                width: 165.0,
                height: 150.0,
            },
        );
    }

    // Connections
    if has_smf {
        connections.push(RecipeConnection {
            from: "smf.heightmap".to_string(),
            to: "bundler.heightmap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "smf.metalmap".to_string(),
            to: "bundler.metalmap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "smf.typemap".to_string(),
            to: "bundler.typemap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "smf.heightmap".to_string(),
            to: "nm.input".to_string(),
        });
        connections.push(RecipeConnection {
            from: "nm.output".to_string(),
            to: "bundler.normalmap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "smf.heightmap".to_string(),
            to: "preview.heightmap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "nm.output".to_string(),
            to: "preview.normal_map".to_string(),
        });
    }
    if scan.smt_abs.is_some() {
        connections.push(RecipeConnection {
            from: "smt.texture".to_string(),
            to: "bundler.texture".to_string(),
        });
        connections.push(RecipeConnection {
            from: "smt.texture".to_string(),
            to: "preview.texture".to_string(),
        });
    }
    if has_pass {
        connections.push(RecipeConnection {
            from: "pass.files".to_string(),
            to: "bundler.files".to_string(),
        });
    }

    let (width, height) = scan.map_dims.unwrap_or((256, 256));
    let (min_height, max_height) = scan.height_range.unwrap_or((0.0, 800.0));
    let map_settings = MapSettings {
        min_height,
        max_height,
        ..MapSettings::default()
    };

    let recipe = Recipe {
        schema_version: RECIPE_SCHEMA_VERSION,
        name: scan.map_name.clone(),
        shortname: None,
        description: String::new(),
        author: None,
        version: None,
        nodes,
        connections,
        output: OutputConfig {
            width,
            height,
            map_settings,
        },
    };

    let layout = EditorLayout {
        node_positions,
        node_sizes,
        canvas_offset: (0.0, 0.0),
        map_info_file,
        groups: Vec::new(),
        open_tabs: Vec::new(),
        active_tab: 0,
    };

    Project { recipe, layout }
}

/// Result of scanning an extracted .sd7 work directory.
#[derive(Debug)]
pub struct WorkDirScan {
    /// Absolute path to the work directory.
    pub work_dir: PathBuf,
    /// Map name derived from the archive filename stem.
    pub map_name: String,
    /// Absolute path to the first `.smf` file found (if any).
    pub smf_abs: Option<PathBuf>,
    /// Archive-relative path to the `.smf` file (e.g. `maps/mymap.smf`).
    pub smf_rel: Option<PathBuf>,
    /// Absolute path to the first `.smt` file found (if any).
    pub smt_abs: Option<PathBuf>,
    /// Archive-relative path to the `.smt` file.
    pub smt_rel: Option<PathBuf>,
    /// Tile grid dimensions `(tiles_x, tiles_y)` read from the SMF header.
    pub tile_grid: Option<(u32, u32)>,
    /// Heightmap pixel dimensions read from the SMF header (`map_x + 1` × `map_y + 1`).
    /// `None` when no SMF file is present.
    pub map_dims: Option<(u32, u32)>,
    /// Terrain height range from the SMF header (world units, same coordinate space as X/Z).
    /// Used to compute an accurate vertical scale for the 3D preview.
    /// `None` when no SMF file is present.
    pub height_range: Option<(f32, f32)>,
    /// All other files as `(absolute_path, archive_relative_path)` pairs.
    pub passthrough_files: Vec<(PathBuf, PathBuf)>,
}
