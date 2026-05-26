//! Node + wire visual style helpers: per-NodeType title bar colour,
//! per-PortKind dot colour, IO-node convex-polygon outline builder,
//! and pure wire-geometry helpers (polyline distance, cubic bezier).
//!
//! Pulled out of `app.rs` so the visual-style decisions live with
//! the canvas renderer instead of the application root.

use bar_graph::{NodeType, PortKind};
use eframe::egui;

use crate::panels::tokens;

pub(crate) fn polyline_distance(p: egui::Pos2, points: &[egui::Pos2]) -> f32 {
    if points.len() < 2 {
        return f32::INFINITY;
    }
    let mut best = f32::INFINITY;
    for i in 0..points.len() - 1 {
        let a = points[i];
        let b = points[i + 1];
        let ab = b - a;
        let len2 = ab.x * ab.x + ab.y * ab.y;
        let t = if len2 > 1e-6 {
            ((p - a).dot(ab)) / len2
        } else {
            0.0
        };
        let t = t.clamp(0.0, 1.0);
        let proj = egui::pos2(a.x + ab.x * t, a.y + ab.y * t);
        let d = proj.distance(p);
        if d < best {
            best = d;
        }
    }
    best
}

pub(crate) fn cubic_bezier(
    p0: egui::Pos2,
    p1: egui::Pos2,
    p2: egui::Pos2,
    p3: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;

    let x = uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x;
    let y = uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y;
    egui::pos2(x, y)
}
pub(crate) fn node_type_color(node_type: &NodeType) -> egui::Color32 {
    match node_type {
        NodeType::PerlinNoise
        | NodeType::SimplexNoise
        | NodeType::WorleyNoise
        | NodeType::RidgedNoise
        | NodeType::Voronoi
        | NodeType::Gradient
        | NodeType::FileInput
        | NodeType::Constant
        | NodeType::Layout => tokens::NODE_CAT_GENERATOR,

        NodeType::HydraulicErosion
        | NodeType::ThermalErosion
        | NodeType::Blur
        | NodeType::Sharpen
        | NodeType::Clamp
        | NodeType::Terrace
        | NodeType::Invert
        | NodeType::Mirror
        | NodeType::Curve
        | NodeType::Normalize
        | NodeType::BiasGain
        | NodeType::Displacement
        | NodeType::Transform
        | NodeType::Warp
        | NodeType::Stratify => tokens::NODE_CAT_FILTER,

        NodeType::Blend
        | NodeType::Add
        | NodeType::Subtract
        | NodeType::Multiply
        | NodeType::Max
        | NodeType::Min
        | NodeType::MaskSelect => tokens::NODE_CAT_COMBINER,

        NodeType::SlopeMap
        | NodeType::HeightSelect
        | NodeType::FlowSelect
        | NodeType::SelectConvexity
        | NodeType::SelectAspect
        | NodeType::TerrainSplat
        | NodeType::AutoTexture
        | NodeType::RockSoil
        | NodeType::Vegetation
        | NodeType::LayerBlend
        | NodeType::TextureWeightmap
        | NodeType::ColorRamp
        | NodeType::NormalMap
        | NodeType::GrassMap
        | NodeType::SpecularMap => tokens::NODE_CAT_TEXTURE,

        NodeType::Mask
        | NodeType::PaintedHeightmap
        | NodeType::PaintedTexture
        | NodeType::MaskThreshold
        | NodeType::MaskApply
        | NodeType::MaskExpand
        | NodeType::MaskShrink => tokens::NODE_CAT_MASK,

        NodeType::FinalComposition | NodeType::FileReference => tokens::NODE_CAT_BUNDLER,

        NodeType::PassThrough | NodeType::ImportedTexture => tokens::NODE_CAT_SOURCE,

        // Distinct dark teal — boundary markers, not generators/filters/combiners.
        NodeType::SubgraphInput | NodeType::SubgraphOutput => tokens::NODE_CAT_IO,
    }
}

pub(crate) fn port_kind_color(kind: &PortKind) -> egui::Color32 {
    match kind {
        PortKind::Heightmap => tokens::PORT_HEIGHTMAP,
        PortKind::Mask => tokens::PORT_MASK,
        PortKind::Color => tokens::PORT_COLOR,
        PortKind::Scalar => tokens::PORT_SCALAR,
        PortKind::File => tokens::PORT_FILE,
        PortKind::FileList => tokens::PORT_FILE_LIST,
        PortKind::Control => tokens::PORT_CONTROL,
        PortKind::Density => tokens::PORT_DENSITY,
    }
}

/// Build the closed convex polygon for an IO-node silhouette
/// (rounded rectangle on one side, chevron point on the other).
/// Quarter-arc corners on the rounded side are sampled into line
/// segments; `Shape::convex_polygon` then fills and strokes the
/// whole shape in one pass, so the border and fill stay in
/// register at every size. Vertices are emitted clockwise in
/// screen coordinates as `convex_polygon` requires.
pub(crate) fn build_io_outline(
    rect: egui::Rect,
    chevron_w: f32,
    body_radius: f32,
    is_input: bool,
) -> Vec<egui::Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    // Six segments per quarter-arc reads as smooth at typical zoom
    // levels and stays cheap (≤ 14 extra vertices per node).
    let segments = 6_usize;
    let mid_y = rect.center().y;
    // Clamp the radius so it can never exceed half the height (which
    // would make the corners overlap) or a quarter of the width
    // (so the rounded side doesn't swallow the body).
    let r = body_radius
        .min(rect.height() / 2.0)
        .min(rect.width() / 4.0)
        .max(0.0);
    let mut pts: Vec<egui::Pos2> = Vec::with_capacity(2 * (segments + 1) + 4);
    let sample_arc = |center: egui::Pos2, start: f32, end: f32, pts: &mut Vec<egui::Pos2>| {
        for i in 0..=segments {
            let t = i as f32 / segments as f32;
            let angle = start + t * (end - start);
            pts.push(egui::pos2(
                center.x + r * angle.cos(),
                center.y + r * angle.sin(),
            ));
        }
    };
    if is_input {
        // CW: top-left arc → top edge → chevron tip → bottom edge →
        // bottom-left arc → close (left edge implicit on close).
        sample_arc(
            egui::pos2(rect.left() + r, rect.top() + r),
            PI,
            PI + FRAC_PI_2,
            &mut pts,
        );
        pts.push(egui::pos2(rect.right() - chevron_w, rect.top()));
        pts.push(egui::pos2(rect.right(), mid_y));
        pts.push(egui::pos2(rect.right() - chevron_w, rect.bottom()));
        sample_arc(
            egui::pos2(rect.left() + r, rect.bottom() - r),
            FRAC_PI_2,
            PI,
            &mut pts,
        );
    } else {
        // CW: chevron tip → chevron-top → top edge → top-right arc →
        // right edge implicit → bottom-right arc → bottom edge →
        // chevron-bottom → close.
        pts.push(egui::pos2(rect.left(), mid_y));
        pts.push(egui::pos2(rect.left() + chevron_w, rect.top()));
        sample_arc(
            egui::pos2(rect.right() - r, rect.top() + r),
            PI + FRAC_PI_2,
            2.0 * PI,
            &mut pts,
        );
        sample_arc(
            egui::pos2(rect.right() - r, rect.bottom() - r),
            0.0,
            FRAC_PI_2,
            &mut pts,
        );
        pts.push(egui::pos2(rect.left() + chevron_w, rect.bottom()));
    }
    pts
}
