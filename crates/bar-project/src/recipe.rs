//! Recipe format: a versioned, human-friendly serializable graph configuration.
//!
//! Recipes use stable string keys for nodes (not internal IDs) and validate
//! on load by constructing the graph through proper APIs.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use bar_graph::{GraphEngine, Node, NodeId, NodeType, ParamValue, PortId};

/// On-disk format version for `Recipe`. Bumped whenever a structural
/// change to the recipe schema lands that needs a migration step (a
/// renamed field, a new required field with no sensible default, a
/// reorganised section). Loaders branch on this value to apply
/// migrations; absence in older files is treated as `1` via
/// `serde(default)`.
///
/// Bump this AND add a migration in `Recipe::load`/`Recipe::from_json`
/// in the same commit. Never bump silently.
pub const RECIPE_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    1
}

/// A complete pipeline recipe — the on-disk format for the editor's graphs.
///
/// Identity fields (`name`, `shortname`, `description`, `author`,
/// `version`) live here as the **single source of truth**. The
/// bundler reads them when generating `mapinfo.lua`; nothing else
/// should keep its own copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// On-disk schema version. Use `RECIPE_SCHEMA_VERSION` for new
    /// recipes; older files without the field load as `1` via
    /// `default_schema_version`. Migrations live in the load path,
    /// not in field-level `serde(alias = …)` patches.
    #[serde(default = "default_schema_version", rename = "schema_version")]
    pub schema_version: u32,
    /// Human-readable map name. Becomes mapinfo's `name` and the
    /// stem of generated map files (`<name>.smf`, `<name>.smt`).
    pub name: String,
    /// Optional short identifier. When `None` the bundler uses
    /// `name` for mapinfo's `shortname`. Lets a long display name
    /// like "Kolmog Estuary 1v1" coexist with a tighter id like
    /// "kolmog_1v1".
    #[serde(default)]
    pub shortname: Option<String>,
    /// In-game map description (becomes mapinfo's `description`).
    /// Empty string is allowed.
    #[serde(default)]
    pub description: String,
    /// Author string for mapinfo. When `None` the bundler falls
    /// back to `"bar-editor"`.
    #[serde(default)]
    pub author: Option<String>,
    /// Map version string. Convention is "MAJOR.MINOR" or a
    /// content-flavoured tag ("v3", "playtest-2"). When `None` the
    /// bundler falls back to `"1.0"`.
    #[serde(default)]
    pub version: Option<String>,
    /// Node definitions, keyed by stable string IDs.
    pub nodes: Vec<RecipeNode>,
    /// Connections between node ports.
    pub connections: Vec<RecipeConnection>,
    /// Output configuration.
    pub output: OutputConfig,
}

/// A node in the recipe, identified by a stable string key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeNode {
    /// Stable string key used to reference this node in connections.
    pub key: String,
    /// Node type (e.g., "PerlinNoise", "Blur", "Bundler").
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Human-readable label.
    #[serde(default)]
    pub label: String,
    /// Parameters for this node.
    #[serde(default)]
    pub params: HashMap<String, ParamValue>,
}

/// A connection between two ports in the recipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeConnection {
    /// Source: "node_key.port_name"
    pub from: String,
    /// Destination: "node_key.port_name"
    pub to: String,
}

/// Output configuration for the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output width in pixels (heightmap resolution).
    pub width: u32,
    /// Output height in pixels (heightmap resolution).
    pub height: u32,
    /// Map-level settings for export (mapinfo.lua generation, DNTS, etc.)
    #[serde(default)]
    pub map_settings: MapSettings,
}

