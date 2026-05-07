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

/// Checkmark icon for the Validate toolbar button.
pub(crate) fn paint_validate_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(2.5, color);
    let cx = rect.center().x;
    let cy = rect.center().y;
    painter.line_segment(
        [egui::pos2(cx - 8.0, cy + 2.0), egui::pos2(cx - 2.0, cy + 7.0)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx - 2.0, cy + 7.0), egui::pos2(cx + 9.0, cy - 6.0)],
        stroke,
    );
}

/// "BAR" wordmark for the Test-in-BAR toolbar button.
pub(crate) fn paint_bar_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "BAR",
        egui::FontId::monospace(14.0),
        color,
    );
}

/// "Document with pencil" icon for the Edit Map Info toolbar button.
pub(crate) fn paint_map_info_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.8, color);
    let cx = rect.center().x;
    let cy = rect.center().y;

    let pw = 10.0_f32;
    let ph = 13.0_f32;
    let px = cx - pw / 2.0 - 2.0;
    let py = cy - ph / 2.0;
    painter.line_segment([egui::pos2(px, py), egui::pos2(px + pw - 3.0, py)], stroke);
    painter.line_segment([egui::pos2(px + pw - 3.0, py), egui::pos2(px + pw, py + 3.0)], stroke);
    painter.line_segment([egui::pos2(px + pw, py + 3.0), egui::pos2(px + pw, py + ph)], stroke);
    painter.line_segment([egui::pos2(px + pw, py + ph), egui::pos2(px, py + ph)], stroke);
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
pub(crate) fn paint_export_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(2.0, color);
    let cx = rect.center().x;
    let cy = rect.center().y;

    let bw = 10.0_f32;
    let bh = 7.0_f32;
    let by = cy + 3.0;
    let gap = 3.5_f32;

    painter.line_segment([egui::pos2(cx - bw, by), egui::pos2(cx - bw, by + bh)], stroke);
    painter.line_segment([egui::pos2(cx - bw, by + bh), egui::pos2(cx + bw, by + bh)], stroke);
    painter.line_segment([egui::pos2(cx + bw, by + bh), egui::pos2(cx + bw, by)], stroke);
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
    painter.add(egui::Shape::closed_line(points, egui::Stroke::new(1.6, color)));
    painter.circle_stroke(egui::pos2(cx, cy), hub_r, egui::Stroke::new(1.6, color));
}

/// Outer rectangle + two corner filled squares, representing a 2-team map.
pub(crate) fn paint_startbox_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
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

/// Rotating dot in the top-right corner of a button rect to signal a running
/// operation. `time` is `ui.input(|i| i.time)` in seconds.
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

/// Arrow-in-frame icon for SubgraphInput / SubgraphOutput nodes.
///
/// 24×24-viewBox source — a rounded square frame at (3.5, 3.5, 17×17, rx=4)
/// plus an arrow path:
///   - input  : `M7 12H14 M11 9 L14 12 L11 15`
///   - output : `M10 12H17 M14 9 L17 12 L14 15`
pub(crate) fn draw_io_icon(painter: &egui::Painter, rect: egui::Rect, is_input: bool) {
    let scale_x = rect.width() / 24.0;
    let scale_y = rect.height() / 24.0;
    let to_local = |x: f32, y: f32| {
        egui::pos2(rect.left() + x * scale_x, rect.top() + y * scale_y)
    };
    // Stroke width scales with the average dimension so non-square icon rects
    // don't end up with anisotropic strokes.
    let stroke_w = (1.5 * (scale_x + scale_y) * 0.5).max(1.0);
    let stroke = egui::Stroke::new(stroke_w, tokens::ICON_STROKE);

    let frame = egui::Rect::from_min_max(to_local(3.5, 3.5), to_local(20.5, 20.5));
    let frame_radius = ((4.0 * scale_x).round() as u8).max(1);
    painter.rect_stroke(frame, frame_radius, stroke, egui::StrokeKind::Inside);

    let (shaft_start, shaft_end, head_top, head_tip, head_bot) = if is_input {
        ((7.0, 12.0), (14.0, 12.0), (11.0, 9.0), (14.0, 12.0), (11.0, 15.0))
    } else {
        ((10.0, 12.0), (17.0, 12.0), (14.0, 9.0), (17.0, 12.0), (14.0, 15.0))
    };
    painter.line_segment(
        [to_local(shaft_start.0, shaft_start.1), to_local(shaft_end.0, shaft_end.1)],
        stroke,
    );
    painter.line_segment(
        [to_local(head_top.0, head_top.1), to_local(head_tip.0, head_tip.1)],
        stroke,
    );
    painter.line_segment(
        [to_local(head_bot.0, head_bot.1), to_local(head_tip.0, head_tip.1)],
        stroke,
    );
}
