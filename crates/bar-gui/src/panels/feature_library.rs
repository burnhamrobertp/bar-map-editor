//! Feature library panel -- type picker for placing map features.
//!
//! Lives in the Sculpt3D layout's side panel when the Features
//! pseudo-layer is active. Hosts a search filter + a two-wide
//! virtualised grid of S3O thumbnails; selecting a cell arms a
//! feature placement (the viewport then handles click-to-place /
//! Esc-to-cancel / Del-to-remove).
//!
//! Detail of the selected feature lives in the floating viewport
//! popover (`panels::feature_popover`), not in the sidebar.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::t;

/// Draw the full feature library panel into `ui`. Caller controls
/// when to show it (today: when the Features pseudo-layer is active
/// in the Sculpt3D layout).
pub(crate) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.strong(t!("editor.feature_library.heading"));
        if app.selected_feature_type.is_some() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("x")
                    .on_hover_text(t!("editor.feature_library.cancel_placement"))
                    .clicked()
                {
                    app.selected_feature_type = None;
                }
            });
        }
    });
    ui.add_space(4.0);

    if app.feature_palette_names.is_empty() {
        ui.weak(t!("editor.feature_library.no_catalog"));
        ui.weak(t!("editor.feature_library.hint_set_archive"));
        return;
    }

    // Search filter. The text input replaces the static "Click
    // terrain to select. Del to remove." hint -- typed letters
    // narrow the catalog instead of relying on the user to scroll
    // through hundreds of types.
    let search_resp = ui.add(
        egui::TextEdit::singleline(&mut app.feature_filter)
            .hint_text(t!("editor.feature_library.search_hint"))
            .desired_width(f32::INFINITY),
    );
    crate::panels::widgets::select_all_on_focus(ui, &search_resp, &app.feature_filter);
    ui.add_space(4.0);

    let filter = app.feature_filter.to_lowercase();
    let names: Vec<String> = if filter.is_empty() {
        app.feature_palette_names.clone()
    } else {
        app.feature_palette_names
            .iter()
            .filter(|n| n.to_lowercase().contains(&filter))
            .cloned()
            .collect()
    };

    if names.is_empty() {
        ui.weak(t!("editor.feature_library.no_matches"));
        return;
    }

    // Two-wide virtualised grid: feature types can number in the
    // hundreds for a full BAR catalog, and rendering every cell every
    // frame -- with a thumbnail texture each -- would chew through
    // texture bandwidth. `ScrollArea::show_rows` only invokes the
    // closure for currently-visible rows.
    const ROW_HEIGHT: f32 = 96.0;
    const COLS: usize = 2;
    let num_rows = names.len().div_ceil(COLS);
    egui::ScrollArea::vertical()
        .id_salt("feature_palette_scroll")
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, num_rows, |ui, row_range| {
            let spacing = ui.spacing().item_spacing.x;
            let item_w = ((ui.available_width() - spacing) / COLS as f32).max(50.0);
            for row in row_range {
                ui.horizontal(|ui| {
                    for col in 0..COLS {
                        let idx = row * COLS + col;
                        let Some(name) = names.get(idx) else { break };
                        draw_feature_cell(ui, app, name, item_w, ROW_HEIGHT - 4.0);
                    }
                });
            }
        });
}

/// One cell of the feature palette grid. Renders the S3O thumbnail at
/// the top + the feature name below; falls back to a placeholder
/// rectangle when the thumbnail isn't ready yet. Records a thumbnail-
/// render request so bar-app's per-frame poll picks it up.
fn draw_feature_cell(
    ui: &mut egui::Ui,
    app: &mut BarEditorApp,
    name: &str,
    width: f32,
    height: f32,
) {
    let selected = app.selected_feature_type.as_deref() == Some(name);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let resp = resp.on_hover_text(name);

    let fill = if selected {
        egui::Color32::from_rgba_unmultiplied(70, 90, 130, 230)
    } else if resp.hovered() {
        egui::Color32::from_rgba_unmultiplied(40, 44, 52, 230)
    } else {
        egui::Color32::from_rgba_unmultiplied(28, 30, 36, 200)
    };
    ui.painter().rect_filled(rect, 4.0, fill);

    let thumb_size = (height - 22.0).max(16.0);
    let thumb_rect = egui::Rect::from_min_size(
        egui::pos2(rect.center().x - thumb_size * 0.5, rect.top() + 2.0),
        egui::vec2(thumb_size, thumb_size),
    );
    let thumb_id = name.to_lowercase();
    if let Some(handle) = app.feature_thumb_cache.get(&thumb_id) {
        ui.painter().image(
            handle.id(),
            thumb_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else {
        // Placeholder while waiting for the thumb. Insert a render
        // request only if the runner isn't already aware -- i.e. the
        // name is neither cached nor in flight. Idempotency of the
        // HashSet doesn't help avoid the egui-mutation signal, so the
        // gate has to be explicit.
        ui.painter().rect_filled(
            thumb_rect,
            3.0,
            egui::Color32::from_rgba_unmultiplied(50, 55, 65, 200),
        );
        if !app.feature_thumb_pending.contains(&thumb_id)
            && !app.feature_thumb_requests.contains(&thumb_id)
        {
            app.feature_thumb_requests.insert(thumb_id);
        }
    }

    let font = egui::FontId::proportional(11.0);
    // Two-wide grid: cells are ~half the panel width. Long feature
    // names (`btreeshrub__nshrub_large`, etc.) overflow into the
    // neighbouring cell when drawn untrimmed. Compute the available
    // text width with a couple of pixels of side padding, then truncate
    // with an ellipsis. The full name remains accessible via the
    // hover tooltip wired up at `resp.on_hover_text(name)` above.
    let max_text_w = (width - 6.0).max(0.0);
    let label = truncate_with_ellipsis(ui.painter(), name, font.clone(), max_text_w);
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 11.0),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        egui::Color32::from_rgba_unmultiplied(230, 230, 240, 240),
    );

    if resp.clicked() {
        app.selected_feature_type = if selected {
            None
        } else {
            Some(name.to_string())
        };
    }
}

/// Shorten `text` with a trailing ellipsis so the rendered galley fits
/// inside `max_w` at the supplied font. Returns the unmodified text when
/// it already fits, or `"..."` alone when even the ellipsis is too wide
/// for the cell. Binary search keeps this O(log n) per cell, which is
/// cheap enough for the virtualised grid's visible rows.
fn truncate_with_ellipsis(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    max_w: f32,
) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    let full = painter.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE);
    if full.size().x <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let ellipsis = "...";
    let mut lo: usize = 0;
    let mut hi: usize = chars.len().saturating_sub(1);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + ellipsis;
        let galley = painter.layout_no_wrap(candidate, font.clone(), egui::Color32::WHITE);
        if galley.size().x <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        ellipsis.to_string()
    } else {
        chars[..lo].iter().collect::<String>() + ellipsis
    }
}
