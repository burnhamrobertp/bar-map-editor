//! Toolbar and canvas icon painting.
//!
//! All toolbar icons are drawn as egui painter primitives so they stay crisp at
//! any DPI without a rasterisation step. Each function accepts a `rect` that
//! defines the drawing area and a `color` parameter so the caller can tint the
//! icon for hover/active states.
//!
//! The `draw_io_icon` function is used by the node canvas for SubgraphInput /
//! SubgraphOutput node decorations and follows the same convention.

use eframe::egui;

use crate::panels::tokens;

/// "BAR" wordmark for the Test-in-BAR toolbar button.
pub(crate) fn paint_bar_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "BAR",
        egui::FontId::monospace(14.0),
        color,
    );
}

/// "Document with pencil" icon for the Edit Map Info toolbar button.
pub(crate) fn paint_map_info_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.8, color);
    let cx = rect.center().x;
    let cy = rect.center().y;

    let pw = 10.0_f32;
    let ph = 13.0_f32;
    let px = cx - pw / 2.0 - 2.0;
    let py = cy - ph / 2.0;
    painter.line_segment([egui::pos2(px, py), egui::pos2(px + pw - 3.0, py)], stroke);
    painter.line_segment(
        [egui::pos2(px + pw - 3.0, py), egui::pos2(px + pw, py + 3.0)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(px + pw, py + 3.0), egui::pos2(px + pw, py + ph)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(px + pw, py + ph), egui::pos2(px, py + ph)],
        stroke,
    );
    painter.line_segment([egui::pos2(px, py + ph), egui::pos2(px, py)], stroke);

    for i in 0..3 {
        let y = py + 4.0 + i as f32 * 3.0;
        painter.line_segment(
            [egui::pos2(px + 2.0, y), egui::pos2(px + pw - 3.0, y)],
            egui::Stroke::new(1.2, color),
        );
    }

    let tip = egui::pos2(rect.right() - 5.0, rect.bottom() - 5.0);
    let base = egui::pos2(tip.x - 6.0, tip.y - 6.0);
    painter.line_segment([base, tip], egui::Stroke::new(2.5, color));
    painter.circle_filled(tip, 1.5, color);
}

/// Upload-arrow icon for the Export / Run toolbar button.
pub(crate) fn paint_export_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(2.0, color);
    let cx = rect.center().x;
    let cy = rect.center().y;

    let bw = 10.0_f32;
    let bh = 7.0_f32;
    let by = cy + 3.0;
    let gap = 3.5_f32;

    painter.line_segment(
        [egui::pos2(cx - bw, by), egui::pos2(cx - bw, by + bh)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx - bw, by + bh), egui::pos2(cx + bw, by + bh)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx + bw, by + bh), egui::pos2(cx + bw, by)],
        stroke,
    );
    painter.line_segment([egui::pos2(cx - bw, by), egui::pos2(cx - gap, by)], stroke);
    painter.line_segment([egui::pos2(cx + gap, by), egui::pos2(cx + bw, by)], stroke);

    let tip_y = cy - 8.0;
    painter.line_segment([egui::pos2(cx, by), egui::pos2(cx, tip_y)], stroke);

    let aw = 5.5_f32;
    let ah = 4.5_f32;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(cx, tip_y - ah),
            egui::pos2(cx - aw, tip_y),
            egui::pos2(cx + aw, tip_y),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// Gear icon for the Map Settings (structured mapinfo form) toolbar button.
pub(crate) fn paint_mapinfo_form_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let teeth = 8usize;
    let outer_r = 9.0_f32;
    let inner_r = 6.5_f32;
    let hub_r = 2.6_f32;
    let half_tooth = std::f32::consts::TAU / (teeth as f32 * 4.0);
    let step = std::f32::consts::TAU / teeth as f32;

    let mut points = Vec::with_capacity(teeth * 4);
    let pt = |r: f32, angle: f32| egui::pos2(cx + r * angle.cos(), cy + r * angle.sin());
    for i in 0..teeth {
        let a = step * i as f32;
        let a_next = step * (i + 1) as f32;
        points.push(pt(outer_r, a - half_tooth));
        points.push(pt(outer_r, a + half_tooth));
        points.push(pt(inner_r, a + half_tooth));
        points.push(pt(inner_r, a_next - half_tooth));
    }
    painter.add(egui::Shape::closed_line(
        points,
        egui::Stroke::new(1.6, color),
    ));
    painter.circle_stroke(egui::pos2(cx, cy), hub_r, egui::Stroke::new(1.6, color));
}

