//! Project-level pre-export validation.
//!
//! These checks run on the in-memory graph + project state, *without*
//! triggering a full evaluation pipeline. They catch problems that would
//! either prevent export from succeeding (no Bundler node, illegal map
//! dimensions) or produce a map that won't load in BAR (start positions
//! outside playable area, missing mapinfo.lua references). Codec-side
//! validation (`SpringSmfCodec::validate`) still runs at export time
//! and catches things that depend on the actual layer data.
//!
//! Runs cheap; safe to call every frame if needed. The GUI's validation
//! panel calls this, surfaces findings, and gates the export button on
//! "no errors" (warnings are advisory).

use crate::recipe::MapSettings;
use bar_graph::{GraphEngine, NodeId, NodeType, ParamValue, PortPlacement};
use std::collections::{HashMap, HashSet};

/// Severity of a validation finding. Errors block export; warnings and
/// info are informational.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// One finding produced by a validator.
#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    /// Short category for grouping in the UI ("dimensions", "physics",
    /// "atmosphere", "startboxes", …). Lowercase, snake-case-ish.
    /// For Map Settings tabs, this matches the tab's lowercase id so
    /// the modal can decorate the right tab.
    pub category: String,
    /// Optional field within the category. When combined with a category,
    /// e.g. category "physics" + field "gravity", the modal can draw a
    /// per-control outline rather than only flagging the whole tab.
    pub field: Option<String>,
    pub message: String,
}

impl Finding {
    fn err(category: &str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            category: category.to_string(),
            field: None,
            message: message.into(),
        }
    }
    fn warn(category: &str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            category: category.to_string(),
            field: None,
            message: message.into(),
        }
    }
    #[allow(dead_code)]
    fn info(category: &str, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            category: category.to_string(),
            field: None,
            message: message.into(),
        }
    }
    /// Tag this finding to a specific field inside its category. Used by
    /// the Map Settings modal to draw a per-control outline.
    fn on_field(mut self, field: &str) -> Self {
        self.field = Some(field.to_string());
        self
    }
}

/// Run all project-level validators against the current graph and
/// project settings. The result is a flat list — empty means "ready
/// to export".
pub fn validate_project(
    graph: &GraphEngine,
    settings: &MapSettings,
    map_width: u32,
    map_height: u32,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    check_bundler(graph, &mut findings);
    check_bundler_inputs(graph, &mut findings);
    check_cycles(graph, &mut findings);
    check_input_sources_present(graph, &mut findings);
    check_disconnected_filter_inputs(graph, &mut findings);
    check_orphaned_nodes(graph, &mut findings);
    check_dimensions(map_width, map_height, &mut findings);
    check_height_range(settings, &mut findings);
    check_start_positions(settings, map_width, map_height, &mut findings);
    check_physics(settings, &mut findings);
    check_atmosphere(settings, &mut findings);
    check_lighting(settings, &mut findings);
    check_water(settings, &mut findings);
    check_passthrough_paths(graph, &mut findings);
    check_passthrough_collisions(graph, &mut findings);
    check_mapinfo_lua_collisions(graph, &mut findings);
    findings
}

/// Whether a node type produces data without consuming any — i.e. it
/// has no inputs and is the start of any pipeline. The eval engine
/// determines this from the node's input port list, but the
/// validator works off the node's *type* so it can flag missing
/// sources in a graph that wouldn't even pass topo sort.
fn is_source_node(t: &NodeType) -> bool {
    matches!(
        t,
        NodeType::PerlinNoise
            | NodeType::SimplexNoise
            | NodeType::WorleyNoise
            | NodeType::RidgedNoise
            | NodeType::Constant
            | NodeType::FileInput
            | NodeType::Voronoi
            | NodeType::Gradient
            | NodeType::SmfImport
            | NodeType::SmtImport
            | NodeType::PaintedHeightmap
            | NodeType::PaintedTexture
    )
}

/// True if the findings contain any blocking errors.
pub fn has_errors(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Error)
}

// ── Individual validators ───────────────────────────────────────────────

/// At least one Bundler node must exist; otherwise there's nothing to
/// export. Inform the user instead of letting them click Export and get
/// silence.
fn check_bundler(graph: &GraphEngine, out: &mut Vec<Finding>) {
    let bundler_count = graph
        .nodes()
        .values()
        .filter(|n| n.node_type == NodeType::Bundler)
        .count();
    if bundler_count == 0 {
        out.push(Finding::err(
            "bundler",
            "No Bundler node in the graph. Add one to enable export.",
        ));
    } else if bundler_count > 1 {
        out.push(Finding::info(
            "bundler",
            format!("{bundler_count} Bundler nodes — each will produce a separate SD7."),
        ));
    }
}

