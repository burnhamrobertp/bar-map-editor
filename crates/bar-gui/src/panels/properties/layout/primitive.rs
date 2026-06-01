//! Primitive layout item: ellipse / rectangle / line.
//!
//! Self-contained -- owns its param read/write, its canvas handles
//! (centre + four corners + rotation), its drag response, and its
//! silhouette drawing. Knows nothing about splines; the layout
//! coordinator (`super`) decides which kind a given item slot is.

use std::collections::HashMap;
use std::f32::consts::PI;

use bar_graph::ParamValue;
use eframe::egui;

use crate::panels::properties::properties_canvas::{
    CanvasTransform, HandleId, HandleKind, HandleSpec,
};

pub(super) const H_CENTRE: HandleId = HandleId(0);
pub(super) const H_TL: HandleId = HandleId(1);
pub(super) const H_TR: HandleId = HandleId(2);
pub(super) const H_BL: HandleId = HandleId(3);
pub(super) const H_BR: HandleId = HandleId(4);
pub(super) const H_ROT: HandleId = HandleId(5);

#[derive(Clone)]
pub(super) struct Primitive {
    /// `ellipse` / `rectangle` / `line`.
    pub shape_type: String,
    pub x: f32,
    pub y: f32,
    pub rx: f32,
    pub ry: f32,
    pub angle: f32,
    pub height: f32,
    pub falloff: f32,
}

impl Primitive {
    pub(super) fn read(params: &HashMap<String, ParamValue>, i: usize) -> Self {
        let shape_type = match params.get(&format!("type_{i}")) {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "ellipse".to_string(),
        };
        Self {
            shape_type,
            x: get_f(params, &format!("x_{i}"), 0.5),
            y: get_f(params, &format!("y_{i}"), 0.5),
            rx: get_f(params, &format!("rx_{i}"), 0.2),
            ry: get_f(params, &format!("ry_{i}"), 0.2),
            angle: get_f(params, &format!("angle_{i}"), 0.0),
            height: get_f(params, &format!("height_{i}"), 0.5),
            falloff: get_f(params, &format!("falloff_{i}"), 0.5),
        }
    }

    pub(super) fn new(x: f32, y: f32) -> Self {
        Self {
            shape_type: "ellipse".to_string(),
            x,
            y,
            rx: 0.1,
            ry: 0.1,
            angle: 0.0,
            height: 0.5,
            falloff: 0.5,
        }
    }

    pub(super) fn write(&self, params: &mut HashMap<String, ParamValue>, i: usize) {
        params.insert(
            format!("type_{i}"),
            ParamValue::String(self.shape_type.clone()),
        );
        params.insert(format!("x_{i}"), ParamValue::Float(self.x));
        params.insert(format!("y_{i}"), ParamValue::Float(self.y));
        params.insert(format!("rx_{i}"), ParamValue::Float(self.rx));
        params.insert(format!("ry_{i}"), ParamValue::Float(self.ry));
        params.insert(format!("angle_{i}"), ParamValue::Float(self.angle));
        params.insert(format!("height_{i}"), ParamValue::Float(self.height));
        params.insert(format!("falloff_{i}"), ParamValue::Float(self.falloff));
    }