/// Outer rectangle + two corner filled squares, representing a 2-team map.
pub(crate) fn paint_startbox_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let half = 8.0_f32;
    let stroke = egui::Stroke::new(1.6, color);
    let outer = egui::Rect::from_min_max(
        egui::pos2(cx - half, cy - half),
        egui::pos2(cx + half, cy + half),
    );
    painter.rect_stroke(outer, 2.0, stroke, egui::StrokeKind::Inside);

    let box_size = 4.5_f32;
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(cx - half + 1.5, cy - half + 1.5),
            egui::pos2(cx - half + 1.5 + box_size, cy - half + 1.5 + box_size),
        ),
        1.0,
        color,
    );
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(cx + half - 1.5 - box_size, cy + half - 1.5 - box_size),
            egui::pos2(cx + half - 1.5, cy + half - 1.5),
        ),
        1.0,
        color,
    );
}

/// Map-pin teardrop for the 2D Inspector toolbar button.
pub(crate) fn paint_inspector_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    painter.circle_stroke(egui::pos2(cx, cy - 3.0), 6.0, egui::Stroke::new(2.0, color));
    painter.circle_filled(egui::pos2(cx, cy - 3.0), 2.5, color);
    painter.line_segment(
        [egui::pos2(cx - 4.5, cy + 1.5), egui::pos2(cx, cy + 9.0)],
        egui::Stroke::new(2.0, color),
    );
    painter.line_segment(
        [egui::pos2(cx + 4.5, cy + 1.5), egui::pos2(cx, cy + 9.0)],
        egui::Stroke::new(2.0, color),
    );
}

