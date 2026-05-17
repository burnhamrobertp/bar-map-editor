//! Preview layout -- read-only 3D viewport showing the compiled native-resolution
//! BC1 texture.
//!
//! This layout claims only a top info bar. The central panel is left unclaimed
//! so bar-app can fill it with the 3D viewport pointed at the BC1 texture.
//! When BC1 is unavailable (unsupported GPU or no compiled state), the central
//! panel is claimed to show an informative placeholder instead.

use eframe::egui;

use crate::app::BarEditorApp;

/// Draw the Preview layout.
///
/// Only the info bar is drawn here. The central panel is left unclaimed --
/// `bar-app`'s layout manager fills it with the BC1 viewport or the
/// "not compiled" placeholder, depending on project state.
pub fn draw(app: &mut BarEditorApp, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    let is_compiled = app
        .project
        .path
        .as_deref()
        .map(|p| {
            bar_project::PackageDir::open(p)
                .map(|pkg| pkg.is_compiled())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    egui::TopBottomPanel::top("preview_info_bar").show(ctx, |ui| {
        draw_info_bar(app, ui, is_compiled);
    });

    if app.supports_bc && is_compiled {
        app.preview.bc_texture_requested = true;
    }
}

fn draw_info_bar(app: &mut BarEditorApp, ui: &mut egui::Ui, is_compiled: bool) {
    ui.horizontal(|ui| {
        ui.strong("Preview");
        ui.separator();

        if !is_compiled {
            ui.label("Not yet compiled");
        } else if let Some(compiled_at) = app.project.compiled_at {
            let secs = compiled_at.elapsed().as_secs();
            let age = if secs < 60 {
                format!("Compiled {secs}s ago")
            } else if secs < 3600 {
                format!("Compiled {}m ago", secs / 60)
            } else {
                format!("Compiled {}h ago", secs / 3600)
            };
            if app.project.compile_dirty {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "Out of date");
                ui.separator();
                ui.weak(&age);
            } else {
                ui.label(age);
            }
        } else {
            if app.project.compile_dirty {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 40), "Out of date");
            } else {
                ui.label("Compiled");
            }
        }
    });
}

/// Corner radius of the animated border + its base outline. The
/// `perimeter_point` walk uses it to arc through the corners cleanly
/// instead of cutting straight across them.
const BORDER_CORNER_R: f32 = 5.0;

/// Draw a bright segment travelling clockwise around `rect` to
/// indicate activity. `rect` should be the button's own rect; the
/// border renders flush with the button edge (no gap) and shares the
/// `BORDER_CORNER_R` corner radius so the animated outline reads as
/// part of the button rather than a separate frame.
pub fn draw_animated_border(ui: &mut egui::Ui, rect: egui::Rect) {
    let time = ui.input(|i| i.time) as f32;
    let phase = (time * 0.75).fract();

    let perimeter = rounded_perimeter(rect, BORDER_CORNER_R);
    let segment_len = perimeter * 0.4;
    let head = phase * perimeter;

    let painter = ui.painter();

    // Dim base outline at the button's own corner radius so it traces the
    // visible button edge instead of floating outside it.
    painter.rect_stroke(
        rect,
        BORDER_CORNER_R,
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(80, 140, 255, 50)),
        egui::StrokeKind::Inside,
    );

    // Travelling glow: N short segments fading from head to tail.
    let steps = 36usize;
    for i in 0..steps {
        let t0 = (head - segment_len * (i as f32 / steps as f32)).rem_euclid(perimeter);
        let t1 = (head - segment_len * ((i + 1) as f32 / steps as f32)).rem_euclid(perimeter);
        // Skip the one segment that wraps around the 0/perimeter seam --
        // it would otherwise draw a diagonal across the rect.
        if t0 < t1 {
            continue;
        }
        let p0 = perimeter_point(rect, BORDER_CORNER_R, t0 / perimeter);
        let p1 = perimeter_point(rect, BORDER_CORNER_R, t1 / perimeter);
        let alpha = ((1.0 - i as f32 / steps as f32) * 230.0) as u8;
        painter.line_segment(
            [p0, p1],
            egui::Stroke::new(
                2.0,
                egui::Color32::from_rgba_unmultiplied(120, 180, 255, alpha),
            ),
        );
    }

    ui.ctx().request_repaint();
}

