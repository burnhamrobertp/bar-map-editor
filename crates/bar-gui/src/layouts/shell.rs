//! Shared shell chrome drawn by every layout: top menu bar, status
//! bar, action bar, modal dialogs, toasts, and pre-frame orchestration
//! (file-dialog poll, autosave gate, validation refresh, keyboard
//! shortcuts, deletion routing).
//!
//! Called by `layouts::dispatch::draw_active` before the layout-specific
//! panels render.

use bar_graph::NodeId;
use eframe::egui;
use std::time::Instant;

use crate::app::{
    paint_bar_icon, paint_busy_dot, paint_export_icon, paint_inspector_icon, paint_map_info_icon,
    paint_mapinfo_form_icon, paint_startbox_icon, BarEditorApp, ConfirmAction, ConfirmDialog,
    ExportStatus, GroupDeleteChoice, InspectorMode, Layout, MapInfoTab, PendingAction,
    UnsavedDecision, CONFIRM_KEY_DELETE_CONNECTED_NODE,
};
use crate::io::is_text_file;
use crate::panels::log::level_color;
use crate::panels::tokens;
use crate::project::path::collect_all_passthrough_files;
use crate::t;

/// Maps a 0-based layout index to its Ctrl+# trigger key (Num1..Num9).
fn layout_num_key(idx: usize) -> Option<egui::Key> {
    const KEYS: &[egui::Key] = &[
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
        egui::Key::Num5,
        egui::Key::Num6,
        egui::Key::Num7,
        egui::Key::Num8,
        egui::Key::Num9,
    ];
    KEYS.get(idx).copied()
}

impl BarEditorApp {
    pub(crate) fn pre_frame_work(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // OS / window close request — route through the unsaved-changes
        // workflow so accidental clicks on the close button don't lose work.
        // `bar-app` is responsible for blocking the actual viewport close until
        // `take_allow_close()` returns true.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.request_close();
        }

