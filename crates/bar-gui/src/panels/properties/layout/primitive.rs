//! Primitive layout item: ellipse / rectangle / ridge.
//!
//! Self-contained -- owns its param read/write, its canvas handles
//! (centre + four corners + rotation), its drag response, and its
//! silhouette drawing. Knows nothing about splines; the layout
//! coordinator (`super`) decides which kind a given item slot is.

use std::collections::HashMap;
use std::f32::consts::PI;

use bar_graph::ParamValue;
use eframe::egui;

use crate::panels::properties::properties_canvas::{CanvasTransform, HandleId, HandleSpec};

pub(super) const H_CENTRE: HandleId = HandleId(0);
pub(super) const H_TL: HandleId = HandleId(1);
pub(super) const H_TR: HandleId = HandleId(2);
pub(super) const H_BL: HandleId = HandleId(3);
pub(super) const H_BR: HandleId = HandleId(4);
pub(super) const H_ROT: HandleId = HandleId(5);

#[derive(Clone)]
pub(super) struct Primitive {
    /// `ellipse` / `rectangle` / `ridge`.
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
        let (cosa, sina) = (self.angle * PI / 180.0).sin_cos();
        let corner = |lx: f32, ly: f32| -> [f32; 2] {
            [
                (self.x + lx * cosa - ly * sina).clamp(-0.5, 1.5),
                (self.y + lx * sina + ly * cosa).clamp(-0.5, 1.5),
            ]
        };
        vec![
            HandleSpec {
                item,
                id: H_CENTRE,
                pos: [self.x, self.y],
                px_radius: 10.0,
            },
            HandleSpec {
                item,
                id: H_TL,
                pos: corner(-self.rx, -self.ry),
                px_radius: 8.0,
            },
            HandleSpec {
                item,
                id: H_TR,
                pos: corner(self.rx, -self.ry),
                px_radius: 8.0,
            },
            HandleSpec {
                item,
                id: H_BL,
                pos: corner(-self.rx, self.ry),
                px_radius: 8.0,
            },
            HandleSpec {
                item,
                id: H_BR,
                pos: corner(self.rx, self.ry),
                px_radius: 8.0,
            },
            HandleSpec {
                item,
                id: H_ROT,
                pos: corner(self.rx * 1.3 + 0.02, 0.0),
                px_radius: 8.0,
            },
        ]
    }

    pub(super) fn apply_drag(&mut self, handle: HandleId, pos: [f32; 2]) {
        match handle {
            H_CENTRE => {
                self.x = pos[0].clamp(0.0, 1.0);
                self.y = pos[1].clamp(0.0, 1.0);
            }
            H_TL | H_TR | H_BL | H_BR => {
                let (cosa, sina) = (self.angle * PI / 180.0).sin_cos();
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
        let (cosa, sina) = (self.angle * PI / 180.0).sin_cos();
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
            "ridge" => {
                painter.line_segment([to_world(-2.0, 0.0), to_world(2.0, 0.0)], stroke);
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
}

fn get_f(params: &HashMap<String, ParamValue>, key: &str, default: f32) -> f32 {
    match params.get(key) {
        Some(ParamValue::Float(v)) => *v,
        _ => default,
    }
}
