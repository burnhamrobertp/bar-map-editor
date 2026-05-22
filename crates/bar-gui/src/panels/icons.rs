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

/// Dashed outer rectangle wrapping a solid inner square. Reads as
/// "playable area surrounded by the map-edge extension"; used for the
/// Map Edge action-bar button.
pub(crate) fn paint_map_edge_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let outer_half = 8.5_f32;
    let inner_half = 4.0_f32;
    let stroke = egui::Stroke::new(1.4, color);

    // Outer dashed frame: four dashes per side, drawn as short line
    // segments rather than a single stroke so it reads as "extends
    // beyond" rather than a solid border.
    let dash_count = 4usize;
    let dash_len = (outer_half * 2.0) / (dash_count as f32 * 2.0 - 1.0);
    for i in 0..dash_count {
        let t0 = i as f32 * 2.0 * dash_len;
        let t1 = t0 + dash_len;
        // Top edge.
        painter.line_segment(
            [
                egui::pos2(cx - outer_half + t0, cy - outer_half),
                egui::pos2(cx - outer_half + t1, cy - outer_half),
            ],
            stroke,
        );
        // Bottom edge.
        painter.line_segment(
            [
                egui::pos2(cx - outer_half + t0, cy + outer_half),
                egui::pos2(cx - outer_half + t1, cy + outer_half),
            ],
            stroke,
        );
        // Left edge.
        painter.line_segment(
            [
                egui::pos2(cx - outer_half, cy - outer_half + t0),
                egui::pos2(cx - outer_half, cy - outer_half + t1),
            ],
            stroke,
        );
        // Right edge.
        painter.line_segment(
            [
                egui::pos2(cx + outer_half, cy - outer_half + t0),
                egui::pos2(cx + outer_half, cy - outer_half + t1),
            ],
            stroke,
        );
    }

    // Inner solid square: the playable area.
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(cx - inner_half, cy - inner_half),
            egui::pos2(cx + inner_half, cy + inner_half),
        ),
        1.0,
        color,
    );
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

/// Terminal prompt `>_` glyph for the Compile toolbar button.
/// Reads as "build/run a command" -- distinct from the Run button's
/// play-triangle and the lighting/gear icons.
pub(crate) fn paint_compile_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(2.0, color);
    // `>` chevron on the left half
    painter.line_segment(
        [egui::pos2(cx - 6.5, cy - 4.5), egui::pos2(cx - 1.5, cy)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx - 1.5, cy), egui::pos2(cx - 6.5, cy + 4.5)],
        stroke,
    );
    // `_` cursor on the right half
    painter.line_segment(
        [
            egui::pos2(cx - 0.5, cy + 4.5),
            egui::pos2(cx + 6.5, cy + 4.5),
        ],
        stroke,
    );
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
/// Geometry is derived from `rect`, so the icon fills its bounding
/// box at any size (matches the visual weight of other painter icons
/// like `paint_info_icon`).
pub fn paint_trash_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.4, color);
    let cx = rect.center().x;
    let s = rect.width().min(rect.height());

    // Vertical extents: handle at the top, body bottom near the rect
    // floor. Leaves ~1px padding on top + bottom.
    let top = rect.top() + s * 0.06;
    let bot = rect.bottom() - s * 0.06;
    let lid_y = top + s * 0.18;
    let body_top_y = lid_y + s * 0.06;

    // Handle: short horizontal cap above the lid.
    let handle_half_w = s * 0.12;
    painter.line_segment(
        [
            egui::pos2(cx - handle_half_w, top),
            egui::pos2(cx + handle_half_w, top),
        ],
        stroke,
    );

    // Lid: wider horizontal line just below the handle.
    let lid_half_w = s * 0.36;
    painter.line_segment(
        [
            egui::pos2(cx - lid_half_w, lid_y),
            egui::pos2(cx + lid_half_w, lid_y),
        ],
        stroke,
    );

    // Body: slightly tapered trapezoid below the lid.
    let body_top_half_w = s * 0.28;
    let body_bot_half_w = s * 0.22;
    painter.line_segment(
        [
            egui::pos2(cx - body_top_half_w, body_top_y),
            egui::pos2(cx - body_bot_half_w, bot),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(cx + body_top_half_w, body_top_y),
            egui::pos2(cx + body_bot_half_w, bot),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(cx - body_bot_half_w, bot),
            egui::pos2(cx + body_bot_half_w, bot),
        ],
        stroke,
    );

    // Two short slats inside the body to read clearly as a trash bin.
    let slat_top_y = body_top_y + s * 0.08;
    let slat_bot_y = bot - s * 0.06;
    let slat_offset = s * 0.09;
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

