//! Spline layout item: a Catmull-Rom control-point sequence.
//!
//! Self-contained -- owns its param read/write, its canvas handles
//! (one per control point), point add/move/delete, and curve drawing.
//! Knows nothing about primitives; the layout coordinator (`super`)
//! decides which kind a given item slot is.

use std::collections::HashMap;

use bar_graph::ParamValue;
use eframe::egui;

use crate::panels::properties::properties_canvas::{CanvasTransform, HandleId, HandleSpec};

#[derive(Clone)]
pub(super) struct Spline {
    pub points: Vec<[f32; 2]>,
    pub closed: bool,
    pub fill: bool,
    pub width: f32,
    pub height: f32,
    pub falloff: f32,
}

impl Spline {
    pub(super) fn read(params: &HashMap<String, ParamValue>, i: usize) -> Self {
        let points = match params.get(&format!("points_{i}")) {
            Some(ParamValue::Spline(p)) => p.clone(),
            _ => Vec::new(),
        };
        Self {
            points,
            closed: matches!(
                params.get(&format!("closed_{i}")),
                Some(ParamValue::Bool(true))
            ),
            fill: matches!(
                params.get(&format!("fill_{i}")),
                Some(ParamValue::Bool(true))
            ),
            width: get_f(params, &format!("width_{i}"), 0.05),
            height: get_f(params, &format!("height_{i}"), 0.5),
            falloff: get_f(params, &format!("falloff_{i}"), 0.5),
        }
    }

    pub(super) fn new() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
            fill: false,
            width: 0.05,
            height: 0.5,
            falloff: 0.5,
        }
    }

    pub(super) fn write(&self, params: &mut HashMap<String, ParamValue>, i: usize) {
        params.insert(
            format!("type_{i}"),
            ParamValue::String("spline".to_string()),
        );
        params.insert(
            format!("points_{i}"),
            ParamValue::Spline(self.points.clone()),
        );
        params.insert(format!("closed_{i}"), ParamValue::Bool(self.closed));
        params.insert(format!("fill_{i}"), ParamValue::Bool(self.fill));
        params.insert(format!("width_{i}"), ParamValue::Float(self.width));
        params.insert(format!("height_{i}"), ParamValue::Float(self.height));
        params.insert(format!("falloff_{i}"), ParamValue::Float(self.falloff));
    }

    /// One handle per control point; the handle id encodes the point
    /// index (splines never use the primitive 0..5 id space).
    pub(super) fn handles(&self, item: usize) -> Vec<HandleSpec> {
        self.points
            .iter()
            .enumerate()
            .map(|(j, p)| HandleSpec {
                item,
                id: HandleId(j as u8),
                pos: *p,
                px_radius: 9.0,
            })
            .collect()
    }

    pub(super) fn add_point(&mut self, pos: [f32; 2]) {
        self.points.push(pos);
    }

    pub(super) fn move_point(&mut self, idx: usize, pos: [f32; 2]) {
        if let Some(p) = self.points.get_mut(idx) {
            p[0] = pos[0].clamp(0.0, 1.0);
            p[1] = pos[1].clamp(0.0, 1.0);
        }
    }

    /// Returns true if a point was removed.
    pub(super) fn remove_point(&mut self, idx: usize) -> bool {
        if idx < self.points.len() {
            self.points.remove(idx);
            true
        } else {
            false
        }
    }

    pub(super) fn draw(&self, painter: &egui::Painter, xform: &CanvasTransform, selected: bool) {
        let col = if selected {
            egui::Color32::from_rgb(255, 200, 60)
        } else {
            egui::Color32::from_rgb(140, 200, 255)
        };
        let stroke = egui::Stroke::new(if selected { 2.0 } else { 1.5 }, col);
        if self.points.len() >= 2 {
            let samples = sample_catmull_rom(&self.points, 24, self.closed || self.fill);
            let mut prev = xform.to_pixel(samples[0]);
            for s in samples.iter().skip(1) {
                let p = xform.to_pixel(*s);
                painter.line_segment([prev, p], stroke);
                prev = p;
            }
        }
        // Draw control points so even a 1-point spline is visible.
        for p in &self.points {
            painter.circle_stroke(xform.to_pixel(*p), 3.0, stroke);
        }
    }
}

/// Catmull-Rom sampler matching the executor's, for the editor preview
/// of spline items. Kept private to this module.
fn sample_catmull_rom(points: &[[f32; 2]], n: usize, closed: bool) -> Vec<[f32; 2]> {
    let m = points.len();
    if m < 2 {
        return points.to_vec();
    }
    let seg_count = if closed { m } else { m - 1 };
    let mut out = Vec::with_capacity(seg_count * n + 1);
    for i in 0..seg_count {
        let p0 = if closed {
            points[(i + m - 1) % m]
        } else if i == 0 {
            [
                2.0 * points[i][0] - points[i + 1][0],
                2.0 * points[i][1] - points[i + 1][1],
            ]
        } else {
            points[i - 1]
        };
        let p1 = points[i];
        let p2 = points[if closed { (i + 1) % m } else { i + 1 }];
        let p3 = if closed {
            points[(i + 2) % m]
        } else if i + 2 >= m {
            [2.0 * p2[0] - p1[0], 2.0 * p2[1] - p1[1]]
        } else {
            points[i + 2]
        };
        for s in 0..n {
            let t = s as f32 / n as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let cx = 0.5
                * ((2.0 * p1[0])
                    + (-p0[0] + p2[0]) * t
                    + (2.0 * p0[0] - 5.0 * p1[0] + 4.0 * p2[0] - p3[0]) * t2
                    + (-p0[0] + 3.0 * p1[0] - 3.0 * p2[0] + p3[0]) * t3);
            let cy = 0.5
                * ((2.0 * p1[1])
                    + (-p0[1] + p2[1]) * t
                    + (2.0 * p0[1] - 5.0 * p1[1] + 4.0 * p2[1] - p3[1]) * t2
                    + (-p0[1] + 3.0 * p1[1] - 3.0 * p2[1] + p3[1]) * t3);
            out.push([cx, cy]);
        }
    }
    if !closed {
        out.push(points[m - 1]);
    }
    out
}

fn get_f(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}
