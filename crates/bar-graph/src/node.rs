use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::port::{Port, PortKind};

/// Unique identifier for a node in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

/// The type/category of a node, determining its behavior and ports.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    // Generators
    PerlinNoise,
    SimplexNoise,
    WorleyNoise,
    RidgedNoise,
    Constant,

    // Filters
    HydraulicErosion,
    ThermalErosion,
    Blur,
    Sharpen,
    Clamp,
    Terrace,

    // Combiners
    Blend,
    Add,
    Subtract,
    Multiply,
    Max,
    Min,

    // Texture/Splat
    SlopeMap,
    HeightSelect,
    SplatMap,
    /// Procedural diffuse texture from a heightmap + slope. Maps
    /// elevation through a biome gradient (water → beach → grass →
    /// forest → dirt → rock → snow) and blends in a rock colour on
    /// steep slopes. Exposes slope/AO/rock-colour controls; the
    /// gradient palette itself is built-in for now.
    AutoTexture,

    // Map layers
    NormalMap,
    GrassMap,
    SpecularMap,

    /// A 2D sculpt layer. Takes a heightmap input, applies a sequence of
    /// recorded brush dabs (stored as JSON in `params["dabs"]`), and
    /// outputs the modified heightmap. Works for any greyscale layer
    /// (terrain height, metalmap, typemap) — wire it wherever you need
    /// hand-authored edits mid-pipeline.
    Sculpt,

    // Mask Operations
    MaskThreshold,
    MaskInvert,
    MaskBlur,
    MaskApply,

    // Utility
    Mask,
    Invert,
    Curve,
    /// A hand-painted greyscale heightmap. Resolution is configurable
    /// via the `resolution` param (default 256). Pixel data is stored
    /// as a hex string in `params["data"]`. Doubles as a mask source —
    /// wire its output into anywhere a Heightmap is accepted.
    PaintedHeightmap,
    /// A hand-painted RGB texture. Output is a Color buffer suitable
    /// as the Bundler's texture input or for compositing with derived
    /// textures. Resolution is fixed at 256 for now.
    PaintedTexture,

    // Additional Generators
    FileInput,
    Voronoi,
    Gradient,

    // Additional Filters
    SimpleTransform,
    Normalize,
    BiasGain,
    Displacement,

    // Additional Combiners
    Chooser,

    // Bundler/Packaging
    /// Packages graph outputs into a deliverable archive.
    Bundler,
    /// External file reference included in a bundle without modification.
    FileReference,

    // Source nodes (import from disk)
    /// Reads a flat .smf file from disk (no archive). Exposes heightmap, metalmap, typemap.
    /// Deserialises legacy "Sd7Import" project nodes without error.
    #[serde(alias = "Sd7Import")]
    SmfImport,
    /// Reads a flat .smt tile file from disk and assembles a texture preview.
    SmtImport,
    /// Holds all extra files from an extracted .sd7 that should pass through to the bundler
    /// without processing (lua configs, sounds, textures, etc.).
    PassThrough,
    /// Mid-pipeline tap point. Pure passthrough — its heightmap output
    /// equals its heightmap input. Exists to give the user an explicit
    /// "show me what the map looks like here" anchor that can be
    /// targeted by the 3D viewport without committing to making the
    /// surrounding subgraph a Bundler.
    Preview,
    /// External input boundary of a SubGraph. Placeable only inside a
    /// subgraph. From OUTSIDE the collapsed subgraph it appears as one
    /// external input port on the collapsed block; from INSIDE it
    /// produces a value (from that outer wire) on its `value` output
    /// for inner consumers. Pure passthrough — `value` output equals
    /// whatever was wired into its `value` input from the outside.
    ///
    /// Each instance carries a `name` param that becomes the
    /// collapsed block's external port label, and a `kind` param
    /// (Heightmap / Color / Mask / Scalar / File / FileList) that
    /// drives the port type on both sides.
    SubgraphInput,
    /// External output boundary of a SubGraph. Mirror image of
    /// `SubgraphInput`: placeable only inside a subgraph; takes a
    /// value from inner producers on its `value` input and exposes
    /// it on the collapsed block's external output port.
    SubgraphOutput,
}