/// Info icon: a stylised question-mark glyph drawn as a thick
/// stroked path (curved hook curling into a vertical stem) plus a
/// filled dot below. Drawn larger and bolder than the surrounding
/// text would render the `?` character, so it reads clearly at the
/// 14 px sizes used next to section headings and field labels.
pub fn paint_info_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let s = rect.width().min(rect.height());
    let cx = rect.center().x;
    let cy = rect.center().y;

    let stroke_w = (s * 0.20).max(1.6);
    let stroke = egui::Stroke::new(stroke_w, color);

    // Hook geometry: arc traces from the LEFT side of the implied
    // circle over the top and down to just past the bottom on the
    // right -- a "?" curve with an open mouth (the curve covers
    // only about two-thirds of the implied circle rather than
    // wrapping nearly all the way around).
    //
    // Screen-space convention: pos = (cx + r*cos(a), cy - r*sin(a)).
    // Visually CCW means *decreasing* math angle (screen y is flipped).
    let hook_cx = cx;
    let hook_cy = cy - s * 0.16;
    let hook_r = s * 0.22;
    let a_start = -std::f32::consts::PI; // -180° (LEFT side)
    let a_end = -std::f32::consts::PI * 0.42; // ~ -76° (just past bottom)
    let total = (a_start - a_end).rem_euclid(std::f32::consts::TAU);

    let n = 18;
    let mut path = Vec::with_capacity(n + 2);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let a = a_start - total * t;
        path.push(egui::pos2(
            hook_cx + hook_r * a.cos(),
            hook_cy - hook_r * a.sin(),
        ));
    }
    // Short straight tail continues the curve downward into a stem.
    if let Some(&arc_end) = path.last() {
        path.push(egui::pos2(arc_end.x, arc_end.y + s * 0.10));
    }
    painter.add(egui::Shape::line(path, stroke));

    // Filled dot below the stem.
    let dot_x = hook_cx + hook_r * a_end.cos();
    let dot_y = hook_cy - hook_r * a_end.sin() + s * 0.10 + stroke_w * 0.5 + s * 0.09;
    painter.circle_filled(egui::pos2(dot_x, dot_y), stroke_w * 0.55, color);
}

/// Stylised cluster of grass blades for the Grass action-bar button.
/// Three upward arcs of different heights pointing at small offsets
/// to read as "tuft of grass" without any added text.
pub(crate) fn paint_grass_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.8, color);
    // Ground line.
    painter.line_segment(
        [
            egui::pos2(cx - 8.0, cy + 6.0),
            egui::pos2(cx + 8.0, cy + 6.0),
        ],
        stroke,
    );
    // Three blade strokes leaning slightly outward.
    painter.line_segment(
        [
            egui::pos2(cx - 5.0, cy + 6.0),
            egui::pos2(cx - 6.5, cy - 4.0),
        ],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx, cy + 6.0), egui::pos2(cx + 0.5, cy - 6.0)],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(cx + 5.0, cy + 6.0),
            egui::pos2(cx + 6.5, cy - 3.0),
        ],
        stroke,
    );
}

/// Tag / label icon for the Identity action-bar button. A rectangle
/// with one chamfered corner, mimicking a name-tag silhouette.
pub(crate) fn paint_identity_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.5, color);
    let pts = vec![
        egui::pos2(cx - 7.0, cy - 5.0),
        egui::pos2(cx + 4.0, cy - 5.0),
        egui::pos2(cx + 7.0, cy - 2.0),
        egui::pos2(cx + 7.0, cy + 5.0),
        egui::pos2(cx - 7.0, cy + 5.0),
    ];
    painter.add(egui::Shape::closed_line(pts, stroke));
    // Small hole at the chamfer for a tag string.
    painter.circle_stroke(egui::pos2(cx + 4.5, cy - 2.5), 1.0, stroke);
}

/// Crossed ruler icon for the Dimensions action-bar button.
pub(crate) fn paint_dimensions_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.4, color);
    // Horizontal ruler.
    painter.rect_stroke(
        egui::Rect::from_min_max(
            egui::pos2(cx - 8.0, cy + 1.0),
            egui::pos2(cx + 8.0, cy + 5.0),
        ),
        1.0,
        stroke,
        egui::StrokeKind::Inside,
    );
    for i in 1..4 {
        let x = cx - 8.0 + (i as f32) * 4.0;
        painter.line_segment([egui::pos2(x, cy + 1.0), egui::pos2(x, cy + 3.0)], stroke);
    }
    // Vertical ruler.
    painter.rect_stroke(
        egui::Rect::from_min_max(
            egui::pos2(cx - 5.0, cy - 8.0),
            egui::pos2(cx - 1.0, cy + 8.0),
        ),
        1.0,
        stroke,
        egui::StrokeKind::Inside,
    );
    for i in 1..4 {
        let y = cy - 8.0 + (i as f32) * 4.0;
        painter.line_segment([egui::pos2(cx - 5.0, y), egui::pos2(cx - 3.0, y)], stroke);
    }
}