/// Hammer glyph for the Compile toolbar button. A rotated rectangular head
/// at the upper-left and a thin handle running diagonally down to the
/// lower-right. The previous icon was a lightning bolt, which read as
/// "fast / energy" rather than the "build" semantics of the compile action.
pub(crate) fn paint_compile_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    // Hammer head: a quad tilted ~25 degrees so it reads as a head, not a
    // brick. Sits in the upper-left of the icon area.
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(cx - 6.5, cy - 5.5),
            egui::pos2(cx - 0.5, cy - 8.0),
            egui::pos2(cx + 1.5, cy - 3.5),
            egui::pos2(cx - 4.5, cy - 1.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
    // Handle: a thin diagonal bar running from just under the head down to
    // the lower-right of the icon area.
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(cx - 3.5, cy - 0.5),
            egui::pos2(cx - 1.0, cy - 2.5),
            egui::pos2(cx + 6.0, cy + 6.0),
            egui::pos2(cx + 3.5, cy + 8.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// Rotating dot in the top-right corner of a button rect to signal a running
/// operation. `time` is `ui.input(|i| i.time)` in seconds. Unused right
/// now that the export button no longer lives on the FC node body --
/// retained because the top action bar may need the same affordance.
#[allow(dead_code)]
pub(crate) fn paint_busy_dot(painter: &egui::Painter, rect: egui::Rect, time: f64) {
    let radius = 3.0_f32;
    let orbit = 6.0_f32;
    let center = egui::pos2(rect.right() - 8.0, rect.top() + 8.0);
    let phase = (time * std::f64::consts::PI) as f32;
    for i in 0..3 {
        let p = phase - i as f32 * 0.6;
        let pos = egui::pos2(center.x + p.cos() * orbit, center.y + p.sin() * orbit);
        let alpha = 240u8.saturating_sub(i as u8 * 80);
        painter.circle_filled(pos, radius, egui::Color32::from_white_alpha(alpha));
    }
}

/// Circular arrow (refresh) icon.
pub fn paint_refresh_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let r = (rect.height().min(rect.width()) * 0.38).max(3.0);
    let stroke = egui::Stroke::new(1.5, color);
    const STEPS: usize = 18;
    let start = std::f32::consts::FRAC_PI_3;
    let end = start + std::f32::consts::PI * 1.5;
    let pts: Vec<egui::Pos2> = (0..=STEPS)
        .map(|i| {
            let a = start + (end - start) * i as f32 / STEPS as f32;
            egui::pos2(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    for w in pts.windows(2) {
        painter.line_segment([w[0], w[1]], stroke);
    }
    let tip = *pts.last().unwrap();
    let prev = pts[pts.len() - 2];
    let dx = tip.x - prev.x;
    let dy = tip.y - prev.y;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (nx, ny) = (dx / len, dy / len);
    let (px, py) = (-ny, nx);
    let arm = r * 0.45;
    painter.line_segment(
        [
            tip,
            egui::pos2(
                tip.x - nx * arm + px * arm * 0.5,
                tip.y - ny * arm + py * arm * 0.5,
            ),
        ],
        stroke,
    );
    painter.line_segment(
        [
            tip,
            egui::pos2(
                tip.x - nx * arm - px * arm * 0.5,
                tip.y - ny * arm - py * arm * 0.5,
            ),
        ],
        stroke,
    );
}

/// X mark for error severity.
pub(crate) fn paint_severity_error(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let r = (rect.height().min(rect.width()) * 0.35).max(2.0);
    let stroke = egui::Stroke::new(2.0, color);
    painter.line_segment(
        [egui::pos2(cx - r, cy - r), egui::pos2(cx + r, cy + r)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx + r, cy - r), egui::pos2(cx - r, cy + r)],
        stroke,
    );
}

/// Triangle with exclamation mark for warning severity.
pub(crate) fn paint_severity_warning(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let half = (rect.height().min(rect.width()) * 0.45).max(3.0);
    painter.add(egui::Shape::closed_line(
        vec![
            egui::pos2(cx, cy - half),
            egui::pos2(cx + half * 0.866, cy + half * 0.5),
            egui::pos2(cx - half * 0.866, cy + half * 0.5),
        ],
        egui::Stroke::new(1.5, color),
    ));
    let top = cy - half * 0.25;
    let bot = cy + half * 0.15;
    painter.line_segment(
        [egui::pos2(cx, top), egui::pos2(cx, bot)],
        egui::Stroke::new(1.5, color),
    );
    painter.circle_filled(egui::pos2(cx, cy + half * 0.38), 0.9, color);
}

/// Circle with 'i' for info severity.
pub(crate) fn paint_severity_info(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let r = (rect.height().min(rect.width()) * 0.42).max(3.0);
    painter.circle_stroke(egui::pos2(cx, cy), r, egui::Stroke::new(1.5, color));
    painter.circle_filled(egui::pos2(cx, cy - r * 0.38), 1.0, color);
    painter.line_segment(
        [egui::pos2(cx, cy - r * 0.1), egui::pos2(cx, cy + r * 0.42)],
        egui::Stroke::new(1.5, color),
    );
}

/// Folder icon for directory headers.
pub(crate) fn paint_folder_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let w = rect.width().min(14.0);
    let h = rect.height().min(12.0);
    let body = egui::Rect::from_min_max(
        egui::pos2(cx - w * 0.5, cy - h * 0.2),
        egui::pos2(cx + w * 0.5, cy + h * 0.5),
    );
    painter.rect_stroke(
        body,
        1.0,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Inside,
    );
    let tab = egui::Rect::from_min_max(
        egui::pos2(cx - w * 0.5, cy - h * 0.5),
        egui::pos2(cx - w * 0.5 + w * 0.38, cy - h * 0.2),
    );
    painter.rect_filled(tab, 1.0, color);
}

/// Diagonal pencil icon for edit actions.
pub(crate) fn paint_pencil_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.5, color);
    let size = (rect.height().min(rect.width()) * 0.36).max(2.0);
    let tip = egui::pos2(cx - size, cy + size);
    let top = egui::pos2(cx + size * 0.7, cy - size * 0.7);
    // Perpendicular offset for shaft width (45-degree pencil)
    let (px, py) = (0.707_f32 * 1.5, -0.707_f32 * 1.5);
    // Two shaft edges
    painter.line_segment(
        [
            egui::pos2(top.x + px, top.y + py),
            egui::pos2(tip.x + px, tip.y + py),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(top.x - px, top.y - py),
            egui::pos2(tip.x - px, tip.y - py),
        ],
        stroke,
    );
    // Eraser cap at top
    painter.line_segment(
        [
            egui::pos2(top.x + px, top.y + py),
            egui::pos2(top.x - px, top.y - py),
        ],
        egui::Stroke::new(2.0, color),
    );
    // Tip lines converging to point
    painter.line_segment([egui::pos2(tip.x + px, tip.y + py), tip], stroke);
    painter.line_segment([egui::pos2(tip.x - px, tip.y - py), tip], stroke);
}

/// Large X for cancel/drop indicators (called directly on a painter with explicit position).
pub(crate) fn paint_cancel_x(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(2.5, color);
    painter.line_segment(
        [
            egui::pos2(center.x - radius, center.y - radius),
            egui::pos2(center.x + radius, center.y + radius),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + radius, center.y - radius),
            egui::pos2(center.x - radius, center.y + radius),
        ],
        stroke,
    );
}