/// Spring's SMF format requires `mapx` and `mapy` (heightmap dim - 1) to
/// be a multiple of 64 and within engine-supported bounds. The user-
/// visible map_width/map_height we work with is the heightmap dim, so
/// the mapx/mapy = dim - 1 transform is implicit.
fn check_dimensions(map_w: u32, map_h: u32, out: &mut Vec<Finding>) {
    // Map width corresponds to the "Width" field; height to "Depth"
    // in the modal. The field tags below let the modal draw the
    // outline on the correct control.
    for (axis, dim, field) in [("width", map_w, "width"), ("depth", map_h, "depth")] {
        if dim < 65 {
            out.push(
                Finding::err(
                    "dimensions",
                    format!("Map {axis} {dim} is too small. SMF requires at least 64 squares (i.e. dim ≥ 65)."),
                )
                .on_field(field),
            );
            continue;
        }
        if dim > 32_769 {
            out.push(
                Finding::err(
                    "dimensions",
                    format!("Map {axis} {dim} exceeds SMF's 32 768-square limit."),
                )
                .on_field(field),
            );
            continue;
        }
        let mapx = dim - 1;
        if mapx % 64 != 0 {
            out.push(
                Finding::err(
                    "dimensions",
                    format!(
                        "Map {axis} {dim}: SMF requires (dim - 1) to be a multiple of 64; got {mapx}."
                    ),
                )
                .on_field(field),
            );
        }
    }
}

/// `min_height` < `max_height` and the spread can't be zero. A map
/// with min == max is unplayable and confuses the engine.
fn check_height_range(settings: &MapSettings, out: &mut Vec<Finding>) {
    let min = settings.min_height;
    let max = settings.max_height;
    if !(min.is_finite() && max.is_finite()) {
        out.push(
            Finding::err("dimensions", "Min / max height must be finite numbers.")
                .on_field("min_height"),
        );
        return;
    }
    if max <= min {
        out.push(
            Finding::err(
                "dimensions",
                format!("Max height ({max}) must be greater than min height ({min})."),
            )
            .on_field("max_height"),
        );
        return;
    }
    let span = max - min;
    if span < 8.0 {
        out.push(
            Finding::warn(
                "dimensions",
                format!("Height span {span} elmos is unusually small; expect a near-flat map."),
            )
            .on_field("max_height"),
        );
    }
    if span > 4096.0 {
        out.push(
            Finding::warn(
                "dimensions",
                format!("Height span {span} elmos is unusually large; double-check this is intentional."),
            )
            .on_field("max_height"),
        );
    }
}

/// Start positions must fall inside the playable area (in elmo units,
/// where 1 heightmap pixel = 8 elmos). We can't (yet) detect water
/// vs. land here without sampling the heightmap, but range checks are
/// cheap and catch transposed coordinates.
fn check_start_positions(settings: &MapSettings, map_w: u32, map_h: u32, out: &mut Vec<Finding>) {
    let world_w = (map_w.saturating_sub(1)) * 8;
    let world_h = (map_h.saturating_sub(1)) * 8;

    if settings.start_positions.is_empty() {
        out.push(
            Finding::warn(
                "startboxes",
                "No start positions defined. BAR will use default fallback spawns.",
            )
            .on_field("start_positions"),
        );
        return;
    }

    for (i, [x, z]) in settings.start_positions.iter().enumerate() {
        if *x > world_w {
            out.push(
                Finding::err(
                    "startboxes",
                    format!("Spawn {i} x={x} is outside map width {world_w} (elmos)."),
                )
                .on_field("start_positions"),
            );
        }
        if *z > world_h {
            out.push(
                Finding::err(
                    "startboxes",
                    format!("Spawn {i} z={z} is outside map depth {world_h} (elmos)."),
                )
                .on_field("start_positions"),
            );
        }
        // Edge proximity — closer than 256 elmos to any edge tends to
        // place units outside the visible map area in BAR.
        let edge = 256u32;
        if *x < edge
            || *x > world_w.saturating_sub(edge)
            || *z < edge
            || *z > world_h.saturating_sub(edge)
        {
            out.push(
                Finding::warn(
                    "startboxes",
                    format!(
                        "Spawn {i} ({x}, {z}) is within {edge} elmos of a map edge — units may spawn off-map."
                    ),
                )
                .on_field("start_positions"),
            );
        }
    }
}