/// Map-level export settings — controls the physics / atmosphere / lighting
/// / water sections of mapinfo.lua, plus DNTS references and team spawns.
/// All fields are optional; sensible defaults are used when not specified.
///
/// Identity fields (`name`, `shortname`, `description`, `author`, `version`)
/// and map dimensions (`width`, `height`) live on the parent `Recipe` /
/// `OutputConfig`, not here — keeping them in one place keeps the mapinfo
/// editor and project save/load agreed on a single source of truth.
///
/// The bundler generates `mapinfo.lua` from `Recipe` + this struct on every
/// SD7 export; nothing else may produce a mapinfo.lua (a PassThrough or
/// FileReference with that destination is rejected at validation time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapSettings {
    /// Minimum height in Spring world units.
    pub min_height: f32,
    /// Maximum height in Spring world units.
    pub max_height: f32,
    /// Map hardness (100 = default).
    pub map_hardness: u32,
    /// Gravity constant.
    pub gravity: f32,
    /// Water damage per second.
    pub water_damage: f32,

    /// Detail Normal Texture Set (DNTS) — paths to tiling textures for each terrain type.
    /// Up to 4 entries (one per splat channel).
    #[serde(default)]
    pub detail_textures: Vec<DetailTexture>,

    /// Whether the map is deformable.
    pub deformable: bool,
    /// `voidWater` — when true, water doesn't render at all; the
    /// area below sea level is just empty space. Useful for sky-
    /// island maps.
    pub void_water: bool,
    /// `voidGround` — when true, the ground texture is replaced by
    /// transparency outside the heightmap's positive range. Niche;
    /// off by default.
    pub void_ground: bool,
    /// Tidal strength.
    pub tidal_strength: f32,
    /// Maximum metal extraction value.
    pub max_metal: f32,
    /// Extractor radius.
    pub extractor_radius: f32,

    /// Atmosphere settings.
    #[serde(default)]
    pub atmosphere: AtmosphereSettings,
    /// Lighting settings.
    #[serde(default)]
    pub lighting: LightingSettings,
    /// Water settings.
    #[serde(default)]
    pub water: WaterSettings,

    /// Team start positions as [(x, z)] in Spring world coordinates.
    /// If empty, auto-generated at 25%/75% corners.
    #[serde(default)]
    pub start_positions: Vec<[u32; 2]>,
}

/// Detail texture entry for DNTS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailTexture {
    /// Path to the diffuse/normal tiling texture (relative to map archive).
    pub path: String,
    /// Texture scale (smaller = more tiled repetition).
    #[serde(default = "default_tex_scale")]
    pub scale: f32,
}

fn default_tex_scale() -> f32 {
    0.02
}

/// Atmosphere configuration for mapinfo.lua.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AtmosphereSettings {
    pub min_wind: f32,
    pub max_wind: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_color: [f32; 3],
}

impl Default for AtmosphereSettings {
    fn default() -> Self {
        Self {
            min_wind: 5.0,
            max_wind: 25.0,
            fog_start: 0.1,
            fog_end: 1.0,
            fog_color: [0.7, 0.7, 0.8],
        }
    }
}

/// Lighting configuration for mapinfo.lua.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightingSettings {
    pub sun_dir: [f32; 3],
    pub ground_ambient: [f32; 3],
    pub ground_diffuse: [f32; 3],
    pub ground_specular: [f32; 3],
    pub spec_exponent: f32,
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self {
            sun_dir: [0.0, 1.0, 2.0],
            ground_ambient: [0.5, 0.5, 0.5],
            ground_diffuse: [0.5, 0.5, 0.5],
            ground_specular: [0.1, 0.1, 0.1],
            spec_exponent: 10.0,
        }
    }
}

/// Water configuration for mapinfo.lua.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterSettings {
    pub damage: f32,
    pub absorb: [f32; 3],
    pub base_color: [f32; 3],
    pub min_color: [f32; 3],
}

impl Default for WaterSettings {
    fn default() -> Self {
        Self {
            damage: 0.0,
            absorb: [0.0, 0.0, 0.0],
            base_color: [0.6, 0.6, 0.8],
            min_color: [0.0, 0.0, 0.0],
        }
    }
}

