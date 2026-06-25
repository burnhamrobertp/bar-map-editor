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
    /// Bakes ambient occlusion + sun shadow from a heightfield into a Color
    /// buffer (R = AO, G = sun visibility, B = AO*sun). Horizon-based AO plus a
    /// ray-marched soft sun shadow. World Machine PRLM (lightmap) equivalent.
    LightmapBake,

    // Channel ops
    /// Splits a Color input into its four channels, each as a Heightmap
    /// (`r`/`g`/`b`/`a`). WM channel-splitter equivalent.
    ChannelSplit,
    /// Merges `r`/`g`/`b` Heightmaps (and optional `a`) into a single Color
    /// output. Missing alpha is treated as fully opaque. WM channel-merge
    /// equivalent.
    ChannelMerge,

    // Map layers
    NormalMap,
    GrassMap,
    SpecularMap,

    /// A 2D sculpt layer. Takes a heightmap input, applies a sequence of
    /// recorded brush dabs (stored as JSON in `params["dabs"]`), and
    // Mask Operations
    MaskThreshold,
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
    /// Slope-range selector (World Machine "Select Slope"). Computes terrain
    /// slope and selects the band between `min_slope` and `max_slope` (in
    /// degrees) with a `falloff` (degrees) and optional `invert`. Equivalent
    /// to a SlopeMap -> HeightSelect chain in one node, with WM-style units.
    SlopeSelect,

    // Generator: 2D layout of primitive shapes + freehand splines.
    /// Composites up to 8 layout items into a heightmap. Each item is
    /// either a primitive (ellipse / rectangle / ridge with position,
    /// size, rotation) or a Catmull-Rom spline (open path or
    /// closed/filled region from a control-point sequence). Items carry
    /// per-item height + falloff and composite by per-pixel maximum;
    /// the node-level `mode` (ridge / valley / mask) then interprets
    /// the field, and a `symmetry` enum mirrors / rotates every item.
    /// Used for shaped landmasses, rivers, roads, ridge lines, plateau
    /// edges, craters, atolls.
    Layout,

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

    // Final Composition (terminal node)
    /// The end of every project's node graph: composites paint layers
    /// (heightmap delta, color RGBA overlay, metalmap / typemap sparse
    /// overlay) on top of the procedural inputs and exposes the result
    /// to the bundler / export action buttons. Singleton -- exactly one
    /// per project, auto-created at bootstrap, can't be deleted. Inputs
    /// mirror everything the SD7 export consumes (heightmap, texture,
    /// normalmap, metalmap, typemap, grassmap, specular, files). Paint
    /// layers are edited from Sculpt3D, not the inspector.
    FinalComposition,
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

    // Misc
    /// Identity passthrough. Copies its input heightmap to its output
    /// unchanged. Exists so imported World Machine graphs that route through
    /// no-op marker devices round-trip without dropping wires.
    Checkpoint,
    /// N-way input selector. Exposes `input_count` heightmap inputs
    /// (`input_0`..`input_{n-1}`, 2..=8) and forwards the one chosen by
    /// `selected` to its output. Routing without rewiring the graph.
    Switch,
    /// Coastal/beach erosion. Smooths terrain toward a gentle beach profile
    /// within a band around `sea_level`, drags near-shore inland terrain down,
    /// and blurs the submerged seabed. World Machine coast-erosion equivalent.
    CoastErosion,
    /// Arbitrary per-pixel math. Evaluates a user-supplied `formula` for every
    /// pixel with the four optional inputs bound to `a`/`b`/`c`/`d` (0 where
    /// unconnected), normalized coordinates `x`/`y`, and `h` aliasing `a`.
    /// World Machine Equation device equivalent.
    Equation,

    // Scalar parameter graph (scalars wired INTO node params)
    /// A single scalar float (WM S_GN). Source node, no inputs; emits its
    /// `value` param on a `Scalar` output for wiring into a param port.
    ScalarValue,
    /// Two-input scalar arithmetic (WM S_AR). `op` selects the operation;
    /// emits a single `Scalar`.
    ScalarMath,
    /// A single scalar integer (WM I_GN). Source node; emits its `value`
    /// param as a `Scalar` (whole numbers) for driving count/seed-style params.
    IntValue,
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
    /// Ordered list of 2D control points in normalised [0..1, 0..1].
    /// Used by canvas-edited nodes (`Layout` spline items) to carry
    /// arbitrarily-long point sequences without resorting to indexed
    /// per-point params. Authors mutate this through the panel's 2D
    /// canvas; the executor reads it as a polyline / Catmull-Rom
    /// source.
    Spline(Vec<[f32; 2]>),
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

    /// Resize the heightmap inputs of a `Switch` node to `n`
    /// (clamped to 2..=8), rebuilding them as `input_0`..`input_{n-1}`.
    /// Caller disconnects any wires to removed ports.
    pub fn resize_switch_ports(&mut self, n: u32) {
        if self.node_type != NodeType::Switch {
            return;
        }
        let n = (n as usize).clamp(2, 8);
        self.inputs = (0..n)
            .map(|i| Port::new(format!("input_{i}"), format!("Input {i}"), PortKind::Heightmap))
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
    crate::nodes::def(node_type)
        .map(crate::nodes::build_ports)
        .unwrap_or_default()
}