/// Sanity-check the physics block: things that crash the engine or
/// produce nonsensical gameplay if zero/negative.
fn check_physics(settings: &MapSettings, out: &mut Vec<Finding>) {
    if settings.gravity <= 0.0 {
        out.push(
            Finding::err(
                "physics",
                format!("Gravity {} must be > 0.", settings.gravity),
            )
            .on_field("gravity"),
        );
    }
    if settings.map_hardness == 0 {
        out.push(
            Finding::warn(
                "physics",
                "Map hardness 0 means terrain takes infinite damage to deform — usually a mistake.",
            )
            .on_field("map_hardness"),
        );
    }
    if settings.tidal_strength < 0.0 {
        out.push(
            Finding::err(
                "physics",
                format!(
                    "Tidal strength {} can't be negative.",
                    settings.tidal_strength
                ),
            )
            .on_field("tidal_strength"),
        );
    }
    if settings.max_metal < 0.0 {
        out.push(
            Finding::err(
                "physics",
                format!("Max metal {} can't be negative.", settings.max_metal),
            )
            .on_field("max_metal"),
        );
    }
    if settings.extractor_radius <= 0.0 {
        out.push(
            Finding::err(
                "physics",
                format!(
                    "Extractor radius {} must be > 0.",
                    settings.extractor_radius
                ),
            )
            .on_field("extractor_radius"),
        );
    }
    if settings.water_damage < 0.0 {
        out.push(
            Finding::err(
                "physics",
                format!("Water damage {} can't be negative.", settings.water_damage),
            )
            .on_field("water_damage"),
        );
    }
}

/// Atmosphere block: wind range ordering and fog range ordering. Engine
/// silently accepts inverted ranges but the visual result is wrong.
fn check_atmosphere(settings: &MapSettings, out: &mut Vec<Finding>) {
    let atm = &settings.atmosphere;
    if atm.min_wind > atm.max_wind {
        out.push(
            Finding::err(
                "atmosphere",
                format!(
                    "Min wind ({}) is greater than max wind ({}).",
                    atm.min_wind, atm.max_wind
                ),
            )
            .on_field("max_wind"),
        );
    }
    if atm.min_wind < 0.0 {
        out.push(
            Finding::err(
                "atmosphere",
                format!("Min wind {} can't be negative.", atm.min_wind),
            )
            .on_field("min_wind"),
        );
    }
    if !(0.0..=1.0).contains(&atm.fog_start) {
        out.push(
            Finding::err(
                "atmosphere",
                format!("Fog start {} must be between 0 and 1.", atm.fog_start),
            )
            .on_field("fog_start"),
        );
    }
    if !(0.0..=1.0).contains(&atm.fog_end) {
        out.push(
            Finding::err(
                "atmosphere",
                format!("Fog end {} must be between 0 and 1.", atm.fog_end),
            )
            .on_field("fog_end"),
        );
    }
    if atm.fog_start > atm.fog_end {
        out.push(
            Finding::err(
                "atmosphere",
                format!(
                    "Fog start ({}) is greater than fog end ({}).",
                    atm.fog_start, atm.fog_end
                ),
            )
            .on_field("fog_end"),
        );
    }
    for (i, c) in atm.fog_color.iter().enumerate() {
        if !(0.0..=1.0).contains(c) {
            out.push(
                Finding::warn(
                    "atmosphere",
                    format!("Fog colour channel {i} = {c} is outside [0, 1]."),
                )
                .on_field("fog_color"),
            );
        }
    }
}

/// Lighting block: sun direction must not be the zero vector and the
/// specular exponent must be positive (engine clamps but warn).
fn check_lighting(settings: &MapSettings, out: &mut Vec<Finding>) {
    let lit = &settings.lighting;
    let sun_mag2 = lit.sun_dir[0] * lit.sun_dir[0]
        + lit.sun_dir[1] * lit.sun_dir[1]
        + lit.sun_dir[2] * lit.sun_dir[2];
    if sun_mag2 <= f32::EPSILON {
        out.push(
            Finding::err(
                "lighting",
                "Sun direction is the zero vector — the engine can't normalise it.",
            )
            .on_field("sun_dir"),
        );
    }
    if lit.spec_exponent <= 0.0 {
        out.push(
            Finding::err(
                "lighting",
                format!("Specular exponent {} must be > 0.", lit.spec_exponent),
            )
            .on_field("spec_exponent"),
        );
    }
    let color_check = |out: &mut Vec<Finding>, label: &str, field: &str, c: &[f32; 3]| {
        for (i, v) in c.iter().enumerate() {
            if !(0.0..=1.0).contains(v) {
                out.push(
                    Finding::warn(
                        "lighting",
                        format!("{label} channel {i} = {v} is outside [0, 1]."),
                    )
                    .on_field(field),
                );
            }
        }
    };
    color_check(out, "Ground ambient", "ground_ambient", &lit.ground_ambient);
    color_check(out, "Ground diffuse", "ground_diffuse", &lit.ground_diffuse);
    color_check(
        out,
        "Ground specular",
        "ground_specular",
        &lit.ground_specular,
    );
}