/// Filled triangle for collapsible section headers. Points down when open, right when closed.
pub(crate) fn paint_triangle_arrow(
    painter: &egui::Painter,
    rect: egui::Rect,
    open: bool,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let half = (rect.height().min(rect.width()) * 0.38).max(2.0);
    let pts = if open {
        vec![
            egui::pos2(cx - half, cy - half * 0.55),
            egui::pos2(cx + half, cy - half * 0.55),
            egui::pos2(cx, cy + half * 0.7),
        ]
    } else {
        vec![
            egui::pos2(cx - half * 0.55, cy - half),
            egui::pos2(cx + half * 0.7, cy),
            egui::pos2(cx - half * 0.55, cy + half),
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// Arrow-in-frame icon for SubgraphInput / SubgraphOutput nodes.
///
/// 24×24-viewBox source — a rounded square frame at (3.5, 3.5, 17×17, rx=4)
/// plus an arrow path:
///   - input  : `M7 12H14 M11 9 L14 12 L11 15`
///   - output : `M10 12H17 M14 9 L17 12 L14 15`
pub(crate) fn draw_io_icon(painter: &egui::Painter, rect: egui::Rect, is_input: bool) {
    let scale_x = rect.width() / 24.0;
    let scale_y = rect.height() / 24.0;
    let to_local = |x: f32, y: f32| egui::pos2(rect.left() + x * scale_x, rect.top() + y * scale_y);
    // Stroke width scales with the average dimension so non-square icon rects
    // don't end up with anisotropic strokes.
    let stroke_w = (1.5 * (scale_x + scale_y) * 0.5).max(1.0);
    let stroke = egui::Stroke::new(stroke_w, tokens::ICON_STROKE);

    let frame = egui::Rect::from_min_max(to_local(3.5, 3.5), to_local(20.5, 20.5));
    let frame_radius = ((4.0 * scale_x).round() as u8).max(1);
    painter.rect_stroke(frame, frame_radius, stroke, egui::StrokeKind::Inside);

    let (shaft_start, shaft_end, head_top, head_tip, head_bot) = if is_input {
        (
            (7.0, 12.0),
            (14.0, 12.0),
            (11.0, 9.0),
            (14.0, 12.0),
            (11.0, 15.0),
        )
    } else {
        (
            (10.0, 12.0),
            (17.0, 12.0),
            (14.0, 9.0),
            (17.0, 12.0),
            (14.0, 15.0),
        )
    };
    painter.line_segment(
        [
            to_local(shaft_start.0, shaft_start.1),
            to_local(shaft_end.0, shaft_end.1),
        ],
        stroke,
    );
    painter.line_segment(
        [
            to_local(head_top.0, head_top.1),
            to_local(head_tip.0, head_tip.1),
        ],
        stroke,
    );
    painter.line_segment(
        [
            to_local(head_bot.0, head_bot.1),
            to_local(head_tip.0, head_tip.1),
        ],
        stroke,
    );
}

/// Trash-can icon for delete buttons. Drawn with line segments so it
/// stays crisp at any DPI and tints cleanly via the `color` argument.
/// Sized to fit a ~20-pixel-square rect.
pub fn paint_trash_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.4, color);
    let cx = rect.center().x;
    let cy = rect.center().y;

    // Handle: short horizontal cap above the lid.
    let handle_half_w = 2.0;
    let handle_y = cy - 6.0;
    painter.line_segment(
        [
            egui::pos2(cx - handle_half_w, handle_y),
            egui::pos2(cx + handle_half_w, handle_y),
        ],
        stroke,
    );

    // Lid: wider horizontal line just below the handle.
    let lid_half_w = 5.0;
    let lid_y = cy - 4.0;
    painter.line_segment(
        [
            egui::pos2(cx - lid_half_w, lid_y),
            egui::pos2(cx + lid_half_w, lid_y),
        ],
        stroke,
    );

    // Body: slightly tapered trapezoid below the lid.
    let body_top_half_w = 4.0;
    let body_bot_half_w = 3.0;
    let body_top_y = lid_y + 1.0;
    let body_bot_y = body_top_y + 8.0;
    painter.line_segment(
        [
            egui::pos2(cx - body_top_half_w, body_top_y),
            egui::pos2(cx - body_bot_half_w, body_bot_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(cx + body_top_half_w, body_top_y),
            egui::pos2(cx + body_bot_half_w, body_bot_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(cx - body_bot_half_w, body_bot_y),
            egui::pos2(cx + body_bot_half_w, body_bot_y),
        ],
        stroke,
    );

    // Two short slats inside the body to read clearly as a trash bin.
    let slat_top_y = body_top_y + 1.5;
    let slat_bot_y = body_bot_y - 1.0;
    let slat_offset = 1.5;
    painter.line_segment(
        [
            egui::pos2(cx - slat_offset, slat_top_y),
            egui::pos2(cx - slat_offset, slat_bot_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(cx + slat_offset, slat_top_y),
            egui::pos2(cx + slat_offset, slat_bot_y),
        ],
        stroke,
    );
}
