//! Contextual properties surface — the floating panel that pops up
//! after a brief hover gate when the user clicks a node, group, or
//! connection. One method per per-NodeType property body
//! (PassThrough, PaintedHeightmap, PaintedTexture, Sculpt, …) plus
//! the dispatch and panel-layout glue. Lives in panels/ because
//! it's a UI surface; methods stay on BarEditorApp (rather than
//! free pub(crate) fn draw(app, …)) so the deep field access
//! remains clean — &mut self already grants what's needed.

use std::collections::HashMap;
use std::time::Instant;

use bar_graph::{self, GraphEngine, NodeId, NodeType, ParamValue};
use eframe::egui;

use crate::app::*;

impl BarEditorApp {
    /// Runs once per frame. Resolves the pending hover-gate into an
    /// active panel, renders the panel as a floating, non-movable,
    /// auto-positioned popup near the target, and closes the panel
    /// when the user clicks anywhere outside it.
    pub(crate) fn tick_props_panel(&mut self, ctx: &egui::Context) {
        // ── Pending → active promotion ────────────────────────────────
        // The user clicked a node / group. Once the cursor has held
        // (mostly) still on top of the same target for the gate's
        // duration, the panel pops open.
        let now = Instant::now();
        let pointer = ctx.pointer_latest_pos();
        let target_now = pointer.and_then(|p| self.props_target_under_pointer(p));
        if let Some(pending) = self.dialog.pending_props_open.clone() {
            let elapsed = now.duration_since(pending.armed_at).as_millis() as u64;
            let drift = pointer
                .map(|p| p.distance(pending.armed_pos))
                .unwrap_or(f32::INFINITY);
            let still_on_target = target_now.as_ref() == Some(&pending.target);
            if !still_on_target || drift > PROPS_OPEN_MOVE_TOLERANCE {
                // User moved on (or clicked an action button etc.).
                self.dialog.pending_props_open = None;
            } else if elapsed >= PROPS_OPEN_DELAY_MS {
                self.active_props = Some(pending.target.clone());
                self.dialog.pending_props_open = None;
            } else {
                // Still inside the gate — request a repaint so we
                // come back and check again before the user has to
                // wiggle the mouse.
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }

        // ── Render ─────────────────────────────────────────────────────
        let Some(target) = self.active_props.clone() else {
            self.active_props_rect = None;
            return;
        };
        // Validate the target still exists; if it doesn't, drop the
        // panel cleanly.
        let target_rect = match self.props_target_screen_rect(&target) {
            Some(r) => r,
            None => {
                self.active_props = None;
                self.active_props_rect = None;
                return;
            }
        };

        // Estimate panel size up front so the position-finder can
        // pick a side that fits. The actual rendered size is captured
        // afterwards for the next frame's click-outside test.
        let est_size = egui::vec2(300.0, 360.0);
        let pos = self.position_props_panel(target_rect, est_size, ctx);

        let mut close_panel = false;
        let area_id = egui::Id::new(("contextual_props", target.id_hash()));
        let resp = egui::Area::new(area_id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .interactable(true)
            .movable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(ui.visuals().window_fill)
                    .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_min_width(280.0);
                        ui.set_max_width(360.0);
                        ui.set_max_height(420.0);
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                self.draw_properties_for(ui, &target);
                                ui.add_space(6.0);
                                ui.separator();
                                if ui.button("Close").clicked() {
                                    close_panel = true;
                                }
                            });
                    });
            });
        let panel_rect = resp.response.rect;
        self.active_props_rect = Some(panel_rect);

        // Click-outside-to-close. Only triggers on the press, not on
        // hold/drag, so dragging from inside the panel doesn't close
        // it. Also closes on Esc.
        let pressed = ctx.input(|i| i.pointer.any_pressed());
        let pointer = ctx.pointer_interact_pos();
        if pressed {
            let inside_panel = pointer.map(|p| panel_rect.contains(p)).unwrap_or(false);
            if !inside_panel {
                close_panel = true;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close_panel = true;
        }
        if close_panel {
            self.active_props = None;
            self.active_props_rect = None;
        }
    }

    /// Which target (if any) the cursor is currently hovering over.
    /// Used to decide whether the post-click hover gate is still
    /// pointing at the same thing it was armed against.
    pub(crate) fn props_target_under_pointer(&self, p: egui::Pos2) -> Option<PropsTarget> {
        // Collapsed SubGraph blocks render on top of the canvas and
        // hide their member nodes, so check them BEFORE walking
        // node_visuals — otherwise a hidden inner node at a
        // coincident position could intercept the hit.
        for (gid, rect) in &self.collapsed_subgraph_rects {
            if rect.contains(p) {
                return Some(PropsTarget::Group(*gid));
            }
        }
        // Skip nodes that aren't being rendered this frame. Without
        // this, hovering an inner subgraph node (which is hidden)
        // would erroneously match it as a Node target and reset the
        // hover gate that was armed for the SubGraph.
        let hidden = self.hidden_nodes_this_frame();
        for (id, visual) in &self.node_visuals {
            if hidden.contains(id) {
                continue;
            }
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    visual.position.x + self.canvas_offset.x,
                    visual.position.y + self.canvas_offset.y,
                ),
                visual.size,
            );
            if rect.contains(p) {
                return Some(PropsTarget::Node(*id));
            }
        }
        // Plain groups (non-collapsed): header and body cached from
        // the previous draw_groups frame.
        for (gid, hr) in &self.group_header_rects {
            if hr.contains(p) {
                return Some(PropsTarget::Group(*gid));
            }
        }
        for (gid, br) in &self.group_body_rects {
            if br.contains(p) {
                return Some(PropsTarget::Group(*gid));
            }
        }
        None
    }

    /// Screen-space rect of the target the popup should anchor to.
    /// Returns `None` when the target no longer exists (e.g. the
    /// node was deleted while the popup was up — the popup then
    /// closes itself).
    pub(crate) fn props_target_screen_rect(&self, target: &PropsTarget) -> Option<egui::Rect> {
        match target {
            PropsTarget::Node(id) => {
                let v = self.node_visuals.get(id)?;
                Some(egui::Rect::from_min_size(
                    egui::pos2(
                        v.position.x + self.canvas_offset.x,
                        v.position.y + self.canvas_offset.y,
                    ),
                    v.size,
                ))
            }
            PropsTarget::Group(gid) => {
                // Collapsed subgraph block rect first — when a group
                // is collapsed, its header / body rects don't exist.
                if let Some(r) = self.collapsed_subgraph_rects.get(gid) {
                    return Some(*r);
                }
                let h = self.group_header_rects.get(gid)?;
                let b = self.group_body_rects.get(gid)?;
                Some(h.union(*b))
            }
        }
    }

    /// Pick a position for the props panel near `target_rect`,
    /// preferring right > left > below > above. Falls back to the
    /// canvas's right margin when the panel doesn't fit anywhere
    /// without straddling a screen edge.
    pub(crate) fn position_props_panel(
        &self,
        target_rect: egui::Rect,
        size: egui::Vec2,
        ctx: &egui::Context,
    ) -> egui::Pos2 {
        let screen = ctx.screen_rect();
        let margin = 8.0;
        // Right
        if target_rect.right() + margin + size.x <= screen.right() {
            return egui::pos2(target_rect.right() + margin, target_rect.top());
        }
        // Left
        if target_rect.left() - margin - size.x >= screen.left() {
            return egui::pos2(target_rect.left() - margin - size.x, target_rect.top());
        }
        // Below
        if target_rect.bottom() + margin + size.y <= screen.bottom() {
            return egui::pos2(target_rect.left(), target_rect.bottom() + margin);
        }
        // Above
        if target_rect.top() - margin - size.y >= screen.top() {
            return egui::pos2(target_rect.left(), target_rect.top() - margin - size.y);
        }
        // Fallback: clamp to top-right of screen with a margin so
        // the panel is at least visible.
        egui::pos2(
            (screen.right() - size.x - margin).max(screen.left() + margin),
            screen.top() + margin,
        )
    }

    /// Render the contents of the contextual properties panel for
    /// the given target. Routes Node targets through the node
    /// properties UI and Group targets through the group properties
    /// UI — same rendering logic the right-side sidebar used to use.
    pub(crate) fn draw_properties_for(&mut self, ui: &mut egui::Ui, target: &PropsTarget) {
        match target {
            PropsTarget::Node(id) => {
                // Mirror the previous draw_properties node path: set
                // selected_node so the existing render code finds
                // the node's data.
                self.selected_node = Some(*id);
                self.draw_properties(ui);
            }
            PropsTarget::Group(gid) => {
                self.draw_group_properties(ui, *gid);
            }
        }
    }

    pub(crate) fn draw_properties(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        // Group selection takes priority over node selection (they're
        // mutually exclusive by invariant, but checking the group
        // first gives a clear ownership story).
        if let Some(gid) = self.selected_group {
            self.draw_group_properties(ui, gid);
            return;
        }
        if let Some(node_id) = self.selected_node {
            // Extract node data upfront so we don't hold a borrow on self.graph
            // while also needing to mutate other fields (e.g. passthrough_edit).
            let node_data = self
                .graph
                .get_node(node_id)
                .map(|n| (n.node_type.clone(), n.label.clone(), n.params.clone()));

            if let Some((node_type, node_label, node_params)) = node_data {
                let is_io = matches!(
                    node_type,
                    NodeType::SubgraphInput | NodeType::SubgraphOutput
                );

                // IO nodes don't carry a meaningful node-level
                // label or type — the visual itself already tells
                // the user it's an "Input" or "Output" of a given
                // kind. Showing the label edit and `Type:` line
                // here is just noise, so suppress both for IO
                // nodes. (`name` and `kind` params still render
                // below via the generic param editor.)
                let mut label_buf = node_label.clone();
                let mut label_changed = false;
                if !is_io {
                    let edit_resp = ui.add(
                        egui::TextEdit::singleline(&mut label_buf)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Heading),
                    );
                    label_changed = edit_resp.changed();
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Type:").weak());
                        ui.label(format!("{:?}", node_type));
                    });
                }

                let mut changed_params: Vec<(String, ParamValue)> = Vec::new();
                let mut new_label: Option<String> = None;
                if label_changed && label_buf != node_label {
                    new_label = Some(label_buf);
                }

                if node_type == NodeType::PassThrough {
                    ui.separator();
                    self.draw_passthrough_properties(ui, node_id, &node_params);
                } else if node_type == NodeType::PaintedHeightmap {
                    ui.separator();
                    self.draw_painted_heightmap_properties(ui, node_id, &node_params);
                } else if node_type == NodeType::PaintedTexture {
                    ui.separator();
                    self.draw_painted_texture_properties(ui, node_id, &node_params);
                } else if node_type == NodeType::Sculpt {
                    ui.separator();
                    self.draw_sculpt_properties(ui, node_id, &node_params);
                } else {
                    // Generic parameter editor — show every param the type
                    // declares, with sorted keys for stable layout.
                    let mut params_to_show: Vec<(String, ParamValue)> =
                        bar_graph::default_params(&node_type).into_iter().collect();
                    params_to_show.sort_by(|a, b| a.0.cmp(&b.0));

                    // IO node kind and name are system-managed; don't
                    // expose them as editable fields.
                    if matches!(
                        node_type,
                        NodeType::SubgraphInput | NodeType::SubgraphOutput
                    ) {
                        params_to_show.retain(|(k, _)| k != "kind" && k != "name");
                    }

                    // Only show the section when there's something to edit.
                    // Nodes with no configurable params (Invert, SlopeMap, etc.)
                    // would otherwise render two back-to-back separators.
                    if !params_to_show.is_empty() {
                        ui.separator();
                        egui::Grid::new(("params_grid", node_id.0))
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                for (key, default_val) in &params_to_show {
                                    let current = node_params.get(key).unwrap_or(default_val);
                                    match current {
                                        ParamValue::Float(v) => {
                                            let mut val = *v;
                                            ui.label(key);
                                            if ui
                                                .add(egui::DragValue::new(&mut val).speed(0.01))
                                                .changed()
                                            {
                                                changed_params
                                                    .push((key.clone(), ParamValue::Float(val)));
                                            }
                                            ui.end_row();
                                        }
                                        ParamValue::UInt(v) => {
                                            let mut val = *v as i32;
                                            ui.label(key);
                                            if ui
                                                .add(egui::DragValue::new(&mut val).range(1..=20))
                                                .changed()
                                            {
                                                changed_params.push((
                                                    key.clone(),
                                                    ParamValue::UInt(val as u32),
                                                ));
                                            }
                                            ui.end_row();
                                        }
                                        ParamValue::Int(v) => {
                                            let mut val = *v;
                                            ui.label(key);
                                            if ui.add(egui::DragValue::new(&mut val)).changed() {
                                                changed_params
                                                    .push((key.clone(), ParamValue::Int(val)));
                                            }
                                            ui.end_row();
                                        }
                                        ParamValue::Bool(v) => {
                                            let mut val = *v;
                                            ui.label("");
                                            if ui.checkbox(&mut val, key).changed() {
                                                changed_params
                                                    .push((key.clone(), ParamValue::Bool(val)));
                                            }
                                            ui.end_row();
                                        }
                                        ParamValue::String(v) => {
                                            let mut val = v.clone();
                                            ui.label(key);
                                            if let Some(choices) =
                                                bar_graph::param_choices(&node_type, key)
                                            {
                                                let id = ("param_choice", node_id.0, key.as_str());
                                                egui::ComboBox::from_id_salt(id)
                                                    .selected_text(&val)
                                                    .show_ui(ui, |ui| {
                                                        for choice in choices {
                                                            if ui
                                                                .selectable_label(
                                                                    val == *choice,
                                                                    *choice,
                                                                )
                                                                .clicked()
                                                            {
                                                                let prev = val.clone();
                                                                val = (*choice).to_string();
                                                                changed_params.push((
                                                                    key.clone(),
                                                                    ParamValue::String(val.clone()),
                                                                ));
                                                                if node_type
                                                                    == NodeType::AutoTexture
                                                                    && key == "biome"
                                                                    && val != prev
                                                                {
                                                                    let bd =
                                                                        bar_graph::biome_defaults(
                                                                            &val,
                                                                        );
                                                                    changed_params.push((
                                                                        "rock_color".to_string(),
                                                                        ParamValue::String(
                                                                            bd.rock_color
                                                                                .to_string(),
                                                                        ),
                                                                    ));
                                                                    changed_params.push((
                                                                        "slope_power".to_string(),
                                                                        ParamValue::Float(
                                                                            bd.slope_power,
                                                                        ),
                                                                    ));
                                                                }
                                                                let is_noise = matches!(
                                                                    node_type,
                                                                    NodeType::PerlinNoise
                                                                        | NodeType::SimplexNoise
                                                                        | NodeType::WorleyNoise
                                                                        | NodeType::RidgedNoise
                                                                );
                                                                if is_noise
                                                                    && key == "character"
                                                                    && val != prev
                                                                {
                                                                    let cd =
                                                                    bar_graph::character_defaults(
                                                                        &node_type, &val,
                                                                    );
                                                                    changed_params.push((
                                                                        "frequency".to_string(),
                                                                        ParamValue::Float(
                                                                            cd.frequency,
                                                                        ),
                                                                    ));
                                                                    changed_params.push((
                                                                        "octaves".to_string(),
                                                                        ParamValue::UInt(
                                                                            cd.octaves,
                                                                        ),
                                                                    ));
                                                                    changed_params.push((
                                                                        "lacunarity".to_string(),
                                                                        ParamValue::Float(
                                                                            cd.lacunarity,
                                                                        ),
                                                                    ));
                                                                    changed_params.push((
                                                                        "persistence".to_string(),
                                                                        ParamValue::Float(
                                                                            cd.persistence,
                                                                        ),
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                    });
                                            } else if bar_graph::param_is_color(&node_type, key) {
                                                let rgb = parse_hex_color(&val)
                                                    .unwrap_or([128, 128, 128]);
                                                let mut c32 =
                                                    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                                if ui.color_edit_button_srgba(&mut c32).changed() {
                                                    let new_hex = format!(
                                                        "{:02X}{:02X}{:02X}",
                                                        c32.r(),
                                                        c32.g(),
                                                        c32.b()
                                                    );
                                                    val = new_hex;
                                                    changed_params.push((
                                                        key.clone(),
                                                        ParamValue::String(val.clone()),
                                                    ));
                                                }
                                            } else {
                                                ui.horizontal(|ui| {
                                                    if ui
                                                        .add(
                                                            egui::TextEdit::singleline(&mut val)
                                                                .desired_width(f32::INFINITY),
                                                        )
                                                        .changed()
                                                    {
                                                        changed_params.push((
                                                            key.clone(),
                                                            ParamValue::String(val.clone()),
                                                        ));
                                                    }
                                                    if key == "path" && ui.button("…").clicked() {
                                                        if let Some(picked) =
                                                            make_path_dialog(self, &node_type)
                                                                .pick_file()
                                                        {
                                                            changed_params.push((
                                                                key.clone(),
                                                                ParamValue::String(
                                                                    picked
                                                                        .to_string_lossy()
                                                                        .to_string(),
                                                                ),
                                                            ));
                                                        }
                                                    }
                                                });
                                            }
                                            ui.end_row();
                                        }
                                        ParamValue::Vec2(_) => {}
                                    }
                                }
                            });
                    } // end if !params_to_show.is_empty()
                } // end else (generic params branch)

                // Apply parameter changes
                if !changed_params.is_empty() {
                    self.push_undo("Change parameter");
                    if let Some(node) = self.graph.get_node_mut(node_id) {
                        for (key, value) in changed_params {
                            node.params.insert(key, value);
                        }
                        node.mark_dirty();
                    }
                }
                if let Some(new_label) = new_label {
                    self.push_undo("Rename node");
                    if let Some(node) = self.graph.get_node_mut(node_id) {
                        node.label = new_label;
                    }
                }

                ui.separator();
            } else {
                self.selected_node = None;
                ui.label("Select a node to edit properties.");
            }
        } else {
            ui.label("Select a node to edit properties.");
        }
    }

    /// Render the PassThrough node's file list editor in the properties panel.
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

    /// Properties UI for a selected group: editable label, colour
    /// picker, member count, subgraph toggle + port management when in
    /// subgraph mode, and delete (with confirmation).
    pub(crate) fn draw_group_properties(&mut self, ui: &mut egui::Ui, gid: u64) {
        // Snapshot the current state into locals so the UI body
        // doesn't have to thread mutable borrows.
        let snapshot = match self.groups.get(&gid) {
            Some(g) => g.clone(),
            None => {
                self.selected_group = None;
                return;
            }
        };
        let mut label_buf = snapshot.label.clone();
        let mut color_idx = snapshot.color_idx;
        let mut is_subgraph = snapshot.is_subgraph;
        let mut collapsed = snapshot.collapsed;
        let mut inputs = snapshot.subgraph_inputs.clone();
        let mut outputs = snapshot.subgraph_outputs.clone();

        // Editable label — same affordance node titles use.
        let resp = ui.add(
            egui::TextEdit::singleline(&mut label_buf)
                .hint_text("Group label")
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Heading),
        );
        let mut dirty = false;
        if resp.changed() {
            dirty = true;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Type:").weak());
            ui.label(if is_subgraph {
                "SubGraph"
            } else {
                "Visual group"
            });
        });
        ui.weak(format!("{} member node(s)", snapshot.member_ids.len()));
        ui.separator();

        // Colour picker — radio buttons over the fixed palette.
        ui.label(egui::RichText::new("Colour").weak());
        ui.horizontal_wrapped(|ui| {
            for (i, _rgb) in GROUP_PALETTE.iter().enumerate() {
                let i = i as u8;
                let tint = group_color(i);
                let size = egui::vec2(22.0, 22.0);
                let (rect, swatch_resp) = ui.allocate_exact_size(size, egui::Sense::click());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 4.0, tint);
                if i == color_idx {
                    painter.rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 220, 120)),
                        egui::StrokeKind::Inside,
                    );
                }
                if swatch_resp.clicked() {
                    color_idx = i;
                    dirty = true;
                }
            }
        });

        ui.separator();
        // SubGraph toggle: a visual group can be promoted to a reusable
        // subgraph with explicit inputs/outputs. Demoting back drops
        // the port definitions.
        if ui
            .checkbox(&mut is_subgraph, "Use as a SubGraph (reusable)")
            .changed()
        {
            dirty = true;
            if !is_subgraph {
                inputs.clear();
                outputs.clear();
                collapsed = false;
            }
        }
        if is_subgraph {
            if ui
                .checkbox(&mut collapsed, "Collapsed (single block)")
                .changed()
            {
                dirty = true;
            }
            ui.add_space(4.0);
            // Build the member-port pool used by binding dropdowns.
            // `inputs_pool` lists each member's INPUT ports (for
            // external input bindings); `outputs_pool` lists each
            // member's OUTPUT ports (for external output bindings).
            let (inputs_pool, outputs_pool) = {
                let mut ip: Vec<(NodeId, String, Vec<(String, String)>)> = Vec::new();
                let mut op: Vec<(NodeId, String, Vec<(String, String)>)> = Vec::new();
                for nid in &snapshot.member_ids {
                    if let Some(node) = self.graph.get_node(*nid) {
                        let label = node.label.clone();
                        let i_ports: Vec<(String, String)> = node
                            .inputs
                            .iter()
                            .map(|p| (p.name.clone(), format!("{:?}", p.kind)))
                            .collect();
                        let o_ports: Vec<(String, String)> = node
                            .outputs
                            .iter()
                            .map(|p| (p.name.clone(), format!("{:?}", p.kind)))
                            .collect();
                        if !i_ports.is_empty() {
                            ip.push((*nid, label.clone(), i_ports));
                        }
                        if !o_ports.is_empty() {
                            op.push((*nid, label, o_ports));
                        }
                    }
                }
                (ip, op)
            };
            // High-level macro parameters: the abstracted-knob layer.
            // Each one writes through directly to the bound inner-node
            // param the moment the user changes the slider, so the
            // user gets the "drop a Mountain Range, twiddle 4 sliders"
            // workflow without ever expanding the SubGraph. Drawn
            // FIRST because it's the casual-tier surface; the port
            // editors below it are advanced.
            let macro_params_snapshot: Vec<crate::state::MacroParamRuntime> =
                snapshot.macro_params.clone();
            if !macro_params_snapshot.is_empty() {
                ui.label(egui::RichText::new("Parameters").strong());
                self.draw_macro_params(ui, &macro_params_snapshot);
                ui.separator();
            }
            // Inputs / outputs are no longer edited here. They're
            // derived from the `SubgraphInput` / `SubgraphOutput`
            // nodes inside the subgraph: drop a `SubgraphInput` /
            // `SubgraphOutput` from the palette into the subgraph
            // canvas, wire it up, and rename it. The collapsed
            // block's external ports follow.
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Inputs").strong());
            if inputs.is_empty() {
                ui.weak(
                    "No external input ports yet. Open the subgraph and \
                     drop a SubgraphInput node from the palette to add one.",
                );
            } else {
                for p in &inputs {
                    ui.horizontal(|ui| {
                        ui.weak("•");
                        ui.label(&p.name);
                        ui.weak(format!("({})", p.kind));
                    });
                }
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Outputs").strong());
            if outputs.is_empty() {
                ui.weak(
                    "No external output ports yet. Open the subgraph and \
                     drop a SubgraphOutput node from the palette to add one.",
                );
            } else {
                for p in &outputs {
                    ui.horizontal(|ui| {
                        ui.weak("•");
                        ui.label(&p.name);
                        ui.weak(format!("({})", p.kind));
                    });
                }
            }
            // Suppress dead-code warnings — pool/dirty list still wired
            // through `draw_subgraph_port_list` callers below for
            // backward-compat, even though we don't render the editor
            // here.
            let _ = (&inputs_pool, &outputs_pool);
        }

        ui.separator();
        let delete_label = if is_subgraph {
            "Delete subgraph"
        } else {
            "Delete group…"
        };
        if ui.button(delete_label).clicked() {
            if is_subgraph {
                self.delete_subgraph_with_contents(gid);
            } else {
                self.pending_group_delete = Some(gid);
            }
        }

        if dirty {
            self.push_undo("Edit group properties");
            if let Some(g) = self.groups.get_mut(&gid) {
                g.label = label_buf;
                g.color_idx = color_idx;
                g.is_subgraph = is_subgraph;
                g.collapsed = collapsed;
                // Note: subgraph_inputs / subgraph_outputs are no longer
                // written from the properties panel — they're derived
                // from the IO nodes inside the subgraph by
                // `recompute_all_subgraph_io` once per frame.
                let _ = (&inputs, &outputs);
                self.project.is_dirty = true;
            }
        }
    }

    /// Render the macro-parameter widgets for a SubGraph. Each
    /// param's value is read live from the bound inner-node param,
    /// and edits are written back immediately. The SubGraph stores
    /// only the binding — the inner node owns the canonical value.
    pub(crate) fn draw_macro_params(
        &mut self,
        ui: &mut egui::Ui,
        params: &[crate::state::MacroParamRuntime],
    ) {
        let mut writes: Vec<(NodeId, String, ParamValue)> = Vec::new();
        egui::Grid::new("macro_params_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for p in params {
                    let Some((nid, param_name)) = p.binding.clone() else {
                        ui.label(&p.label);
                        ui.weak("(unbound)");
                        ui.end_row();
                        continue;
                    };
                    let cur = self
                        .graph
                        .get_node(nid)
                        .and_then(|n| n.params.get(&param_name).cloned());
                    ui.label(&p.label);
                    match (p.kind.as_str(), cur) {
                        ("Float", Some(ParamValue::Float(v))) => {
                            let mut val = v;
                            let mut drag = egui::DragValue::new(&mut val).speed(0.01);
                            if let (Some(lo), Some(hi)) = (p.min, p.max) {
                                drag = drag.range(lo..=hi);
                            }
                            if ui.add(drag).changed() {
                                writes.push((nid, param_name.clone(), ParamValue::Float(val)));
                            }
                        }
                        ("UInt", Some(ParamValue::UInt(v))) => {
                            let mut val = v as i64;
                            let mut drag = egui::DragValue::new(&mut val);
                            if let (Some(lo), Some(hi)) = (p.min, p.max) {
                                drag = drag.range((lo as i64)..=(hi as i64));
                            }
                            if ui.add(drag).changed() {
                                writes.push((
                                    nid,
                                    param_name.clone(),
                                    ParamValue::UInt(val.max(0) as u32),
                                ));
                            }
                        }
                        ("Int", Some(ParamValue::Int(v))) => {
                            let mut val = v;
                            let mut drag = egui::DragValue::new(&mut val);
                            if let (Some(lo), Some(hi)) = (p.min, p.max) {
                                drag = drag.range((lo as i32)..=(hi as i32));
                            }
                            if ui.add(drag).changed() {
                                writes.push((nid, param_name.clone(), ParamValue::Int(val)));
                            }
                        }
                        ("Bool", Some(ParamValue::Bool(v))) => {
                            let mut val = v;
                            if ui.checkbox(&mut val, "").changed() {
                                writes.push((nid, param_name.clone(), ParamValue::Bool(val)));
                            }
                        }
                        ("String", Some(ParamValue::String(v))) => {
                            let mut val = v;
                            let bound_node_type =
                                self.graph.get_node(nid).map(|n| n.node_type.clone());
                            let mut new_val: Option<String> = None;
                            if let Some(nt) = &bound_node_type {
                                if let Some(choices) = bar_graph::param_choices(nt, &param_name) {
                                    let id = ("macro_param_choice", nid.0, param_name.as_str());
                                    egui::ComboBox::from_id_salt(id)
                                        .selected_text(&val)
                                        .show_ui(ui, |ui| {
                                            for c in choices {
                                                if ui.selectable_label(val == *c, *c).clicked() {
                                                    new_val = Some((*c).to_string());
                                                }
                                            }
                                        });
                                } else if bar_graph::param_is_color(nt, &param_name) {
                                    let rgb = parse_hex_color(&val).unwrap_or([128, 128, 128]);
                                    let mut c32 = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                    if ui.color_edit_button_srgba(&mut c32).changed() {
                                        new_val = Some(format!(
                                            "{:02X}{:02X}{:02X}",
                                            c32.r(),
                                            c32.g(),
                                            c32.b()
                                        ));
                                    }
                                } else if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut val)
                                            .desired_width(f32::INFINITY),
                                    )
                                    .changed()
                                {
                                    new_val = Some(val);
                                }
                            } else if ui
                                .add(
                                    egui::TextEdit::singleline(&mut val)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed()
                            {
                                new_val = Some(val);
                            }
                            if let Some(nv) = new_val {
                                let pv = ParamValue::String(nv.clone());
                                writes.push((nid, param_name.clone(), pv.clone()));
                                if let Some(nt) = &bound_node_type {
                                    for (k, v) in
                                        bar_graph::param_side_effects(nt, &param_name, &pv)
                                    {
                                        writes.push((nid, k, v));
                                    }
                                }
                            }
                        }
                        _ => {
                            ui.weak("(missing or kind mismatch)");
                        }
                    }
                    ui.end_row();
                }
            });
        if !writes.is_empty() {
            // One undo entry per macro-param change keeps the history
            // granular; if the user sweeps a slider the undo stack
            // ends up with one entry per discrete value, matching
            // every other param widget in the editor.
            self.push_undo("Edit macro parameter");
            for (nid, name, val) in writes {
                if let Some(node) = self.graph.get_node_mut(nid) {
                    node.params.insert(name, val);
                    node.mark_dirty();
                }
            }
            self.project.is_dirty = true;
        }
    }

    /// Interactive paint canvas for a `PaintedHeightmap` node.
    /// The canvas resolution is read from `params["resolution"]` and
    /// locked once the user has painted (resolution dropdown becomes
    /// disabled to avoid mid-paint resize). Wire its output into any
    /// Heightmap input — it doubles as a mask when used that way.
    pub(crate) fn draw_painted_heightmap_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        node_params: &HashMap<String, ParamValue>,
    ) {
        const DISPLAY: f32 = 240.0;

        let mut resolution = match node_params.get("resolution") {
            Some(ParamValue::UInt(n)) => (*n).max(1) as usize,
            _ => 256,
        };
        let data_str = match node_params.get("data") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let mut pixels = mask_hex_decode(&data_str);
        let has_painted = pixels.len() == resolution * resolution && pixels.iter().any(|p| *p != 0);
        if pixels.len() != resolution * resolution {
            pixels = vec![0u8; resolution * resolution];
        }

        // Resolution dropdown — disabled once the user has painted.
        let mut new_resolution = resolution;
        ui.horizontal(|ui| {
            ui.label("Resolution:");
            ui.add_enabled_ui(!has_painted, |ui| {
                egui::ComboBox::from_id_salt(("painted_hm_res", node_id.0))
                    .selected_text(format!("{0}×{0}", resolution))
                    .show_ui(ui, |ui| {
                        for &choice in &[64usize, 128, 256, 512] {
                            ui.selectable_value(
                                &mut new_resolution,
                                choice,
                                format!("{0}×{0}", choice),
                            );
                        }
                    });
            });
            if has_painted {
                ui.label("(locked — clear to change)");
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brush size:");
            ui.add(egui::Slider::new(&mut self.paint.paint_brush_radius, 1.0..=32.0).integer());
        });
        ui.label("Left drag: raise  ·  Right drag: erase");
        ui.add_space(4.0);

        let canvas_size = egui::Vec2::splat(DISPLAY);
        let (canvas_rect, canvas_resp) =
            ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        let ctx = ui.ctx().clone();
        let mut changed = false;
        if canvas_resp.dragged_by(egui::PointerButton::Primary)
            || canvas_resp.dragged_by(egui::PointerButton::Secondary)
        {
            let erase = canvas_resp.dragged_by(egui::PointerButton::Secondary);
            let val = if erase { 0u8 } else { 255u8 };
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let rel = pos - canvas_rect.min;
                let px = (rel.x / DISPLAY * resolution as f32) as i32;
                let py = (rel.y / DISPLAY * resolution as f32) as i32;
                let br = self.paint.paint_brush_radius as i32;
                for dy in -br..=br {
                    for dx in -br..=br {
                        if dx * dx + dy * dy <= br * br {
                            let nx = px + dx;
                            let ny = py + dy;
                            if nx >= 0
                                && ny >= 0
                                && nx < resolution as i32
                                && ny < resolution as i32
                            {
                                pixels[ny as usize * resolution + nx as usize] = val;
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        let color_image = egui::ColorImage {
            size: [resolution, resolution],
            pixels: pixels
                .iter()
                .map(|&g| egui::Color32::from_gray(g))
                .collect(),
        };
        let needs_reupload = changed
            || self
                .paint
                .mask_textures
                .get(&node_id)
                .map(|t| t.size() != [resolution, resolution])
                .unwrap_or(true);
        let tex_handle = self.paint.mask_textures.entry(node_id).or_insert_with(|| {
            ctx.load_texture(
                "painted_heightmap",
                color_image.clone(),
                egui::TextureOptions::NEAREST,
            )
        });
        if needs_reupload {
            tex_handle.set(color_image, egui::TextureOptions::NEAREST);
        }

        ui.painter().image(
            tex_handle.id(),
            canvas_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            canvas_rect,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
            egui::StrokeKind::Outside,
        );

        ui.add_space(6.0);
        let mut cleared = false;
        if ui.button("Clear Canvas").clicked() {
            pixels = vec![0u8; resolution * resolution];
            changed = true;
            cleared = true;
        }

        let resolution_changed = new_resolution != resolution && !has_painted;
        if resolution_changed {
            resolution = new_resolution;
            pixels = vec![0u8; resolution * resolution];
            changed = true;
        }

        if changed {
            let new_data = mask_hex_encode(&pixels);
            self.push_undo("Paint heightmap");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                node.params
                    .insert("data".to_string(), ParamValue::String(new_data));
                if resolution_changed || cleared {
                    node.params.insert(
                        "resolution".to_string(),
                        ParamValue::UInt(resolution as u32),
                    );
                }
                node.mark_dirty();
            }
        }
    }

    /// Interactive paint canvas for a `PaintedTexture` node. RGB at
    /// 256×256. Reads brush colour from `params["brush_color"]`.
    pub(crate) fn draw_painted_texture_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        node_params: &HashMap<String, ParamValue>,
    ) {
        const PAINT_RES: usize = 256;
        const DISPLAY: f32 = 240.0;

        let data_str = match node_params.get("data") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        let mut pixels = mask_hex_decode(&data_str);
        if pixels.len() != PAINT_RES * PAINT_RES * 3 {
            pixels = vec![0u8; PAINT_RES * PAINT_RES * 3];
        }

        // Brush colour — packed 0xRRGGBB stored as a hex string in
        // params so it persists across edits.
        let color_hex = match node_params.get("brush_color") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "8B7355".to_string(),
        };
        let mut rgb = parse_hex_color(&color_hex).unwrap_or([0x8B, 0x73, 0x55]);
        let mut color_changed = false;
        ui.horizontal(|ui| {
            ui.label("Brush colour:");
            let mut c32 = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            if ui.color_edit_button_srgba(&mut c32).changed() {
                rgb = [c32.r(), c32.g(), c32.b()];
                color_changed = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brush size:");
            ui.add(egui::Slider::new(&mut self.paint.paint_brush_radius, 1.0..=32.0).integer());
        });
        ui.label("Left drag: paint  ·  Right drag: erase");
        ui.add_space(4.0);

        let canvas_size = egui::Vec2::splat(DISPLAY);
        let (canvas_rect, canvas_resp) =
            ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        let ctx = ui.ctx().clone();
        let mut changed = false;
        if canvas_resp.dragged_by(egui::PointerButton::Primary)
            || canvas_resp.dragged_by(egui::PointerButton::Secondary)
        {
            let erase = canvas_resp.dragged_by(egui::PointerButton::Secondary);
            let stamp = if erase { [0u8, 0, 0] } else { rgb };
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let rel = pos - canvas_rect.min;
                let px = (rel.x / DISPLAY * PAINT_RES as f32) as i32;
                let py = (rel.y / DISPLAY * PAINT_RES as f32) as i32;
                let br = self.paint.paint_brush_radius as i32;
                for dy in -br..=br {
                    for dx in -br..=br {
                        if dx * dx + dy * dy <= br * br {
                            let nx = px + dx;
                            let ny = py + dy;
                            if nx >= 0 && ny >= 0 && nx < PAINT_RES as i32 && ny < PAINT_RES as i32
                            {
                                let idx = (ny as usize * PAINT_RES + nx as usize) * 3;
                                pixels[idx] = stamp[0];
                                pixels[idx + 1] = stamp[1];
                                pixels[idx + 2] = stamp[2];
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        let color_image = egui::ColorImage {
            size: [PAINT_RES, PAINT_RES],
            pixels: (0..PAINT_RES * PAINT_RES)
                .map(|i| {
                    let o = i * 3;
                    egui::Color32::from_rgb(pixels[o], pixels[o + 1], pixels[o + 2])
                })
                .collect(),
        };
        let needs_reupload = changed
            || self
                .paint
                .mask_textures
                .get(&node_id)
                .map(|t| t.size() != [PAINT_RES, PAINT_RES])
                .unwrap_or(true);
        let tex_handle = self.paint.mask_textures.entry(node_id).or_insert_with(|| {
            ctx.load_texture(
                "painted_texture",
                color_image.clone(),
                egui::TextureOptions::NEAREST,
            )
        });
        if needs_reupload {
            tex_handle.set(color_image, egui::TextureOptions::NEAREST);
        }

        ui.painter().image(
            tex_handle.id(),
            canvas_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            canvas_rect,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
            egui::StrokeKind::Outside,
        );

        ui.add_space(6.0);
        if ui.button("Clear Canvas").clicked() {
            pixels = vec![0u8; PAINT_RES * PAINT_RES * 3];
            changed = true;
        }

        if changed || color_changed {
            self.push_undo("Paint texture");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                if changed {
                    let new_data = mask_hex_encode(&pixels);
                    node.params
                        .insert("data".to_string(), ParamValue::String(new_data));
                }
                if color_changed {
                    let hex = format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                    node.params
                        .insert("brush_color".to_string(), ParamValue::String(hex));
                }
                node.mark_dirty();
            }
        }
    }

    /// Interactive paint canvas for a `Sculpt` node. Stores a delta
    /// buffer (u8, 128 = no change) that is added to the upstream input
    /// at eval time. Visual style and brush values adapt based on which
    /// Bundler port the node ultimately feeds into.
    pub(crate) fn draw_sculpt_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        node_params: &HashMap<String, ParamValue>,
    ) {
        const DISPLAY: f32 = 240.0;

        let layer = infer_sculpt_layer(node_id, &self.graph);

        // Read-only layer badge so the user can see what mode they are in.
        let layer_label = match layer {
            SculptLayer::Heightmap => "Layer: Heightmap",
            SculptLayer::Metalmap => "Layer: Metal Map",
            SculptLayer::Typemap => "Layer: Type Map",
        };
        ui.label(egui::RichText::new(layer_label).color(sculpt_layer_color(layer)));
        ui.add_space(2.0);

        let mut resolution = match node_params.get("resolution") {
            Some(ParamValue::UInt(n)) => (*n).max(1) as usize,
            _ => 256,
        };
        let data_str = match node_params.get("data") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => String::new(),
        };

        // Decode or initialise. An empty string means pure passthrough;
        // once the canvas has any data we lock the resolution.
        let has_painted = !data_str.is_empty();
        let mut pixels: Vec<u8> = if has_painted {
            let decoded = mask_hex_decode(&data_str);
            if decoded.len() == resolution * resolution {
                decoded
            } else {
                vec![128u8; resolution * resolution]
            }
        } else {
            vec![128u8; resolution * resolution]
        };

        // Resolution dropdown — locked once the canvas has been touched.
        let mut new_resolution = resolution;
        ui.horizontal(|ui| {
            ui.label("Resolution:");
            ui.add_enabled_ui(!has_painted, |ui| {
                egui::ComboBox::from_id_salt(("sculpt_res", node_id.0))
                    .selected_text(format!("{0}x{0}", resolution))
                    .show_ui(ui, |ui| {
                        for &choice in &[64usize, 128, 256, 512] {
                            ui.selectable_value(
                                &mut new_resolution,
                                choice,
                                format!("{0}x{0}", choice),
                            );
                        }
                    });
            });
            if has_painted {
                ui.label("(locked -- clear to change)");
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brush size:");
            ui.add(egui::Slider::new(&mut self.paint.paint_brush_radius, 1.0..=32.0).integer());
        });

        // Strength slider only meaningful for heightmap (soft delta);
        // metalmap / typemap use binary stamps and hide it.
        if layer == SculptLayer::Heightmap {
            ui.horizontal(|ui| {
                ui.label("Strength:");
                ui.add(
                    egui::Slider::new(&mut self.paint.sculpt_brush_strength, 0.05..=1.0)
                        .step_by(0.05),
                );
            });
        }

        let hint = match layer {
            SculptLayer::Heightmap => {
                "Left drag: raise  *  Right drag: lower  *  Middle/Ctrl: reset"
            }
            SculptLayer::Metalmap => "Left drag: add metal  *  Right drag: remove metal",
            SculptLayer::Typemap => "Left drag: set type  *  Right drag: clear type",
        };
        ui.label(hint);
        ui.add_space(4.0);

        // Stamp values for each button. Heightmap uses strength-scaled
        // values; metalmap and typemap use binary extremes.
        let (raise_val, lower_val) = match layer {
            SculptLayer::Heightmap => {
                let s = self.paint.sculpt_brush_strength;
                (
                    (128.0 + s * 127.0).round() as u8,
                    (128.0 - s * 127.0).round() as u8,
                )
            }
            SculptLayer::Metalmap | SculptLayer::Typemap => (255u8, 0u8),
        };

        let canvas_size = egui::Vec2::splat(DISPLAY);
        let (canvas_rect, canvas_resp) =
            ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());

        let ctx = ui.ctx().clone();
        let mut changed = false;

        let primary = canvas_resp.dragged_by(egui::PointerButton::Primary);
        let secondary = canvas_resp.dragged_by(egui::PointerButton::Secondary);
        let middle = canvas_resp.dragged_by(egui::PointerButton::Middle)
            || (primary && ctx.input(|i| i.modifiers.ctrl));

        if primary || secondary || middle {
            let val = if middle {
                128u8
            } else if secondary {
                lower_val
            } else {
                raise_val
            };
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let rel = pos - canvas_rect.min;
                let px = (rel.x / DISPLAY * resolution as f32) as i32;
                let py = (rel.y / DISPLAY * resolution as f32) as i32;
                let br = self.paint.paint_brush_radius as i32;
                for dy in -br..=br {
                    for dx in -br..=br {
                        if dx * dx + dy * dy <= br * br {
                            let nx = px + dx;
                            let ny = py + dy;
                            if nx >= 0
                                && ny >= 0
                                && nx < resolution as i32
                                && ny < resolution as i32
                            {
                                pixels[ny as usize * resolution + nx as usize] = val;
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        // Build display texture with layer-appropriate colorization.
        let color_image = egui::ColorImage {
            size: [resolution, resolution],
            pixels: pixels
                .iter()
                .map(|&v| sculpt_delta_color(v, layer))
                .collect(),
        };
        let needs_reupload = changed
            || self
                .paint
                .mask_textures
                .get(&node_id)
                .map(|t| t.size() != [resolution, resolution])
                .unwrap_or(true);
        let tex_handle = self.paint.mask_textures.entry(node_id).or_insert_with(|| {
            ctx.load_texture(
                "sculpt_delta",
                color_image.clone(),
                egui::TextureOptions::NEAREST,
            )
        });
        if needs_reupload {
            tex_handle.set(color_image, egui::TextureOptions::NEAREST);
        }

        ui.painter().image(
            tex_handle.id(),
            canvas_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            canvas_rect,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(100)),
            egui::StrokeKind::Outside,
        );

        ui.add_space(6.0);
        let mut cleared = false;
        if ui.button("Clear Canvas").clicked() {
            // Back to pure passthrough -- empty string, not all-128.
            changed = true;
            cleared = true;
        }

        let resolution_changed = new_resolution != resolution && !has_painted;
        if resolution_changed {
            resolution = new_resolution;
            pixels = vec![128u8; resolution * resolution];
            changed = true;
        }

        if changed {
            self.push_undo("Sculpt delta");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                let new_data = if cleared {
                    String::new()
                } else {
                    mask_hex_encode(&pixels)
                };
                node.params
                    .insert("data".to_string(), ParamValue::String(new_data));
                if resolution_changed || cleared {
                    node.params.insert(
                        "resolution".to_string(),
                        ParamValue::UInt(resolution as u32),
                    );
                }
                node.mark_dirty();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sculpt helpers
// ---------------------------------------------------------------------------

/// Which Bundler layer a Sculpt node ultimately feeds. Inferred by
/// walking the downstream connection chain; never stored on the node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SculptLayer {
    Heightmap,
    Metalmap,
    Typemap,
}

/// Walk the graph BFS-style from `node_id`'s "output" port and return
/// the first Bundler port name found downstream. Falls back to Heightmap.
fn infer_sculpt_layer(node_id: NodeId, graph: &GraphEngine) -> SculptLayer {
    let mut queue = vec![node_id];
    let mut visited = std::collections::HashSet::new();
    while let Some(current) = queue.pop() {
        if !visited.insert(current) {
            continue;
        }
        for conn in graph.connections() {
            if conn.from.node_id != current {
                continue;
            }
            if let Some(target) = graph.get_node(conn.to.node_id) {
                if target.node_type == NodeType::Bundler {
                    return match conn.to.port_name.as_str() {
                        "metalmap" => SculptLayer::Metalmap,
                        "typemap" => SculptLayer::Typemap,
                        _ => SculptLayer::Heightmap,
                    };
                }
                queue.push(conn.to.node_id);
            }
        }
    }
    SculptLayer::Heightmap
}

/// Accent color used for the layer badge.
fn sculpt_layer_color(layer: SculptLayer) -> egui::Color32 {
    match layer {
        SculptLayer::Heightmap => egui::Color32::from_rgb(160, 200, 160),
        SculptLayer::Metalmap => egui::Color32::from_rgb(220, 185, 80),
        SculptLayer::Typemap => egui::Color32::from_rgb(100, 180, 220),
    }
}

/// Per-pixel canvas color for the sculpt delta display.
///
/// Heightmap: standard gray-scale gradient centered at mid-gray (128=neutral).
/// Metalmap:  neutral=dark, add=warm gold, remove=deep shadow.
/// Typemap:   neutral=dark, add=cool teal, remove=deep shadow.
fn sculpt_delta_color(val: u8, layer: SculptLayer) -> egui::Color32 {
    match layer {
        SculptLayer::Heightmap => egui::Color32::from_gray(val),
        SculptLayer::Metalmap => {
            if val >= 128 {
                let t = (val - 128) as f32 / 127.0;
                egui::Color32::from_rgb(
                    (50.0 + t * 205.0) as u8,
                    (40.0 + t * 155.0) as u8,
                    (20.0 + t * 20.0) as u8,
                )
            } else {
                let t = val as f32 / 128.0;
                egui::Color32::from_gray((t * 50.0) as u8)
            }
        }
        SculptLayer::Typemap => {
            if val >= 128 {
                let t = (val - 128) as f32 / 127.0;
                egui::Color32::from_rgb(
                    (20.0 + t * 20.0) as u8,
                    (60.0 + t * 160.0) as u8,
                    (80.0 + t * 175.0) as u8,
                )
            } else {
                let t = val as f32 / 128.0;
                egui::Color32::from_gray((t * 50.0) as u8)
            }
        }
    }
}