/// Water block — non-negative damage, colours in range.
fn check_water(settings: &MapSettings, out: &mut Vec<Finding>) {
    let w = &settings.water;
    if w.damage < 0.0 {
        out.push(
            Finding::err(
                "water",
                format!("Water damage {} can't be negative.", w.damage),
            )
            .on_field("damage"),
        );
    }
    let color_check = |out: &mut Vec<Finding>, label: &str, field: &str, c: &[f32; 3]| {
        for (i, v) in c.iter().enumerate() {
            if !(0.0..=1.0).contains(v) {
                out.push(
                    Finding::warn(
                        "water",
                        format!("{label} channel {i} = {v} is outside [0, 1]."),
                    )
                    .on_field(field),
                );
            }
        }
    };
    color_check(out, "Absorb", "absorb", &w.absorb);
    color_check(out, "Base colour", "base_color", &w.base_color);
    color_check(out, "Min colour", "min_color", &w.min_color);
}

/// PassThrough nodes carry source paths that must exist on disk at
/// export time. The bundle path (after `|`) was already validated at
/// the codec level; here we catch the "the source file vanished"
/// case before the export thread fails.
fn check_passthrough_paths(graph: &GraphEngine, out: &mut Vec<Finding>) {
    for (id, node) in graph.nodes() {
        if node.node_type != NodeType::PassThrough {
            continue;
        }
        let Some(ParamValue::String(s)) = node.params.get("files") else {
            continue;
        };
        for line in s.lines() {
            let mut parts = line.splitn(2, '|');
            let abs = parts.next().unwrap_or("").trim();
            if abs.is_empty() || abs.starts_with("bar://") {
                // `bar://` is the project-relative scheme; resolution
                // happens at evaluation time. Skip here.
                continue;
            }
            if !std::path::Path::new(abs).exists() {
                out.push(Finding::err(
                    "files",
                    format!("PassThrough node {} references missing file: {abs}", id.0),
                ));
            }
        }
    }
}

/// Every Bundler must have at least one input connected; an empty
/// Bundler exports an empty SD7 (or a broken one) — the user almost
/// certainly didn't mean to do that.
fn check_bundler_inputs(graph: &GraphEngine, out: &mut Vec<Finding>) {
    let bundler_ids: Vec<NodeId> = graph
        .nodes()
        .iter()
        .filter(|(_, n)| n.node_type == NodeType::Bundler)
        .map(|(id, _)| *id)
        .collect();
    for id in bundler_ids {
        let has_input = graph.connections().iter().any(|c| c.to.node_id == id);
        if !has_input {
            let label = graph
                .nodes()
                .get(&id)
                .map(|n| n.label.clone())
                .unwrap_or_else(|| format!("#{}", id.0));
            out.push(Finding::err(
                "bundler",
                format!("Bundler '{label}' has no inputs connected — exporting it would produce an empty map."),
            ));
        }
    }
}

/// Walk the graph backward from each Bundler. Any node not reachable
/// from at least one Bundler contributes nothing to the output and is
/// likely a leftover from a discarded experiment. Worth a Warning so
/// the user can clean up — not an error because half-built graphs
/// are normal during editing.
fn check_orphaned_nodes(graph: &GraphEngine, out: &mut Vec<Finding>) {
    // Build reverse adjacency: who feeds whom.
    let mut feeders: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for conn in graph.connections() {
        feeders
            .entry(conn.to.node_id)
            .or_default()
            .push(conn.from.node_id);
    }
    // BFS from every Bundler.
    let mut reachable: HashSet<NodeId> = HashSet::new();
    let mut frontier: Vec<NodeId> = graph
        .nodes()
        .iter()
        .filter(|(_, n)| n.node_type == NodeType::Bundler)
        .map(|(id, _)| *id)
        .collect();
    while let Some(id) = frontier.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(srcs) = feeders.get(&id) {
            for s in srcs {
                if !reachable.contains(s) {
                    frontier.push(*s);
                }
            }
        }
    }
    let mut orphans: Vec<String> = graph
        .nodes()
        .iter()
        .filter(|(id, n)| {
            !reachable.contains(id)
                // Bundlers themselves are seeds; ignore.
                && n.node_type != NodeType::Bundler
        })
        .map(|(_, n)| n.label.clone())
        .collect();
    orphans.sort();
    if !orphans.is_empty() {
        out.push(Finding::warn(
            "orphans",
            format!(
                "{} node(s) not connected to any Bundler — they won't affect export: {}",
                orphans.len(),
                orphans.join(", "),
            ),
        ));
    }
}

