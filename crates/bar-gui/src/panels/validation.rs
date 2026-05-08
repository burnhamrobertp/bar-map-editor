//! Validation surface — the sidebar summary block (counts +
//! refresh icon + "all clear") and the floating details window
//! (severity tab strip + scrollable findings list). Both read
//! findings cached on `BarEditorApp::validation_findings`; the
//! cache is refreshed at the top of every frame by
//! `refresh_validation_if_dirty` (in `app.rs`) so the panel never
//! has to re-run validation itself, only render what's already
//! there.

use eframe::egui;

use crate::app::{BarEditorApp, ValidationFilter};
use crate::panels::tokens;
use crate::t;

/// Sidebar summary: heading + per-severity rows. Clicking a row
/// opens the details window filtered to that severity. Hover on
/// the heading reveals a refresh affordance for re-running
/// validation manually (the per-frame auto-refresh covers most
/// cases; this is for explicit "I changed something the
/// fingerprint doesn't see" situations).
pub(crate) fn draw_summary(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    let (errors, warnings, infos) = app.validation_counts();

    // Track per-panel hover state across frames so the refresh icon
    // can fade in only when the user is actually pointing at the
    // sidebar block (à la JetBrains' tool-window action toolbar).
    // We also reserve the icon's slot every frame so the header row
    // doesn't twitch when the icon appears.
    let hover_id = ui.id().with("validation_summary_hover");
    let was_hovered = ui
        .memory(|m| m.data.get_temp::<bool>(hover_id))
        .unwrap_or(false);
    let bg_layer = egui::LayerId::new(
        egui::Order::Background,
        ui.id().with("validation_summary_row_bg"),
    );

    let scope = ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().button_padding = egui::vec2(4.0, 1.0);

        // Header row.
        ui.horizontal(|ui| {
            let header = ui.add(egui::SelectableLabel::new(
                false,
                egui::RichText::new(t!("editor.validation.heading"))
                    .size(13.0)
                    .strong(),
            ));
            if header.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if header.clicked() {
                app.set_validation_filter(ValidationFilter::All);
                app.set_dialog_show_validation_panel(true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(20.0, ui.spacing().interact_size.y),
                    egui::Sense::hover(),
                );
                if was_hovered {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    ));
                    let resp = child
                        .add(egui::SelectableLabel::new(false, "\u{27F3}"))
                        .on_hover_text(t!("editor.validation.rerun"));
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        app.run_validation();
                        app.refresh_validation_fingerprint();
                    }
                }
            });
        });

        // Severity rows.
        let mut clicked_filter: Option<ValidationFilter> = None;
        let label_errors = t!("common.errors");
        let label_warnings = t!("common.warnings");
        let label_info = t!("common.info");
        let rows: [(ValidationFilter, &str, usize, egui::Color32); 3] = [
            (
                ValidationFilter::Error,
                label_errors.as_str(),
                errors,
                tokens::SEVERITY_ERROR,
            ),
            (
                ValidationFilter::Warning,
                label_warnings.as_str(),
                warnings,
                tokens::SEVERITY_WARN,
            ),
            (
                ValidationFilter::Info,
                label_info.as_str(),
                infos,
                tokens::SEVERITY_INFO,
            ),
        ];
        let avail_w = ui.available_width();
        for (filter, label, count, color) in rows {
            let row_h = ui.spacing().interact_size.y.max(18.0);
            let (row_rect, row_resp) =
                ui.allocate_exact_size(egui::vec2(avail_w, row_h), egui::Sense::click());
            if row_resp.hovered() {
                let bg = ui.visuals().widgets.hovered.bg_fill;
                ui.ctx().layer_painter(bg_layer).rect_filled(
                    row_rect.expand2(egui::vec2(2.0, 0.0)),
                    3.0,
                    bg,
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.colored_label(color, format!("• {label}"));
            child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(count.to_string());
            });
            if row_resp.clicked() {
                clicked_filter = Some(filter);
            }
        }
        if let Some(f) = clicked_filter {
            app.set_validation_filter(f);
            app.set_dialog_show_validation_panel(true);
        }

        if errors + warnings + infos == 0 {
            ui.add_space(2.0);
            ui.weak(t!("editor.validation.all_clear"));
        }
    });

    let now_hovered = scope.response.hovered();
    if now_hovered != was_hovered {
        ui.memory_mut(|m| m.data.insert_temp(hover_id, now_hovered));
        ui.ctx().request_repaint();
    }
}