/// Downward gravity arrow over a ground line for the Physics button.
pub(crate) fn paint_physics_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.6, color);
    // Shaft.
    painter.line_segment([egui::pos2(cx, cy - 7.0), egui::pos2(cx, cy + 4.0)], stroke);
    // Arrow head.
    painter.line_segment(
        [egui::pos2(cx - 3.0, cy + 1.0), egui::pos2(cx, cy + 4.0)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx + 3.0, cy + 1.0), egui::pos2(cx, cy + 4.0)],
        stroke,
    );
    // Ground line (dashed).
    for i in 0..3 {
        let x0 = cx - 8.0 + (i as f32) * 6.0;
        painter.line_segment(
            [egui::pos2(x0, cy + 7.5), egui::pos2(x0 + 4.0, cy + 7.5)],
            stroke,
        );
    }
}

/// Solid cloud silhouette for the Atmosphere action-bar button.
/// Three overlapping filled circles form the stylised cloud shape.
pub(crate) fn paint_atmosphere_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    painter.circle_filled(egui::pos2(cx - 4.0, cy + 1.0), 3.5, color);
    painter.circle_filled(egui::pos2(cx, cy - 1.5), 4.5, color);
    painter.circle_filled(egui::pos2(cx + 4.5, cy + 1.0), 3.5, color);
}

/// Sun with rays for the Lighting action-bar button.
pub(crate) fn paint_lighting_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.6, color);
    painter.circle_stroke(egui::pos2(cx, cy), 4.0, stroke);
    let ray_inner = 5.5_f32;
    let ray_outer = 8.5_f32;
    for i in 0..8 {
        let a = (i as f32) * std::f32::consts::TAU / 8.0;
        painter.line_segment(
            [
                egui::pos2(cx + ray_inner * a.cos(), cy + ray_inner * a.sin()),
                egui::pos2(cx + ray_outer * a.cos(), cy + ray_outer * a.sin()),
            ],
            stroke,
        );
    }
}

/// Solid water-droplet silhouette for the Water action-bar button.
/// Constructed as a filled circular bulge with a triangular tip
/// stacked on top; the slight overlap at the join hides the seam.
pub(crate) fn paint_water_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let r = 5.0_f32;
    let tip = egui::pos2(cx, cy - 8.0);
    let bulge_center = egui::pos2(cx, cy + 2.0);
    painter.circle_filled(bulge_center, r, color);
    let shoulder_y = bulge_center.y - r * 0.7;
    let shoulder_dx = r * 0.72;
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            egui::pos2(cx + shoulder_dx, shoulder_y),
            egui::pos2(cx - shoulder_dx, shoulder_y),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// Horizontal misty lines for the Fog action-bar button.
pub(crate) fn paint_fog_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.6, color);
    // Four lines of decreasing length top-to-bottom to suggest fading mist.
    for (i, (y_off, x_shrink)) in [(-4.5, 1.5), (-1.5, 0.0), (1.5, 2.0), (4.5, 3.5)]
        .iter()
        .enumerate()
    {
        let _ = i;
        painter.line_segment(
            [
                egui::pos2(cx - 7.5 + x_shrink, cy + y_off),
                egui::pos2(cx + 7.5 - x_shrink, cy + y_off),
            ],
            stroke,
        );
    }
}

/// Up-arrow with a horizontal line at the top for the Publish
/// action-bar button. Reads as "push up / submit / publish".
pub(crate) fn paint_publish_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(2.0, color);
    // Horizontal destination line at top
    painter.line_segment(
        [
            egui::pos2(cx - 6.0, cy - 6.5),
            egui::pos2(cx + 6.0, cy - 6.5),
        ],
        stroke,
    );
    // Arrow stem
    painter.line_segment([egui::pos2(cx, cy - 5.5), egui::pos2(cx, cy + 5.0)], stroke);
    // Arrowhead (upward-pointing chevron)
    painter.line_segment(
        [egui::pos2(cx - 4.0, cy - 1.5), egui::pos2(cx, cy - 5.5)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx, cy - 5.5), egui::pos2(cx + 4.0, cy - 1.5)],
        stroke,
    );
}

/// 2x2 grid of squares for the Resources (texture-splat) button.
pub(crate) fn paint_resources_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let stroke = egui::Stroke::new(1.4, color);
    let size = 6.0_f32;
    let gap = 1.5_f32;
    let half_total = size + gap * 0.5;
    let cells = [
        (cx - half_total, cy - half_total),
        (cx + gap * 0.5, cy - half_total),
        (cx - half_total, cy + gap * 0.5),
        (cx + gap * 0.5, cy + gap * 0.5),
    ];
    for (i, (x, y)) in cells.iter().enumerate() {
        let r = egui::Rect::from_min_max(egui::pos2(*x, *y), egui::pos2(*x + size, *y + size));
        if i == 0 || i == 3 {
            painter.rect_filled(r, 1.0, color);
        } else {
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
        }
    }
}