    pub(super) fn handles(&self, item: usize) -> Vec<HandleSpec> {
        let (sina, cosa) = (self.angle * PI / 180.0).sin_cos();
        let corner = |lx: f32, ly: f32| -> [f32; 2] {
            [
                (self.x + lx * cosa - ly * sina).clamp(-0.5, 1.5),
                (self.y + lx * sina + ly * cosa).clamp(-0.5, 1.5),
            ]
        };
        // Cursor per handle. Diagonal corners get their actual
        // diagonal-resize cursor (matched against the unrotated frame
        // -- when the shape is rotated, the cursor's diagonal no
        // longer aligns visually, but the cost of a perfect rotated
        // mapping isn't worth the complexity for now).
        vec![
            HandleSpec {
                item,
                id: H_CENTRE,
                kind: HandleKind::Centre,
                pos: [self.x, self.y],
                px_radius: 8.0,
                cursor: egui::CursorIcon::Move,
            },
            HandleSpec {
                item,
                id: H_TL,
                kind: HandleKind::Corner,
                pos: corner(-self.rx, -self.ry),
                px_radius: 6.0,
                cursor: egui::CursorIcon::ResizeNwSe,
            },
            HandleSpec {
                item,
                id: H_TR,
                kind: HandleKind::Corner,
                pos: corner(self.rx, -self.ry),
                px_radius: 6.0,
                cursor: egui::CursorIcon::ResizeNeSw,
            },
            HandleSpec {
                item,
                id: H_BL,
                kind: HandleKind::Corner,
                pos: corner(-self.rx, self.ry),
                px_radius: 6.0,
                cursor: egui::CursorIcon::ResizeNeSw,
            },
            HandleSpec {
                item,
                id: H_BR,
                kind: HandleKind::Corner,
                pos: corner(self.rx, self.ry),
                px_radius: 6.0,
                cursor: egui::CursorIcon::ResizeNwSe,
            },
            HandleSpec {
                item,
                id: H_ROT,
                kind: HandleKind::Rotation,
                pos: corner(self.rx * 1.3 + 0.02, 0.0),
                px_radius: 7.0,
                cursor: egui::CursorIcon::Crosshair,
            },
        ]
    }

    /// True if `pos` (normalised) is inside the primitive's silhouette.
    /// Used by the canvas widget to let an author click an unselected
    /// shape's body to select it.
    pub(super) fn contains(&self, pos: [f32; 2]) -> bool {
        let (sina, cosa) = (self.angle * PI / 180.0).sin_cos();
        let dx = pos[0] - self.x;
        let dy = pos[1] - self.y;
        let lx = dx * cosa + dy * sina;
        let ly = -dx * sina + dy * cosa;
        let rx = self.rx.max(1e-4);
        let ry = self.ry.max(1e-4);
        match self.shape_type.as_str() {
            "rectangle" => lx.abs() <= rx && ly.abs() <= ry,
            // A line item is a bounded segment along the local X
            // axis. Hit area: inside the segment's projection, plus
            // a minimum 0.015-normalised band perpendicular so thin
            // lines are still clickable.
            "line" => lx.abs() <= rx && ly.abs() <= ry.max(0.015),
            _ => {
                let nx = lx / rx;
                let ny = ly / ry;
                nx * nx + ny * ny <= 1.0
            }
        }
    }

    /// Apply a handle drag. `anchor` is the world position the opposite
    /// corner was at when the user pressed (only present for corner
    /// handles). `shift` is the live state of the Shift modifier --
    /// when held, corner drags fall back to center-fixed scaling
    /// regardless of the anchor; releasing Shift mid-drag returns to
    /// anchor-based resize.
    pub(super) fn apply_drag(
        &mut self,
        handle: HandleId,
        pos: [f32; 2],
        anchor: Option<[f32; 2]>,
        shift: bool,
    ) {
        match handle {
            H_CENTRE => {
                self.x = pos[0].clamp(0.0, 1.0);
                self.y = pos[1].clamp(0.0, 1.0);
            }
            H_TL | H_TR | H_BL | H_BR => {
                // Default: anchor the opposite corner (set at press
                // time). The centre moves to the midpoint of the
                // cursor and the anchor. Shift held: keep the centre
                // where it is and scale around it (the prior
                // behavior).
                if let (Some(a), false) = (anchor, shift) {
                    let new_cx = ((pos[0] + a[0]) * 0.5).clamp(0.0, 1.0);
                    let new_cy = ((pos[1] + a[1]) * 0.5).clamp(0.0, 1.0);
                    self.x = new_cx;
                    self.y = new_cy;
                }
                let (sina, cosa) = (self.angle * PI / 180.0).sin_cos();
                let dx = pos[0] - self.x;
                let dy = pos[1] - self.y;
                let local_x = dx * cosa + dy * sina;
                let local_y = -dx * sina + dy * cosa;
                self.rx = local_x.abs().clamp(0.01, 1.0);
                self.ry = local_y.abs().clamp(0.01, 1.0);
            }
            H_ROT => {
                let dx = pos[0] - self.x;
                let dy = pos[1] - self.y;
                self.angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            }
            _ => {}
        }
    }

