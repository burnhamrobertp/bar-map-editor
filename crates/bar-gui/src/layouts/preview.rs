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

/// Draw a bright segment travelling clockwise around `rect` to indicate activity.
pub fn draw_animated_border(ui: &mut egui::Ui, rect: egui::Rect) {
    let time = ui.input(|i| i.time) as f32;
    let phase = (time * 0.75).fract();

    let rect = rect.expand(2.0);
    let w = rect.width();
    let h = rect.height();
    let perimeter = 2.0 * (w + h);
    let segment_len = perimeter * 0.4;
    let head = phase * perimeter;

    let painter = ui.painter();

    // Dim base outline so the button has a visible border even between frames.
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(80, 140, 255, 50)),
        egui::StrokeKind::Outside,
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
        let p0 = perimeter_point(rect, t0 / perimeter);
        let p1 = perimeter_point(rect, t1 / perimeter);
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

/// Map `t` in [0, 1) to a point on the perimeter of `rect`, clockwise from
/// the top-left corner.
fn perimeter_point(rect: egui::Rect, t: f32) -> egui::Pos2 {
    let w = rect.width();
    let h = rect.height();
    let pos = t * 2.0 * (w + h);
    if pos < w {
        egui::pos2(rect.left() + pos, rect.top())
    } else if pos < w + h {
        egui::pos2(rect.right(), rect.top() + pos - w)
    } else if pos < 2.0 * w + h {
        egui::pos2(rect.right() - (pos - w - h), rect.bottom())
    } else {
        egui::pos2(rect.left(), rect.bottom() - (pos - 2.0 * w - h))
    }
}