impl Default for MapSettings {
    fn default() -> Self {
        Self {
            min_height: 0.0,
            max_height: 800.0,
            map_hardness: 100,
            gravity: 130.0,
            water_damage: 0.0,
            detail_textures: Vec::new(),
            deformable: true,
            void_water: false,
            void_ground: false,
            tidal_strength: 0.0,
            max_metal: 0.02,
            extractor_radius: 500.0,
            atmosphere: AtmosphereSettings::default(),
            lighting: LightingSettings::default(),
            water: WaterSettings::default(),
            start_positions: Vec::new(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            // 8x8 BAR squares: (8 * 64 + 1) = 513 px, 4096 x 4096 elmos.
            width: 513,
            height: 513,
            map_settings: MapSettings::default(),
        }
    }
}

impl Recipe {
    /// Load a recipe from a JSON file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        Self::from_json(&content)
    }

    /// Parse a recipe from a JSON string.
    ///
    /// Reads `schema_version` first and refuses to load anything
    /// newer than the build understands — better to fail loudly than
    /// to silently drop unrecognised fields. Older versions are
    /// migrated in-place through `migrate_to_current`.
    pub fn from_json(json: &str) -> Result<Self> {
        let mut recipe: Self = serde_json::from_str(json).context("Failed to parse recipe JSON")?;
        if recipe.schema_version > RECIPE_SCHEMA_VERSION {
            bail!(
                "Recipe schema_version {} is newer than this build supports ({}); \
                 upgrade bar-editor to open it.",
                recipe.schema_version,
                RECIPE_SCHEMA_VERSION,
            );
        }
        recipe.migrate_to_current();
        recipe.validate()?;
        Ok(recipe)
    }

    /// Apply any field-level migrations needed to bring an older
    /// schema version up to `RECIPE_SCHEMA_VERSION`. Today there are
    /// no migrations — this is a placeholder so future bumps have an
    /// obvious home and don't regress into ad-hoc branches scattered
    /// through the load path.
    fn migrate_to_current(&mut self) {
        // v1 → vN migrations land here, oldest-first.
        self.schema_version = RECIPE_SCHEMA_VERSION;
    }

