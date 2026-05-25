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
    paint_atmosphere_icon, paint_bar_icon, paint_compile_icon, paint_dimensions_icon,
    paint_export_icon, paint_fog_icon, paint_grass_icon, paint_identity_icon, paint_lava_icon,
    paint_lighting_icon, paint_map_edge_icon, paint_physics_icon, paint_publish_icon,
    paint_resources_icon, paint_water_icon, BarEditorApp, ConfirmAction, ConfirmDialog,
    ExportStatus, GroupDeleteChoice, Layout, PendingAction, UnsavedDecision,
    CONFIRM_KEY_DELETE_CONNECTED_NODE,
};
use crate::editor::validation::{BlockingAction, ModalId, ValidationSummary};
use crate::panels::log::level_color;
use crate::panels::tokens;
use crate::t;

/// Paint a small severity badge in the top-right corner of `btn_rect`
/// when `summary` has any non-zero counts. Red dot for errors,
/// yellow for warnings (Error wins if both present). `Info` alone
/// renders nothing -- low-signal, would just clutter the bar.
fn paint_validation_badge(ui: &egui::Ui, btn_rect: egui::Rect, summary: &ValidationSummary) {
    if summary.is_clean() || (summary.errors == 0 && summary.warnings == 0) {
        return;
    }
    let (color, count) = if summary.errors > 0 {
        (egui::Color32::from_rgb(220, 80, 70), summary.errors)
    } else {
        (egui::Color32::from_rgb(230, 180, 60), summary.warnings)
    };
    let radius = 7.0;
    // Anchor outside the button's top-right corner so the badge
    // doesn't clip the icon underneath. ~60% of the badge sits past
    // the rect edges; the painter's clip is expanded twice the
    // radius to leave room for the overflow.
    let offset = radius * 0.6;
    let center = egui::pos2(btn_rect.max.x + offset, btn_rect.min.y - offset);
    let painter = ui.painter_at(btn_rect.expand(radius * 2.0));
    painter.circle_filled(center, radius, color);
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200)),
    );
    if count > 0 {
        let label = if count > 9 {
            "9+".to_string()
        } else {
            count.to_string()
        };
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(10.0),
            egui::Color32::WHITE,
        );
    }
}

/// Position of a button within a visually-joined group, used to
/// decide which corners get rounding and which sides get clipped
/// flush to the neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupPos {
    /// Standalone button -- full rounding on all four corners.
    #[allow(dead_code)]
    Single,
    /// Left edge of a group -- left corners rounded, right corners flush.
    Left,
    /// Interior of a group -- no corner rounding.
    Mid,
    /// Right edge of a group -- right corners rounded, left corners flush.
    Right,
}

impl GroupPos {
    fn corner_radius(self) -> egui::CornerRadius {
        match self {
            GroupPos::Single => egui::CornerRadius::same(5),
            GroupPos::Left => egui::CornerRadius {
                nw: 5,
                sw: 5,
                ne: 0,
                se: 0,
            },
            GroupPos::Mid => egui::CornerRadius::ZERO,
            GroupPos::Right => egui::CornerRadius {
                nw: 0,
                sw: 0,
                ne: 5,
                se: 5,
            },
        }
    }
}

/// Draw one of the per-tab Map Info action-bar buttons. Returns true
/// if clicked. Caller supplies the per-tab colour triple and the
/// button's position within the group so corners join cleanly with
/// neighbours.
#[allow(clippy::too_many_arguments)]
fn draw_mapinfo_tab_button(
    ui: &mut egui::Ui,
    btn_size: egui::Vec2,
    icon: fn(&egui::Painter, egui::Rect, egui::Color32),
    category: &str,
    tooltip: &str,
    colors: (egui::Color32, egui::Color32, egui::Color32),
    pos: GroupPos,
    validation: &crate::editor::validation::ValidationState,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (normal, hover, press) = colors;
        let bg = if resp.is_pointer_button_down_on() {
            press
        } else if resp.hovered() {
            hover
        } else {
            normal
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, pos.corner_radius(), bg);
        icon(&painter, rect, egui::Color32::WHITE);
    }
    let summary = validation.summary_for_category(category);
    paint_validation_badge(ui, rect, &summary);
    let hover = hover_with_summary(tooltip, &summary, "");
    let resp = resp.on_hover_text(hover);
    resp.clicked()
}

