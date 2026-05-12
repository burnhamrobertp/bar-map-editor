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
/// Creates PaintedHeightmap nodes for the heightmap, metalmap, and typemap
/// (data pre-extracted and hex-encoded by the engine layer), a PaintedTexture
/// for the assembled SMT texture, a NormalMap, a PassThrough for ancillary
/// files, and a Bundler. No external file dependencies remain after this.
pub fn scan_to_project(scan: &WorkDirScan) -> Project {
    let source_x = 80.0_f32;
    let bundler_x = 700.0_f32;
    let nm_x = (source_x + 165.0 + bundler_x) / 2.0;

    let mut nodes: Vec<RecipeNode> = Vec::new();
    let mut connections: Vec<RecipeConnection> = Vec::new();
    let mut node_positions: HashMap<String, Position> = HashMap::new();
    let mut node_sizes: HashMap<String, NodeSize> = HashMap::new();

    // Passthrough covers ancillary files only (mapinfo.lua, scripts, sounds,
    // etc.). The original .smf/.smt are NOT included -- they are regenerated
    // by the Bundler from the embedded node data below.
    let passthrough_entries: Vec<(PathBuf, PathBuf)> = scan.passthrough_files.clone();

    let map_info_file: Option<String> = passthrough_entries
        .iter()
        .map(|(_, rel)| rel.to_string_lossy().replace('\\', "/"))
        .find(|p| p.eq_ignore_ascii_case("mapinfo.lua"));

    // Heightmap node (PaintedHeightmap with embedded data)
    let has_heightmap = !scan.heightmap_hex.is_empty();
    if has_heightmap {
        let mut params = HashMap::new();
        params.insert(
            "data".to_string(),
            ParamValue::String(scan.heightmap_hex.clone()),
        );
        params.insert(
            "resolution".to_string(),
            ParamValue::UInt(scan.heightmap_res),
        );
        nodes.push(RecipeNode {
            key: "hm".to_string(),
            node_type: NodeType::PaintedHeightmap,
            label: "Heightmap".to_string(),
            params,
        });
        node_positions.insert(
            "hm".to_string(),
            Position {
                x: source_x,
                y: 80.0,
            },
        );
        node_sizes.insert(
            "hm".to_string(),
            NodeSize {
                width: 165.0,
                height: 80.0,
            },
        );
    }

    // Metalmap node
    if !scan.metalmap_hex.is_empty() {
        let mut params = HashMap::new();
        params.insert(
            "data".to_string(),
            ParamValue::String(scan.metalmap_hex.clone()),
        );
        params.insert(
            "resolution".to_string(),
            ParamValue::UInt(scan.metalmap_res),
        );
        nodes.push(RecipeNode {
            key: "metal".to_string(),
            node_type: NodeType::PaintedHeightmap,
            label: "Metal Map".to_string(),
            params,
        });
        node_positions.insert(
            "metal".to_string(),
            Position {
                x: source_x,
                y: 220.0,
            },
        );
        node_sizes.insert(
            "metal".to_string(),
            NodeSize {
                width: 165.0,
                height: 80.0,
            },
        );
    }

    // Typemap node
    if !scan.typemap_hex.is_empty() {
        let mut params = HashMap::new();
        params.insert(
            "data".to_string(),
            ParamValue::String(scan.typemap_hex.clone()),
        );
        params.insert("resolution".to_string(), ParamValue::UInt(scan.typemap_res));
        nodes.push(RecipeNode {
            key: "type".to_string(),
            node_type: NodeType::PaintedHeightmap,
            label: "Type Map".to_string(),
            params,
        });
        node_positions.insert(
            "type".to_string(),
            Position {
                x: source_x,
                y: 360.0,
            },
        );
        node_sizes.insert(
            "type".to_string(),
            NodeSize {
                width: 165.0,
                height: 80.0,
            },
        );
    }

    // Texture node (PaintedTexture with embedded RGB data)
    let has_texture = !scan.texture_hex.is_empty();
    if has_texture {
        let mut params = HashMap::new();
        params.insert(
            "data".to_string(),
            ParamValue::String(scan.texture_hex.clone()),
        );
        nodes.push(RecipeNode {
            key: "tex".to_string(),
            node_type: NodeType::PaintedTexture,
            label: "Texture".to_string(),
            params,
        });
        node_positions.insert(
            "tex".to_string(),
            Position {
                x: source_x,
                y: 500.0,
            },
        );
        node_sizes.insert(
            "tex".to_string(),
            NodeSize {
                width: 165.0,
                height: 80.0,
            },
        );
    }

    // PassThrough for ancillary files (mapinfo.lua, scripts, etc.)
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
                y: 640.0,
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

    // NormalMap (derives from the heightmap)
    if has_heightmap {
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

    // Preview (only when heightmap is present)
    if has_heightmap {
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
    if has_heightmap {
        connections.push(RecipeConnection {
            from: "hm.output".to_string(),
            to: "bundler.heightmap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "hm.output".to_string(),
            to: "nm.input".to_string(),
        });
        connections.push(RecipeConnection {
            from: "nm.output".to_string(),
            to: "bundler.normalmap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "hm.output".to_string(),
            to: "preview.heightmap".to_string(),
        });
        connections.push(RecipeConnection {
            from: "nm.output".to_string(),
            to: "preview.normal_map".to_string(),
        });
    }
    if !scan.metalmap_hex.is_empty() {
        connections.push(RecipeConnection {
            from: "metal.output".to_string(),
            to: "bundler.metalmap".to_string(),
        });
    }
    if !scan.typemap_hex.is_empty() {
        connections.push(RecipeConnection {
            from: "type.output".to_string(),
            to: "bundler.typemap".to_string(),
        });
    }
    if has_texture {
        connections.push(RecipeConnection {
            from: "tex.output".to_string(),
            to: "bundler.texture".to_string(),
        });
        connections.push(RecipeConnection {
            from: "tex.output".to_string(),
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
        features: scan.features.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{NodeType, ParamValue as PV};
    use std::path::PathBuf;

    fn empty_scan(name: &str) -> WorkDirScan {
        WorkDirScan {
            work_dir: PathBuf::from("/tmp"),
            map_name: name.to_string(),
            smf_abs: None,
            smf_rel: None,
            smt_abs: None,
            smt_rel: None,
            tile_grid: None,
            map_dims: None,
            height_range: None,
            passthrough_files: Vec::new(),
            heightmap_hex: String::new(),
            heightmap_res: 0,
            metalmap_hex: String::new(),
            metalmap_res: 0,
            typemap_hex: String::new(),
            typemap_res: 0,
            texture_hex: String::new(),
            texture_res: 0,
            features: Vec::new(),
        }
    }

    fn node_keys(p: &Project) -> Vec<&str> {
        p.recipe.nodes.iter().map(|n| n.key.as_str()).collect()
    }

    #[test]
    fn empty_scan_produces_only_bundler() {
        let scan = empty_scan("test_map");
        let p = scan_to_project(&scan);
        assert_eq!(p.recipe.nodes.len(), 1);
        assert_eq!(p.recipe.nodes[0].key, "bundler");
        assert_eq!(p.recipe.nodes[0].node_type, NodeType::Bundler);
        assert!(p.recipe.connections.is_empty());
    }

    #[test]
    fn heightmap_only_adds_hm_nm_preview_not_others() {
        let mut scan = empty_scan("test");
        scan.heightmap_hex = "ff".repeat(16);
        scan.heightmap_res = 4;
        scan.map_dims = Some((512, 512));
        let p = scan_to_project(&scan);
        let keys = node_keys(&p);
        for k in ["hm", "nm", "bundler", "preview"] {
            assert!(keys.contains(&k), "missing: {k}");
        }
        for k in ["metal", "type", "tex", "pass"] {
            assert!(!keys.contains(&k), "unexpected: {k}");
        }
    }

    #[test]
    fn full_scan_all_nodes_present() {
        let mut scan = empty_scan("full");
        scan.heightmap_hex = "ab".repeat(16);
        scan.heightmap_res = 4;
        scan.metalmap_hex = "cd".repeat(16);
        scan.metalmap_res = 4;
        scan.typemap_hex = "ef".repeat(16);
        scan.typemap_res = 4;
        scan.texture_hex = "012345".repeat(16);
        scan.texture_res = 4;
        scan.passthrough_files = vec![(PathBuf::from("/tmp/a.lua"), PathBuf::from("a.lua"))];
        let p = scan_to_project(&scan);
        let keys = node_keys(&p);
        for k in [
            "hm", "metal", "type", "tex", "nm", "pass", "bundler", "preview",
        ] {
            assert!(keys.contains(&k), "missing: {k}");
        }
    }

    #[test]
    fn connections_wire_heightmap_through_nm_to_bundler() {
        let mut scan = empty_scan("wire");
        scan.heightmap_hex = "aa".repeat(16);
        scan.heightmap_res = 4;
        let p = scan_to_project(&scan);
        let froms: Vec<&str> = p
            .recipe
            .connections
            .iter()
            .map(|c| c.from.as_str())
            .collect();
        let tos: Vec<&str> = p.recipe.connections.iter().map(|c| c.to.as_str()).collect();
        assert!(froms.contains(&"hm.output"), "hm.output not in froms");
        assert!(
            tos.contains(&"bundler.heightmap"),
            "bundler.heightmap not in tos"
        );
        assert!(tos.contains(&"nm.input"), "nm.input not in tos");
        assert!(
            tos.contains(&"preview.heightmap"),
            "preview.heightmap not in tos"
        );
    }

    #[test]
    fn map_name_set_in_bundler_params() {
        let scan = empty_scan("my_cool_map");
        let p = scan_to_project(&scan);
        let bundler = p.recipe.nodes.iter().find(|n| n.key == "bundler").unwrap();
        assert!(
            matches!(
                bundler.params.get("map_name"),
                Some(PV::String(s)) if s == "my_cool_map"
            ),
            "map_name param not set correctly"
        );
    }

    #[test]
    fn height_range_propagates_to_map_settings() {
        let mut scan = empty_scan("heights");
        scan.height_range = Some((100.0, 900.0));
        let p = scan_to_project(&scan);
        assert_eq!(p.recipe.output.map_settings.min_height, 100.0);
        assert_eq!(p.recipe.output.map_settings.max_height, 900.0);
    }

    #[test]
    fn no_heightmap_skips_nm_and_preview() {
        let mut scan = empty_scan("nohm");
        scan.metalmap_hex = "ff".repeat(16);
        scan.metalmap_res = 4;
        let p = scan_to_project(&scan);
        let keys = node_keys(&p);
        assert!(!keys.contains(&"hm"), "unexpected hm");
        assert!(!keys.contains(&"nm"), "unexpected nm");
        assert!(!keys.contains(&"preview"), "unexpected preview");
        assert!(keys.contains(&"metal"), "missing metal");
        assert!(keys.contains(&"bundler"), "missing bundler");
    }

    #[test]
    fn features_propagate_to_recipe() {
        let mut scan = empty_scan("feat_test");
        scan.features = vec![crate::recipe::PlacedFeature {
            feature_type: "arborreal".to_string(),
            x: 100.0,
            y: 0.0,
            z: 200.0,
            angle: 1.57,
            taken_damage: 0,
        }];
        let p = scan_to_project(&scan);
        assert_eq!(p.recipe.features.len(), 1);
        assert_eq!(p.recipe.features[0].feature_type, "arborreal");
        assert!((p.recipe.features[0].x - 100.0).abs() < 0.001);
    }

    #[test]
    fn passthrough_files_create_pass_node_and_connect_to_bundler() {
        let mut scan = empty_scan("pass");
        scan.passthrough_files = vec![(
            PathBuf::from("/tmp/mapinfo.lua"),
            PathBuf::from("mapinfo.lua"),
        )];
        let p = scan_to_project(&scan);
        let keys = node_keys(&p);
        assert!(keys.contains(&"pass"), "missing pass");
        let has_conn = p
            .recipe
            .connections
            .iter()
            .any(|c| c.from == "pass.files" && c.to == "bundler.files");
        assert!(has_conn, "pass.files -> bundler.files connection missing");
    }
}

/// Result of scanning an extracted .sd7 work directory.
#[derive(Debug)]
pub struct WorkDirScan {
    /// Absolute path to the work directory.
    pub work_dir: PathBuf,
    /// Map name derived from the archive filename stem.
    pub map_name: String,
    /// Absolute path to the first `.smf` file found (if any). Kept for
    /// PassThrough file-list construction (other callers that need the path).
    pub smf_abs: Option<PathBuf>,
    /// Archive-relative path to the `.smf` file (e.g. `maps/mymap.smf`).
    pub smf_rel: Option<PathBuf>,
    /// Absolute path to the first `.smt` file found (if any).
    pub smt_abs: Option<PathBuf>,
    /// Archive-relative path to the `.smt` file.
    pub smt_rel: Option<PathBuf>,
    /// Tile grid dimensions `(tiles_x, tiles_y)` read from the SMF header.
    pub tile_grid: Option<(u32, u32)>,
    /// Heightmap pixel dimensions read from the SMF header (`map_x + 1` x `map_y + 1`).
    /// `None` when no SMF file is present.
    pub map_dims: Option<(u32, u32)>,
    /// Terrain height range from the SMF header (world units).
    /// `None` when no SMF file is present.
    pub height_range: Option<(f32, f32)>,
    /// All other files as `(absolute_path, archive_relative_path)` pairs.
    /// Does NOT include the .smf or .smt themselves -- those are embedded
    /// in the node data below and regenerated by the Bundler on export.
    pub passthrough_files: Vec<(PathBuf, PathBuf)>,

    // Embedded node data (pre-extracted and hex-encoded by bar-engine).
    // Empty string means the data was not available (e.g. no SMF found).
    /// Hex-encoded u8 grayscale heightmap at `heightmap_res x heightmap_res`.
    pub heightmap_hex: String,
    pub heightmap_res: u32,

    /// Hex-encoded u8 grayscale metalmap.
    pub metalmap_hex: String,
    pub metalmap_res: u32,

    /// Hex-encoded u8 grayscale typemap.
    pub typemap_hex: String,
    pub typemap_res: u32,

    /// Hex-encoded RGB (3 bytes/pixel) assembled texture at `texture_res x texture_res`.
    pub texture_hex: String,
    pub texture_res: u32,

    /// Feature placements extracted from the SMF feature section.
    pub features: Vec<crate::recipe::PlacedFeature>,
}