/// Total perimeter of a rounded rectangle with corner radius `r`.
/// Each sharp corner contributes `2r` of straight edge; each rounded
/// corner replaces that with a quarter-arc of length `(π/2)·r`. Net
/// difference per corner: `r·(π/2 - 2)` (slightly shorter than the
/// sharp-corner version).
fn rounded_perimeter(rect: egui::Rect, r: f32) -> f32 {
    let w = rect.width();
    let h = rect.height();
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    2.0 * (w - 2.0 * r) + 2.0 * (h - 2.0 * r) + 2.0 * std::f32::consts::PI * r
}

/// Map `t` in [0, 1) to a point on the perimeter of `rect` with
/// corner radius `r`, walking clockwise from the top edge.
///
/// Walk order: top edge -> top-right arc -> right edge -> bottom-right
/// arc -> bottom edge -> bottom-left arc -> left edge -> top-left arc.
/// Each arc spans π/2 radians; arc-length parameter `s` maps to angle
/// `s/r`. Reduces to the sharp-cornered rectangle when r = 0.
fn perimeter_point(rect: egui::Rect, r: f32, t: f32) -> egui::Pos2 {
    let w = rect.width();
    let h = rect.height();
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let perimeter = rounded_perimeter(rect, r);

    let l_edge_top = w - 2.0 * r;
    let l_edge_side = h - 2.0 * r;
    let l_arc = std::f32::consts::FRAC_PI_2 * r;

    let mut pos = t * perimeter;

    // Top edge: (left + r, top) -> (right - r, top).
    if pos < l_edge_top {
        return egui::pos2(rect.left() + r + pos, rect.top());
    }
    pos -= l_edge_top;

    // Top-right arc. Center (right - r, top + r); parameter
    // theta in [0, pi/2], point = center + (r*sin(theta), -r*cos(theta)).
    if pos < l_arc {
        let theta = if r > 0.0 { pos / r } else { 0.0 };
        let cx = rect.right() - r;
        let cy = rect.top() + r;
        return egui::pos2(cx + r * theta.sin(), cy - r * theta.cos());
    }
    pos -= l_arc;

    // Right edge: (right, top + r) -> (right, bottom - r).
    if pos < l_edge_side {
        return egui::pos2(rect.right(), rect.top() + r + pos);
    }
    pos -= l_edge_side;

    // Bottom-right arc. Center (right - r, bottom - r); point =
    // center + (r*cos(theta), r*sin(theta)).
    if pos < l_arc {
        let theta = if r > 0.0 { pos / r } else { 0.0 };
        let cx = rect.right() - r;
        let cy = rect.bottom() - r;
        return egui::pos2(cx + r * theta.cos(), cy + r * theta.sin());
    }
    pos -= l_arc;

    // Bottom edge: (right - r, bottom) -> (left + r, bottom).
    if pos < l_edge_top {
        return egui::pos2(rect.right() - r - pos, rect.bottom());
    }
    pos -= l_edge_top;

    // Bottom-left arc. Center (left + r, bottom - r); point =
    // center + (-r*sin(theta), r*cos(theta)).
    if pos < l_arc {
        let theta = if r > 0.0 { pos / r } else { 0.0 };
        let cx = rect.left() + r;
        let cy = rect.bottom() - r;
        return egui::pos2(cx - r * theta.sin(), cy + r * theta.cos());
    }
    pos -= l_arc;

    // Left edge: (left, bottom - r) -> (left, top + r).
    if pos < l_edge_side {
        return egui::pos2(rect.left(), rect.bottom() - r - pos);
    }
    pos -= l_edge_side;

    // Top-left arc. Center (left + r, top + r); point =
    // center + (-r*cos(theta), -r*sin(theta)).
    let theta = if r > 0.0 { pos / r } else { 0.0 };
    let cx = rect.left() + r;
    let cy = rect.top() + r;
    egui::pos2(cx - r * theta.cos(), cy - r * theta.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h))
    }

    fn approx(a: egui::Pos2, b: egui::Pos2) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[test]
    fn rounded_perimeter_zero_radius_equals_2wh() {
        // r = 0 -> rounded perimeter degenerates to sharp-corner case.
        let p = rounded_perimeter(rect(100.0, 50.0), 0.0);
        assert!((p - 300.0).abs() < 1e-3);
    }

    #[test]
    fn rounded_perimeter_shorter_than_sharp() {
        // Each rounded corner is shorter than the two `r`-length
        // straight segments it replaces (chord < arc only for inward
        // arcs; here the corner cuts inside the sharp version).
        let r = 5.0;
        let sharp = 2.0 * (100.0 + 50.0);
        let rounded = rounded_perimeter(rect(100.0, 50.0), r);
        assert!(
            rounded < sharp,
            "rounded ({rounded}) should be < sharp ({sharp})"
        );
    }

    #[test]
    fn perimeter_walk_starts_at_top_edge_after_tl_arc() {
        // t = 0 -> first point on the top-edge run, which begins at
        // (left + r, top) (i.e. immediately after the top-left arc
        // ends).
        let r = 5.0;
        let p = perimeter_point(rect(100.0, 50.0), r, 0.0);
        assert!(approx(p, egui::pos2(r, 0.0)), "expected (r, 0), got {p:?}");
    }

    #[test]
    fn perimeter_walk_hits_each_arc_midpoint() {
        // Midpoint of each quarter-arc lies on the 45-degree diagonal
        // from the corner center, at distance r. Verifies that the
        // walk does NOT cut straight across corners (the original
        // bug). For r = 5 in a 100x50 rect, the TR corner center is
        // (95, 5); midpoint of the TR arc is (95 + r*sin(pi/4), 5 -
        // r*cos(pi/4)) ~= (98.536, 1.464).
        let r = 5.0;
        let w = 100.0;
        let h = 50.0;
        let rect_ = rect(w, h);
        let perim = rounded_perimeter(rect_, r);
        let l_edge_top = w - 2.0 * r;
        let l_arc = std::f32::consts::FRAC_PI_2 * r;

        // TR mid: top edge + half arc.
        let t_tr_mid = (l_edge_top + l_arc * 0.5) / perim;
        let tr_mid = perimeter_point(rect_, r, t_tr_mid);
        let s = std::f32::consts::FRAC_1_SQRT_2; // sin(pi/4) = cos(pi/4)
        let cx = w - r;
        let cy = r;
        assert!(
            approx(tr_mid, egui::pos2(cx + r * s, cy - r * s)),
            "TR arc midpoint: expected ({}, {}), got {tr_mid:?}",
            cx + r * s,
            cy - r * s,
        );
        // Distance from corner center should equal r (point sits on
        // the arc, not inside / outside the rounded edge).
        let dx = tr_mid.x - cx;
        let dy = tr_mid.y - cy;
        let d = (dx * dx + dy * dy).sqrt();
        assert!(
            (d - r).abs() < 1e-3,
            "TR arc midpoint distance from center: expected {r}, got {d}"
        );
    }

    #[test]
    fn perimeter_walk_stays_inside_bounding_rect() {
        // No sampled point should ever lie outside the rect (the bug
        // was the glow CUTTING across corners, i.e. inside; verify it
        // also doesn't escape outward).
        let r = 5.0;
        let rect_ = rect(120.0, 60.0);
        for i in 0..200 {
            let t = i as f32 / 200.0;
            let p = perimeter_point(rect_, r, t);
            assert!(
                p.x >= rect_.left() - 1e-3 && p.x <= rect_.right() + 1e-3,
                "x out of bounds at t = {t}: {p:?}"
            );
            assert!(
                p.y >= rect_.top() - 1e-3 && p.y <= rect_.bottom() + 1e-3,
                "y out of bounds at t = {t}: {p:?}"
            );
        }
    }
}
