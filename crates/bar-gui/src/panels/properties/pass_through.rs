//! `PassThrough` node properties body. Distributed
//! `impl BarEditorApp` block: file list editing.

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::*;

impl BarEditorApp {
    pub(crate) fn draw_passthrough_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        node_params: &std::collections::HashMap<String, ParamValue>,
    ) {
        let files_str = match node_params.get("files") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => String::new(),
        };

        let files: Vec<(String, String)> = files_str
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, '|');
                let abs = parts.next()?.trim().to_string();
                let rel = parts.next()?.trim().to_string();
                if abs.is_empty() {
                    None
                } else {
                    Some((abs, rel))
                }
            })
            .collect();

        ui.label(format!("Files ({})", files.len()));

        let tree = build_path_tree(&files);
        let mut edit_request: Option<(String, String)> = None;

        egui::ScrollArea::vertical()
            .max_height(220.0)
            .id_salt("pt_files")
            .show(ui, |ui| {
                draw_path_tree(ui, &tree, 0, &mut edit_request);
            });

        if let Some((abs, arc)) = edit_request {
            let content = std::fs::read_to_string(&abs).unwrap_or_default();
            self.project.passthrough_edit = Some(PassthroughEdit {
                node_id,
                abs_path: abs,
                archive_path: arc,
                content,
                is_dirty: false,
            });
        }

        let show_editor = self
            .project
            .passthrough_edit
            .as_ref()
            .map(|e| e.node_id == node_id)
            .unwrap_or(false);

        if show_editor {
            let mut save_requested = false;
            let mut close_requested = false;

            if let Some(edit) = &mut self.project.passthrough_edit {
                ui.separator();
                let filename = std::path::Path::new(&edit.archive_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| edit.archive_path.clone());
                ui.label(format!("Editing: {filename}"));

                let resp = ui.add(
                    egui::TextEdit::multiline(&mut edit.content)
                        .desired_width(f32::INFINITY)
                        .desired_rows(10)
                        .code_editor(),
                );
                if resp.changed() {
                    edit.is_dirty = true;
                }

                let dirty = edit.is_dirty;
                ui.horizontal(|ui| {
                    if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
                        save_requested = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            }

            // Apply deferred actions after releasing the borrow on passthrough_edit
            if save_requested {
                if let Some(edit) = &mut self.project.passthrough_edit {
                    if let Err(e) = std::fs::write(&edit.abs_path, &edit.content) {
                        eprintln!("PassThrough save error for '{}': {e}", edit.abs_path);
                    } else {
                        edit.is_dirty = false;
                    }
                }
            }
            if close_requested {
                self.project.passthrough_edit = None;
            }
        }
    }
}