        // Poll any in-flight Open dialog (see `open_file_dialog_async`).
        // Non-blocking; either the worker has produced a result or the
        // user is still picking. When a result arrives, dispatch it
        // through the same `start_open_path` the synchronous code paths
        // used to call directly.
        if let Some(rx) = self.project.pending_open_rx.as_ref() {
            match rx.try_recv() {
                Ok(maybe_path) => {
                    self.project.pending_open_rx = None;
                    if let Some(path) = maybe_path {
                        self.start_open_path(path);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Dialog still open; redraw next frame so we keep
                    // polling — egui won't otherwise tick on its own.
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Worker panicked; drop the receiver so the user
                    // can try again.
                    self.project.pending_open_rx = None;
                }
            }
        }

        // Refresh subgraph IO from contained nodes. Each subgraph's
        // `subgraph_inputs/outputs` list is *derived* from the
        // `SubgraphInput` / `SubgraphOutput` member nodes — the user
        // adds / removes / renames / re-types ports by editing those
        // nodes directly, not via a properties-panel form. Doing
        // this once per frame keeps the collapsed-block port
        // rendering in sync without anyone having to remember to
        // call a refresh function.
        self.recompute_all_subgraph_io();

        // Continuous validation — runs at the start of every frame
        // when any validation-relevant input has changed (graph
        // structure / params, map dimensions, or map settings).
        // Cheap; cached findings drive the sidebar summary and the
        // bundle-button gate without anyone having to click "validate".
        self.refresh_validation_if_dirty();

        // Tick auto-save. Cheap: a single Instant comparison per frame.
        if self.settings.autosave_enabled
            && self.project.is_dirty
            && self.dialog.pending_action.is_none()
        {
            let interval = std::time::Duration::from_secs(self.settings.autosave_interval_secs);
            let due = self
                .project
                .last_autosave_at
                .map(|t| t.elapsed() >= interval)
                .unwrap_or(true);
            if due {
                self.autosave_now();
            }
        }

        // Expire toast notifications.
        if let Some((_, until)) = &self.dialog.toast {
            if Instant::now() >= *until {
                self.dialog.toast = None;
            }
        }

        // Update window title to reflect loaded name and dirty state
        let dirty_marker = if self.project.is_dirty { " *" } else { "" };
        let title = match self
            .project
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .or_else(|| self.project.loaded_name.clone())
        {
            Some(name) => format!("{name}{dirty_marker} — BAR - Map Editor"),
            None => "BAR - Map Editor".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // Keyboard shortcuts. Suppress while a modal is open (the dialog has
        // its own buttons) and while a text widget has focus (so typing 'Z'
        // inside a text field doesn't undo the graph).
        let modal_open = self.dialog.pending_action.is_some()
            || self.dialog.confirm_dialog.is_some()
            || self.dialog.show_settings
            || self.dialog.show_about;
        let typing = ctx.wants_keyboard_input();
        if !modal_open {
            let (do_undo, do_redo, do_save, do_save_as, do_open, do_new, layout_idx) =
                ctx.input(|i| {
                    let ctrl = i.modifiers.ctrl || i.modifiers.command;
                    let shift = i.modifiers.shift;
                    let layout_idx = if !typing && ctrl && !shift {
                        Layout::ALL.iter().enumerate().find_map(|(idx, _)| {
                            layout_num_key(idx)
                                .filter(|&k| i.key_pressed(k))
                                .map(|_| idx)
                        })
                    } else {
                        None
                    };
                    (
                        !typing && ctrl && !shift && i.key_pressed(egui::Key::Z),
                        !typing
                            && ctrl
                            && ((!shift && i.key_pressed(egui::Key::Y))
                                || (shift && i.key_pressed(egui::Key::Z))),
                        !typing && ctrl && !shift && i.key_pressed(egui::Key::S),
                        !typing && ctrl && shift && i.key_pressed(egui::Key::S),
                        !typing && ctrl && i.key_pressed(egui::Key::O),
                        !typing && ctrl && i.key_pressed(egui::Key::N),
                        layout_idx,
                    )
                });
            if do_undo {
                self.undo();
            }
            if do_redo {
                self.redo();
            }
            if do_save {
                self.save_or_save_as();
            }
            if do_save_as {
                self.save_as();
            }
            if do_open {
                self.open_file_dialog_async();
            }
            if do_new {
                self.start_new_project();
            }
            if let Some(idx) = layout_idx {
                if self.has_project() {
                    if let Some(&layout) = Layout::ALL.get(idx) {
                        self.set_active_layout(layout);
                    }
                }
            }
        }

        // Delete selected node via Delete / Backspace. Routes through the
        // confirm dialog when the user has destructive-confirmation enabled.
        let do_delete = !modal_open
            && !typing
            && ctx
                .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if do_delete {
            // Selection precedence: connection > group > node. A
            // user with a wire highlighted who hits Delete probably
            // wants the wire gone; a group-selected user wants the
            // group gone. The selection helpers keep these mutually
            // exclusive so we never have to disambiguate.
            if let Some((from, to)) = self.selection.connection.clone() {
                self.push_undo("Delete connection");
                self.graph.disconnect(&from, &to);
                self.selection.connection = None;
            } else if let Some(gid) = self.selection.group {
                let is_subgraph = self
                    .visuals
                    .groups
                    .get(&gid)
                    .map(|g| g.is_subgraph)
                    .unwrap_or(false);
                if is_subgraph {
                    // SubGraphs always delete with their members —
                    // splitting the SubGraph wrapper from its inner
                    // pipeline almost never matches user intent.
                    // The full state snapshot taken by push_undo
                    // covers every inner node + connection, so undo
                    // restores the whole subgraph.
                    self.delete_subgraph_with_contents(gid);
                } else {
                    // Visual groups still get the modal: they wrap
                    // arbitrary nodes the user might want to keep.
                    self.selection.pending_group_delete = Some(gid);
                }
            } else if self.selection.node.is_some() {
                // Only ask for confirmation when the user is about to
                // tear down something with wires attached. Lone /
                // recently-dropped nodes vanish straight away — the
                // modal-on-every-Delete pattern was annoying.
                let selection: Vec<NodeId> = if !self.selection.nodes.is_empty() {
                    self.selection.nodes.iter().copied().collect()
                } else if let Some(id) = self.selection.node {
                    vec![id]
                } else {
                    Vec::new()
                };
                let has_connections = selection.iter().any(|nid| {
                    self.graph
                        .connections()
                        .iter()
                        .any(|c| c.from.node_id == *nid || c.to.node_id == *nid)
                });
                let suppressed = self
                    .settings
                    .suppressed_confirmations
                    .contains(CONFIRM_KEY_DELETE_CONNECTED_NODE);
                if has_connections && !suppressed {
                    let msg = if selection.len() > 1 {
                        format!(
                            "Delete {} nodes and disconnect all of their wires?",
                            selection.len()
                        )
                    } else {
                        "Delete this node and disconnect all of its wires?".to_string()
                    };
                    self.dialog.confirm_dialog = Some(ConfirmDialog {
                        title: "Delete node?".to_string(),
                        message: msg,
                        affirm_label: "Delete".to_string(),
                        on_affirm: ConfirmAction::DeleteSelected,
                        suppression_key: Some(CONFIRM_KEY_DELETE_CONNECTED_NODE.to_string()),
                        dont_ask_again: false,
                    });
                } else {
                    self.delete_selected_node();
                }
            }
        }
    }

    pub(crate) fn draw_shell(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top menu bar — desktop-app styling. The panel itself has no
        // inner margin so the first entry sits flush with the left
        // edge of the window. Inside the bar we zero out horizontal
        // item_spacing (entries butt up against each other, no gap)
        // and bump button_padding so each entry's hover/click hit
        // box covers the full vertical span of the bar instead of
        // tightly hugging the text.
        egui::TopBottomPanel::top("menu_bar")
            .frame(
                egui::Frame::default()
                    .fill(ctx.style().visuals.panel_fill)
                    // Asymmetric vertical: button_padding is
                    // symmetric (one Vec2.y for both edges), so we
                    // get the smaller "top" amount from
                    // button_padding and add the extra bottom
                    // distance via the panel's inner margin. Net
                    // effect: 5 px above the text, ~6.7 px below.
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 0,
                        bottom: 2,
                    }),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    // `menu::bar` resets spacing/button_padding on its
                    // internal Ui — these overrides have to live INSIDE
                    // the closure to survive that reset.
                    //
                    // Asymmetric top/bottom: button_padding.y = 5 puts
                    // 5 px above and 5 px below the text inside each
                    // entry's rect (so hover highlights cover both
                    // bands). The extra ~1.7 px of bottom margin lives
                    // on the panel itself (above) — totals 5 above,
                    // ~6.7 below.
                    //
                    // Symmetric left/right at 7.78 px: the hover rect
                    // fully owns the padding on both sides, adjacent
                    // entries butt edge-to-edge with no panel-fill
                    // strip between them.
                    ui.style_mut().spacing.button_padding = egui::vec2(7.78, 5.0);
                    ui.style_mut().spacing.item_spacing.x = 0.0;
                    let v = &mut ui.style_mut().visuals;
                    let base = v.panel_fill;
                    let towards = if v.dark_mode {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::BLACK
                    };
                    let hover_fill = base.lerp_to_gamma(towards, 0.14);
                    let active_fill = base.lerp_to_gamma(towards, 0.26);
                    v.widgets.hovered.weak_bg_fill = hover_fill;
                    v.widgets.hovered.bg_fill = hover_fill;
                    v.widgets.active.weak_bg_fill = active_fill;
                    v.widgets.active.bg_fill = active_fill;
                    v.widgets.open.weak_bg_fill = active_fill;
                    v.widgets.open.bg_fill = active_fill;
                    // Square corners so adjacent entries look like one
                    // continuous strip rather than rounded chips.
                    v.widgets.hovered.corner_radius = egui::CornerRadius::ZERO;
                    v.widgets.active.corner_radius = egui::CornerRadius::ZERO;
                    v.widgets.open.corner_radius = egui::CornerRadius::ZERO;
                    ui.menu_button(t!("editor.menu.file"), |ui| {
                        ui.spacing_mut().item_spacing.x = 60.0;
                        let sc = ui.visuals().text_color();
                        if ui
                            .add(
                                egui::Button::new(t!("editor.menu.new_project"))
                                    .shortcut_text(egui::RichText::new("Ctrl+N").color(sc)),
                            )
                            .clicked()
                        {
                            self.start_new_project();
                            ui.close_menu();
                        }
                        let mut macro_to_load: Option<String> = None;
                        ui.menu_button(t!("editor.menu.new_from_preset"), |ui| {
                            for group in crate::macros::BUILTIN_MACRO_GROUPS {
                                ui.menu_button(group.name, |ui| {
                                    for entry in group.entries {
                                        if ui.button(entry.display_name).clicked() {
                                            macro_to_load = Some(entry.full_name.to_string());
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        });
                        if let Some(name) = macro_to_load {
                            self.start_load_macro(&name);
                        }
                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new(t!("editor.menu.open"))
                                    .shortcut_text(egui::RichText::new("Ctrl+O").color(sc)),
                            )
                            .clicked()
                        {
                            self.open_file_dialog_async();
                            ui.close_menu();
                        }
                        if ui.button(t!("editor.menu.import_sd7")).clicked() {
                            self.import_sd7_dialog_async();
                            ui.close_menu();
                        }
                        let mut recent_pick: Option<std::path::PathBuf> = None;
                        let recent_empty = self.settings.recent_files.is_empty();
                        ui.add_enabled_ui(!recent_empty, |ui| {
                            ui.menu_button(t!("editor.menu.open_recent"), |ui| {
                                for p in self.settings.recent_files.iter() {
                                    let label = p
                                        .file_name()
                                        .map(|s| s.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| p.display().to_string());
                                    let parent = p
                                        .parent()
                                        .map(|s| s.display().to_string())
                                        .unwrap_or_default();
                                    let response = ui.button(&label).on_hover_text(&parent);
                                    if response.clicked() {
                                        recent_pick = Some(p.clone());
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                if ui.button(t!("editor.menu.clear_recent")).clicked() {
                                    self.settings.recent_files.clear();
                                    self.settings.save();
                                    ui.close_menu();
                                }
                            });
                        });
                        if let Some(p) = recent_pick {
                            self.start_open_path(p);
                        }
                        ui.separator();
                        let in_project = self.has_project();
                        if ui
                            .add_enabled(
                                in_project,
                                egui::Button::new(t!("editor.menu.save_project"))
                                    .shortcut_text(egui::RichText::new("Ctrl+S").color(sc)),
                            )
                            .clicked()
                        {
                            self.save_or_save_as();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                in_project,
                                egui::Button::new(t!("editor.menu.save_project_as"))
                                    .shortcut_text(egui::RichText::new("Ctrl+Shift+S").color(sc)),
                            )
                            .clicked()
                        {
                            self.save_as();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(t!("editor.menu.exit")).clicked() {
                            // Route through the dirty-check path; don't slam the
                            // window shut on unsaved work.
                            self.request_close();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(t!("editor.menu.edit"), |ui| {
                        ui.spacing_mut().item_spacing.x = 60.0;
                        let sc = ui.visuals().text_color();
                        let undo_label = if self.history.can_undo() {
                            format!("{} ({})", t!("editor.menu.undo"), self.history.undo_depth())
                        } else {
                            t!("editor.menu.undo").to_string()
                        };
                        if ui
                            .add_enabled(
                                self.history.can_undo(),
                                egui::Button::new(undo_label)
                                    .shortcut_text(egui::RichText::new("Ctrl+Z").color(sc)),
                            )
                            .clicked()
                        {
                            self.undo();
                            ui.close_menu();
                        }
                        let redo_label = if self.history.can_redo() {
                            format!("{} ({})", t!("editor.menu.redo"), self.history.redo_depth())
                        } else {
                            t!("editor.menu.redo").to_string()
                        };
                        if ui
                            .add_enabled(
                                self.history.can_redo(),
                                egui::Button::new(redo_label)
                                    .shortcut_text(egui::RichText::new("Ctrl+Shift+Z").color(sc)),
                            )
                            .clicked()
                        {
                            self.redo();
                            ui.close_menu();
                        }
                        ui.separator();
                        // Auto Layout — only meaningful on the NodeGraph layout.
                        if ui
                            .add_enabled(
                                self.has_project() && self.active_layout == Layout::Standard,
                                egui::Button::new(t!("editor.menu.auto_layout")),
                            )
                            .clicked()
                        {
                            self.auto_layout_selection();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(t!("editor.menu.preferences")).clicked() {
                            self.dialog.show_settings = true;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(t!("editor.menu.view"), |ui| {
                        ui.spacing_mut().item_spacing.x = 60.0;
                        let sc = ui.visuals().text_color();
                        let has_proj = self.has_project();
                        for (idx, &layout) in Layout::ALL.iter().enumerate() {
                            let is_active = has_proj && self.active_layout == layout;
                            let shortcut = layout_num_key(idx).map(|_| format!("Ctrl+{}", idx + 1));
                            let mut btn =
                                egui::Button::new(t!(layout.i18n_key())).selected(is_active);
                            if let Some(s) = shortcut {
                                btn = btn.shortcut_text(egui::RichText::new(s).color(sc));
                            }
                            if ui.add_enabled(has_proj, btn).clicked() {
                                self.set_active_layout(layout);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        if ui
                            .selectable_label(self.dialog.show_log, t!("editor.menu.log"))
                            .clicked()
                        {
                            self.dialog.show_log = !self.dialog.show_log;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(t!("editor.menu.help"), |ui| {
                        ui.spacing_mut().item_spacing.x = 60.0;
                        if ui.button(t!("editor.app.about")).clicked() {
                            self.dialog.show_about = true;
                            ui.close_menu();
                        }
                    });
                });
            });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Clickable map size opens the unified Map Settings modal
                // (the same one the toolbar's "Edit Map Info" opens), so
                // dimensions and the rest of the map metadata live in one
                // place instead of a separate side dialog.
                if ui
                    .small_button(format!(
                        "Map: {}×{}",
                        self.map.width.saturating_sub(1) / 64,
                        self.map.height.saturating_sub(1) / 64,
                    ))
                    .on_hover_text(t!("editor.status.open_map_settings"))
                    .clicked()
                {
                    self.dialog.show_mapinfo_editor = true;
                    self.set_mapinfo_tab(MapInfoTab::Dimensions);
                }
                ui.separator();
                let status_resp = if let Some(ref msg) = self.dialog.status_message {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(msg).color(level_color(self.dialog.status_level)),
                        )
                        .sense(egui::Sense::click()),
                    )
                } else if let Some(id) = self.selection.node {
                    ui.add(
                        egui::Label::new(format!("Selected: {:?}", id)).sense(egui::Sense::click()),
                    )
                } else {
                    ui.add(
                        egui::Label::new(egui::RichText::new("No selection").weak())
                            .sense(egui::Sense::click()),
                    )
                };
                if status_resp
                    .on_hover_text(t!("editor.log.open_hint"))
                    .clicked()
                {
                    self.dialog.show_log = true;
                }
            });
        });

        // Properties no longer live in a permanent right-side panel —
        // they pop up next to the selected node / group in a floating
        // panel that opens after a short hover-after-click delay,
        // and closes on click-outside. See `tick_props_panel`.
        self.tick_props_panel(ctx);

        // ── Modal: unsaved-changes prompt ────────────────────────────────────
        if let Some(action) = self.dialog.pending_action.clone() {
            let mut close = false;
            let mut decision: Option<UnsavedDecision> = None;
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    let action_label = match &action {
                        PendingAction::Close => "close BAR - Map Editor",
                        PendingAction::NewProject => "start a new project",
                        PendingAction::OpenPath(_) => "open this file",
                        PendingAction::LoadMacro { .. } => "load this preset",
                    };
                    ui.label(format!(
                        "Your project has unsaved changes. Save before you {action_label}?"
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            decision = Some(UnsavedDecision::Save);
                        }
                        if ui.button("Discard").clicked() {
                            decision = Some(UnsavedDecision::Discard);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(UnsavedDecision::Cancel);
                        }
                    });
                });
            if let Some(d) = decision {
                close = true;
                match d {
                    UnsavedDecision::Save => {
                        self.save_or_save_as();
                        // If the save succeeded, is_dirty is now false; if the
                        // user cancelled the Save As dialog it's still true and
                        // we keep the prompt open.
                        if !self.project.is_dirty {
                            self.apply_pending_action(action);
                        } else {
                            close = false;
                        }
                    }
                    UnsavedDecision::Discard => {
                        // Skip dirty check; force-apply.
                        self.project.is_dirty = false;
                        self.apply_pending_action(action);
                    }
                    UnsavedDecision::Cancel => {
                        // Just dismiss — keep editing.
                    }
                }
            }
            if close {
                self.dialog.pending_action = None;
            }
        }

        // ── Modal: group delete (three-way: keep nodes / delete all / cancel) ─
        if let Some(gid) = self.selection.pending_group_delete {
            let label = self
                .visuals
                .groups
                .get(&gid)
                .map(|g| {
                    if g.label.is_empty() {
                        format!("Group {gid}")
                    } else {
                        g.label.clone()
                    }
                })
                .unwrap_or_else(|| format!("Group {gid}"));
            let member_count = self
                .visuals
                .groups
                .get(&gid)
                .map(|g| g.member_ids.len())
                .unwrap_or(0);
            let mut decision: Option<GroupDeleteChoice> = None;
            egui::Window::new(format!("Delete '{label}'?"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!(
                        "This group contains {member_count} node(s). What should happen to them?"
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete group only").clicked() {
                            decision = Some(GroupDeleteChoice::GroupOnly);
                        }
                        if ui.button("Delete group and its nodes").clicked() {
                            decision = Some(GroupDeleteChoice::GroupAndMembers);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(GroupDeleteChoice::Cancel);
                        }
                    });
                });
            if let Some(choice) = decision {
                self.selection.pending_group_delete = None;
                match choice {
                    GroupDeleteChoice::GroupOnly => {
                        self.push_undo("Dissolve group");
                        self.dissolve_group(gid);
                        if self.selection.group == Some(gid) {
                            self.selection.group = None;
                        }
                    }
                    GroupDeleteChoice::GroupAndMembers => {
                        // Push once for the whole "delete group + nodes"
                        // action so undo treats it atomically. The
                        // delete_selected_node path below would push
                        // its own undo entry; suppress that by
                        // stashing the snapshot here.
                        self.push_undo("Delete group with members");
                        let members: Vec<NodeId> = self
                            .visuals
                            .groups
                            .get(&gid)
                            .map(|g| g.member_ids.iter().copied().collect())
                            .unwrap_or_default();
                        self.dissolve_group(gid);
                        if self.selection.group == Some(gid) {
                            self.selection.group = None;
                        }
                        self.selection.nodes = members.iter().copied().collect();
                        self.selection.node = members.first().copied();
                        // Delete nodes inline (don't go through
                        // delete_selected_node, which would push another
                        // undo and split the action).
                        let to_delete: Vec<NodeId> = self.selection.nodes.iter().copied().collect();
                        for node_id in &to_delete {
                            let _ = self.graph.remove_node(*node_id);
                            self.visuals.node_visuals.remove(node_id);
                            self.remove_node_from_group(*node_id);
                            if self.preview.node == Some(*node_id) {
                                self.preview.node = None;
                                self.preview.open = false;
                            }
                        }
                        self.project.passthrough_edit = None;
                        self.clear_selection();
                    }
                    GroupDeleteChoice::Cancel => {}
                }
            }
        }

        // ── Modal: generic confirm (delete, etc.) ────────────────────────────
        if let Some(mut dialog) = self.dialog.confirm_dialog.clone() {
            let mut decision: Option<bool> = None;
            egui::Window::new(&dialog.title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(&dialog.message);
                    if dialog.suppression_key.is_some() {
                        ui.add_space(6.0);
                        ui.checkbox(&mut dialog.dont_ask_again, "Don't ask again");
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(&dialog.affirm_label).clicked() {
                            decision = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(false);
                        }
                    });
                });
            if let Some(affirm) = decision {
                // If the user ticked "Don't ask again" while
                // confirming, add this modal's key to the suppressed
                // set and persist. Suppression is per-key: this
                // affects only this modal type, not other confirms.
                if affirm && dialog.dont_ask_again {
                    if let Some(key) = dialog.suppression_key.as_ref() {
                        self.settings.suppressed_confirmations.insert(key.clone());
                        self.settings.save();
                    }
                }
                self.dialog.confirm_dialog = None;
                if affirm {
                    match dialog.on_affirm {
                        ConfirmAction::DeleteSelected => self.delete_selected_node(),
                    }
                }
            }
        }

        // ── Modal: Preferences ───────────────────────────────────────────────
        crate::panels::dialogs::draw_settings(self, ctx);

        // ── Modal: Edit Map Info picker ──────────────────────────────────────
        if self.dialog.show_map_info_picker {
            let candidates = collect_all_passthrough_files(&self.graph);
            // Heuristic: text files first, with .lua nudged to the top so
            // mapinfo.lua appears at the obvious spot for BAR/Spring users.
            let mut sorted = candidates.clone();
            sorted.sort_by_key(|(_, archive)| {
                let lua = archive.to_lowercase().ends_with("mapinfo.lua");
                let text = is_text_file(archive);
                (!lua, !text, archive.clone())
            });

            let mut open = self.dialog.show_map_info_picker;
            let mut chosen: Option<(String, String)> = None;
            let mut cleared = false;
            egui::Window::new("Choose map info file")
                .open(&mut open)
                .resizable(true)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    if sorted.is_empty() {
                        ui.label(
                            "No passthrough files in this project. Open or import a map \
                             with a mapinfo.lua first.",
                        );
                    } else {
                        ui.label("Pick the file that holds this project's map configuration:");
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .show(ui, |ui| {
                                for (abs, archive) in &sorted {
                                    let label_text = if is_text_file(archive) {
                                        archive.clone()
                                    } else {
                                        format!("{archive} (binary — won't open in text editor)")
                                    };
                                    if ui.button(label_text).on_hover_text(abs).clicked() {
                                        chosen = Some((abs.clone(), archive.clone()));
                                    }
                                }
                            });
                    }
                    ui.add_space(8.0);
                    if self.project.map_info_file.is_some()
                        && ui.button("Clear current selection").clicked()
                    {
                        cleared = true;
                    }
                });
            self.dialog.show_map_info_picker = open;
            if cleared {
                self.project.map_info_file = None;
                self.project.is_dirty = true;
                self.dialog.show_map_info_picker = false;
            }
            if let Some((abs, archive)) = chosen {
                self.project.map_info_file = Some(archive.clone());
                self.project.is_dirty = true;
                self.dialog.show_map_info_picker = false;
                self.open_file_editor(abs, archive);
            }
        }

        // ── Modal: in-app file editor ────────────────────────────────────────
        if self.dialog.file_editor.is_some() {
            let mut save_request = false;
            let mut close_request = false;
            // Take ownership briefly so we can borrow the editor mutably for
            // the text widget while still calling self.* methods after.
            let mut editor = self.dialog.file_editor.take().expect("checked Some above");
            let dirty_marker = if editor.is_dirty { " *" } else { "" };
            let title = format!("Edit — {}{}", editor.archive_path, dirty_marker);
            let mut open = true;
            egui::Window::new(title)
                .id(egui::Id::new("file_editor_window"))
                .resizable(true)
                .collapsible(false)
                .default_size(egui::vec2(640.0, 480.0))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.weak(&editor.abs_path);
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let resp = ui.add_sized(
                                ui.available_size() - egui::vec2(0.0, 32.0),
                                egui::TextEdit::multiline(&mut editor.content)
                                    .code_editor()
                                    .desired_width(f32::INFINITY),
                            );
                            if resp.changed() {
                                editor.is_dirty = true;
                            }
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(editor.is_dirty, egui::Button::new("Save"))
                            .clicked()
                        {
                            save_request = true;
                        }
                        if ui.button("Close").clicked() {
                            close_request = true;
                        }
                    });
                });

            // The X-button on the window translates to !open; treat it as Close.
            if !open {
                close_request = true;
            }

            if save_request {
                match std::fs::write(&editor.abs_path, &editor.content) {
                    Ok(()) => {
                        editor.is_dirty = false;
                        self.log_info(format!("Saved {}", editor.archive_path));
                    }
                    Err(e) => {
                        self.log_error(format!("Save failed: {e}"));
                    }
                }
            }

            if close_request {
                // If unsaved, drop the changes silently for now — user explicitly
                // dismissed. (We could prompt later if this becomes a footgun.)
                self.dialog.file_editor = None;
            } else {
                self.dialog.file_editor = Some(editor);
            }
        }

        // ── Modal: About ─────────────────────────────────────────────────────
        crate::panels::dialogs::draw_about(self, ctx);

        // ── Modal: Log window ────────────────────────────────────────────────
        self.draw_log_window(ctx);

        if self.dialog.show_inspector {
            self.draw_inspector_window(ctx);
        }

        if self.dialog.show_mapinfo_editor {
            self.draw_mapinfo_editor_window(ctx);
        }

        crate::panels::validation::draw_details(self, ctx);

        // Action bar -- only shown inside a project.
        if self.has_project() {
            egui::TopBottomPanel::top("action_bar").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(37.0, 30.0);
                    let busy = self.preview.export_status == ExportStatus::All;
                    let any_running = self.preview.export_status.is_running();
                    let sense = if any_running {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click()
                    };
                    let (rect, response) = ui.allocate_exact_size(btn_size, sense);

                    if ui.is_rect_visible(rect) {
                        let bg = if busy {
                            tokens::BTN_EXPORT_BUSY
                        } else if any_running {
                            tokens::BTN_EXPORT_BLOCKED
                        } else if response.is_pointer_button_down_on() {
                            tokens::BTN_EXPORT_PRESS
                        } else if response.hovered() {
                            tokens::BTN_EXPORT_HOVER
                        } else {
                            tokens::BTN_EXPORT_NORMAL
                        };
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 5.0, bg);
                        paint_export_icon(&painter, rect, egui::Color32::WHITE);
                        if busy {
                            // Tiny corner spinner so the busy state reads clearly.
                            paint_busy_dot(&painter, rect, ui.input(|i| i.time));
                        }
                    }

                    let tooltip = if busy {
                        "Exporting…"
                    } else if any_running {
                        "Another export is running"
                    } else {
                        "Export all Bundler nodes"
                    };
                    let response = response.on_hover_text(tooltip);
                    if !any_running
                        && response.clicked()
                        && self.validate_before_export("Bundle all")
                    {
                        self.preview.run_requested = true;
                    }

                    // Compile button
                    ui.add_space(4.0);
                    let compile_running = self.preview.compile_running;
                    let compile_dirty = self.project.compile_dirty;
                    let compile_label = if compile_running {
                        "Compiling..."
                    } else if compile_dirty {
                        "Compile (out of date)"
                    } else {
                        "Compile"
                    };
                    let can_compile = !compile_running && !any_running;
                    let compile_resp =
                        ui.add_enabled(can_compile, egui::Button::new(compile_label));
                    if compile_resp.clicked() && can_compile {
                        self.preview.compile_requested = true;
                    }
                    if let Some(compiled_at) = self.project.compiled_at {
                        let secs = compiled_at.elapsed().as_secs();
                        let age = if secs < 60 {
                            format!("{secs}s ago")
                        } else {
                            format!("{}m ago", secs / 60)
                        };
                        compile_resp.on_hover_text(format!("Last compiled {age}"));
                    }

                    // Edit Map Info button — opens the project's designated map
                    // info file in the OS default editor. Prompts for the file
                    // on first use.
                    ui.add_space(4.0);
                    let (info_rect, info_resp) =
                        ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(info_rect) {
                        let bg = if info_resp.is_pointer_button_down_on() {
                            tokens::BTN_MAPINFO_PRESS
                        } else if info_resp.hovered() {
                            tokens::BTN_MAPINFO_HOVER
                        } else {
                            tokens::BTN_MAPINFO_NORMAL
                        };
                        let painter = ui.painter_at(info_rect);
                        painter.rect_filled(info_rect, 5.0, bg);
                        paint_map_info_icon(&painter, info_rect, egui::Color32::WHITE);
                    }
                    let info_resp = info_resp.on_hover_text(
                        "Edit Map Info — open the project's map info file (e.g. mapinfo.lua)",
                    );
                    if info_resp.clicked() {
                        self.handle_edit_map_info_clicked();
                    }

                    // Test in BAR -- export and launch directly in the engine.
                    // When multiple game/engine versions are installed a small
                    // chevron button appears to the right of the main button
                    // for picking which version to use.
                    ui.add_space(4.0);
                    let has_choice = self.bar_versions.has_choice();
                    let chevron_w = if has_choice { 14.0 } else { 0.0 };
                    let group_size = egui::vec2(btn_size.x + chevron_w, btn_size.y);
                    let (group_rect, _) = ui.allocate_exact_size(group_size, egui::Sense::hover());

                    let bar_rect = egui::Rect::from_min_size(group_rect.min, btn_size);
                    let bar_resp =
                        ui.interact(bar_rect, ui.id().with("bar_btn"), egui::Sense::click());

                    if ui.is_rect_visible(bar_rect) {
                        let bg = if any_running {
                            tokens::BTN_BAR_BLOCKED
                        } else if bar_resp.is_pointer_button_down_on() {
                            tokens::BTN_BAR_PRESS
                        } else if bar_resp.hovered() {
                            tokens::BTN_BAR_HOVER
                        } else {
                            tokens::BTN_BAR_NORMAL
                        };
                        let painter = ui.painter_at(bar_rect);
                        let rounding = if has_choice {
                            egui::CornerRadius {
                                nw: 5,
                                sw: 5,
                                ne: 0,
                                se: 0,
                            }
                        } else {
                            egui::CornerRadius::same(5)
                        };
                        painter.rect_filled(bar_rect, rounding, bg);
                        paint_bar_icon(&painter, bar_rect, egui::Color32::WHITE);
                    }
                    let bar_resp = bar_resp.on_hover_text(
                        "Test in BAR — export and launch a skirmish directly in the BAR engine",
                    );
                    if !any_running && bar_resp.clicked() {
                        self.run_validation();
                        if bar_project::has_errors(&self.validation.findings) {
                            self.dialog.show_validation_panel = true;
                            self.log_warning(t!("editor.toolbar.validate_first"));
                        } else {
                            self.preview.test_in_bar_requested = true;
                        }
                    }

                    // Chevron -- only rendered when multiple versions exist.
                    let popup_id = ui.make_persistent_id("bar_version_picker");
                    if has_choice {
                        let chevron_rect = egui::Rect::from_min_size(
                            egui::pos2(bar_rect.max.x, group_rect.min.y),
                            egui::vec2(chevron_w, btn_size.y),
                        );
                        let chevron_resp = ui.interact(
                            chevron_rect,
                            ui.id().with("bar_chevron"),
                            egui::Sense::click(),
                        );
                        if ui.is_rect_visible(chevron_rect) {
                            let bg = if chevron_resp.is_pointer_button_down_on() {
                                tokens::BTN_BAR_PRESS
                            } else if chevron_resp.hovered() {
                                tokens::BTN_BAR_HOVER
                            } else {
                                tokens::BTN_BAR_NORMAL
                            };
                            let painter = ui.painter_at(chevron_rect);
                            let rounding = egui::CornerRadius {
                                nw: 0,
                                sw: 0,
                                ne: 5,
                                se: 5,
                            };
                            painter.rect_filled(chevron_rect, rounding, bg);
                            // 1px divider
                            painter.line_segment(
                                [chevron_rect.left_top(), chevron_rect.left_bottom()],
                                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(60)),
                            );
                            // Down-pointing triangle
                            let cx = chevron_rect.center().x;
                            let cy = chevron_rect.center().y;
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    egui::pos2(cx - 4.0, cy - 2.0),
                                    egui::pos2(cx + 4.0, cy - 2.0),
                                    egui::pos2(cx, cy + 2.5),
                                ],
                                egui::Color32::WHITE,
                                egui::Stroke::NONE,
                            ));
                        }
                        if chevron_resp.clicked() {
                            ui.memory_mut(|m| m.toggle_popup(popup_id));
                        }
                    }

                    // Version picker popover.
                    if ui.memory(|m| m.is_popup_open(popup_id)) {
                        let popup_pos = egui::pos2(group_rect.min.x, group_rect.max.y + 4.0);
                        let area_resp = egui::Area::new(popup_id)
                            .order(egui::Order::Foreground)
                            .fixed_pos(popup_pos)
                            .interactable(true)
                            .movable(false)
                            .show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.set_min_width(180.0);
                                    if self.bar_versions.game_labels.len() > 1 {
                                        ui.label("Game");
                                        for i in 0..self.bar_versions.game_labels.len() {
                                            let label = self.bar_versions.game_labels[i].clone();
                                            ui.radio_value(
                                                &mut self.bar_versions.selected_game,
                                                i,
                                                label,
                                            );
                                        }
                                    }
                                    if self.bar_versions.game_labels.len() > 1
                                        && self.bar_versions.engine_labels.len() > 1
                                    {
                                        ui.separator();
                                    }
                                    if self.bar_versions.engine_labels.len() > 1 {
                                        ui.label("Engine");
                                        for i in 0..self.bar_versions.engine_labels.len() {
                                            let label = self.bar_versions.engine_labels[i].clone();
                                            ui.radio_value(
                                                &mut self.bar_versions.selected_engine,
                                                i,
                                                label,
                                            );
                                        }
                                    }
                                });
                            });
                        // Close on click outside the popup and outside the
                        // chevron (clicking the chevron uses toggle_popup).
                        let outside = ui.input(|i| {
                            i.pointer.any_click()
                                && !i
                                    .pointer
                                    .interact_pos()
                                    .is_none_or(|p| area_resp.response.rect.contains(p))
                                && !i
                                    .pointer
                                    .interact_pos()
                                    .is_none_or(|p| group_rect.contains(p))
                        });
                        if outside {
                            ui.memory_mut(|m| m.close_popup());
                        }
                    }

                    // The toolbar Validate button used to live here. It's
                    // been removed in favour of the live Validation panel
                    // in the left sidebar (counts auto-refresh as you
                    // edit) and an automatic validation gate on the
                    // bundle / bundle-all buttons. The "Show details"
                    // button in the sidebar opens the same findings
                    // window the toolbar button used to open.

                    // 2D Inspector — top-down heightmap view with draggable
                    // start-position markers.
                    ui.add_space(4.0);
                    let (insp_rect, insp_resp) =
                        ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(insp_rect) {
                        let bg = if insp_resp.is_pointer_button_down_on() {
                            tokens::BTN_INSPECTOR_PRESS
                        } else if insp_resp.hovered() {
                            tokens::BTN_INSPECTOR_HOVER
                        } else {
                            tokens::BTN_INSPECTOR_NORMAL
                        };
                        let painter = ui.painter_at(insp_rect);
                        painter.rect_filled(insp_rect, 5.0, bg);
                        paint_inspector_icon(&painter, insp_rect, egui::Color32::WHITE);
                    }
                    let insp_resp = insp_resp
                        .on_hover_text("2D Inspector — top-down map view, place start positions");
                    if insp_resp.clicked() {
                        self.dialog.show_inspector = !self.dialog.show_inspector;
                    }

                    // Structured Map Info editor — form for atmosphere /
                    // lighting / water / physics / heights. The Edit Map
                    // Info button (pencil icon, opens raw lua) stays for
                    // power users; this is the friendly path.
                    ui.add_space(4.0);
                    let (mi_rect, mi_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(mi_rect) {
                        let bg = if mi_resp.is_pointer_button_down_on() {
                            tokens::BTN_MAPSET_PRESS
                        } else if mi_resp.hovered() {
                            tokens::BTN_MAPSET_HOVER
                        } else {
                            tokens::BTN_MAPSET_NORMAL
                        };
                        let painter = ui.painter_at(mi_rect);
                        painter.rect_filled(mi_rect, 5.0, bg);
                        paint_mapinfo_form_icon(&painter, mi_rect, egui::Color32::WHITE);
                    }
                    let mi_resp = mi_resp.on_hover_text(t!("editor.toolbar.map_settings"));
                    if mi_resp.clicked() {
                        self.dialog.show_mapinfo_editor = !self.dialog.show_mapinfo_editor;
                    }

                    // Startboxes — opens the 2D inspector at Spawns mode so
                    // the user can drag spawn markers. Lives in its own
                    // button (rather than a tab inside Map Settings) because
                    // box-authoring is a spatial task that wants the full
                    // inspector canvas, not a side-panel form.
                    ui.add_space(4.0);
                    let (sb_rect, sb_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(sb_rect) {
                        let bg = if sb_resp.is_pointer_button_down_on() {
                            tokens::BTN_SPAWNS_PRESS
                        } else if sb_resp.hovered() {
                            tokens::BTN_SPAWNS_HOVER
                        } else {
                            tokens::BTN_SPAWNS_NORMAL
                        };
                        let painter = ui.painter_at(sb_rect);
                        painter.rect_filled(sb_rect, 5.0, bg);
                        paint_startbox_icon(&painter, sb_rect, egui::Color32::WHITE);
                    }
                    let sb_resp = sb_resp.on_hover_text(t!("editor.toolbar.startboxes"));
                    if sb_resp.clicked() {
                        self.dialog.show_inspector = true;
                        self.paint.inspector_mode = InspectorMode::Spawns;
                    }
                });
                ui.add_space(4.0);
            });
        }

        // ── Toast notification (e.g. "Autosaved …") ─────────────────────────
        if let Some((msg, _)) = self.dialog.toast.clone() {
            let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("toast"));
            let painter = ctx.layer_painter(layer);
            let screen = ctx.screen_rect();
            let font = egui::FontId::proportional(13.0);
            let text_color = egui::Color32::from_rgb(220, 230, 230);
            let pad = egui::vec2(14.0, 8.0);
            let galley = painter.layout_no_wrap(msg.clone(), font.clone(), text_color);
            let size = galley.size() + pad * 2.0;
            // Bottom-center, lifted 30 px above the status bar.
            let center = egui::pos2(screen.center().x, screen.bottom() - size.y / 2.0 - 50.0);
            let rect = egui::Rect::from_center_size(center, size);
            painter.rect_filled(rect, 6.0, egui::Color32::from_black_alpha(210));
            painter.rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(1.0, tokens::BTN_INSPECTOR_HOVER),
                egui::StrokeKind::Outside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &msg,
                font,
                text_color,
            );
            // Toast expires on its own; request a repaint so the timer ticks.
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}