/// Append a severity-summary suffix to a button's hover text. Empty
/// summary -> base text unchanged. Used so every action-bar button
/// surfaces its own validation findings on hover without the user
/// having to open the sidebar.
fn hover_with_summary(base: &str, summary: &ValidationSummary, blocking_msg: &str) -> String {
    if summary.is_clean() {
        return base.to_string();
    }
    let mut s = base.to_string();
    s.push('\n');
    if summary.errors > 0 {
        s.push_str(&t!(
            "editor.validation.hover_errors_suffix",
            n = summary.errors
        ));
    }
    if summary.warnings > 0 {
        s.push_str(&t!(
            "editor.validation.hover_warnings_suffix",
            n = summary.warnings
        ));
    }
    if !blocking_msg.is_empty() {
        s.push_str(&t!("editor.validation.hover_blocking_prefix"));
        s.push_str(blocking_msg);
    }
    s
}

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
            Some(name) => t!(
                "editor.app.title_with_project",
                name = name,
                dirty = dirty_marker
            ),
            None => t!("editor.app.title"),
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
                        t!(
                            "editor.dialogs.confirm.delete_node_message_plural",
                            n = selection.len()
                        )
                    } else {
                        t!("editor.dialogs.confirm.delete_node_message_singular")
                    };
                    self.dialog.confirm_dialog = Some(ConfirmDialog {
                        title: t!("editor.dialogs.confirm.delete_node_title"),
                        message: msg,
                        affirm_label: t!("common.delete"),
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
                        // Hide the currently-open project from its own
                        // Recent submenu -- re-picking it would just
                        // be a no-op (and a confusing one if the user
                        // has unsaved changes). Clone the filtered
                        // list so the menu closure can mutate
                        // `self.settings` (Clear recent) without a
                        // borrow conflict.
                        let current_project = self.project.path.clone();
                        let recent_visible: Vec<std::path::PathBuf> = self
                            .settings
                            .recent_files
                            .iter()
                            .filter(|p| Some(p.as_path()) != current_project.as_deref())
                            .cloned()
                            .collect();
                        let recent_empty = recent_visible.is_empty();
                        ui.add_enabled_ui(!recent_empty, |ui| {
                            ui.menu_button(t!("editor.menu.open_recent"), |ui| {
                                for p in &recent_visible {
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
                                self.has_project() && self.active_layout == Layout::NodeGraph,
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
                            // 3D layouts require a GPU; lock them out
                            // on software adapters (no hardware Vulkan
                            // discoverable -- lavapipe under WSLg or
                            // similar) and surface the reason on hover.
                            let blocked = self.layout_blocked_by_software(layout);
                            let enabled = has_proj && !blocked;
                            let resp = ui.add_enabled(enabled, btn);
                            let resp = if blocked {
                                resp.on_disabled_hover_text(t!("editor.menu.layout_no_gpu"))
                            } else {
                                resp
                            };
                            if resp.clicked() {
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
                    .small_button(t!(
                        "editor.status.map_size",
                        w = self.map.width.saturating_sub(1) / 64,
                        h = self.map.height.saturating_sub(1) / 64,
                    ))
                    .on_hover_text(t!("editor.status.open_map_settings"))
                    .clicked()
                {
                    self.dialog.show_dimensions_editor = true;
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
                        egui::Label::new(t!("editor.status.selected", id = format!("{:?}", id)))
                            .sense(egui::Sense::click()),
                    )
                } else {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(t!("editor.status.no_selection")).weak(),
                        )
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
            egui::Window::new(t!("editor.dialogs.unsaved.title"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    let action_label = match &action {
                        PendingAction::Close => t!("editor.dialogs.unsaved.action_close"),
                        PendingAction::NewProject => {
                            t!("editor.dialogs.unsaved.action_new_project")
                        }
                        PendingAction::OpenPath(_) => t!("editor.dialogs.unsaved.action_open_file"),
                        PendingAction::LoadMacro { .. } => {
                            t!("editor.dialogs.unsaved.action_load_preset")
                        }
                    };
                    ui.label(t!("editor.dialogs.unsaved.message", action = action_label));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t!("common.save")).clicked() {
                            decision = Some(UnsavedDecision::Save);
                        }
                        if ui.button(t!("common.discard")).clicked() {
                            decision = Some(UnsavedDecision::Discard);
                        }
                        if ui.button(t!("common.cancel")).clicked() {
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
                        t!("editor.dialogs.group_delete.untitled_group", id = gid)
                    } else {
                        g.label.clone()
                    }
                })
                .unwrap_or_else(|| t!("editor.dialogs.group_delete.untitled_group", id = gid));
            let member_count = self
                .visuals
                .groups
                .get(&gid)
                .map(|g| g.member_ids.len())
                .unwrap_or(0);
            let mut decision: Option<GroupDeleteChoice> = None;
            egui::Window::new(t!("editor.dialogs.group_delete.title", label = label))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(t!("editor.dialogs.group_delete.message", n = member_count));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(t!("editor.dialogs.group_delete.affirm_group_only"))
                            .clicked()
                        {
                            decision = Some(GroupDeleteChoice::GroupOnly);
                        }
                        if ui
                            .button(t!("editor.dialogs.group_delete.affirm_group_and_members"))
                            .clicked()
                        {
                            decision = Some(GroupDeleteChoice::GroupAndMembers);
                        }
                        if ui.button(t!("common.cancel")).clicked() {
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
                        // undo and split the action). Skip
                        // FinalComposition -- it's a singleton terminal
                        // and `remove_node` would refuse anyway, but
                        // doing the filter here keeps the visuals /
                        // group-membership cleanup consistent.
                        let to_delete: Vec<NodeId> = self
                            .selection
                            .nodes
                            .iter()
                            .copied()
                            .filter(|id| self.graph.can_delete_node(*id))
                            .collect();
                        for node_id in &to_delete {
                            let _ = self.graph.remove_node(*node_id);
                            self.visuals.node_visuals.remove(node_id);
                            self.remove_node_from_group(*node_id);
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
                        ui.checkbox(
                            &mut dialog.dont_ask_again,
                            t!("editor.dialogs.confirm.dont_ask_again"),
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(&dialog.affirm_label).clicked() {
                            decision = Some(true);
                        }
                        if ui.button(t!("common.cancel")).clicked() {
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

        // ── Modal: About ─────────────────────────────────────────────────────
        crate::panels::dialogs::draw_about(self, ctx);

        // ── Modal: SD7 import progress ───────────────────────────────────────
        // No-op when no import is in flight; otherwise centered
        // non-dismissable modal showing the current import step.
        crate::panels::dialogs::draw_import_progress(self, ctx);

        // ── Modal: Log window ────────────────────────────────────────────────
        self.draw_log_window(ctx);

        if self.dialog.show_inspector {
            self.draw_inspector_window(ctx);
        }

        // Action-bar modals -- each module short-circuits when its
        // dialog flag is false, so call them all unconditionally.
        crate::panels::action_bar_modals::identity::draw(self, ctx);
        crate::panels::action_bar_modals::dimensions::draw(self, ctx);
        crate::panels::action_bar_modals::physics::draw(self, ctx);
        crate::panels::action_bar_modals::atmosphere::draw(self, ctx);
        crate::panels::action_bar_modals::fog::draw(self, ctx);
        crate::panels::action_bar_modals::lighting::draw(self, ctx);
        crate::panels::action_bar_modals::water::draw(self, ctx);
        crate::panels::action_bar_modals::resources::draw(self, ctx);
        crate::panels::action_bar_modals::grass::draw(self, ctx);
        crate::panels::action_bar_modals::map_edge::draw(self, ctx);

        crate::panels::assemble_map::draw(self, ctx);

        crate::panels::validation::draw_details(self, ctx);

        // Action bar -- only shown inside a project.
        if self.has_project() {
            egui::TopBottomPanel::top("action_bar").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let btn_size = egui::vec2(37.0, 30.0);
                    let busy = self.preview.export_status == ExportStatus::All;
                    let any_running = self.preview.export_status.is_running();
                    // "Build / ship" group: Compile -> Test in BAR (+ optional
                    // chevron) -> Bundle. Members keep individual
                    // rounded-rectangle styling -- they belong to the
                    // same conceptual group (separated from the next
                    // group by a divider) but read more clearly as
                    // discrete actions than as one continuous strip.
                    let compile_running = self.preview.compile_running;
                    let compile_dirty = self.project.compile_dirty;
                    let compile_blocked = self.validation.is_blocking(BlockingAction::Compile);
                    let can_compile = !compile_running && !any_running && !compile_blocked;
                    let compile_sense = if can_compile || compile_running {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    };
                    let (compile_rect, compile_resp) =
                        ui.allocate_exact_size(btn_size, compile_sense);
                    if ui.is_rect_visible(compile_rect) {
                        let bg = if compile_running {
                            tokens::BTN_COMPILE_BUSY
                        } else if !can_compile {
                            tokens::BTN_COMPILE_BLOCKED
                        } else if compile_resp.is_pointer_button_down_on() {
                            tokens::BTN_COMPILE_PRESS
                        } else if compile_resp.hovered() {
                            tokens::BTN_COMPILE_HOVER
                        } else {
                            tokens::BTN_COMPILE_NORMAL
                        };
                        let painter = ui.painter_at(compile_rect);
                        painter.rect_filled(compile_rect, 5.0, bg);
                        paint_compile_icon(&painter, compile_rect, egui::Color32::WHITE);
                    }
                    // No badge on the build / ship buttons -- the
                    // disabled state already communicates "blocked
                    // by validation errors", and the hover tooltip
                    // lists the offending findings via `blocking_msg`.
                    let compile_summary =
                        self.validation.summary_for_action(BlockingAction::Compile);
                    let base_hover = if compile_running {
                        t!("editor.actions.cancel_hover")
                    } else {
                        t!("editor.actions.compile")
                    };
                    // Suppress unused-warning until compile age is
                    // re-surfaced through a different UI affordance.
                    let _ = compile_dirty;
                    let blocking_msg = if compile_blocked {
                        self.validation.blocking_summary(BlockingAction::Compile, 3)
                    } else {
                        String::new()
                    };
                    let hover = hover_with_summary(&base_hover, &compile_summary, &blocking_msg);
                    let compile_clicked = compile_resp.clicked();
                    compile_resp.on_hover_text(hover);
                    if compile_clicked {
                        tracing::info!(
                            compile_running,
                            compile_blocked,
                            any_running,
                            "Compile button clicked"
                        );
                    }
                    if compile_running && compile_clicked {
                        self.preview.cancel_compile_requested = true;
                    } else if !compile_running && !compile_blocked && compile_clicked {
                        self.preview.compile_requested = true;
                    }
                    if compile_running {
                        crate::layouts::preview::draw_animated_border(ui, compile_rect);
                    }

                    // Test in BAR -- the iteration-loop action. Comes
                    // before Bundle in the action bar because (a) it's
                    // the more-used button during authoring, and (b)
                    // it's independent of Bundle (writes a `.sdd`
                    // directly into the BAR install) rather than
                    // downstream of it.
                    //
                    // When multiple game/engine versions are installed
                    // a small chevron button appears to the right for
                    // picking which version to use.
                    ui.add_space(4.0);
                    let test_in_bar_busy = self.preview.export_status == ExportStatus::TestInBar;
                    let has_choice = self.bar_versions.has_choice();
                    let chevron_w = if has_choice { 14.0 } else { 0.0 };
                    let group_size = egui::vec2(btn_size.x + chevron_w, btn_size.y);
                    let (group_rect, _) = ui.allocate_exact_size(group_size, egui::Sense::hover());

                    let bar_rect = egui::Rect::from_min_size(group_rect.min, btn_size);
                    let bar_blocked = self.validation.is_blocking(BlockingAction::TestInBar);
                    let bar_sense = if test_in_bar_busy || (!any_running && !bar_blocked) {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    };
                    let bar_resp = ui.interact(bar_rect, ui.id().with("bar_btn"), bar_sense);

                    if ui.is_rect_visible(bar_rect) {
                        let bg = if test_in_bar_busy {
                            tokens::BTN_BAR_NORMAL
                        } else if any_running {
                            tokens::BTN_BAR_BLOCKED
                        } else if bar_resp.is_pointer_button_down_on() {
                            tokens::BTN_BAR_PRESS
                        } else if bar_resp.hovered() {
                            tokens::BTN_BAR_HOVER
                        } else {
                            tokens::BTN_BAR_NORMAL
                        };
                        let painter = ui.painter_at(bar_rect);
                        // Bar button keeps its left corners rounded;
                        // when a chevron is present, its right corners
                        // stay flush so the two read as one widget.
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
                    if test_in_bar_busy {
                        crate::layouts::preview::draw_animated_border(ui, bar_rect);
                    }
                    let bar_summary = self
                        .validation
                        .summary_for_action(BlockingAction::TestInBar);
                    let base_tooltip = if test_in_bar_busy {
                        t!("editor.actions.cancel_hover")
                    } else {
                        t!("editor.actions.test_in_bar")
                    };
                    let blocking_msg = if bar_blocked {
                        self.validation
                            .blocking_summary(BlockingAction::TestInBar, 3)
                    } else {
                        String::new()
                    };
                    let bar_tooltip =
                        hover_with_summary(&base_tooltip, &bar_summary, &blocking_msg);
                    let bar_resp = bar_resp.on_hover_text(bar_tooltip);
                    if test_in_bar_busy && bar_resp.clicked() {
                        self.preview.cancel_export_requested = true;
                    } else if !any_running && !bar_blocked && bar_resp.clicked() {
                        self.run_validation();
                        if bar_project::has_errors(&self.validation.findings) {
                            self.dialog.show_validation_panel = true;
                            self.log_warning(t!("editor.toolbar.validate_first"));
                        } else {
                            self.preview.test_in_bar_requested = true;
                        }
                    }

                    // Bundle (Export all Bundler nodes) -- the ship-it
                    // action and rightmost button of the build group.
                    ui.add_space(4.0);
                    let bundle_blocked = self.validation.is_blocking(BlockingAction::Bundle);
                    let bundle_sense = if busy || (!any_running && !bundle_blocked) {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    };
                    let (rect, response) = ui.allocate_exact_size(btn_size, bundle_sense);

                    if ui.is_rect_visible(rect) {
                        let bg = if busy {
                            tokens::BTN_EXPORT_BUSY
                        } else if any_running || bundle_blocked {
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
                    }
                    if busy {
                        crate::layouts::preview::draw_animated_border(ui, rect);
                    }

                    let bundle_summary = self.validation.summary_for_action(BlockingAction::Bundle);
                    let base_tooltip = if busy {
                        t!("editor.actions.cancel_hover")
                    } else {
                        t!("editor.actions.bundle.hover")
                    };
                    let blocking_msg = if bundle_blocked {
                        self.validation.blocking_summary(BlockingAction::Bundle, 3)
                    } else {
                        String::new()
                    };
                    let tooltip = hover_with_summary(&base_tooltip, &bundle_summary, &blocking_msg);
                    let response = response.on_hover_text(tooltip);
                    if busy && response.clicked() {
                        self.preview.cancel_export_requested = true;
                    } else if !any_running
                        && !bundle_blocked
                        && response.clicked()
                        && self.validate_before_export(&t!(
                            "editor.actions.bundle.label_for_validation"
                        ))
                    {
                        self.preview.run_requested = true;
                    }

                    // Publish -- disabled placeholder; not wired to any action yet.
                    // TODO: wire to a map-publishing flow (upload to BAR lobby / itch.io / etc.)
                    ui.add_space(4.0);
                    let (pub_rect, _pub_resp) =
                        ui.allocate_exact_size(btn_size, egui::Sense::hover());
                    if ui.is_rect_visible(pub_rect) {
                        let painter = ui.painter_at(pub_rect);
                        painter.rect_filled(pub_rect, 5.0, tokens::BTN_PUBLISH_DISABLED);
                        paint_publish_icon(&painter, pub_rect, egui::Color32::from_white_alpha(60));
                    }
                    _pub_resp.on_hover_text(t!("editor.actions.publish.coming_soon"));

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
                            // Chevron is the right edge of the bar
                            // button widget.
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
                                        ui.label(t!("editor.actions.version_picker.game"));
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
                                        ui.label(t!("editor.actions.version_picker.engine"));
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

                    // Visual separator: end of the build / ship group
                    // (Compile | Test in BAR | Bundle) and start of
                    // the project-metadata group.
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Metadata group: action-bar buttons whose modals
                    // describe the project itself rather than its
                    // rendered world.  Each entry's `flag_for` returns
                    // the `&mut bool` on `DialogState` that the
                    // button toggles directly -- no shared mapinfo
                    // tab indirection.
                    type IconFn = fn(&egui::Painter, egui::Rect, egui::Color32);
                    type ColorTriple = (egui::Color32, egui::Color32, egui::Color32);
                    type FlagFn = fn(&mut crate::dialog::DialogState) -> &mut bool;
                    // Tab tooltip strings are i18n keys -- resolved
                    // at the call site so we keep the static array
                    // small and language-agnostic.
                    let metadata_tabs: &[(IconFn, FlagFn, &str, &str, ColorTriple)] = &[
                        (
                            paint_identity_icon,
                            |d| &mut d.show_identity_editor,
                            "identity",
                            "editor.actions.tabs.identity",
                            (
                                tokens::BTN_TAB_IDENTITY_NORMAL,
                                tokens::BTN_TAB_IDENTITY_HOVER,
                                tokens::BTN_TAB_IDENTITY_PRESS,
                            ),
                        ),
                        (
                            paint_dimensions_icon,
                            |d| &mut d.show_dimensions_editor,
                            "dimensions",
                            "editor.actions.tabs.dimensions",
                            (
                                tokens::BTN_TAB_DIMENSIONS_NORMAL,
                                tokens::BTN_TAB_DIMENSIONS_HOVER,
                                tokens::BTN_TAB_DIMENSIONS_PRESS,
                            ),
                        ),
                        (
                            paint_physics_icon,
                            |d| &mut d.show_physics_editor,
                            "physics",
                            "editor.actions.tabs.physics",
                            (
                                tokens::BTN_TAB_PHYSICS_NORMAL,
                                tokens::BTN_TAB_PHYSICS_HOVER,
                                tokens::BTN_TAB_PHYSICS_PRESS,
                            ),
                        ),
                        (
                            paint_resources_icon,
                            |d| &mut d.show_resources_editor,
                            "resources",
                            "editor.actions.tabs.resources",
                            (
                                tokens::BTN_TAB_RESOURCES_NORMAL,
                                tokens::BTN_TAB_RESOURCES_HOVER,
                                tokens::BTN_TAB_RESOURCES_PRESS,
                            ),
                        ),
                    ];
                    // Environment group: world-appearance modals.
                    // Atmosphere / lighting / water are all
                    // `MapSettings` blocks; Grass is its own modal but
                    // groups here so the four world-appearance
                    // controls cluster on the action bar.
                    let env_tabs: &[(IconFn, FlagFn, &str, &str, ColorTriple)] = &[
                        (
                            paint_atmosphere_icon,
                            |d| &mut d.show_atmosphere_editor,
                            "atmosphere",
                            "editor.actions.tabs.atmosphere",
                            (
                                tokens::BTN_TAB_ATMOSPHERE_NORMAL,
                                tokens::BTN_TAB_ATMOSPHERE_HOVER,
                                tokens::BTN_TAB_ATMOSPHERE_PRESS,
                            ),
                        ),
                        (
                            paint_fog_icon,
                            |d| &mut d.show_fog_editor,
                            "fog",
                            "editor.actions.tabs.fog",
                            (
                                tokens::BTN_TAB_FOG_NORMAL,
                                tokens::BTN_TAB_FOG_HOVER,
                                tokens::BTN_TAB_FOG_PRESS,
                            ),
                        ),
                        (
                            paint_lighting_icon,
                            |d| &mut d.show_lighting_editor,
                            "lighting",
                            "editor.actions.tabs.lighting",
                            (
                                tokens::BTN_TAB_LIGHTING_NORMAL,
                                tokens::BTN_TAB_LIGHTING_HOVER,
                                tokens::BTN_TAB_LIGHTING_PRESS,
                            ),
                        ),
                        // Water / Lava: same modal, but the action-bar
                        // affordance follows the map's current mode so
                        // the user can tell at a glance which form
                        // will open. Mode lives on
                        // `MapSettings::fluid_mode`; the exporter and
                        // modal key off the same enum.
                        if matches!(
                            self.map_settings().fluid_mode,
                            Some(bar_project::recipe::FluidMode::Lava)
                        ) {
                            (
                                paint_lava_icon as IconFn,
                                (|d| &mut d.show_water_editor) as FlagFn,
                                "lava",
                                "editor.actions.tabs.lava",
                                (
                                    tokens::BTN_TAB_LAVA_NORMAL,
                                    tokens::BTN_TAB_LAVA_HOVER,
                                    tokens::BTN_TAB_LAVA_PRESS,
                                ),
                            )
                        } else {
                            (
                                paint_water_icon as IconFn,
                                (|d| &mut d.show_water_editor) as FlagFn,
                                "water",
                                "editor.actions.tabs.water",
                                (
                                    tokens::BTN_TAB_WATER_NORMAL,
                                    tokens::BTN_TAB_WATER_HOVER,
                                    tokens::BTN_TAB_WATER_PRESS,
                                ),
                            )
                        },
                    ];

                    // Draw the metadata group: leading separator
                    // distinguishes it from the build group; group-
                    // internal item spacing drops to zero so the four
                    // buttons render flush.
                    let saved_spacing = ui.spacing().item_spacing.x;
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let mut clicked_flag: Option<FlagFn> = None;
                    let last_meta = metadata_tabs.len() - 1;
                    for (i, (icon, flag, category, tooltip, colors)) in
                        metadata_tabs.iter().enumerate()
                    {
                        let pos = if i == 0 {
                            GroupPos::Left
                        } else if i == last_meta {
                            GroupPos::Right
                        } else {
                            GroupPos::Mid
                        };
                        if draw_mapinfo_tab_button(
                            ui,
                            btn_size,
                            *icon,
                            category,
                            &t!(tooltip),
                            *colors,
                            pos,
                            &self.validation,
                        ) {
                            clicked_flag = Some(*flag);
                        }
                    }
                    ui.spacing_mut().item_spacing.x = saved_spacing;

                    // Environment group: atmosphere / lighting / water
                    // (schema-driven Map Info sections, Left/Mid/Mid)
                    // followed by Grass on the right edge.
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, (icon, flag, category, tooltip, colors)) in env_tabs.iter().enumerate()
                    {
                        let pos = if i == 0 {
                            GroupPos::Left
                        } else {
                            GroupPos::Mid
                        };
                        if draw_mapinfo_tab_button(
                            ui,
                            btn_size,
                            *icon,
                            category,
                            &t!(tooltip),
                            *colors,
                            pos,
                            &self.validation,
                        ) {
                            clicked_flag = Some(*flag);
                        }
                    }
                    // Grass: right edge of the environment group.
                    let (gr_rect, gr_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(gr_rect) {
                        let bg = if gr_resp.is_pointer_button_down_on() {
                            tokens::BTN_GRASS_PRESS
                        } else if gr_resp.hovered() {
                            tokens::BTN_GRASS_HOVER
                        } else {
                            tokens::BTN_GRASS_NORMAL
                        };
                        let painter = ui.painter_at(gr_rect);
                        painter.rect_filled(gr_rect, GroupPos::Right.corner_radius(), bg);
                        paint_grass_icon(&painter, gr_rect, egui::Color32::WHITE);
                    }
                    let gr_summary = self.validation.summary_for_modal(ModalId::Grass);
                    paint_validation_badge(ui, gr_rect, &gr_summary);
                    let gr_hover =
                        hover_with_summary(&t!("editor.actions.tabs.grass"), &gr_summary, "");
                    let gr_resp = gr_resp.on_hover_text(gr_hover);
                    if gr_resp.clicked() {
                        self.dialog.show_grass_editor = !self.dialog.show_grass_editor;
                    }
                    ui.spacing_mut().item_spacing.x = saved_spacing;

                    if let Some(flag_fn) = clicked_flag {
                        let flag = flag_fn(&mut self.dialog);
                        *flag = !*flag;
                    }

                    // Map Edge — dedicated panel for the mirrored map-edge
                    // extension. Holds the `grassShadingTex` picker /
                    // preview today; future map-edge knobs (curvature
                    // bend, atmosphere fog tuning) land in the same
                    // panel rather than crowding the main Map Settings
                    // modal.
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    let (me_rect, me_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(me_rect) {
                        let bg = if me_resp.is_pointer_button_down_on() {
                            tokens::BTN_MAPEDGE_PRESS
                        } else if me_resp.hovered() {
                            tokens::BTN_MAPEDGE_HOVER
                        } else {
                            tokens::BTN_MAPEDGE_NORMAL
                        };
                        let painter = ui.painter_at(me_rect);
                        painter.rect_filled(me_rect, 5.0, bg);
                        paint_map_edge_icon(&painter, me_rect, egui::Color32::WHITE);
                    }
                    let me_summary = self.validation.summary_for_modal(ModalId::MapEdge);
                    paint_validation_badge(ui, me_rect, &me_summary);
                    let me_hover =
                        hover_with_summary(&t!("editor.actions.tabs.map_edge"), &me_summary, "");
                    let me_resp = me_resp.on_hover_text(me_hover);
                    if me_resp.clicked() {
                        self.dialog.show_map_edge_editor = !self.dialog.show_map_edge_editor;
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