    /// Serialize this recipe to a pretty-printed JSON string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("Failed to serialize recipe")
    }

    /// Save this recipe to a file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = self.to_json()?;
        std::fs::write(path, json).with_context(|| format!("Cannot write {}", path.display()))?;
        Ok(())
    }

    /// Validate the recipe is well-formed.
    pub fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            bail!("Recipe has no nodes");
        }

        // Check for duplicate keys
        let mut keys: HashMap<&str, usize> = HashMap::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.key.is_empty() {
                bail!("Node at index {} has an empty key", i);
            }
            if let Some(prev) = keys.insert(&node.key, i) {
                bail!(
                    "Duplicate node key '{}' at indices {} and {}",
                    node.key,
                    prev,
                    i
                );
            }
        }

        // Validate connections reference existing nodes and ports
        for conn in &self.connections {
            Self::validate_port_ref(&conn.from, &keys, "from")?;
            Self::validate_port_ref(&conn.to, &keys, "to")?;
        }

        // Validate output dimensions
        if self.output.width == 0 || self.output.height == 0 {
            bail!("Output dimensions must be > 0");
        }

        // Validate every node's params against its schema. Type
        // mismatches are hard errors — a hand-edited recipe with the
        // wrong type would otherwise silently substitute a default
        // and produce wrong-but-not-broken evaluations. Unknown keys
        // are tolerated for now (they fall through to evaluation
        // unread); see `param_spec` module docs for the rationale.
        for (i, node) in self.nodes.iter().enumerate() {
            for err in bar_graph::validate_node_params(&node.node_type, &node.params) {
                if let bar_graph::ParamError::TypeMismatch {
                    key, expected, got, ..
                } = err
                {
                    bail!(
                        "Node {}/{} ({:?}): param `{}` is {:?}, expected {:?}",
                        i,
                        node.key,
                        node.node_type,
                        key,
                        got,
                        expected,
                    );
                }
            }
        }

        Ok(())
    }

    fn validate_port_ref(
        port_ref: &str,
        keys: &HashMap<&str, usize>,
        direction: &str,
    ) -> Result<()> {
        let parts: Vec<&str> = port_ref.splitn(2, '.').collect();
        if parts.len() != 2 {
            bail!(
                "Invalid {} port reference '{}': expected 'node_key.port_name'",
                direction,
                port_ref
            );
        }
        let node_key = parts[0];
        let port_name = parts[1];

        if !keys.contains_key(node_key) {
            bail!(
                "Connection {} references unknown node '{}'",
                direction,
                node_key
            );
        }
        if port_name.is_empty() {
            bail!(
                "Connection {} has empty port name for node '{}'",
                direction,
                node_key
            );
        }
        Ok(())
    }

    /// Build a `GraphEngine` from this recipe by constructing nodes and connections
    /// through the validated APIs.
    pub fn build_graph(&self) -> Result<GraphEngine> {
        let mut graph = GraphEngine::new();
        let mut key_to_id: HashMap<&str, NodeId> = HashMap::new();

        // Add nodes
        for recipe_node in &self.nodes {
            let label = if recipe_node.label.is_empty() {
                &recipe_node.key
            } else {
                &recipe_node.label
            };
            // Node::new pre-populates `params` with the type's default
            // values; recipe-specified params merge on top so explicit
            // values win and missing keys fall back to sensible defaults.
            let mut node = Node::new(NodeId(0), recipe_node.node_type.clone(), label);
            for (k, v) in recipe_node.params.iter() {
                node.params.insert(k.clone(), v.clone());
            }
            if node.node_type == NodeType::TextureWeightmap {
                if let Some(ParamValue::UInt(lc)) = node.params.get("layer_count") {
                    node.resize_texture_weightmap_ports(*lc);
                }
            }
            if matches!(
                node.node_type,
                NodeType::SubgraphInput | NodeType::SubgraphOutput
            ) {
                node.sync_subgraph_io_kind();
            }
            let id = graph.add_node(node);
            key_to_id.insert(&recipe_node.key, id);
        }

        // Add connections
        for conn in &self.connections {
            let (from_key, from_port) = parse_port_ref(&conn.from)?;
            let (to_key, to_port) = parse_port_ref(&conn.to)?;

            let from_id = key_to_id
                .get(from_key)
                .with_context(|| format!("Unknown node key in connection: '{}'", from_key))?;
            let to_id = key_to_id
                .get(to_key)
                .with_context(|| format!("Unknown node key in connection: '{}'", to_key))?;

            graph
                .connect(
                    PortId {
                        node_id: *from_id,
                        port_name: from_port.to_string(),
                    },
                    PortId {
                        node_id: *to_id,
                        port_name: to_port.to_string(),
                    },
                )
                .with_context(|| {
                    format!(
                        "Failed to connect {}.{} → {}.{}",
                        from_key, from_port, to_key, to_port
                    )
                })?;
        }

        // Verify no cycles
        graph
            .topological_sort()
            .context("Recipe graph contains a cycle")?;

        Ok(graph)
    }

    /// Generate a sample recipe demonstrating the format.
    pub fn sample() -> Self {
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "Sample Terrain".to_string(),
            shortname: None,
            description: "A basic ridged noise terrain with blur smoothing".to_string(),
            author: None,
            version: None,
            nodes: vec![
                RecipeNode {
                    key: "base_terrain".to_string(),
                    node_type: NodeType::RidgedNoise,
                    label: "Base Terrain".to_string(),
                    params: HashMap::from([
                        ("frequency".to_string(), ParamValue::Float(3.0)),
                        ("octaves".to_string(), ParamValue::UInt(6)),
                        ("lacunarity".to_string(), ParamValue::Float(2.0)),
                        ("seed".to_string(), ParamValue::UInt(42)),
                    ]),
                },
                RecipeNode {
                    key: "detail".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: "Detail Noise".to_string(),
                    params: HashMap::from([
                        ("frequency".to_string(), ParamValue::Float(8.0)),
                        ("octaves".to_string(), ParamValue::UInt(4)),
                        ("persistence".to_string(), ParamValue::Float(0.4)),
                        ("seed".to_string(), ParamValue::UInt(7)),
                    ]),
                },
                RecipeNode {
                    key: "blend".to_string(),
                    node_type: NodeType::Blend,
                    label: "Blend".to_string(),
                    params: HashMap::from([("factor".to_string(), ParamValue::Float(0.3))]),
                },
                RecipeNode {
                    key: "smooth".to_string(),
                    node_type: NodeType::Blur,
                    label: "Smooth".to_string(),
                    params: HashMap::from([("radius".to_string(), ParamValue::Float(1.5))]),
                },
                RecipeNode {
                    key: "output".to_string(),
                    node_type: NodeType::Bundler,
                    label: "Export".to_string(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![
                RecipeConnection {
                    from: "base_terrain.output".to_string(),
                    to: "blend.a".to_string(),
                },
                RecipeConnection {
                    from: "detail.output".to_string(),
                    to: "blend.b".to_string(),
                },
                RecipeConnection {
                    from: "blend.output".to_string(),
                    to: "smooth.input".to_string(),
                },
                RecipeConnection {
                    from: "smooth.output".to_string(),
                    to: "output.heightmap".to_string(),
                },
            ],
            output: OutputConfig {
                width: 257,
                height: 257,
                map_settings: MapSettings::default(),
            },
        }
    }
}

fn parse_port_ref(s: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    if parts.len() != 2 {
        bail!("Invalid port reference '{}': expected 'node.port'", s);
    }
    Ok((parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_recipe_validates() {
        let recipe = Recipe::sample();
        recipe.validate().unwrap();
    }

    #[test]
    fn test_sample_recipe_roundtrip_json() {
        let recipe = Recipe::sample();
        let json = recipe.to_json().unwrap();
        let parsed = Recipe::from_json(&json).unwrap();
        assert_eq!(parsed.name, "Sample Terrain");
        assert_eq!(parsed.nodes.len(), 5);
        assert_eq!(parsed.connections.len(), 4);
    }

    #[test]
    fn test_sample_recipe_builds_graph() {
        let recipe = Recipe::sample();
        let graph = recipe.build_graph().unwrap();
        assert_eq!(graph.nodes().len(), 5);
        assert_eq!(graph.connections().len(), 4);
        // Topological sort should work (no cycles)
        let order = graph.topological_sort().unwrap();
        assert_eq!(order.len(), 5);
    }

    #[test]
    fn test_invalid_recipe_zero_dimensions() {
        let recipe = Recipe {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "Bad".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            nodes: vec![RecipeNode {
                key: "noise".to_string(),
                node_type: NodeType::PerlinNoise,
                label: String::new(),
                params: HashMap::new(),
            }],
            connections: vec![],
            output: OutputConfig {
                width: 0,
                height: 0,
                map_settings: MapSettings::default(),
            },
        };
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn test_invalid_recipe_bad_connection() {
        let recipe = Recipe {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "Bad".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            nodes: vec![
                RecipeNode {
                    key: "noise".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: String::new(),
                    params: HashMap::new(),
                },
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::Bundler,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![RecipeConnection {
                from: "nonexistent.output".to_string(),
                to: "out.heightmap".to_string(),
            }],
            output: OutputConfig::default(),
        };
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn test_invalid_recipe_duplicate_keys() {
        let recipe = Recipe {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "Bad".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            nodes: vec![
                RecipeNode {
                    key: "dupe".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: String::new(),
                    params: HashMap::new(),
                },
                RecipeNode {
                    key: "dupe".to_string(),
                    node_type: NodeType::Bundler,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![],
            output: OutputConfig::default(),
        };
        assert!(recipe.validate().is_err());
    }

    #[test]
    fn schema_version_defaults_to_one_when_absent() {
        // Old recipes (pre-versioning) had no `schema_version`
        // field; serde's `default` should fill it in as 1 so the
        // load path doesn't reject them.
        let json = r#"{
            "name": "legacy",
            "nodes": [{"key": "n", "type": "PerlinNoise", "params": {}}],
            "connections": [],
            "output": {"width": 256, "height": 256}
        }"#;
        let recipe = Recipe::from_json(json).expect("legacy recipe should load");
        assert_eq!(recipe.schema_version, RECIPE_SCHEMA_VERSION);
    }

    #[test]
    fn schema_version_newer_than_build_is_rejected() {
        // Loaders must refuse to silently drop unknown fields when
        // the file declares a schema newer than the build supports.
        let json = format!(
            r#"{{
                "schema_version": {},
                "name": "future",
                "nodes": [{{"key": "n", "type": "PerlinNoise", "params": {{}}}}],
                "connections": [],
                "output": {{"width": 256, "height": 256}}
            }}"#,
            RECIPE_SCHEMA_VERSION + 1,
        );
        let err = Recipe::from_json(&json).expect_err("newer schema must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("schema_version") && msg.contains("newer"),
            "error should mention schema_version + newer; got: {msg}",
        );
    }

    #[test]
    fn recipe_with_wrong_typed_param_is_rejected() {
        // A hand-edited recipe with `Blur.radius` typed as a String
        // (it's `Float`) used to silently fall back to the default
        // and produce wrong-but-not-broken evaluations. Now it
        // refuses to load with a clear error citing the node + key.
        // `ParamValue` serialises with external tagging
        // (`{"Float": 1.5}` etc.), so a String-typed value is
        // `{"String": "..."}` — that *parses* fine but fails the
        // schema validator we wired into `Recipe::validate`.
        let json = r#"{
            "schema_version": 1,
            "name": "typo",
            "nodes": [
                {"key": "n", "type": "Blur",
                 "params": {"radius": {"String": "definitely_not_a_float"}}}
            ],
            "connections": [],
            "output": {"width": 256, "height": 256}
        }"#;
        let err = Recipe::from_json(json).expect_err("type mismatch must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("radius") && msg.contains("Blur"),
            "error should cite the offending node + key; got: {msg}",
        );
    }

    #[test]
    fn recipe_with_unknown_param_loads_anyway() {
        // Unknown keys are tolerated — old projects with deprecated
        // params should still load. (See `param_spec` module docs;
        // this becomes strict if we ever bump the schema_version
        // and want to enforce it.)
        let json = r#"{
            "schema_version": 1,
            "name": "extra",
            "nodes": [
                {"key": "n", "type": "Blur",
                 "params": {
                    "radius": {"Float": 1.5},
                    "totally_legacy_param": {"UInt": 42}
                 }}
            ],
            "connections": [],
            "output": {"width": 256, "height": 256}
        }"#;
        Recipe::from_json(json).expect("unknown keys must not block load");
    }

    #[test]
    fn schema_version_round_trips_through_save_load() {
        let recipe = Recipe::sample();
        assert_eq!(recipe.schema_version, RECIPE_SCHEMA_VERSION);
        let json = recipe.to_json().unwrap();
        let loaded = Recipe::from_json(&json).unwrap();
        assert_eq!(loaded.schema_version, RECIPE_SCHEMA_VERSION);
    }

    // Recipe round-trip scenarios: these mirror the three ways a user
    // creates or opens a project in the editor.

    /// A manually-built recipe (no preset, no SD7) round-trips through
    /// JSON and can have its graph evaluated without errors.
    #[test]
    fn manual_project_recipe_roundtrip() {
        let recipe = Recipe {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "Manual test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            nodes: vec![
                RecipeNode {
                    key: "noise".to_string(),
                    node_type: NodeType::PerlinNoise,
                    label: "Noise".to_string(),
                    params: HashMap::new(),
                },
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::Bundler,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![RecipeConnection {
                from: "noise.output".to_string(),
                to: "out.heightmap".to_string(),
            }],
            output: OutputConfig {
                width: 513,
                height: 513,
                map_settings: MapSettings::default(),
            },
        };
        let json = recipe.to_json().unwrap();
        let loaded = Recipe::from_json(&json).unwrap();
        let graph = loaded.build_graph().unwrap();
        assert_eq!(graph.nodes().len(), 2);
        assert_eq!(graph.connections().len(), 1);
    }

    /// Preset project with semantic kind strings (display labels rather than
    /// PortKind names) round-trips and builds without IncompatiblePorts.
    #[test]
    fn preset_project_semantic_kind_roundtrip() {
        // SubgraphOutput kind params written as display labels ("Texture",
        // "Output", "Slope") must survive load via the io_value_bypass path.
        let make_subout = |key: &str, kind: &str| RecipeNode {
            key: key.to_string(),
            node_type: NodeType::SubgraphOutput,
            label: String::new(),
            params: {
                let mut p = HashMap::new();
                p.insert("kind".to_string(), ParamValue::String(kind.to_string()));
                p
            },
        };
        let recipe = Recipe {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "Alpine 8x8".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            nodes: vec![
                make_subout("sub_terrain", "Output"), // was "Heightmap" before recompute ran
                make_subout("sub_texture", "Texture"), // was "Color"
                make_subout("sub_slope", "Slope"),    // was "Heightmap"
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::Bundler,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![
                RecipeConnection {
                    from: "sub_terrain.value".to_string(),
                    to: "out.heightmap".to_string(),
                },
                RecipeConnection {
                    from: "sub_texture.value".to_string(),
                    to: "out.texture".to_string(),
                },
                RecipeConnection {
                    from: "sub_slope.value".to_string(),
                    to: "out.normalmap".to_string(),
                },
            ],
            // Simulates the user changing width/height before saving.
            output: OutputConfig {
                width: 1025,
                height: 1025,
                map_settings: MapSettings::default(),
            },
        };
        let json = recipe.to_json().unwrap();
        let loaded = Recipe::from_json(&json).unwrap();
        // Verify size change survives the round-trip.
        assert_eq!(loaded.output.width, 1025);
        assert_eq!(loaded.output.height, 1025);
        // Must not error with IncompatiblePorts despite semantic kind strings.
        let graph = loaded.build_graph().unwrap();
        assert_eq!(graph.connections().len(), 3);
    }

    /// A recipe that mirrors the SD7 import pipeline (SmfImport + Bundler)
    /// round-trips through JSON and builds its graph cleanly.
    #[test]
    fn sd7_import_style_recipe_roundtrip() {
        let recipe = Recipe {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "SD7 import test".to_string(),
            shortname: None,
            description: String::new(),
            author: None,
            version: None,
            nodes: vec![
                RecipeNode {
                    key: "smf".to_string(),
                    node_type: NodeType::SmfImport,
                    label: "SMF".to_string(),
                    params: {
                        let mut p = HashMap::new();
                        p.insert(
                            "path".to_string(),
                            ParamValue::String("/tmp/test.smf".to_string()),
                        );
                        p
                    },
                },
                RecipeNode {
                    key: "out".to_string(),
                    node_type: NodeType::Bundler,
                    label: String::new(),
                    params: HashMap::new(),
                },
            ],
            connections: vec![RecipeConnection {
                from: "smf.heightmap".to_string(),
                to: "out.heightmap".to_string(),
            }],
            output: OutputConfig {
                width: 513,
                height: 513,
                map_settings: MapSettings::default(),
            },
        };
        let json = recipe.to_json().unwrap();
        let loaded = Recipe::from_json(&json).unwrap();
        let graph = loaded.build_graph().unwrap();
        assert_eq!(graph.nodes().len(), 2);
        assert_eq!(graph.connections().len(), 1);
    }
}
