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
    /// Slope/altitude-driven splat-weight generator. Outputs one band per slope
    /// zone for use with a Spring SMF typemap.
    TerrainSplat,
    /// Procedural diffuse texture from a heightmap + slope. Maps
    /// elevation through a biome gradient (water → beach → grass →
    /// forest → dirt → rock → snow) and blends in a rock colour on
    /// steep slopes. Exposes slope/AO/rock-colour controls; the
    /// gradient palette itself is built-in for now.
    AutoTexture,
    /// Slope-driven two-tone colorizer: soil on flat terrain, rock on steep.
    /// Smoothstep transition across a configurable threshold band.
    RockSoil,
    /// Altitude+slope colorizer with alpha-encoded vegetation coverage.
    /// Green below altitude_max + slope_cutoff; fades to dry/bare above either.
    Vegetation,
    /// Porter-Duff compositor for Color layers. Blends overlay on top of base
    /// using an optional distribution heightmap (falls back to overlay alpha).
    /// Supports over/multiply/screen/add blend modes.
    LayerBlend,
    /// Multi-input texture compositor. Accepts up to 8 texture+weight pairs and
    /// composites them using either normalized weighted blending or a
    /// priority/exclusion system where higher-priority layers claim canvas area
    /// from lower-priority ones. Replaces cascaded TextureOverlay chains.
    TextureWeightmap,
    /// Maps heightmap values through a user-defined color gradient. Up to 8
    /// color stops, each with a position [0,1] and an RRGGBB color. Outputs a
    /// full-resolution Color buffer; the mask input controls per-pixel alpha.
    ColorRamp,

    // Map layers
    NormalMap,
    GrassMap,
    SpecularMap,

    /// A 2D sculpt layer. Takes a heightmap input, applies a sequence of
    /// recorded brush dabs (stored as JSON in `params["dabs"]`), and
    // Mask Operations
    MaskThreshold,
    MaskInvert,
    MaskBlur,
    MaskApply,

    // Utility
    Mask,
    Invert,
    /// Axis/rotational symmetry filter. Reflects or rotates the canonical half/quadrant
    /// across the entire map. `mode` selects the symmetry: mirror_x, mirror_y, mirror_xy,
    /// rotate_180, rotate_90_4way.
    Mirror,
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
    /// An imported Spring Map Texture (.smt) stored in the project's
    /// asset directory. Outputs a Color buffer assembled from the SMT
    /// tile atlas at the requested texture resolution. Source-only node;
    /// no inputs. Runtime params `asset_path` and `tile_index_path` are
    /// injected at load time and stripped before save.
    ImportedTexture,

    // Additional Generators
    FileInput,
    Voronoi,
    Gradient,

    // Additional Filters
    Normalize,
    BiasGain,
    Displacement,

    // Selectors (derived analysis maps)
    /// Threshold-based selector for erosion flow/wear/deposit maps. Isolates
    /// cells where flow intensity exceeds a threshold, with a smooth falloff.
    /// Works on any Heightmap input but designed for erosion secondary outputs.
    FlowSelect,
    /// Surface curvature selector derived from the Laplacian of the heightmap.
    /// Outputs high values on ridges/peaks (mode="ridges"), in valleys/bowls
    /// (mode="valleys"), or a full curvature map (mode="full") where 0.5 = flat.
    SelectConvexity,

    // Generator: primitive shape heightmap
    /// Composites up to 8 primitive shapes (ellipse / rectangle / ridge-line)
    /// into a heightmap. Each shape has position, size, rotation, peak height,
    /// and falloff. Shapes are composited by taking the per-pixel maximum.
    LayoutGenerator,

    // Filters (transform / warp / strata)
    /// Translate, scale, and rotate a heightmap. Inverse-mapped bilinear
    /// sampling; pixels outside the source clamp to zero.
    Transform,
    /// Dual-axis domain warp. Displaces each lookup position by separate X
    /// and Y displacement maps. Enables directional terrain warping.
    Warp,
    /// Procedural horizontal rock strata. Snaps heights into `layer_count`
    /// discrete bands with noise-perturbed boundaries for natural irregularity.
    Stratify,

    // Morphological mask operations
    /// Morphological dilation: expands bright regions by taking the local
    /// maximum within a circular neighbourhood of the given radius.
    MaskExpand,
    /// Morphological erosion: shrinks bright regions by taking the local
    /// minimum within a circular neighbourhood of the given radius.
    MaskShrink,

    // Aspect selector
    /// Masks by slope-facing direction. Outputs 1 where terrain faces the
    /// given compass bearing, with a configurable band width and falloff.
    SelectAspect,

    // Additional Combiners
    /// Selects between two heightmap inputs based on a mask threshold.
    MaskSelect,

    // Bundler/Packaging
    /// Final composition step before bundling. Mandatory; one per
    /// project. Accepts every kind the Bundler accepts and forwards
    /// each, optionally compositing a per-kind paint layer (heightmap
    /// delta, color RGBA overlay, metalmap / typemap sparse overlay)
    /// authored by the Sculpt3D viewport on top of the procedural
    /// input. Layers live in `.barproj/final_composition/`; the node's
    /// internals are edited via Sculpt3D, not the inspector.
    FinalComposition,
    /// Packages graph outputs into a deliverable archive.
    Bundler,
    /// External file reference included in a bundle without modification.
    FileReference,

    // Source nodes (import from disk)
    /// Holds all extra files from an extracted .sd7 that should pass through to the bundler
    /// without processing (lua configs, sounds, textures, etc.).
    PassThrough,
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

    /// Directly set both port kinds on a SubgraphInput / SubgraphOutput
    /// node. Used when the kind is inferred from a live connection (whose
    /// label may not round-trip through PortKind::parse_name).
    pub fn set_io_port_kind(&mut self, kind: crate::port::PortKind) {
        if !matches!(
            self.node_type,
            NodeType::SubgraphInput | NodeType::SubgraphOutput
        ) {
            return;
        }
        for p in self.inputs.iter_mut().chain(self.outputs.iter_mut()) {
            p.kind = kind;
        }
    }

    /// Resize the texture inputs of a `TextureWeightmap` node to `layer_count`
    /// (clamped to 2..=8). Caller is responsible for disconnecting any
    /// connections to removed ports before or after calling this.
    pub fn resize_texture_weightmap_ports(&mut self, layer_count: u32) {
        if self.node_type != NodeType::TextureWeightmap {
            return;
        }
        let n = (layer_count as usize).clamp(2, 8);
        self.inputs = (0..n)
            .map(|i| {
                Port::new(
                    format!("texture_{i}"),
                    format!("Texture {i}"),
                    PortKind::Color,
                )
            })
            .collect();
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
            vec![Port::new("control", "Control", PortKind::Control)],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),

        NodeType::Constant => (
            vec![],
            vec![Port::new("output", "Value", PortKind::Heightmap)],
        ),

        NodeType::HydraulicErosion => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![
                Port::new("output", "Output", PortKind::Heightmap),
                Port::new("flow", "Flow", PortKind::Heightmap),
                Port::new("wear", "Wear", PortKind::Heightmap),
                Port::new("deposit", "Deposit", PortKind::Heightmap),
            ],
        ),

        // Filters with Control + Mask
        NodeType::ThermalErosion | NodeType::Blur | NodeType::Clamp => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::Terrace | NodeType::Sharpen => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Filter with Mask only
        NodeType::Invert | NodeType::Mirror => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Combiners
        NodeType::Blend => (
            vec![
                Port::new("a", "Input A", PortKind::Heightmap),
                Port::new("b", "Input B", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::Add | NodeType::Subtract | NodeType::Multiply => (
            vec![
                Port::new("a", "Input A", PortKind::Heightmap),
                Port::new("b", "Input B", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::Max | NodeType::Min => (
            vec![
                Port::new("a", "Input A", PortKind::Heightmap),
                Port::new("b", "Input B", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Texture/Splat operations
        NodeType::SlopeMap => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
            ],
            vec![Port::new("output", "Slope", PortKind::Heightmap)],
        ),
        NodeType::HeightSelect => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
            ],
            vec![Port::new("output", "Mask", PortKind::Heightmap)],
        ),
        NodeType::TerrainSplat => (
            vec![
                Port::new("slope", "Slope Map", PortKind::Heightmap),
                Port::new("band0", "Band 0", PortKind::Heightmap),
                Port::new("band1", "Band 1", PortKind::Heightmap),
                Port::new("band2", "Band 2", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Splat", PortKind::Heightmap)],
        ),
        NodeType::AutoTexture => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),
        NodeType::RockSoil => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),
        NodeType::Vegetation => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),
        NodeType::LayerBlend => (
            vec![
                Port::new("base", "Base", PortKind::Color),
                Port::new("overlay", "Overlay", PortKind::Color),
                Port::new("distribution", "Distribution", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),
        NodeType::TextureWeightmap => (
            // Default 2 texture inputs; resize_texture_weightmap_ports adjusts at runtime.
            vec![
                Port::new("texture_0", "Texture 0", PortKind::Color),
                Port::new("texture_1", "Texture 1", PortKind::Color),
            ],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),
        NodeType::ColorRamp => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Color", PortKind::Color)],
        ),

        // Map layer generators
        NodeType::NormalMap => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Normal Map", PortKind::Color)],
        ),
        NodeType::GrassMap => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("density", "Density", PortKind::Density),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Grass Density", PortKind::Heightmap)],
        ),
        NodeType::SpecularMap => (
            vec![
                Port::new("input", "Heightmap", PortKind::Heightmap),
                Port::new("slope", "Slope Map", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Specular", PortKind::Heightmap)],
        ),

        // Mask: generates a mask output
        NodeType::Mask => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
            ],
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
        NodeType::ImportedTexture => (
            vec![],
            vec![Port::new("output", "Texture", PortKind::Color)],
        ),

        // Mask operations
        NodeType::MaskThreshold => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
            ],
            vec![Port::new("output", "Mask", PortKind::Heightmap)],
        ),
        NodeType::MaskInvert => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::MaskBlur => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::MaskApply => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("background", "Background", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // Curve: remaps values via a transfer function
        NodeType::Curve => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // --- Additional Generators ---
        NodeType::FileInput => (
            vec![],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),
        NodeType::Voronoi => (
            vec![Port::new("control", "Control", PortKind::Control)],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),
        NodeType::Gradient => (
            vec![Port::new("control", "Control", PortKind::Control)],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),

        NodeType::Normalize => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::BiasGain => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),
        NodeType::Displacement => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("displacement", "Displacement", PortKind::Heightmap),
                Port::new("control", "Control", PortKind::Control),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::FlowSelect => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Mask", PortKind::Heightmap)],
        ),

        NodeType::SelectConvexity => (
            vec![Port::new("input", "Heightmap", PortKind::Heightmap)],
            vec![Port::new("output", "Curvature", PortKind::Heightmap)],
        ),

        NodeType::LayoutGenerator => (
            vec![Port::new("mask", "Mask", PortKind::Mask)],
            vec![Port::new("output", "Heightmap", PortKind::Heightmap)],
        ),

        NodeType::Transform => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::Warp => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("warp_x", "Warp X", PortKind::Heightmap),
                Port::new("warp_y", "Warp Y", PortKind::Heightmap),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::Stratify => (
            vec![
                Port::new("input", "Input", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::MaskExpand | NodeType::MaskShrink => (
            vec![Port::new("input", "Input", PortKind::Heightmap)],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        NodeType::SelectAspect => (
            vec![Port::new("input", "Heightmap", PortKind::Heightmap)],
            vec![Port::new("output", "Aspect Mask", PortKind::Heightmap)],
        ),

        // --- Additional Combiners ---
        NodeType::MaskSelect => (
            vec![
                Port::new("a", "Input A", PortKind::Heightmap),
                Port::new("b", "Input B", PortKind::Heightmap),
                Port::new("mask", "Mask", PortKind::Mask),
            ],
            vec![Port::new("output", "Output", PortKind::Heightmap)],
        ),

        // --- Bundler/Packaging ---
        // FinalComposition mirrors Bundler's input set exactly. Each
        // input has a same-named, same-kind output; the Bundler is
        // wired strictly to FC's outputs (1-to-1), so the eval surface
        // exposed to the bundler is unchanged. Paintable kinds
        // (heightmap, texture, metalmap, typemap) gain compositing in
        // Phase 2; until then FC is a pure pass-through.
        NodeType::FinalComposition => (
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
        ),
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

        NodeType::FileReference => (vec![], vec![Port::new("file", "File", PortKind::File)]),

        NodeType::PassThrough => (
            vec![],
            vec![Port::new("files", "Files", PortKind::FileList)],
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