/// Floating details window. Severity tab strip + scrollable
/// findings list. Closes via the window's own X (or by toggling
/// `dialog.show_validation_panel`).
pub(crate) fn draw_details(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog_show_validation_panel() {
        return;
    }
    let mut open = app.dialog_show_validation_panel();
    egui::Window::new(t!("editor.validation.window_title"))
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_size([520.0, 360.0])
        .default_pos(ctx.screen_rect().center() - egui::vec2(260.0, 180.0))
        .show(ctx, |ui| {
            let findings = app.validation_findings();
            let errors = findings
                .iter()
                .filter(|f| f.severity == bar_project::Severity::Error)
                .count();
            let warnings = findings
                .iter()
                .filter(|f| f.severity == bar_project::Severity::Warning)
                .count();
            let infos = findings
                .iter()
                .filter(|f| f.severity == bar_project::Severity::Info)
                .count();

            let total = errors + warnings + infos;
            let red = tokens::SEVERITY_ERROR;
            let yellow = tokens::SEVERITY_WARN;
            let blue = tokens::SEVERITY_INFO;
            let neutral = egui::Color32::from_rgb(200, 200, 210);

            // Severity tab strip — All / Error / Warning / Info,
            // each colored to match its row in the sidebar
            // summary. The active tab gets its color saturated
            // and a 2-px underline so the tab metaphor reads
            // visually, not just by selection state.
            ui.horizontal(|ui| {
                let mut active_filter = app.validation_filter();
                let mut tab = |ui: &mut egui::Ui,
                               variant: ValidationFilter,
                               text: String,
                               color: egui::Color32| {
                    let active = active_filter == variant;
                    let label_color = if active {
                        color
                    } else {
                        color.linear_multiply(0.55)
                    };
                    let resp = ui.add(egui::SelectableLabel::new(
                        active,
                        egui::RichText::new(text).color(label_color).strong(),
                    ));
                    if active {
                        let r = resp.rect;
                        ui.painter().line_segment(
                            [
                                egui::pos2(r.left() + 2.0, r.bottom() + 1.0),
                                egui::pos2(r.right() - 2.0, r.bottom() + 1.0),
                            ],
                            egui::Stroke::new(2.0, color),
                        );
                    }
                    if resp.clicked() {
                        active_filter = variant;
                    }
                };
                tab(
                    ui,
                    ValidationFilter::All,
                    t!("editor.validation.tab_all", n = total),
                    neutral,
                );
                tab(
                    ui,
                    ValidationFilter::Error,
                    t!("editor.validation.tab_errors", n = errors),
                    red,
                );
                tab(
                    ui,
                    ValidationFilter::Warning,
                    t!("editor.validation.tab_warnings", n = warnings),
                    yellow,
                );
                tab(
                    ui,
                    ValidationFilter::Info,
                    t!("editor.validation.tab_info", n = infos),
                    blue,
                );
                drop(tab);
                app.set_validation_filter(active_filter);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("\u{27F3}")
                        .on_hover_text(t!("editor.validation.rerun"))
                        .clicked()
                    {
                        app.run_validation();
                    }
                    if errors == 0 && warnings == 0 {
                        ui.colored_label(
                            tokens::PORT_HEIGHTMAP,
                            t!("editor.validation.ready_to_export"),
                        );
                    }
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let active = app.validation_filter();
                    let mut shown = 0usize;
                    for f in app.validation_findings().iter().filter(|f| match active {
                        ValidationFilter::All => true,
                        ValidationFilter::Error => f.severity == bar_project::Severity::Error,
                        ValidationFilter::Warning => f.severity == bar_project::Severity::Warning,
                        ValidationFilter::Info => f.severity == bar_project::Severity::Info,
                    }) {
                        let (icon, color) = match f.severity {
                            bar_project::Severity::Error => ("\u{2716}", red),
                            bar_project::Severity::Warning => ("\u{26A0}", yellow),
                            bar_project::Severity::Info => ("\u{24D8}", blue),
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.colored_label(color, icon);
                            ui.colored_label(
                                egui::Color32::from_rgb(180, 180, 200), // muted category label
                                format!("[{}]", f.category),
                            );
                            ui.label(&f.message);
                        });
                        shown += 1;
                    }
                    if shown == 0 {
                        ui.weak(t!("editor.validation.no_issues"));
                    }
                });
        });
    app.set_dialog_show_validation_panel(open);
}
