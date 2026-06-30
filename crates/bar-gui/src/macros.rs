//! Macro templates — pre-built SubGraph chunks that drop onto the
//! canvas as a single labelled block.
//!
//! A macro is the "chip" model from the SubGraph design (group with
//! external ports + inner-port bindings) prepackaged as a reusable
//! unit. The hobbyist tier of bar-editor uses these to skip the
//! "assemble noise + erosion + blur in the right order" learning
//! curve: drop a `Ridge` (or any feature) macro and blend its
//! `terrain` output onto a base, done.
//!
//! Macros are JSON files embedded at compile time. Each defines a
//! list of inner nodes, the connections between them, and a
//! SubGraph wrapper (label, colour, port bindings).

use bar_graph::{GraphEngine, Node, NodeId, NodeType, ParamValue, PortId};
use eframe::egui;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use crate::app::{IO_NODE_SIZE, PORT_Y_BASE, PORT_Y_STEP};
use crate::state::{GroupRuntime, NodeVisual, SubgraphPortRuntime};

/// On-disk shape of a macro. JSON-deserialised once per drop.
#[derive(Debug, Deserialize)]
pub struct MacroTemplate {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub nodes: Vec<MacroNode>,
    #[serde(default)]
    pub connections: Vec<MacroConnection>,
    pub subgraph: MacroSubgraph,
    /// High-level domain parameters this macro exposes on its
    /// SubGraph block. Each one binds to a specific inner-node param;
    /// editing the macro param writes through to the inner node
    /// immediately. Empty when the macro has no abstracted params
    /// (the user expands the SubGraph to tune inner nodes directly).
    #[serde(default)]
    pub macro_params: Vec<MacroParamTemplate>,
}