impl NodeType {
    /// True iff this node type is only meaningful inside a subgraph
    /// view. The palette filters by this in `CanvasView::SubGraph`
    /// vs. `CanvasView::Main` so users can't accidentally drop these
    /// at the top level of a graph.
    pub fn is_subgraph_only(&self) -> bool {
        matches!(self, NodeType::SubgraphInput | NodeType::SubgraphOutput)
    }
}

/// A node instance in the graph with its parameters and connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub node_type: NodeType,
    pub label: String,
    pub position: [f32; 2],
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub params: HashMap<String, ParamValue>,
    pub dirty: bool,
}

/// Parameter values that can be set on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    UInt(u32),
    Bool(bool),
    String(String),
    Vec2([f32; 2]),
}

impl Node {
    pub fn new(id: NodeId, node_type: NodeType, label: impl Into<String>) -> Self {
        let label = label.into();
        let (inputs, outputs) = default_ports(&node_type);
        let params = crate::defaults::default_params(&node_type);

        Self {
            id,
            node_type,
            label,
            position: [0.0, 0.0],
            inputs,
            outputs,
            params,
            dirty: true,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Synchronise both ports of a SubgraphInput / SubgraphOutput
    /// node with the current `kind` param. Both sides flip together
    /// so the boundary stays type-consistent. No-op for any other
    /// node type.
    pub fn sync_subgraph_io_kind(&mut self) {
        if !matches!(
            self.node_type,
            NodeType::SubgraphInput | NodeType::SubgraphOutput
        ) {
            return;
        }
        let Some(ParamValue::String(kind_str)) = self.params.get("kind") else {
            return;
        };
        let Some(kind) = crate::port::PortKind::parse_name(kind_str) else {
            return;
        };
        for p in self.inputs.iter_mut().chain(self.outputs.iter_mut()) {
            p.kind = kind;
        }
    }
}

/// Get default input/output ports for a node type.
fn default_ports(node_type: &NodeType) -> (Vec<Port>, Vec<Port>) {
    match node_type {
        // Generators: no inputs, one heightmap output
        NodeType::PerlinNoise
        | NodeType::SimplexNoise
        | NodeType::WorleyNoise
        | NodeType::RidgedNoise => (
            vec![],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),

        NodeType::Constant => (
            vec![],
            vec![Port::new("output", "Value", PortKind::Heightmap)],
        ),

        // Filters: one input, one output
        NodeType::HydraulicErosion
        | NodeType::ThermalErosion
        | NodeType::Blur
        | NodeType::Sharpen
        | NodeType::Clamp
        | NodeType::Terrace
        | NodeType::Invert => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Preview is a terminal sink — drives the 3D viewport but
        // produces nothing downstream. Heightmap is required (no
        // mesh = nothing to draw); texture / normal_map / specular_map
        // are optional layers the renderer composites on top.
        // Decoupled from the Bundler on purpose: export and preview
        // are separate concerns.
        NodeType::Preview => (
            vec![
                Port::new("heightmap", "Heightmap", PortKind::Heightmap),
                Port::new("texture", "Texture", PortKind::Color),
                Port::new("normal_map", "Normal Map", PortKind::Color),
                Port::new("specular_map", "Specular Map", PortKind::Heightmap),
            ],
            vec![],
        ),

        // Combiners: two inputs, one output
        NodeType::Blend | NodeType::Add | NodeType::Subtract | NodeType::Multiply => (
            vec![
                Port::new("a", "Input A", PortKind::Heightmap),
                Port::new("b", "Input B", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::Max | NodeType::Min => (
            vec![
                Port::new("a", "Input A", PortKind::Heightmap),
                Port::new("b", "Input B", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Texture/Splat operations
        NodeType::SlopeMap => (
            vec![Port::new("input", "Heightmap", PortKind::Heightmap)],
            vec![Port::new("output", "Slope", PortKind::Heightmap)],
        ),
        NodeType::HeightSelect => (
            vec![Port::new("input", "Heightmap", PortKind::Heightmap)],
            vec![Port::new("output", "Mask", PortKind::Heightmap)],
        ),
        NodeType::SplatMap => (
            vec![
                Port::new("slope", "Slope Map", PortKind::Heightmap),
                Port::new("band0", "Band 0", PortKind::Heightmap),
                Port::new("band1", "Band 1", PortKind::Heightmap),
                Port::new("band2", "Band 2", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Splat", PortKind::Heightmap)],
        ),
        NodeType::AutoTexture => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),

        // Map layer generators
        NodeType::NormalMap => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Normal Map", PortKind::Color)],
        ),
        NodeType::GrassMap => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Grass Density", PortKind::Heightmap)],
        ),
        NodeType::SpecularMap => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Specular", PortKind::Heightmap)],
        ),

        NodeType::Sculpt => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Mask: generates a mask output
        NodeType::Mask => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("mask", "Mask", PortKind::Mask)],
        ),

        // Painted heightmap: a paint canvas whose output is a full
        // Heightmap. Doubles as "draw a map by hand" and as a mask
        // source — anywhere a Heightmap input is accepted.
        NodeType::PaintedHeightmap => (
            vec![],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),
        // Painted texture: hand-painted RGB ground texture for the
        // map. Output is a Color buffer suitable for the Bundler's
        // texture input.
        NodeType::PaintedTexture => (
            vec![],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),

        // Mask operations
        NodeType::MaskThreshold => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Mask", PortKind::Heightmap)],
        ),
        NodeType::MaskInvert => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::MaskBlur => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::MaskApply => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Heightmap),
                Port::new("background", "Background", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Curve: remaps values via a transfer function
        NodeType::Curve => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // --- Additional Generators ---
        NodeType::FileInput => (
            vec![],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),
        NodeType::Voronoi => (
            vec![],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),
        NodeType::Gradient => (
            vec![],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),

        // --- Additional Filters ---
        NodeType::SimpleTransform => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::Normalize => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::BiasGain => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::Displacement => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("displacement", "Displacement", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // --- Additional Combiners ---
        NodeType::Chooser => (
            vec![
                Port::new("a", "Input A", PortKind::Heightmap),
                Port::new("b", "Input B", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // --- Bundler/Packaging ---
        NodeType::Bundler => (
            vec![
                Port::new("heightmap", "Heightmap", PortKind::Heightmap),
                Port::new("texture", "Texture", PortKind::Color),
                Port::new("normalmap", "Normal Map", PortKind::Color),
                Port::new("metalmap", "Metal Map", PortKind::Heightmap),
                Port::new("typemap", "Type Map", PortKind::Heightmap),
                Port::new("grassmap", "Grass Map", PortKind::Heightmap),
                Port::new("specular", "Specular", PortKind::Heightmap),
                Port::new_many("files", "Files", PortKind::FileList),
            ],
            vec![], // terminal node — action buttons rendered directly in node body
        ),

        NodeType::FileReference => (
            vec![],
            vec![Port::new("file", "File", PortKind::File)],
        ),

        // --- Import/Export ---
        NodeType::SmfImport => (
            vec![],
            vec![
                Port::new("heightmap", "Heightmap", PortKind::Heightmap),
                Port::new("metalmap", "Metal Map", PortKind::Heightmap),
                Port::new("typemap", "Type Map", PortKind::Heightmap),
            ],
        ),

        NodeType::SmtImport => (
            vec![],
            vec![
                Port::new("texture", "Texture", PortKind::Color),
            ],
        ),

        NodeType::PassThrough => (
            vec![],
            vec![
                Port::new("files", "Files", PortKind::FileList),
            ],
        ),

        // SubgraphInput: 1 input ("value") + 1 output ("value"). Both
        // start as Heightmap; the `kind` param swaps both ports' kinds
        // in lockstep when the user picks a different type. From
        // outside the collapsed subgraph the `value` INPUT is the
        // external port; the `value` OUTPUT lives entirely inside the
        // subgraph and is invisible from the outer canvas.
        NodeType::SubgraphInput => (
            vec![Port::new("value", "Value", PortKind::Heightmap)],
            vec![Port::new("value", "Value", PortKind::Heightmap)],
        ),
        // SubgraphOutput: mirror of SubgraphInput. Inner producers
        // wire into its `value` input; the `value` output is the
        // collapsed block's external output port.
        NodeType::SubgraphOutput => (
            vec![Port::new("value", "Value", PortKind::Heightmap)],
            vec![Port::new("value", "Value", PortKind::Heightmap)],
        ),

    }
}