    pub(super) fn draw(&self, painter: &egui::Painter, xform: &CanvasTransform, selected: bool) {
        let col = if selected {
            egui::Color32::from_rgb(255, 200, 60)
        } else {
            egui::Color32::from_rgb(180, 180, 200)
        };
        let stroke = egui::Stroke::new(if selected { 2.0 } else { 1.0 }, col);
        let (sina, cosa) = (self.angle * PI / 180.0).sin_cos();
        let to_world = |lx: f32, ly: f32| -> egui::Pos2 {
            xform.to_pixel([
                self.x + lx * cosa - ly * sina,
                self.y + lx * sina + ly * cosa,
            ])
        };
        match self.shape_type.as_str() {
            "rectangle" => {
                let p = [
                    to_world(-self.rx, -self.ry),
                    to_world(self.rx, -self.ry),
                    to_world(self.rx, self.ry),
                    to_world(-self.rx, self.ry),
                ];
                for k in 0..4 {
                    painter.line_segment([p[k], p[(k + 1) % 4]], stroke);
                }
            }
            "line" => {
                // Bounded segment from one endpoint to the other along
                // the local X axis. Width (ry) is implied; users see
                // it via the corner handles when selected.
                painter.line_segment([to_world(-self.rx, 0.0), to_world(self.rx, 0.0)], stroke);
            }
            _ => {
                let mut prev = to_world(self.rx, 0.0);
                for k in 1..=24 {
                    let t = k as f32 / 24.0 * std::f32::consts::TAU;
                    let next = to_world(self.rx * t.cos(), self.ry * t.sin());
                    painter.line_segment([prev, next], stroke);
                    prev = next;
                }
            }
        }
    }

    /// Draw the shape's silhouette as a drag-to-create preview: a
    /// translucent yellow fill plus a yellow outline, in the shape
    /// kind the user will get on release. Identical geometry to
    /// `draw`, but distinct styling so the preview reads as
    /// "about-to-create" rather than "selected existing shape".
    pub(super) fn draw_preview(&self, painter: &egui::Painter, xform: &CanvasTransform) {
        let stroke_col = egui::Color32::from_rgb(255, 200, 60);
        let fill_col = egui::Color32::from_rgba_unmultiplied(255, 200, 60, 50);
        let stroke = egui::Stroke::new(1.5, stroke_col);
        let (sina, cosa) = (self.angle * PI / 180.0).sin_cos();
        let to_world = |lx: f32, ly: f32| -> egui::Pos2 {
            xform.to_pixel([
                self.x + lx * cosa - ly * sina,
                self.y + lx * sina + ly * cosa,
            ])
        };
        match self.shape_type.as_str() {
            "rectangle" => {
                let pts = vec![
                    to_world(-self.rx, -self.ry),
                    to_world(self.rx, -self.ry),
                    to_world(self.rx, self.ry),
                    to_world(-self.rx, self.ry),
                ];
                painter.add(egui::Shape::convex_polygon(pts, fill_col, stroke));
            }
            "line" => {
                // Bounded segment from press-point to release-point
                // (rx is half the drag length, so this draws from one
                // drag endpoint to the other).
                painter.line_segment([to_world(-self.rx, 0.0), to_world(self.rx, 0.0)], stroke);
            }
            _ => {
                let n = 32;
                let pts: Vec<egui::Pos2> = (0..n)
                    .map(|k| {
                        let t = k as f32 / n as f32 * std::f32::consts::TAU;
                        to_world(self.rx * t.cos(), self.ry * t.sin())
                    })
                    .collect();
                painter.add(egui::Shape::convex_polygon(pts, fill_col, stroke));
            }
        }
    }
}

fn get_f(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}
