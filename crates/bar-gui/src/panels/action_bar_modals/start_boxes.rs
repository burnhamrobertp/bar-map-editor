//! Start Boxes modal -- edit team spawn positions through a list of
//! `(x, z)` pairs. Bespoke because spawn positions are a
//! `Vec<[u32; 2]>` (not the `Option<T>` shape the schema's
//! `FieldSpec` enforces), and each row needs Add / Remove
//! affordances around the two channel drags.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::panels::action_bar_modals::shared::modal_frame;
use crate::panels::field_editor::{process_intent, FieldIntent};

pub(crate) fn draw(app: &mut BarEditorApp, ctx: &egui::Context) {
    if !app.dialog.show_start_boxes_editor {
        return;
    }
    let (map_w, map_h) = (app.map.width, app.map.height);
    let world_w = (map_w.saturating_sub(1)) * 8;
    let world_h = (map_h.saturating_sub(1)) * 8;
    let mut open = app.dialog.show_start_boxes_editor;
    let mut add_clicked = false;
    let mut remove_index: Option<usize> = None;
    let mut edit_intents: Vec<(usize, FieldIntent, &'static str)> = Vec::new();

    modal_frame(
        ctx,
        &mut open,
        "Start Boxes",
        "start_boxes_editor_modal",
        |ui| {
            let positions = app.map.settings.start_positions.clone();
            for (i, [x, z]) in positions.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("[{}]", i));
                    ui.label("x:");
                    let mut x_val = *x;
                    let resp = ui.add(egui::DragValue::new(&mut x_val).range(0..=world_w));
                    if resp.drag_started() || resp.gained_focus() {
                        edit_intents.push((i, FieldIntent::EditStarted, "spawn x"));
                    }
                    if x_val != *x {
                        app.map.settings.start_positions[i][0] = x_val;
                    }
                    if resp.drag_stopped() || resp.lost_focus() {
                        edit_intents.push((i, FieldIntent::EditCommitted, "spawn x"));
                    }

                    ui.label("z:");
                    let mut z_val = *z;
                    let resp = ui.add(egui::DragValue::new(&mut z_val).range(0..=world_h));
                    if resp.drag_started() || resp.gained_focus() {
                        edit_intents.push((i, FieldIntent::EditStarted, "spawn z"));
                    }
                    if z_val != *z {
                        app.map.settings.start_positions[i][1] = z_val;
                    }
                    if resp.drag_stopped() || resp.lost_focus() {
                        edit_intents.push((i, FieldIntent::EditCommitted, "spawn z"));
                    }

                    if ui.button("✕").on_hover_text("Remove this spawn").clicked() {
                        remove_index = Some(i);
                    }
                });
            }

            ui.add_space(8.0);
            if ui.button("+ Add spawn").clicked() {
                add_clicked = true;
            }
        },
    );

    app.dialog.show_start_boxes_editor = open;

    for (_i, intent, label) in edit_intents {
        process_intent(app, label, intent);
    }
    if add_clicked {
        let snap = app.snapshot("Add start position");
        let default_pos = [world_w / 2, world_h / 2];
        app.map.settings.start_positions.push(default_pos);
        app.history.push(snap);
        app.mark_dirty();
    }
    if let Some(idx) = remove_index {
        let snap = app.snapshot("Remove start position");
        app.map.settings.start_positions.remove(idx);
        app.history.push(snap);
        app.mark_dirty();
    }
}