/// `mapinfo.lua` is generated by the bundler from the project's
/// `MapSettings` — it's the single source of truth for that file.
/// Any PassThrough or FileReference whose bundle destination
/// resolves to `mapinfo.lua` (in the archive root) would either
/// collide with the auto-generated copy or silently override it
/// with whatever the user dragged in. Reject it at validation
/// time so the contract is enforced before export.
fn check_mapinfo_lua_collisions(graph: &GraphEngine, out: &mut Vec<Finding>) {
    let target = "mapinfo.lua";
    for (id, node) in graph.nodes() {
        match node.node_type {
            NodeType::PassThrough => {
                let Some(ParamValue::String(s)) = node.params.get("files") else {
                    continue;
                };
                for line in s.lines() {
                    let parts: Vec<&str> = line.splitn(2, '|').collect();
                    if parts.len() < 2 {
                        continue;
                    }
                    let bundle_path = parts[1].trim().replace('\\', "/");
                    let stripped = bundle_path.strip_prefix("./").unwrap_or(&bundle_path);
                    if stripped.eq_ignore_ascii_case(target) {
                        out.push(Finding::err(
                            "mapinfo",
                            format!(
                                "PassThrough #{} ({}) writes `mapinfo.lua` — that's generated by the bundler from Map Settings. Edit Map Settings instead.",
                                id.0, node.label,
                            ),
                        ));
                    }
                }
            }
            NodeType::FileReference => {
                let Some(ParamValue::String(p)) = node.params.get("bundle_path") else {
                    continue;
                };
                let path = p.trim().replace('\\', "/");
                let stripped = path.strip_prefix("./").unwrap_or(&path);
                if stripped.eq_ignore_ascii_case(target) {
                    out.push(Finding::err(
                        "mapinfo",
                        format!(
                            "FileReference #{} ({}) writes `mapinfo.lua` — that's generated by the bundler from Map Settings. Edit Map Settings instead.",
                            id.0, node.label,
                        ),
                    ));
                }
            }
            _ => {}
        }
    }
}

/// PassThrough nodes write files into the SD7 at user-chosen bundle
/// paths. Two PassThrough lines pointing to the same archive path is
/// almost always a copy-paste mistake (the second silently overwrites
/// the first); flag it.
fn check_passthrough_collisions(graph: &GraphEngine, out: &mut Vec<Finding>) {
    let mut seen: HashMap<String, String> = HashMap::new();
    for (id, node) in graph.nodes() {
        if node.node_type != NodeType::PassThrough {
            continue;
        }
        let Some(ParamValue::String(s)) = node.params.get("files") else {
            continue;
        };
        for line in s.lines() {
            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() < 2 {
                continue;
            }
            let bundle_path = parts[1].trim().replace('\\', "/");
            if bundle_path.is_empty() {
                continue;
            }
            let owner = format!("PassThrough #{} ({})", id.0, node.label);
            if let Some(prev) = seen.get(&bundle_path) {
                if prev != &owner {
                    out.push(Finding::err(
                        "files",
                        format!(
                            "Two nodes write to the same archive path '{bundle_path}': {prev} and {owner}.",
                        ),
                    ));
                }
            } else {
                seen.insert(bundle_path, owner);
            }
        }
    }
}

/// Cycle check. The graph engine's `topological_sort` already detects
/// cycles internally; we surface that as a validator-level Error so
/// the user sees it before clicking Bundle. Catches the
/// infinite-recursion case the user worried about with subgraphs:
/// because subgraph external ports rebind to inner nodes (not a
/// separate computation), any cycle in inner / outer wiring shows up
/// as a cycle here.
fn check_cycles(graph: &GraphEngine, out: &mut Vec<Finding>) {
    if graph.topological_sort().is_err() {
        out.push(Finding::err(
            "topology",
            "Graph contains a cycle — at least one connection eventually feeds back into its own upstream. Eval will refuse to run.",
        ));
    }
}

/// Every connected pipeline needs at least one source node (noise
/// generator, file import, painted mask) somewhere upstream of every
/// Bundler. A graph of only filters / combiners has nothing to
/// transform and will produce empty output.
fn check_input_sources_present(graph: &GraphEngine, out: &mut Vec<Finding>) {
    let mut feeders: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for conn in graph.connections() {
        feeders
            .entry(conn.to.node_id)
            .or_default()
            .push(conn.from.node_id);
    }
    for (bid, bnode) in graph.nodes() {
        if bnode.node_type != NodeType::Bundler {
            continue;
        }
        // BFS upstream from this Bundler; stop when we find any source
        // node. If the BFS exhausts the reachable set without one,
        // that Bundler is sourceless.
        let mut visited: HashSet<NodeId> = HashSet::new();
        let mut frontier: Vec<NodeId> = vec![*bid];
        let mut found = false;
        while let Some(id) = frontier.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let Some(node) = graph.nodes().get(&id) {
                if is_source_node(&node.node_type) {
                    found = true;
                    break;
                }
            }
            if let Some(srcs) = feeders.get(&id) {
                for s in srcs {
                    if !visited.contains(s) {
                        frontier.push(*s);
                    }
                }
            }
        }
        if !found {
            // Don't double-flag when the Bundler also has no inputs at
            // all; check_bundler_inputs already covers that case.
            let has_any_input = graph.connections().iter().any(|c| c.to.node_id == *bid);
            if has_any_input {
                let label = bnode.label.clone();
                out.push(Finding::err(
                    "sources",
                    format!(
                        "Bundler '{label}' has no source node (noise / import / painted mask) upstream — its output would be empty."
                    ),
                ));
            }
        }
    }
}