/// JSON shape for a macro parameter spec inside a `MacroTemplate`.
/// Mirrors `bar_project::MacroParamSpec`.
#[derive(Debug, Deserialize)]
pub struct MacroParamTemplate {
    pub name: String,
    pub label: String,
    pub kind: String,
    pub binding: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct MacroNode {
    pub key: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub params: HashMap<String, ParamValue>,
}

#[derive(Debug, Deserialize)]
pub struct MacroConnection {
    /// `"node_key.port_name"` pair. Matches the `Recipe` connection
    /// shape so the parser is the same.
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
pub struct MacroSubgraph {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color_idx: u8,
    #[serde(default = "default_collapsed")]
    pub collapsed: bool,
    #[serde(default)]
    pub inputs: Vec<MacroPort>,
    #[serde(default)]
    pub outputs: Vec<MacroPort>,
}

fn default_collapsed() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct MacroPort {
    pub name: String,
    pub label: String,
    pub kind: String,
    /// `"node_key:port_name"` reference into the macro's own inner
    /// nodes. Resolved against `key_to_id` at instantiation time.
    pub binding: Option<String>,
}

/// Output of `instantiate`: the new node IDs, connection list, and
/// the assembled `GroupRuntime` for the SubGraph wrapper. Caller
/// inserts them into its own state — the function itself stays pure
/// so it's easy to test.
pub struct Instantiation {
    pub member_ids: HashSet<NodeId>,
    pub group: GroupRuntime,
    /// Visual positions for the newly-created inner nodes,
    /// arranged left-to-right starting at `drop_pos`.
    pub visuals: Vec<(NodeId, NodeVisual)>,
}

/// Drop the macro into the supplied graph at `drop_pos`. Adds inner
/// nodes, wires their connections, and builds a `GroupRuntime` whose
/// member set is the freshly-created node IDs. Doesn't touch the
/// caller's group map / visual map — the caller folds the returned
/// instantiation into its own state (so undo can capture the whole
/// drop as one snapshot).
pub fn instantiate(
    template: &MacroTemplate,
    graph: &mut GraphEngine,
    drop_pos: egui::Pos2,
) -> Result<Instantiation, String> {
    let mut key_to_id: HashMap<String, NodeId> = HashMap::new();
    let mut member_ids: HashSet<NodeId> = HashSet::new();
    let mut visuals: Vec<(NodeId, NodeVisual)> = Vec::new();

    // Inner nodes are laid out left-to-right at the drop position so
    // the user can see what got created if they expand the SubGraph.
    // One seed value per drop — used to overwrite any inner-node
    // `seed` UInt parameter the template defines, so dragging two
    // copies of the same macro produces two different terrains
    // instead of identical ones.
    let drop_seed = fresh_seed();
    let step = egui::vec2(180.0, 0.0);
    for (i, n) in template.nodes.iter().enumerate() {
        let mut node = Node::new(NodeId(0), n.node_type.clone(), n.label.clone());
        // Param overrides from the template fold over the type's
        // defaults — anything the macro doesn't specify keeps the
        // node's own default.
        for (k, v) in &n.params {
            node.params.insert(k.clone(), v.clone());
        }
        if node.node_type == NodeType::TextureWeightmap {
            if let Some(ParamValue::UInt(lc)) = node.params.get("layer_count") {
                node.resize_texture_weightmap_ports(*lc);
            }
        }
        // Replace any UInt-kind `seed` parameter with a fresh
        // per-drop value. Mixed in the param key's hash so multi-
        // node macros (e.g. two noise generators feeding a Blend)
        // don't end up with identical seeds across their inner
        // generators.
        for (k, v) in node.params.clone() {
            if k == "seed" {
                if let ParamValue::UInt(_) = v {
                    let mixed = mix_seed(drop_seed, &k, i);
                    node.params.insert(k, ParamValue::UInt(mixed));
                }
            }
        }
        let n_ports = node.inputs.len().max(node.outputs.len());
        let default_size = match node.node_type {
            NodeType::PassThrough => egui::vec2(180.0, 200.0),
            NodeType::FinalComposition => egui::vec2(210.0, 240.0),
            _ => egui::vec2(
                150.0,
                (PORT_Y_BASE + n_ports as f32 * PORT_Y_STEP + 10.0).max(60.0),
            ),
        };
        let id = graph.add_node(node);
        key_to_id.insert(n.key.clone(), id);
        member_ids.insert(id);
        let pos = egui::pos2(drop_pos.x + step.x * i as f32, drop_pos.y);
        visuals.push((
            id,
            NodeVisual {
                position: pos,
                size: default_size,
            },
        ));
    }

    for c in &template.connections {
        let (from_id, from_port) = parse_node_port(&c.from, &key_to_id, ".")?;
        let (to_id, to_port) = parse_node_port(&c.to, &key_to_id, ".")?;
        graph
            .connect(
                PortId {
                    node_id: from_id,
                    port_name: from_port,
                },
                PortId {
                    node_id: to_id,
                    port_name: to_port,
                },
            )
            .map_err(|e| format!("connect failed: {e:?}"))?;
    }

    // Each declared external port becomes a real `SubgraphInput` /
    // `SubgraphOutput` node placed inside the subgraph. The runtime
    // `subgraph_inputs/outputs` list is left empty here — the
    // editor's per-frame `recompute_all_subgraph_io` derives it
    // from these IO nodes the next time the frame ticks. The
    // declarative JSON shape (template.subgraph.inputs/outputs) is
    // therefore *generative*: it produces nodes, not metadata.
    //
    // For each declared output, the IO node sits to the right of the
    // last inner node (offset further per port). The IO node's
    // `value` input is wired to whatever the binding points at so
    // the value flows through it on evaluation. For inputs we wire
    // the IO node's `value` output to the inner consumer.
    for (io_index, p) in template.subgraph.inputs.iter().enumerate() {
        // IO nodes ship with no `name` and no node-level label by
        // default. The wrapper block's external port name and the
        // visible label on the IO node are both derived from the
        // port `kind` (with an auto-generated suffix when the
        // subgraph carries multiple ports of the same kind, see
        // `recompute_all_subgraph_io`). Macro authors used to
        // hard-code `p.name` (e.g. "terrain", "slope_map") here
        // but that surfaced as macro-set "names" in the IO node's
        // properties panel, which the user found undesirable —
        // IO nodes should be unnamed by default.
        let mut node = Node::new(NodeId(0), NodeType::SubgraphInput, String::new());
        node.params
            .insert("kind".to_string(), ParamValue::String(p.kind.clone()));
        if !p.label.is_empty() {
            node.params
                .insert("name".to_string(), ParamValue::String(p.label.clone()));
        }
        node.sync_subgraph_io_kind();
        let id = graph.add_node(node);
        member_ids.insert(id);
        // Lay out IO nodes in a column to the right of the inner pipeline.
        let pos = egui::pos2(drop_pos.x - 220.0, drop_pos.y + io_index as f32 * 90.0);
        visuals.push((
            id,
            NodeVisual {
                position: pos,
                size: IO_NODE_SIZE,
            },
        ));
        // Connect the IO node's value output to the inner node port
        // declared in the template binding (if it parses).
        if let Some(binding_str) = p.binding.as_deref() {
            if let Ok((inner_id, inner_port)) = parse_node_port(binding_str, &key_to_id, ":") {
                let _ = graph.connect(
                    PortId {
                        node_id: id,
                        port_name: "value".to_string(),
                    },
                    PortId {
                        node_id: inner_id,
                        port_name: inner_port,
                    },
                );
            }
        }
    }
    for (io_out_index, p) in template.subgraph.outputs.iter().enumerate() {
        // See the SubgraphInput block above for why `name` and the
        // node-level label are deliberately left empty here.
        let mut node = Node::new(NodeId(0), NodeType::SubgraphOutput, String::new());
        node.params
            .insert("kind".to_string(), ParamValue::String(p.kind.clone()));
        if !p.label.is_empty() {
            node.params
                .insert("name".to_string(), ParamValue::String(p.label.clone()));
        }
        node.sync_subgraph_io_kind();
        let id = graph.add_node(node);
        member_ids.insert(id);
        let pos = egui::pos2(
            drop_pos.x + step.x * (template.nodes.len() as f32 + 0.6),
            drop_pos.y + io_out_index as f32 * 90.0,
        );
        visuals.push((
            id,
            NodeVisual {
                position: pos,
                size: IO_NODE_SIZE,
            },
        ));
        if let Some(binding_str) = p.binding.as_deref() {
            if let Ok((inner_id, inner_port)) = parse_node_port(binding_str, &key_to_id, ":") {
                let _ = graph.connect(
                    PortId {
                        node_id: inner_id,
                        port_name: inner_port,
                    },
                    PortId {
                        node_id: id,
                        port_name: "value".to_string(),
                    },
                );
            }
        }
    }
    // Empty runtime ports — the per-frame derive will populate them
    // based on the IO nodes we just created.
    let inputs: Vec<SubgraphPortRuntime> = Vec::new();
    let outputs: Vec<SubgraphPortRuntime> = Vec::new();

    let label = if template.subgraph.label.is_empty() {
        template.name.clone()
    } else {
        template.subgraph.label.clone()
    };
    let macro_params: Vec<crate::state::MacroParamRuntime> = template
        .macro_params
        .iter()
        .filter_map(|p| {
            let (id, param_name) = parse_node_port(&p.binding, &key_to_id, ":").ok()?;
            Some(crate::state::MacroParamRuntime {
                name: p.name.clone(),
                label: p.label.clone(),
                kind: p.kind.clone(),
                binding: Some((id, param_name)),
                min: p.min,
                max: p.max,
            })
        })
        .collect();
    let group = GroupRuntime {
        label,
        member_ids: member_ids.clone(),
        color_idx: template.subgraph.color_idx,
        collapsed: template.subgraph.collapsed,
        is_subgraph: true,
        subgraph_inputs: inputs,
        subgraph_outputs: outputs,
        macro_params,
    };

    Ok(Instantiation {
        member_ids,
        group,
        visuals,
    })
}

/// Per-drop seed source. Combines wall-clock nanos with the process
/// id so two macros dropped in the same frame still get distinct
/// seeds. Not cryptographically anything; we just want variety.
fn fresh_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
        .unwrap_or(0);
    nanos ^ std::process::id().rotate_left(13)
}

/// Blend the drop-seed with a per-node-and-key tag so multiple inner
/// `seed` params end up with distinct values rather than all the
/// same one.
fn mix_seed(drop_seed: u32, key: &str, node_idx: usize) -> u32 {
    let mut h: u32 = drop_seed.wrapping_mul(2654435769);
    for b in key.bytes() {
        h = h.wrapping_add(b as u32).wrapping_mul(16777619);
    }
    h ^ (node_idx as u32).rotate_left(7)
}

fn parse_node_port(
    s: &str,
    key_to_id: &HashMap<String, NodeId>,
    sep: &str,
) -> Result<(NodeId, String), String> {
    let mut parts = s.splitn(2, sep);
    let key = parts.next().ok_or_else(|| format!("bad ref '{s}'"))?;
    let port = parts
        .next()
        .ok_or_else(|| format!("bad ref '{s}' (missing port)"))?;
    let id = key_to_id
        .get(key)
        .copied()
        .ok_or_else(|| format!("unknown node key '{key}' in macro"))?;
    Ok((id, port.to_string()))
}

/// One feature within a macro group. `full_name` is the canonical
/// lookup key; `display_name` is the label shown in menus.
pub struct MacroEntry {
    pub full_name: &'static str,
    pub display_name: &'static str,
    pub json: &'static str,
}

/// A group of related macro variants sharing a common archetype.
/// The first entry in `entries` is the standard/base variant.
pub struct MacroGroup {
    pub name: &'static str,
    pub entries: &'static [MacroEntry],
}

// Feature macros: each is a standalone generator emitting a `terrain` patch
// (and, for shaped features, a `mask`) that the user composites onto a base.
// Raises blend with `add`, carves with `subtract`, and absolute/replace
// features (crater, island, coastline, lake) combine with `MaskApply`.
pub static BUILTIN_MACRO_GROUPS: &[MacroGroup] = &[
    MacroGroup {
        name: "Base Terrain",
        entries: &[
            MacroEntry {
                full_name: "Flat Plain",
                display_name: "Flat Plain",
                json: include_str!("../../../assets/macros/base-flat-plain.json"),
            },
            MacroEntry {
                full_name: "Rolling Lowland",
                display_name: "Rolling Lowland",
                json: include_str!("../../../assets/macros/base-rolling-lowland.json"),
            },
            MacroEntry {
                full_name: "Coastal Shelf",
                display_name: "Coastal Shelf",
                json: include_str!("../../../assets/macros/base-coastal-shelf.json"),
            },
        ],
    },
    MacroGroup {
        name: "Relief",
        entries: &[
            MacroEntry {
                full_name: "Ridge",
                display_name: "Ridge",
                json: include_str!("../../../assets/macros/ridge.json"),
            },
            MacroEntry {
                full_name: "Cliff",
                display_name: "Cliff",
                json: include_str!("../../../assets/macros/cliff.json"),
            },
            MacroEntry {
                full_name: "Mountain",
                display_name: "Mountain",
                json: include_str!("../../../assets/macros/mountain.json"),
            },
            MacroEntry {
                full_name: "Plateau",
                display_name: "Plateau",
                json: include_str!("../../../assets/macros/plateau.json"),
            },
        ],
    },
    MacroGroup {
        name: "Tableland",
        entries: &[
            MacroEntry {
                full_name: "Butte",
                display_name: "Butte",
                json: include_str!("../../../assets/macros/butte.json"),
            },
            MacroEntry {
                full_name: "Mesa",
                display_name: "Mesa",
                json: include_str!("../../../assets/macros/mesa.json"),
            },
        ],
    },
    MacroGroup {
        name: "Carved",
        entries: &[
            MacroEntry {
                full_name: "Crater",
                display_name: "Crater",
                json: include_str!("../../../assets/macros/crater.json"),
            },
            MacroEntry {
                full_name: "Canyon",
                display_name: "Canyon",
                json: include_str!("../../../assets/macros/canyon.json"),
            },
            MacroEntry {
                full_name: "Basin",
                display_name: "Basin",
                json: include_str!("../../../assets/macros/basin.json"),
            },
        ],
    },
    MacroGroup {
        name: "Water & Coast",
        entries: &[
            MacroEntry {
                full_name: "River",
                display_name: "River",
                json: include_str!("../../../assets/macros/river.json"),
            },
            MacroEntry {
                full_name: "Stream",
                display_name: "Stream",
                json: include_str!("../../../assets/macros/stream.json"),
            },
            MacroEntry {
                full_name: "Lake",
                display_name: "Lake",
                json: include_str!("../../../assets/macros/lake.json"),
            },
            MacroEntry {
                full_name: "Island",
                display_name: "Island",
                json: include_str!("../../../assets/macros/island.json"),
            },
            MacroEntry {
                full_name: "Archipelago",
                display_name: "Archipelago",
                json: include_str!("../../../assets/macros/archipelago.json"),
            },
            MacroEntry {
                full_name: "Coastline",
                display_name: "Coastline",
                json: include_str!("../../../assets/macros/coastline.json"),
            },
        ],
    },
    MacroGroup {
        name: "Gameplay",
        entries: &[
            MacroEntry {
                full_name: "Chokepoint",
                display_name: "Chokepoint",
                json: include_str!("../../../assets/macros/chokepoint.json"),
            },
            MacroEntry {
                full_name: "Ramp",
                display_name: "Ramp",
                json: include_str!("../../../assets/macros/ramp.json"),
            },
            MacroEntry {
                full_name: "Expansion Plateau",
                display_name: "Expansion Plateau",
                json: include_str!("../../../assets/macros/expansion-plateau.json"),
            },
            MacroEntry {
                full_name: "Land Bridge",
                display_name: "Land Bridge",
                json: include_str!("../../../assets/macros/land-bridge.json"),
            },
        ],
    },
];

/// Parse one of the built-in macros by its canonical full name.
pub fn parse(name: &str) -> Option<MacroTemplate> {
    for group in BUILTIN_MACRO_GROUPS {
        for entry in group.entries {
            if entry.full_name == name {
                return serde_json::from_str(entry.json).ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtin_macros_parse_and_instantiate() {
        for group in BUILTIN_MACRO_GROUPS {
            for entry in group.entries {
                let name = entry.full_name;
                let t = parse(name).unwrap_or_else(|| panic!("macro '{name}' failed to parse"));
                assert!(!t.nodes.is_empty(), "macro '{name}' has no nodes");
                assert!(
                    !t.subgraph.outputs.is_empty(),
                    "macro '{name}' has no subgraph outputs"
                );
                let mut g = GraphEngine::new();
                instantiate(&t, &mut g, egui::pos2(0.0, 0.0))
                    .unwrap_or_else(|e| panic!("macro '{name}' failed to instantiate: {e}"));
            }
        }
    }

    #[test]
    fn ridge_parses() {
        let t = parse("Ridge").expect("ridge macro should parse");
        assert!(!t.nodes.is_empty());
        assert!(!t.subgraph.outputs.is_empty());
    }

    #[test]
    fn instantiate_produces_consistent_group() {
        let t = parse("Ridge").unwrap();
        let mut g = GraphEngine::new();
        let inst = instantiate(&t, &mut g, egui::pos2(0.0, 0.0)).unwrap();
        let io_count = t.subgraph.inputs.len() + t.subgraph.outputs.len();
        // Each inner node + IO node has a corresponding visual entry.
        // IO nodes are created in addition to the template's inner
        // nodes — one per declared external port.
        assert_eq!(inst.visuals.len(), t.nodes.len() + io_count);
        // All inner connections + IO-binding connections landed in
        // the graph.
        assert_eq!(g.connections().len(), t.connections.len() + io_count);
        // The runtime ports list is empty post-instantiation; the
        // editor's per-frame `recompute_all_subgraph_io` fills it
        // from the `SubgraphInput` / `SubgraphOutput` member nodes.
        assert!(inst.group.subgraph_inputs.is_empty());
        assert!(inst.group.subgraph_outputs.is_empty());
        // But the IO nodes themselves exist — verify by checking the
        // group's member set has the expected count.
        let io_members = inst
            .group
            .member_ids
            .iter()
            .filter(|id| {
                g.get_node(**id).is_some_and(|n| {
                    matches!(
                        n.node_type,
                        bar_graph::NodeType::SubgraphInput | bar_graph::NodeType::SubgraphOutput
                    )
                })
            })
            .count();
        assert_eq!(io_members, io_count);
    }
}