/// Filter / combiner nodes whose input ports aren't connected to
/// anything will pass through default-zero values and produce a
/// flat / unhelpful output. Warning, not error: the user might be
/// in the middle of wiring things up.
fn check_disconnected_filter_inputs(graph: &GraphEngine, out: &mut Vec<Finding>) {
    let connected_inputs: HashSet<(NodeId, String)> = graph
        .connections()
        .iter()
        .map(|c| (c.to.node_id, c.to.port_name.clone()))
        .collect();
    let mut warned: Vec<(String, String)> = Vec::new();
    for (id, node) in graph.nodes() {
        // Source nodes have no inputs by definition.
        if is_source_node(&node.node_type) {
            continue;
        }
        // PassThrough / FileReference / Bundler aren't filters in this
        // sense — handled by other validators.
        if matches!(
            node.node_type,
            NodeType::PassThrough | NodeType::FileReference | NodeType::Bundler
        ) {
            continue;
        }
        for port in &node.inputs {
            // Control / Density / Mask inputs are optional modulators;
            // leaving them unconnected is normal operation. MaskApply is
            // the exception -- its mask is the primary input, not an
            // optional modulator.
            let is_modulator = !matches!(PortPlacement::for_input(port.kind), PortPlacement::Left);
            let is_required_mask =
                matches!(node.node_type, NodeType::MaskApply) && port.name == "mask";
            if is_modulator && !is_required_mask {
                continue;
            }
            if !connected_inputs.contains(&(*id, port.name.clone())) {
                warned.push((node.label.clone(), port.label.clone()));
            }
        }
    }
    warned.sort();
    if !warned.is_empty() {
        let preview: Vec<String> = warned
            .iter()
            .take(5)
            .map(|(n, p)| format!("'{n}'.{p}"))
            .collect();
        let extra = if warned.len() > 5 {
            format!(" and {} more", warned.len() - 5)
        } else {
            String::new()
        };
        out.push(Finding::warn(
            "wiring",
            format!(
                "{} unconnected input port(s){extra}: {}",
                warned.len(),
                preview.join(", ")
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{Node, NodeId};

    fn empty_graph() -> GraphEngine {
        GraphEngine::new()
    }

    #[test]
    fn flags_missing_bundler() {
        let graph = empty_graph();
        let f = validate_project(&graph, &MapSettings::default(), 1025, 1025);
        assert!(
            f.iter()
                .any(|x| x.category == "bundler" && x.severity == Severity::Error),
            "expected bundler error in {:?}",
            f
        );
    }

    #[test]
    fn dimensions_must_be_multiple_of_64_plus_one() {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "b"));
        let bad = validate_project(&graph, &MapSettings::default(), 256, 256);
        assert!(
            bad.iter()
                .any(|x| x.category == "dimensions" && x.severity == Severity::Error),
            "expected dimension error for 256: {:?}",
            bad
        );
        let good = validate_project(&graph, &MapSettings::default(), 257, 257);
        assert!(
            good.iter().all(|x| x.category != "dimensions"),
            "expected no dimension error for 257: {:?}",
            good
        );
    }

    #[test]
    fn rejects_inverted_height_range() {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "b"));
        let mut settings = MapSettings::default();
        settings.min_height = 100.0;
        settings.max_height = 50.0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            f.iter().any(|x| x.category == "dimensions"
                && x.severity == Severity::Error
                && x.field.as_deref() == Some("max_height")),
            "expected height-range error: {:?}",
            f
        );
    }

    #[test]
    fn flags_spawn_outside_map() {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "b"));
        let mut settings = MapSettings::default();
        settings.start_positions.push([99_999, 99_999]);
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            f.iter()
                .any(|x| x.category == "startboxes" && x.severity == Severity::Error),
            "expected spawn-outside-map error: {:?}",
            f
        );
    }

    #[test]
    fn flags_bundler_without_inputs() {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        let f = validate_project(&graph, &MapSettings::default(), 257, 257);
        assert!(
            f.iter().any(|x| x.category == "bundler"
                && x.severity == Severity::Error
                && x.message.contains("no inputs")),
            "expected 'no inputs' error: {:?}",
            f
        );
    }

    #[test]
    fn flags_orphaned_node_as_warning() {
        use bar_graph::PortId;
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        graph.add_node(Node::new(NodeId(1), NodeType::PerlinNoise, "Connected"));
        graph.add_node(Node::new(NodeId(2), NodeType::PerlinNoise, "Orphan"));
        // Connect node 1 to the bundler so only #2 is orphaned.
        graph
            .connect(
                PortId {
                    node_id: NodeId(1),
                    port_name: "out".to_string(),
                },
                PortId {
                    node_id: NodeId(0),
                    port_name: "heightmap".to_string(),
                },
            )
            .ok();
        let f = validate_project(&graph, &MapSettings::default(), 257, 257);
        let orphans: Vec<&Finding> = f.iter().filter(|x| x.category == "orphans").collect();
        assert_eq!(orphans.len(), 1, "expected one orphan finding: {:?}", f);
        assert_eq!(orphans[0].severity, Severity::Warning);
        assert!(
            orphans[0].message.contains("Orphan"),
            "expected orphan label in message: {}",
            orphans[0].message,
        );
    }

    #[test]
    fn flags_missing_source_when_bundler_has_only_filters() {
        use bar_graph::PortId;
        let mut graph = empty_graph();
        let bundler = graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        let blur = graph.add_node(Node::new(NodeId(0), NodeType::Blur, "Blur"));
        // Wire Blur → Bundler.heightmap with no source feeding Blur.
        graph
            .connect(
                PortId {
                    node_id: blur,
                    port_name: "output".to_string(),
                },
                PortId {
                    node_id: bundler,
                    port_name: "heightmap".to_string(),
                },
            )
            .expect("connect should succeed");
        let f = validate_project(&graph, &MapSettings::default(), 257, 257);
        assert!(
            f.iter()
                .any(|x| x.category == "sources" && x.severity == Severity::Error),
            "expected sources error: {:?}",
            f
        );
    }

    #[test]
    fn flags_disconnected_filter_input_as_warning() {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        graph.add_node(Node::new(NodeId(0), NodeType::Blur, "Blur"));
        let f = validate_project(&graph, &MapSettings::default(), 257, 257);
        assert!(
            f.iter()
                .any(|x| x.category == "wiring" && x.severity == Severity::Warning),
            "expected wiring warning: {:?}",
            f
        );
    }

    #[test]
    fn does_not_flag_filter_with_connected_input() {
        use bar_graph::PortId;
        let mut graph = empty_graph();
        let bundler = graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        let source = graph.add_node(Node::new(NodeId(0), NodeType::PerlinNoise, "Source"));
        let blur = graph.add_node(Node::new(NodeId(0), NodeType::Blur, "Blur"));
        graph
            .connect(
                PortId {
                    node_id: source,
                    port_name: "output".to_string(),
                },
                PortId {
                    node_id: blur,
                    port_name: "input".to_string(),
                },
            )
            .expect("source → blur connect should succeed");
        graph
            .connect(
                PortId {
                    node_id: blur,
                    port_name: "output".to_string(),
                },
                PortId {
                    node_id: bundler,
                    port_name: "heightmap".to_string(),
                },
            )
            .expect("blur → bundler connect should succeed");
        let f = validate_project(&graph, &MapSettings::default(), 257, 257);
        assert!(
            !f.iter().any(|x| x.category == "wiring"
                && x.severity == Severity::Warning
                && x.message.contains("'Blur'")),
            "Blur shouldn't be flagged when its input is connected: {:?}",
            f
        );
    }

    #[test]
    fn flags_passthrough_path_collision() {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "Bundler"));
        let mut a = Node::new(NodeId(1), NodeType::PassThrough, "A");
        a.params.insert(
            "files".to_string(),
            ParamValue::String("bar://a.txt|maps/foo.txt".to_string()),
        );
        graph.add_node(a);
        let mut b = Node::new(NodeId(2), NodeType::PassThrough, "B");
        b.params.insert(
            "files".to_string(),
            // Same archive path → collision. Backslash variant should
            // still match after normalization.
            ParamValue::String("bar://b.txt|maps\\foo.txt".to_string()),
        );
        graph.add_node(b);
        let f = validate_project(&graph, &MapSettings::default(), 257, 257);
        assert!(
            f.iter().any(|x| x.category == "files"
                && x.severity == Severity::Error
                && x.message.contains("same archive path")),
            "expected collision error: {:?}",
            f
        );
    }

    // ── Map-property validators ──────────────────────────────────────
    //
    // Helper: build a settings + graph pair that's already passing the
    // basic project-level checks so we can isolate each map-property
    // validator. A bundler node + valid dimensions + non-trivial height
    // range keep the unrelated validators quiet.
    fn quiet_setup() -> (GraphEngine, MapSettings) {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "b"));
        let mut s = MapSettings::default();
        s.start_positions.push([512, 512]);
        s.start_positions.push([1536, 1536]);
        (graph, s)
    }

    fn has_finding(f: &[Finding], category: &str, field: Option<&str>, severity: Severity) -> bool {
        f.iter().any(|x| {
            x.category == category && x.severity == severity && x.field.as_deref() == field
        })
    }

    #[test]
    fn physics_passes_with_default_settings() {
        let (graph, settings) = quiet_setup();
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            !f.iter().any(|x| x.category == "physics"),
            "default settings shouldn't flag physics: {:?}",
            f
        );
    }

    #[test]
    fn physics_flags_zero_gravity() {
        let (graph, mut settings) = quiet_setup();
        settings.gravity = 0.0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "physics", Some("gravity"), Severity::Error),
            "expected physics/gravity error: {:?}",
            f
        );
    }

    #[test]
    fn physics_flags_zero_hardness_as_warning() {
        let (graph, mut settings) = quiet_setup();
        settings.map_hardness = 0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "physics", Some("map_hardness"), Severity::Warning),
            "expected physics/map_hardness warning: {:?}",
            f
        );
    }

    #[test]
    fn physics_flags_negative_tidal_strength() {
        let (graph, mut settings) = quiet_setup();
        settings.tidal_strength = -1.0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "physics", Some("tidal_strength"), Severity::Error),
            "expected physics/tidal_strength error: {:?}",
            f
        );
    }

    #[test]
    fn atmosphere_flags_inverted_wind_range() {
        let (graph, mut settings) = quiet_setup();
        settings.atmosphere.min_wind = 30.0;
        settings.atmosphere.max_wind = 5.0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "atmosphere", Some("max_wind"), Severity::Error),
            "expected atmosphere/max_wind error: {:?}",
            f
        );
    }

    #[test]
    fn atmosphere_flags_negative_min_wind() {
        let (graph, mut settings) = quiet_setup();
        settings.atmosphere.min_wind = -1.0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "atmosphere", Some("min_wind"), Severity::Error),
            "expected atmosphere/min_wind error: {:?}",
            f
        );
    }

    #[test]
    fn atmosphere_flags_inverted_fog_range() {
        let (graph, mut settings) = quiet_setup();
        settings.atmosphere.fog_start = 0.9;
        settings.atmosphere.fog_end = 0.1;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "atmosphere", Some("fog_end"), Severity::Error),
            "expected atmosphere/fog_end error: {:?}",
            f
        );
    }

    #[test]
    fn atmosphere_flags_out_of_range_fog_color() {
        let (graph, mut settings) = quiet_setup();
        settings.atmosphere.fog_color = [1.5, 0.0, 0.0];
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "atmosphere", Some("fog_color"), Severity::Warning),
            "expected atmosphere/fog_color warning: {:?}",
            f
        );
    }

    #[test]
    fn lighting_flags_zero_sun_dir() {
        let (graph, mut settings) = quiet_setup();
        settings.lighting.sun_dir = [0.0, 0.0, 0.0];
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "lighting", Some("sun_dir"), Severity::Error),
            "expected lighting/sun_dir error: {:?}",
            f
        );
    }

    #[test]
    fn lighting_flags_zero_specular_exponent() {
        let (graph, mut settings) = quiet_setup();
        settings.lighting.spec_exponent = 0.0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "lighting", Some("spec_exponent"), Severity::Error),
            "expected lighting/spec_exponent error: {:?}",
            f
        );
    }

    #[test]
    fn water_flags_negative_damage() {
        let (graph, mut settings) = quiet_setup();
        settings.water.damage = -10.0;
        let f = validate_project(&graph, &settings, 257, 257);
        assert!(
            has_finding(&f, "water", Some("damage"), Severity::Error),
            "expected water/damage error: {:?}",
            f
        );
    }

    #[test]
    fn dimensions_field_tagged_correctly() {
        let mut graph = empty_graph();
        graph.add_node(Node::new(NodeId(0), NodeType::Bundler, "b"));
        // Bad width but valid depth.
        let f = validate_project(&graph, &MapSettings::default(), 256, 257);
        let width_findings: Vec<&Finding> = f
            .iter()
            .filter(|x| x.category == "dimensions" && x.field.as_deref() == Some("width"))
            .collect();
        assert!(
            !width_findings.is_empty(),
            "expected dimensions/width error: {:?}",
            f
        );
        let depth_findings: Vec<&Finding> = f
            .iter()
            .filter(|x| x.category == "dimensions" && x.field.as_deref() == Some("depth"))
            .collect();
        assert!(
            depth_findings.is_empty(),
            "depth should be valid (257), but got: {:?}",
            depth_findings
        );
    }

    #[test]
    fn finding_field_round_trips_through_builder() {
        let f = Finding::err("physics", "test").on_field("gravity");
        assert_eq!(f.category, "physics");
        assert_eq!(f.field.as_deref(), Some("gravity"));
        assert_eq!(f.severity, Severity::Error);
    }
}
