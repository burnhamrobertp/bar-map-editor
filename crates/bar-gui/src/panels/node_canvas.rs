//! Node graph canvas — the centerpiece of the editor.
//! Renders nodes, ports, wires, group frames, collapsed-subgraph
//! blocks, the canvas tabs, marquee selection, and palette drop
//! handling. Lives as impl BarEditorApp blocks (rather than
//! free pub(crate) fn draw(app, …)) because the methods touch
//! the full breadth of BarEditorApp's state — &mut self
//! grants what's needed without a wall of accessors.

use std::collections::HashMap;
use std::time::Instant;

use bar_graph::{NodeId, NodeType, ParamValue, PortId, PortPlacement};
use eframe::egui;

use crate::app::*;
use crate::panels::tokens;
use crate::state::GroupRuntime;
use crate::t;

/// Return type of `draw_collapsed_subgraphs`: bounding rects keyed by
/// group id, external-port handle positions, and the in-progress
/// connection start/end if one was initiated this frame.
type CollapsedSubgraphsDraw = (
    HashMap<u64, egui::Rect>,
    HashMap<(NodeId, String), egui::Pos2>,
    Option<DragConnection>,
    Option<(NodeId, String)>,
);

/// Geometry and colour constants for the standard (non-IO) node body.
/// All node drawing passes read from here so magic numbers live in one place.
pub(crate) struct NodeStyle {
    pub bg: egui::Color32,
    pub bg_sel: egui::Color32,
    pub bg_pri: egui::Color32,
    pub border: egui::Color32,
    pub border_sel: egui::Color32,
    pub border_w: f32,
    pub border_w_sel: f32,
    pub rounding: f32,
    pub title_h: f32,
    pub title_rounding: egui::CornerRadius,
}

impl NodeStyle {
    pub(crate) fn default() -> Self {
        Self {
            bg: tokens::NODE_BG,
            bg_sel: tokens::NODE_BG_SEL,
            bg_pri: tokens::NODE_BG_PRI,
            border: tokens::NODE_BORDER,
            border_sel: tokens::NODE_BORDER_SEL,
            border_w: 1.5,
            border_w_sel: 2.0,
            rounding: 4.0,
            title_h: 20.0,
            title_rounding: egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 0,
                se: 0,
            },
        }
    }
}

fn draw_port_circle(
    painter: &egui::Painter,
    pos: egui::Pos2,
    radius: f32,
    color: egui::Color32,
    hovered: bool,
) {
    painter.circle_filled(pos, radius, color);
    if hovered {
        painter.circle_stroke(
            pos,
            radius + 2.5,
            egui::Stroke::new(
                1.5,
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120),
            ),
        );
    }
}

impl BarEditorApp {
    /// Paint a labelled rectangle behind every group. The rect's
    /// bounding box is the union of the member nodes' rects, expanded
    /// by a margin so the group reads as a frame around them rather
    /// than touching their edges.
    pub(crate) fn draw_groups(&mut self, painter: &egui::Painter, offset: egui::Vec2) {
        // Reset cached header/body rects each frame; we repopulate as
        // we draw. Hit-testing in the click pass below uses these.
        self.group_header_rects.clear();
        self.group_body_rects.clear();
        // On a SubGraph tab we don't draw any group decoration —
        // the canvas is showing only that subgraph's contents, so a
        // backdrop rectangle would be misleading.
        if matches!(self.current_view(), CanvasView::SubGraph(_)) {
            return;
        }
        let margin = 14.0_f32;
        let header_h = 20.0_f32;
        for (gid, group) in &self.groups {
            // Collapsed subgraphs draw as a compact block in a
            // separate pass after nodes (so they render on top, like
            // nodes themselves). Skip them in this backdrop pass.
            if group.is_subgraph && group.collapsed {
                continue;
            }
            // Compute union of member rects in canvas-screen space.
            let mut min: Option<egui::Pos2> = None;
            let mut max: Option<egui::Pos2> = None;
            for nid in &group.member_ids {
                let Some(visual) = self.node_visuals.get(nid) else {
                    continue;
                };
                let p0 = egui::pos2(visual.position.x + offset.x, visual.position.y + offset.y);
                let p1 = egui::pos2(p0.x + visual.size.x, p0.y + visual.size.y);
                min = Some(match min {
                    Some(m) => egui::pos2(m.x.min(p0.x), m.y.min(p0.y)),
                    None => p0,
                });
                max = Some(match max {
                    Some(m) => egui::pos2(m.x.max(p1.x), m.y.max(p1.y)),
                    None => p1,
                });
            }
            let (Some(min), Some(max)) = (min, max) else {
                continue;
            };
            let rect = egui::Rect::from_min_max(
                egui::pos2(min.x - margin, min.y - margin - header_h),
                egui::pos2(max.x + margin, max.y + margin),
            );
            let tint = group_color(group.color_idx);
            let is_selected = self.selection.group == Some(*gid);
            // Translucent body so wires + nodes drawn after this still
            // read clearly.
            painter.rect_filled(
                rect,
                6.0,
                egui::Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), 32),
            );
            // Header band — opaque enough to read the label clearly.
            // Painted BEFORE the border so the border lands on top.
            let header_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top()),
                egui::vec2(rect.width(), header_h),
            );
            painter.rect_filled(
                header_rect,
                egui::CornerRadius {
                    nw: 6,
                    ne: 6,
                    sw: 0,
                    se: 0,
                },
                egui::Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), 200),
            );
            // Border is painted last so the header fill never covers
            // it — keeps groups visually consistent with nodes (which
            // don't lose their border to the title block).
            // The selected style mirrors nodes: same blue at 1.5 px so
            // the user reads "this is the active selection" the same
            // way regardless of what kind of thing is selected.
            let stroke = if is_selected {
                egui::Stroke::new(1.5, tokens::NODE_BORDER_SEL)
            } else {
                egui::Stroke::new(1.5, tint.gamma_multiply(0.9))
            };
            painter.rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Outside);
            let label_text = if group.label.is_empty() {
                format!("Group {gid}")
            } else {
                group.label.clone()
            };
            painter.text(
                egui::pos2(rect.left() + 8.0, header_rect.center().y),
                egui::Align2::LEFT_CENTER,
                label_text,
                egui::FontId::proportional(11.5),
                egui::Color32::WHITE,
            );
            // Body rect = full minus header, for click hit-testing.
            let body_rect =
                egui::Rect::from_min_max(egui::pos2(rect.left(), header_rect.bottom()), rect.max);
            self.group_header_rects.insert(*gid, header_rect);
            self.group_body_rects.insert(*gid, body_rect);
        }
    }

    /// Reflow nodes into a left-to-right column layout that respects
    /// each unit's actual bounding box. The previous version used a
    /// fixed row pitch and could place a 240 px tall Bundler under
    /// itself; this one stacks using real heights so no two units
    /// overlap.
    ///
    /// Priorities (in order):
    /// 1. **No overlap.** Column widths = max unit width in that
    ///    column + horizontal gap; intra-column stacks add up actual
    ///    heights + vertical gap. Both invariants are guaranteed by
    ///    the geometry, not by hope.
    /// 2. **Short connectors.** Within a column, units are ordered
    ///    by the average Y of their already-placed sources
    ///    (barycentric sort). A node sits near the centroid of its
    ///    inputs, so wires don't have to climb across the canvas.
    /// 3. **Fewer crossings.** Falls out of the barycentric order
    ///    above for free in most cases — well-known to approximate
    ///    a minimum-crossing placement on layered DAGs without the
    ///    cost of full ILP-style optimisation.
    ///
    /// Selection rules:
    /// - `selected_group` set → that group as one rigid block.
    /// - Individual nodes selected → those nodes; collapsed-subgraph
    ///   members get bundled with their group.
    /// - Otherwise → every top-level unit.
    pub(crate) fn auto_layout_selection(&mut self) {
        let target_units = self.collect_layout_units();
        if target_units.is_empty() {
            return;
        }
        // "Whole-graph" layout (no selection narrowing) wants its
        // result anchored to the visible viewport so the user can
        // see the reflow. "Selection" layout must NOT pin to
        // viewport — moving a selected subset into view while their
        // external connections stay put would produce 3000-px wires
        // across the canvas. The selection case keeps the existing
        // "anchor at the target's current top-left" behaviour.
        let layout_everything = self.selection.group.is_none() && self.selection.nodes.is_empty();

        self.push_undo("Auto Layout");

        // Per-unit bounding-box size (width × height). For a
        // subgraph, this is the bounding box of every member node.
        let sizes: Vec<egui::Vec2> = target_units.iter().map(|u| u.bounding_size(self)).collect();

        // Topological depth → column index for each unit.
        let depths = self.compute_layout_depths(&target_units);
        let mut columns: std::collections::BTreeMap<u32, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (idx, unit) in target_units.iter().enumerate() {
            let d = depths.get(&unit.representative_id()).copied().unwrap_or(0);
            columns.entry(d).or_default().push(idx);
        }

        // Pre-compute incoming-edge map for barycentric sort.
        // edges_from[u] = set of unit indices feeding into u (within
        // the target set; external feeds are ignored).
        let edges_from = self.compute_incoming_unit_edges(&target_units);

        // Origin selection. Whole-graph: pin to the visible canvas
        // top-left (translated into world space via canvas_offset)
        // so the result lands on screen. Selection: stay at the
        // current bounding-rect top-left to avoid yanking nodes
        // away from external connections.
        let origin = if layout_everything && self.canvas.rect_last.is_positive() {
            // canvas_rect_last is in screen space; node positions
            // are world space. World = screen - canvas_offset. A
            // 40 px screen margin keeps the layout off the very
            // edge of the canvas where it'd butt against the
            // palette / scrollbar.
            const VIEWPORT_MARGIN: f32 = 40.0;
            egui::pos2(
                self.canvas.rect_last.left() + VIEWPORT_MARGIN - self.canvas.offset.x,
                self.canvas.rect_last.top() + VIEWPORT_MARGIN - self.canvas.offset.y,
            )
        } else {
            target_units
                .iter()
                .map(|u| u.current_top_left(self))
                .reduce(|acc, p| egui::pos2(acc.x.min(p.x), acc.y.min(p.y)))
                .unwrap_or(egui::pos2(80.0, 80.0))
        };

        const H_GAP: f32 = 80.0;
        const V_GAP: f32 = 40.0;

        // Process columns left-to-right so each column's barycentric
        // sort can read the already-placed Y of its predecessors.
        let mut placed_top_y: std::collections::HashMap<usize, f32> =
            std::collections::HashMap::new();
        let mut col_x = origin.x;
        let mut translations: Vec<(usize, egui::Vec2)> = Vec::new();
        for (_depth, indices) in columns.iter() {
            let mut indices = indices.clone();
            // Barycentric sort: each unit's key = mean Y of its
            // already-placed sources (which sit in earlier columns,
            // already in `placed_top_y`). A unit with no in-target
            // sources falls back to its current Y so the user's
            // manual ordering is preserved for unconnected units.
            indices.sort_by(|&a, &b| {
                let ka =
                    barycentric_key(a, &edges_from, &sizes, &placed_top_y, &target_units, self);
                let kb =
                    barycentric_key(b, &edges_from, &sizes, &placed_top_y, &target_units, self);
                ka.total_cmp(&kb)
            });

            // Stack vertically using each unit's actual height so
            // tall units don't overlap their neighbours below.
            let col_w = indices.iter().map(|&i| sizes[i].x).fold(0.0_f32, f32::max);
            let mut y = origin.y;
            for &i in &indices {
                let target_pos = egui::pos2(col_x, y);
                let current = target_units[i].current_top_left(self);
                translations.push((i, target_pos - current));
                placed_top_y.insert(i, y);
                y += sizes[i].y + V_GAP;
            }
            col_x += col_w + H_GAP;
        }

        // Apply all translations in one pass at the end so reads of
        // current positions (during sort / barycentric) see the
        // pre-layout state, not a half-moved graph.
        for (i, delta) in translations {
            target_units[i].translate(self, delta);
        }

        self.project.is_dirty = true;
        self.dialog.status_message = Some("Auto Layout applied.".to_string());
    }

    /// Map every unit index to the set of unit indices feeding INTO
    /// it. Used for barycentric ordering. Connections from outside
    /// the target set are ignored — those edges hang off the layout
    /// and don't influence in-set placement.
    pub(crate) fn compute_incoming_unit_edges(&self, units: &[LayoutUnit]) -> Vec<Vec<usize>> {
        let mut node_to_unit: std::collections::HashMap<NodeId, usize> =
            std::collections::HashMap::new();
        for (idx, u) in units.iter().enumerate() {
            for nid in u.member_ids() {
                node_to_unit.insert(nid, idx);
            }
        }
        let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); units.len()];
        for conn in self.graph.connections() {
            let Some(&to_idx) = node_to_unit.get(&conn.to.node_id) else {
                continue;
            };
            let Some(&from_idx) = node_to_unit.get(&conn.from.node_id) else {
                continue;
            };
            if from_idx == to_idx {
                continue;
            }
            if !incoming[to_idx].contains(&from_idx) {
                incoming[to_idx].push(from_idx);
            }
        }
        incoming
    }

    /// Build the list of units to lay out — one entry per regular
    /// node, or one entry per subgraph (member ids bundled together).
    /// Honours selection priority: group-only → group, otherwise
    /// node selection, otherwise everything visible at top level.
    pub(crate) fn collect_layout_units(&self) -> Vec<LayoutUnit> {
        // 0. Inside a subgraph view, Auto Layout only touches that
        //    subgraph's members. Without this, triggering Auto Layout
        //    while editing a subgraph re-laid out the *outer* graph
        //    instead — leaving the subgraph's contents unchanged and
        //    silently shuffling everything outside.
        if let Some(CanvasView::SubGraph(gid)) = self.canvas.tabs.get(self.canvas.active_tab) {
            if let Some(group) = self.groups.get(gid) {
                // If a subset is explicitly selected inside the subgraph,
                // honour that selection. Otherwise lay out every member.
                let candidates: std::collections::HashSet<NodeId> =
                    group.member_ids.iter().copied().collect();
                let mut units: Vec<LayoutUnit> = Vec::new();
                if !self.selection.nodes.is_empty() {
                    for &nid in &self.selection.nodes {
                        if candidates.contains(&nid) {
                            units.push(LayoutUnit::Node(nid));
                        }
                    }
                } else {
                    for nid in &group.member_ids {
                        units.push(LayoutUnit::Node(*nid));
                    }
                }
                return units;
            }
        }

        // 1. Subgraph alone is selected → just that group.
        if let Some(gid) = self.selection.group {
            if let Some(group) = self.groups.get(&gid) {
                if group.is_subgraph {
                    return vec![LayoutUnit::Subgraph {
                        members: group.member_ids.iter().copied().collect(),
                    }];
                }
            }
        }

        // 2. Individual node selection.
        if !self.selection.nodes.is_empty() {
            // For nodes that are members of collapsed subgraphs,
            // collapse to the subgraph unit. Other nodes go in
            // individually.
            let mut units: Vec<LayoutUnit> = Vec::new();
            let mut emitted_groups: std::collections::HashSet<u64> =
                std::collections::HashSet::new();
            for &nid in &self.selection.nodes {
                if let Some(gid) = self.node_to_group.get(&nid).copied() {
                    if let Some(group) = self.groups.get(&gid) {
                        if group.is_subgraph && group.collapsed {
                            if emitted_groups.insert(gid) {
                                units.push(LayoutUnit::Subgraph {
                                    members: group.member_ids.iter().copied().collect(),
                                });
                            }
                            continue;
                        }
                    }
                }
                units.push(LayoutUnit::Node(nid));
            }
            return units;
        }

        // 3. Everything visible at top level. Members of collapsed
        // subgraphs are bundled into their group; everyone else is
        // a standalone node.
        let mut units: Vec<LayoutUnit> = Vec::new();
        let mut handled: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        for (gid, group) in &self.groups {
            if group.is_subgraph && group.collapsed {
                let members: Vec<NodeId> = group.member_ids.iter().copied().collect();
                for m in &members {
                    handled.insert(*m);
                }
                let _ = gid;
                units.push(LayoutUnit::Subgraph { members });
            }
        }
        for nid in self.graph.nodes().keys() {
            if handled.contains(nid) {
                continue;
            }
            units.push(LayoutUnit::Node(*nid));
        }
        units
    }

    /// Topological depth per unit, keyed by the unit's
    /// representative NodeId. Edges from sources outside the target
    /// set are ignored — those count as "external feeds" that don't
    /// influence in-set depth, so a subset can be re-laid out
    /// without dragging the rest of the graph along.
    pub(crate) fn compute_layout_depths(
        &self,
        units: &[LayoutUnit],
    ) -> std::collections::HashMap<NodeId, u32> {
        // Map every member NodeId to the unit's representative.
        let mut node_to_rep: std::collections::HashMap<NodeId, NodeId> =
            std::collections::HashMap::new();
        for unit in units {
            let rep = unit.representative_id();
            for nid in unit.member_ids() {
                node_to_rep.insert(nid, rep);
            }
        }

        // Iterate in graph topo order so a unit's dependencies are
        // resolved before it.
        let topo = self.graph.topological_sort().unwrap_or_default();

        let mut depths: std::collections::HashMap<NodeId, u32> = std::collections::HashMap::new();
        for nid in topo {
            let Some(rep) = node_to_rep.get(&nid).copied() else {
                continue;
            };
            // Look at every connection landing on this node.
            let mut max_dep: Option<u32> = None;
            for conn in self.graph.connections() {
                if conn.to.node_id != nid {
                    continue;
                }
                let Some(src_rep) = node_to_rep.get(&conn.from.node_id).copied() else {
                    continue; // external feed — doesn't influence depth
                };
                if src_rep == rep {
                    continue; // intra-unit connection (e.g. inside a subgraph)
                }
                let from_depth = depths.get(&src_rep).copied().unwrap_or(0);
                let candidate = from_depth + 1;
                max_dep = Some(max_dep.map_or(candidate, |m| m.max(candidate)));
            }
            // Update the rep's depth to the max over all member nodes.
            let new_dep = max_dep.unwrap_or(0);
            let entry = depths.entry(rep).or_insert(0);
            if new_dep > *entry {
                *entry = new_dep;
            }
        }
        depths
    }

    /// Screen-space centre of a node's named output port. Mirrors
    /// the layout the node-rendering pass uses (right edge of the
    /// node rect, vertically stacked at PORT_Y_BASE + i*PORT_Y_STEP),
    /// so a wire originating here lines up exactly with the painted
    /// port handle. Used by the disconnect-by-input gesture to
    /// re-anchor the in-flight wire at the source's output.
    pub(crate) fn output_port_screen_pos(
        &self,
        node_id: NodeId,
        port_name: &str,
        offset: egui::Vec2,
    ) -> Option<egui::Pos2> {
        let node = self.graph.get_node(node_id)?;
        let visual = self.node_visuals.get(&node_id)?;
        let port_index = node.outputs.iter().position(|p| p.name == port_name)?;
        let node_rect = egui::Rect::from_min_size(
            egui::pos2(visual.position.x + offset.x, visual.position.y + offset.y),
            visual.size,
        );
        Some(node_port_pos(
            &node.node_type,
            node_rect,
            PortPlacement::Right,
            port_index,
        ))
    }

    /// Count input ports across `ids` that have no incoming
    /// connection. Used to enable / disable the Auto Wire menu item:
    /// nothing to do when every input is already wired.
    pub(crate) fn count_unwired_inputs(&self, ids: &[NodeId]) -> usize {
        let mut total = 0;
        for &nid in ids {
            let Some(node) = self.graph.get_node(nid) else {
                continue;
            };
            for input_port in &node.inputs {
                let wired = self
                    .graph
                    .connections()
                    .iter()
                    .any(|c| c.to.node_id == nid && c.to.port_name == input_port.name);
                if !wired {
                    total += 1;
                }
            }
        }
        total
    }

    /// Auto-wire the unwired inputs of every node in `ids`.
    ///
    /// For each unwired input on a target node, candidate sources
    /// must (a) have their right edge to the left of the target's
    /// left edge, (b) sit within `AUTO_WIRE_MAX_DX` horizontally
    /// and `AUTO_WIRE_MAX_DY` vertically of the target. Within that
    /// bounded window, the closest candidate by Euclidean distance
    /// (right-edge midpoint to left-edge midpoint) wins; vertical
    /// and horizontal separation cost the same.
    ///
    /// The hard distance bound serves two purposes: it keeps the
    /// search O(N) over the candidate window rather than O(N) over
    /// the whole graph for large maps, and it stops the heuristic
    /// from quietly creating very long wires when the only matching
    /// source happens to live across the canvas.
    ///
    /// Multi-select: targets are processed left-to-right so a later
    /// target can pick up a connection an earlier target just made
    /// — useful when the user selected a chain of nodes and asked
    /// to wire the whole thing.
    ///
    /// Type compatibility delegates to `GraphEngine::connect`; the
    /// helper just attempts each candidate in distance order and
    /// keeps the first that connects successfully.
    ///
    /// Returns the count of new connections — 0 means "nothing
    /// landed" (either the inputs were all wired, no compatible
    /// source was within range, or none of the in-range candidates
    /// could connect).
    pub(crate) fn auto_wire_nodes(&mut self, ids: &[NodeId]) -> usize {
        // Hard window for candidate sources, in canvas pixels. Wide
        // enough to cover most layouts that span more than one
        // viewport without sweeping the whole canvas.
        const AUTO_WIRE_MAX_DX: f32 = 1600.0;
        const AUTO_WIRE_MAX_DY: f32 = 1000.0;

        // Process targets left-to-right so later wiring can chain
        // off earlier wiring within the same selection.
        let mut sorted: Vec<NodeId> = ids.to_vec();
        sorted.sort_by(|a, b| {
            let ax = self
                .node_visuals
                .get(a)
                .map(|v| v.position.x)
                .unwrap_or(0.0);
            let bx = self
                .node_visuals
                .get(b)
                .map(|v| v.position.x)
                .unwrap_or(0.0);
            ax.total_cmp(&bx)
        });

        let mut connections_made = 0usize;

        for target_id in sorted {
            let Some(target_visual) = self.node_visuals.get(&target_id).cloned() else {
                continue;
            };
            let Some(target_node) = self.graph.get_node(target_id).cloned() else {
                continue;
            };
            // Anchor for distance ranking: the midpoint of the
            // target's left edge, where input ports cluster.
            let target_left = target_visual.position.x;
            let target_anchor = egui::pos2(
                target_left,
                target_visual.position.y + target_visual.size.y * 0.5,
            );

            for input_port in &target_node.inputs {
                // Skip already-wired inputs.
                let already_wired = self
                    .graph
                    .connections()
                    .iter()
                    .any(|c| c.to.node_id == target_id && c.to.port_name == input_port.name);
                if already_wired {
                    continue;
                }

                // Outputs of every node sitting to the left and
                // within the search window are candidates. The two
                // axis-aligned bounds short-circuit before we compute
                // a square root, so the cost stays linear in nodes
                // that are roughly anywhere near the target rather
                // than every node in the graph. Within the window,
                // rank by Euclidean distance from the candidate's
                // right-edge midpoint (where outputs cluster) to
                // the target's left-edge midpoint. Type
                // compatibility is left to graph.connect() so the
                // auto-wire heuristic can't drift from canonical
                // rules.
                let mut candidates: Vec<(NodeId, String, f32)> = Vec::new();
                for (other_id, other_node) in self.graph.nodes() {
                    if *other_id == target_id {
                        continue;
                    }
                    let Some(other_visual) = self.node_visuals.get(other_id) else {
                        continue;
                    };
                    let other_right = other_visual.position.x + other_visual.size.x;
                    if other_right > target_left {
                        continue;
                    }
                    let dx = target_left - other_right;
                    if dx > AUTO_WIRE_MAX_DX {
                        continue;
                    }
                    let other_anchor = egui::pos2(
                        other_right,
                        other_visual.position.y + other_visual.size.y * 0.5,
                    );
                    let dy = (target_anchor.y - other_anchor.y).abs();
                    if dy > AUTO_WIRE_MAX_DY {
                        continue;
                    }
                    let dist = (target_anchor - other_anchor).length();
                    for output_port in &other_node.outputs {
                        candidates.push((*other_id, output_port.name.clone(), dist));
                    }
                }
                candidates.sort_by(|a, b| a.2.total_cmp(&b.2));

                // First candidate that actually connects wins.
                // graph.connect() validates port-kind compatibility
                // and rejects anything that would create a cycle.
                for (src_id, src_port, _) in candidates {
                    let from = PortId {
                        node_id: src_id,
                        port_name: src_port,
                    };
                    let to = PortId {
                        node_id: target_id,
                        port_name: input_port.name.clone(),
                    };
                    if self.graph.connect(from, to).is_ok() {
                        connections_made += 1;
                        break;
                    }
                }
            }
        }

        if connections_made > 0 {
            self.project.is_dirty = true;
        }
        connections_made
    }

    /// Create a new empty group and return its id. The caller is
    /// expected to push undo BEFORE this if it represents a discrete
    /// user action; bulk operations (e.g. "create group from
    /// selection") push once at the call site.
    pub(crate) fn create_group(&mut self, label: impl Into<String>) -> u64 {
        let id = self.next_group_id;
        self.next_group_id += 1;
        let color_idx = (id as u8) % (GROUP_PALETTE.len() as u8);
        self.groups.insert(
            id,
            GroupRuntime {
                label: label.into(),
                member_ids: std::collections::HashSet::new(),
                color_idx,
                collapsed: false,
                is_subgraph: false,
                subgraph_inputs: Vec::new(),
                subgraph_outputs: Vec::new(),
                macro_params: Vec::new(),
            },
        );
        self.project.is_dirty = true;
        id
    }

    /// Add a node to a group. Removes it from any previous group first
    /// (a node can only live in one group at a time — same as folder
    /// membership in a filesystem). Caller is responsible for the
    /// `push_undo` if the move should be undoable on its own; the
    /// helper itself doesn't push so callers that perform bulk moves
    /// ("group selection of N nodes") only push once.
    pub(crate) fn add_node_to_group(&mut self, node_id: NodeId, group_id: u64) {
        if let Some(prev) = self.node_to_group.get(&node_id).copied() {
            if prev == group_id {
                return;
            }
            if let Some(g) = self.groups.get_mut(&prev) {
                g.member_ids.remove(&node_id);
            }
        }
        if let Some(g) = self.groups.get_mut(&group_id) {
            g.member_ids.insert(node_id);
            self.node_to_group.insert(node_id, group_id);
            self.project.is_dirty = true;
        }
    }

    /// Remove a node from its group (if any). If that empties the
    /// group, the group is deleted to avoid orphaned empty rectangles
    /// piling up. Same caller-pushes-undo contract as
    /// `add_node_to_group`.
    pub(crate) fn remove_node_from_group(&mut self, node_id: NodeId) {
        let Some(group_id) = self.node_to_group.remove(&node_id) else {
            return;
        };
        let mut delete = false;
        if let Some(g) = self.groups.get_mut(&group_id) {
            g.member_ids.remove(&node_id);
            delete = g.member_ids.is_empty();
        }
        if delete {
            self.groups.remove(&group_id);
        }
        self.project.is_dirty = true;
    }

    /// Dissolve a group entirely (members keep their positions, just
    /// lose group membership). Caller-pushes-undo as above.
    pub(crate) fn dissolve_group(&mut self, group_id: u64) {
        let Some(g) = self.groups.remove(&group_id) else {
            return;
        };
        for nid in &g.member_ids {
            self.node_to_group.remove(nid);
        }
        self.project.is_dirty = true;
    }
    /// Layout-only computation of every collapsed subgraph's rect and
    /// the screen-space position of each of its external port handles.
    /// Called BEFORE wire rendering so the wire pass can reroute
    /// hidden inner endpoints through the visible external port.
    /// Cheap — no painting, no allocation beyond the result maps.
    pub(crate) fn collapsed_subgraph_layout(
        &self,
        offset: egui::Vec2,
    ) -> (
        HashMap<u64, egui::Rect>,
        HashMap<(NodeId, String), egui::Pos2>,
    ) {
        let mut rects = HashMap::new();
        let mut handles: HashMap<(NodeId, String), egui::Pos2> = HashMap::new();
        if matches!(self.current_view(), CanvasView::SubGraph(_)) {
            return (rects, handles);
        }
        let block_w = 180.0_f32;
        let header_h = 22.0_f32;
        let row_h = 18.0_f32;
        for (gid, group) in &self.groups {
            if !(group.is_subgraph && group.collapsed) {
                continue;
            }
            let mut cx = 0.0_f32;
            let mut cy = 0.0_f32;
            let mut n = 0_f32;
            for nid in &group.member_ids {
                if let Some(v) = self.node_visuals.get(nid) {
                    cx += v.position.x + v.size.x * 0.5 + offset.x;
                    cy += v.position.y + v.size.y * 0.5 + offset.y;
                    n += 1.0;
                }
            }
            let centre = if n > 0.0 {
                egui::pos2(cx / n, cy / n)
            } else {
                egui::pos2(300.0, 200.0)
            };
            let rows = group
                .subgraph_inputs
                .len()
                .max(group.subgraph_outputs.len());
            let block_h = header_h + (rows.max(1) as f32) * row_h + 10.0;
            let rect = egui::Rect::from_min_size(
                egui::pos2(centre.x - block_w * 0.5, centre.y - block_h * 0.5),
                egui::vec2(block_w, block_h),
            );
            for (i, port) in group.subgraph_inputs.iter().enumerate() {
                let y = rect.top() + header_h + 8.0 + i as f32 * row_h;
                let p = egui::pos2(rect.left(), y);
                if let Some((nid, pname)) = &port.binding {
                    handles.insert((*nid, pname.clone()), p);
                }
            }
            for (i, port) in group.subgraph_outputs.iter().enumerate() {
                let y = rect.top() + header_h + 8.0 + i as f32 * row_h;
                let p = egui::pos2(rect.right(), y);
                if let Some((nid, pname)) = &port.binding {
                    handles.insert((*nid, pname.clone()), p);
                }
            }
            rects.insert(*gid, rect);
        }
        (rects, handles)
    }

    /// Returns `(per-group rect, bound-inner-port → external-handle-pos)`
    /// for every collapsed subgraph drawn this frame. The handle map
    /// feeds two things: visual rerouting of wires whose endpoints are
    /// hidden inner nodes, and (future) wire creation at subgraph
    /// external ports.
    pub(crate) fn draw_collapsed_subgraphs(
        &mut self,
        ui: &mut egui::Ui,
        offset: egui::Vec2,
    ) -> CollapsedSubgraphsDraw {
        // Reset the cached collapsed-block rects every frame; we
        // refill below as each block is drawn so the props-popup
        // hit-test sees current positions.
        self.collapsed_subgraph_rects.clear();
        let mut rects = HashMap::new();
        // Bound inner port → external handle position. Used by the
        // wire-render pass below to reroute connections from hidden
        // inner endpoints onto the visible external port.
        let mut handle_positions: HashMap<(NodeId, String), egui::Pos2> = HashMap::new();
        let mut conn_start: Option<DragConnection> = None;
        let mut conn_end: Option<(NodeId, String)> = None;
        if matches!(self.current_view(), CanvasView::SubGraph(_)) {
            return (rects, handle_positions, conn_start, conn_end);
        }
        let painter = ui.painter().clone();
        let block_w = 180.0_f32;
        let header_h = 22.0_f32;
        let row_h = 18.0_f32;
        for (gid, group) in &self.groups {
            if !(group.is_subgraph && group.collapsed) {
                continue;
            }
            // Centroid of members in canvas-screen space.
            let mut cx = 0.0_f32;
            let mut cy = 0.0_f32;
            let mut n = 0_f32;
            for nid in &group.member_ids {
                if let Some(v) = self.node_visuals.get(nid) {
                    cx += v.position.x + v.size.x * 0.5 + offset.x;
                    cy += v.position.y + v.size.y * 0.5 + offset.y;
                    n += 1.0;
                }
            }
            let centre = if n > 0.0 {
                egui::pos2(cx / n, cy / n)
            } else {
                egui::pos2(300.0, 200.0)
            };
            let rows = group
                .subgraph_inputs
                .len()
                .max(group.subgraph_outputs.len());
            let block_h = header_h + (rows.max(1) as f32) * row_h + 10.0;
            let rect = egui::Rect::from_min_size(
                egui::pos2(centre.x - block_w * 0.5, centre.y - block_h * 0.5),
                egui::vec2(block_w, block_h),
            );
            let tint = group_color(group.color_idx);
            // Body — opaque so it reads as a node, not a translucent
            // backdrop.
            painter.rect_filled(
                rect,
                6.0,
                egui::Color32::from_rgba_unmultiplied(tint.r(), tint.g(), tint.b(), 230),
            );
            // Header band.
            let header_rect =
                egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_h));
            painter.rect_filled(
                header_rect,
                egui::CornerRadius {
                    nw: 6,
                    ne: 6,
                    sw: 0,
                    se: 0,
                },
                tint.gamma_multiply(0.7),
            );
            let label_text = if group.label.is_empty() {
                format!("SubGraph {gid}")
            } else {
                group.label.clone()
            };
            painter.text(
                egui::pos2(rect.left() + 10.0, header_rect.center().y),
                egui::Align2::LEFT_CENTER,
                label_text,
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            );
            // Border last so the header doesn't cover it (matches
            // node + group rendering).
            let is_selected = self.selection.group == Some(*gid);
            let stroke = if is_selected {
                egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 160, 255))
            } else {
                egui::Stroke::new(1.5, egui::Color32::BLACK)
            };
            painter.rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Outside);
            // Port handles + labels. Inputs on the left, outputs on
            // the right. The actual wiring of these handles to the
            // surrounding graph lands in the next phase along with
            // subgraph eval.
            let hit_size = egui::vec2(14.0, 14.0);
            for (i, port) in group.subgraph_inputs.iter().enumerate() {
                let y = rect.top() + header_h + 8.0 + i as f32 * row_h;
                let p = egui::pos2(rect.left(), y);
                let port_resp = ui.interact(
                    egui::Rect::from_center_size(p, hit_size),
                    egui::Id::new(("subgraph_port_in", *gid, i as u32)),
                    egui::Sense::click_and_drag(),
                );
                draw_port_circle(
                    &painter,
                    p,
                    4.0,
                    tokens::PORT_HEIGHTMAP,
                    port_resp.hovered(),
                );
                painter.text(
                    egui::pos2(p.x + 8.0, p.y),
                    egui::Align2::LEFT_CENTER,
                    &port.label,
                    egui::FontId::proportional(11.0),
                    egui::Color32::WHITE,
                );
                if let Some((nid, pname)) = &port.binding {
                    handle_positions.insert((*nid, pname.clone()), p);
                    if self.canvas.drag_connection.is_some()
                        && ui.input(|inp| inp.pointer.primary_released())
                        && port_resp.contains_pointer()
                    {
                        conn_end = Some((*nid, pname.clone()));
                    }
                }
            }
            for (i, port) in group.subgraph_outputs.iter().enumerate() {
                let y = rect.top() + header_h + 8.0 + i as f32 * row_h;
                let p = egui::pos2(rect.right(), y);
                let port_resp = ui.interact(
                    egui::Rect::from_center_size(p, hit_size),
                    egui::Id::new(("subgraph_port_out", *gid, i as u32)),
                    egui::Sense::click_and_drag(),
                );
                draw_port_circle(
                    &painter,
                    p,
                    4.0,
                    tokens::PORT_HEIGHTMAP,
                    port_resp.hovered(),
                );
                painter.text(
                    egui::pos2(p.x - 8.0, p.y),
                    egui::Align2::RIGHT_CENTER,
                    &port.label,
                    egui::FontId::proportional(11.0),
                    egui::Color32::WHITE,
                );
                if let Some((nid, pname)) = &port.binding {
                    handle_positions.insert((*nid, pname.clone()), p);
                    if port_resp.drag_started_by(egui::PointerButton::Primary)
                        && self.canvas.drag_connection.is_none()
                    {
                        conn_start = Some(DragConnection {
                            from_node: *nid,
                            from_port: pname.clone(),
                            from_pos: p,
                        });
                    }
                }
            }
            rects.insert(*gid, rect);
            self.collapsed_subgraph_rects.insert(*gid, rect);
        }
        (rects, handle_positions, conn_start, conn_end)
    }

    /// Render the canvas tab bar across the top of the canvas area.
    /// Main is always present at index 0 and never moves or closes;
    /// other tabs (SubGraph, Sculpt) carry an `✕` close button and
    /// can be reordered by dragging horizontally.
    pub(crate) fn draw_canvas_tabs(&mut self, ui: &mut egui::Ui) {
        let mut switch_to: Option<usize> = None;
        let mut close: Option<usize> = None;
        let mut tab_rects: Vec<(usize, egui::Rect)> = Vec::new();
        let mut drag_release_index: Option<usize> = None;

        // Visual constants for the tab strip. Tabs are flat rects
        // with rounded top corners, joined to a baseline separator
        // that the active tab visually breaks. Inactive tabs sit a
        // pixel lower than the active one for that "depressed"
        // bottom-tab feel.
        let tab_height = 26.0_f32;
        let min_tab_width = 110.0_f32;
        let max_tab_width = 220.0_f32;
        let strip_h = tab_height + 1.0;
        let (strip_rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), strip_h),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(strip_rect);

        let neutral_active = tokens::TAB_BG_ACTIVE;
        let neutral_inactive = tokens::TAB_BG_INACTIVE;
        let neutral_hover = tokens::TAB_BG_HOVER;
        let baseline = tokens::TAB_BASELINE;

        // Pick the tab's tinted base colour. SubGraph tabs use their
        // group's palette colour so two SubGraphs with different
        // colours look visibly different in the tab strip. Main and
        // any tab whose target is missing fall back to neutral.
        let tab_tint =
            |view: &CanvasView, groups: &HashMap<u64, GroupRuntime>| -> Option<egui::Color32> {
                match view {
                    CanvasView::Main => None,
                    CanvasView::SubGraph(gid) => groups.get(gid).map(|g| group_color(g.color_idx)),
                }
            };

        // Baseline along the bottom of the strip.
        painter.line_segment(
            [
                egui::pos2(strip_rect.left(), strip_rect.bottom() - 0.5),
                egui::pos2(strip_rect.right(), strip_rect.bottom() - 0.5),
            ],
            egui::Stroke::new(1.0, baseline),
        );

        let pointer = ui.ctx().pointer_latest_pos();
        let mut x = strip_rect.left() + 2.0;
        for (i, view) in self.canvas.tabs.iter().enumerate() {
            let label = match view {
                CanvasView::Main => "Main".to_string(),
                CanvasView::SubGraph(gid) => self
                    .groups
                    .get(gid)
                    .map(|g| {
                        if g.label.is_empty() {
                            format!("SubGraph {gid}")
                        } else {
                            g.label.clone()
                        }
                    })
                    .unwrap_or_else(|| format!("SubGraph {gid}")),
            };
            let is_active = i == self.canvas.active_tab;
            let closable = i != 0;

            // Lay out the tab text + (optional) close button and
            // figure out the tab's width.
            let font = egui::FontId::proportional(13.0);
            let label_galley = painter.layout_no_wrap(
                label.clone(),
                font.clone(),
                if is_active {
                    tokens::TAB_LABEL_ACTIVE
                } else {
                    tokens::TAB_LABEL_DIM
                },
            );
            let close_w = if closable { 18.0 } else { 0.0 };
            let raw_w = label_galley.size().x + 24.0 + close_w;
            let tab_w = raw_w.clamp(min_tab_width, max_tab_width);

            let tab_rect = egui::Rect::from_min_size(
                egui::pos2(x, strip_rect.top()),
                egui::vec2(tab_w, tab_height),
            );
            tab_rects.push((i, tab_rect));

            let resp = ui.interact(
                tab_rect,
                egui::Id::new(("canvas_tab", i)),
                egui::Sense::click_and_drag(),
            );

            // Body fill. Tinted tabs use their group/node colour for
            // identity at a glance; Main keeps the neutral palette.
            // Active variants are brighter than inactive so the
            // currently-selected tab pops without losing its tint.
            let bg = match tab_tint(view, &self.groups) {
                Some(tint) => {
                    if is_active {
                        // Mix toward active_bg so the active tab still
                        // visually merges with the canvas below it.
                        blend(tint, neutral_active, 0.55)
                    } else if resp.hovered() {
                        blend(tint, neutral_hover, 0.55)
                    } else {
                        blend(tint, neutral_inactive, 0.65)
                    }
                }
                None => {
                    if is_active {
                        neutral_active
                    } else if resp.hovered() {
                        neutral_hover
                    } else {
                        neutral_inactive
                    }
                }
            };
            painter.rect_filled(
                tab_rect,
                egui::CornerRadius {
                    nw: 6,
                    ne: 6,
                    sw: 0,
                    se: 0,
                },
                bg,
            );
            // Side + top stroke. Skip the bottom side for the active
            // tab so it bleeds into the content below.
            let stroke = egui::Stroke::new(1.0, baseline);
            // Top
            painter.line_segment(
                [
                    egui::pos2(tab_rect.left() + 6.0, tab_rect.top()),
                    egui::pos2(tab_rect.right() - 6.0, tab_rect.top()),
                ],
                stroke,
            );
            // Top-left curve approximation
            painter.line_segment(
                [
                    egui::pos2(tab_rect.left(), tab_rect.top() + 6.0),
                    egui::pos2(tab_rect.left(), tab_rect.bottom()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(tab_rect.right(), tab_rect.top() + 6.0),
                    egui::pos2(tab_rect.right(), tab_rect.bottom()),
                ],
                stroke,
            );
            // Cover the baseline under the active tab so it appears
            // joined to the content below. Use the active tab's own
            // bg colour so the join is invisible.
            if is_active {
                painter.line_segment(
                    [
                        egui::pos2(tab_rect.left() + 1.0, tab_rect.bottom() - 0.5),
                        egui::pos2(tab_rect.right() - 1.0, tab_rect.bottom() - 0.5),
                    ],
                    egui::Stroke::new(1.5, bg),
                );
            }

            // Truncate label if it would overflow the tab.
            let label_x = tab_rect.left() + 12.0;
            let label_y = tab_rect.center().y;
            let max_label_w = tab_rect.width() - 24.0 - close_w;
            if label_galley.size().x <= max_label_w {
                painter.galley(
                    egui::pos2(label_x, label_y - label_galley.size().y * 0.5),
                    label_galley,
                    egui::Color32::WHITE,
                );
            } else {
                // Render an ellipsis-truncated copy.
                let mut truncated = label.clone();
                while !truncated.is_empty() {
                    truncated.pop();
                    let test = format!("{truncated}…");
                    let galley =
                        painter.layout_no_wrap(test.clone(), font.clone(), egui::Color32::WHITE);
                    if galley.size().x <= max_label_w {
                        painter.galley(
                            egui::pos2(label_x, label_y - galley.size().y * 0.5),
                            galley,
                            egui::Color32::WHITE,
                        );
                        break;
                    }
                }
            }

            // Close button.
            if closable {
                let close_rect = egui::Rect::from_center_size(
                    egui::pos2(tab_rect.right() - 12.0, tab_rect.center().y),
                    egui::vec2(14.0, 14.0),
                );
                let close_resp = ui.interact(
                    close_rect,
                    egui::Id::new(("canvas_tab_close", i)),
                    egui::Sense::click(),
                );
                let close_color = if close_resp.hovered() {
                    tokens::SEVERITY_ERROR
                } else {
                    tokens::TAB_LABEL_DIM
                };
                let m = 4.0_f32;
                painter.line_segment(
                    [
                        egui::pos2(close_rect.left() + m, close_rect.top() + m),
                        egui::pos2(close_rect.right() - m, close_rect.bottom() - m),
                    ],
                    egui::Stroke::new(1.5, close_color),
                );
                painter.line_segment(
                    [
                        egui::pos2(close_rect.right() - m, close_rect.top() + m),
                        egui::pos2(close_rect.left() + m, close_rect.bottom() - m),
                    ],
                    egui::Stroke::new(1.5, close_color),
                );
                if close_resp.clicked() {
                    close = Some(i);
                }
            }

            // Tab interactions: click to switch, drag to reorder.
            if resp.clicked() {
                switch_to = Some(i);
            }
            if i != 0 && resp.drag_stopped_by(egui::PointerButton::Primary) {
                drag_release_index = Some(i);
            }

            x += tab_w;
            // Suppress unused warning when pointer is None.
            let _ = pointer;
        }

        if let Some(idx) = switch_to {
            self.set_active_tab(idx);
        }
        if let Some(idx) = close {
            self.close_tab(idx);
        }
        if let Some(from) = drag_release_index {
            if let Some(cursor) = ui.ctx().pointer_latest_pos() {
                let target = tab_rects
                    .iter()
                    .find(|(_, r)| r.contains(cursor))
                    .map(|(i, _)| *i)
                    .unwrap_or(from);
                let to = target.max(1);
                if to != from && to < self.canvas.tabs.len() {
                    let active_view = self.canvas.tabs.get(self.canvas.active_tab).cloned();
                    let item = self.canvas.tabs.remove(from);
                    self.canvas.tabs.insert(to, item);
                    if let Some(av) = active_view {
                        self.canvas.active_tab = self.canvas.tabs.iter().position(|v| v == &av).unwrap_or(0);
                    }
                }
            }
        }
    }

    /// Switch the active tab and remember the previously-active one
    /// for Ctrl+Tab back-and-forth. Use this everywhere the active
    /// tab changes — direct assignment to `active_tab` skips the
    /// last-active tracking.
    pub(crate) fn set_active_tab(&mut self, idx: usize) {
        if idx == self.canvas.active_tab || idx >= self.canvas.tabs.len() {
            return;
        }
        self.canvas.last_active_tab = self.canvas.active_tab;
        self.canvas.active_tab = idx;
    }

    /// Open a tab if not already open, then make it active. Returns
    /// the tab index. If a matching tab already exists, that one is
    /// reused — opening a SubGraph tab twice doesn't make two of
    /// them.
    pub(crate) fn open_or_activate_tab(&mut self, view: CanvasView) -> usize {
        if let Some(idx) = self.canvas.tabs.iter().position(|v| v == &view) {
            self.set_active_tab(idx);
            return idx;
        }
        self.canvas.tabs.push(view);
        let new_idx = self.canvas.tabs.len() - 1;
        self.set_active_tab(new_idx);
        new_idx
    }

    /// Close a tab by index. The Main tab (index 0) is never closed.
    /// If the closed tab was active, focus shifts to the previous
    /// tab (or Main if there's no previous).
    pub(crate) fn close_tab(&mut self, idx: usize) {
        if idx == 0 || idx >= self.canvas.tabs.len() {
            return;
        }
        self.canvas.tabs.remove(idx);
        if self.canvas.active_tab >= self.canvas.tabs.len() {
            self.canvas.active_tab = self.canvas.tabs.len() - 1;
        } else if self.canvas.active_tab > idx {
            self.canvas.active_tab -= 1;
        } else if self.canvas.active_tab == idx {
            // Closed the active one; pick the tab that was before it.
            self.canvas.active_tab = idx.saturating_sub(1);
        }
    }

    /// Drop tabs whose target no longer exists. Called whenever the
    /// graph or groups change so the tab bar can never display a
    /// reference to a deleted thing. The Main tab is preserved.
    pub(crate) fn prune_dangling_tabs(&mut self) {
        let valid_groups: std::collections::HashSet<u64> = self.groups.keys().copied().collect();
        let mut new_tabs: Vec<CanvasView> = Vec::with_capacity(self.canvas.tabs.len());
        let prev_active = self.canvas.tabs.get(self.canvas.active_tab).cloned();
        for tab in &self.canvas.tabs {
            let keep = match tab {
                CanvasView::Main => true,
                CanvasView::SubGraph(gid) => valid_groups.contains(gid),
            };
            if keep {
                new_tabs.push(tab.clone());
            }
        }
        if new_tabs.is_empty() {
            new_tabs.push(CanvasView::Main);
        }
        self.canvas.tabs = new_tabs;
        self.canvas.active_tab = match prev_active {
            Some(prev) => self.canvas.tabs.iter().position(|v| v == &prev).unwrap_or(0),
            None => 0,
        };
    }

    /// Returns the active tab's view.
    pub(crate) fn current_view(&self) -> CanvasView {
        self.canvas.tabs
            .get(self.canvas.active_tab)
            .cloned()
            .unwrap_or(CanvasView::Main)
    }

    /// Set of node ids that should NOT render this frame. Two cases:
    /// (1) members of a *collapsed* subgraph (replaced visually by
    /// the subgraph's compact block), (2) anything outside the
    /// current confined-edit scope.
    pub(crate) fn hidden_nodes_this_frame(&self) -> std::collections::HashSet<NodeId> {
        let mut hidden = std::collections::HashSet::new();
        if let CanvasView::SubGraph(scope) = self.current_view() {
            // Subgraph tab: hide every node that isn't a member of
            // the active scope.
            let visible: std::collections::HashSet<NodeId> = self
                .groups
                .get(&scope)
                .map(|g| g.member_ids.iter().copied().collect())
                .unwrap_or_default();
            for id in self.node_visuals.keys() {
                if !visible.contains(id) {
                    hidden.insert(*id);
                }
            }
        } else {
            // Whole-graph view: hide members of collapsed subgraphs.
            for g in self.groups.values() {
                if g.is_subgraph && g.collapsed {
                    for id in &g.member_ids {
                        hidden.insert(*id);
                    }
                }
            }
        }
        hidden
    }

    pub(crate) fn draw_node_graph(&mut self, ui: &mut egui::Ui) {
        // Tab bar — clean up first so we don't render tabs for
        // sub-graphs / nodes that no longer exist. The tab strip
        // draws its own baseline separator that the active tab
        // visually breaks, so no extra ui.separator() is needed.
        self.prune_dangling_tabs();
        self.draw_canvas_tabs(ui);

        // Pressing Escape on a non-Main tab returns to Main without
        // closing the tab — same speed-back affordance the old
        // confined-edit Esc had, but you don't lose the open tab.
        if self.canvas.active_tab != 0 && ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.set_active_tab(0);
        }
        // Ctrl/Cmd+W closes the current tab. Main is unclosable; the
        // shortcut is a no-op when Main is active.
        if ui
            .ctx()
            .input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(egui::Key::W))
            && self.canvas.active_tab != 0
        {
            self.close_tab(self.canvas.active_tab);
        }
        // Ctrl/Cmd+Tab swaps with the previously-active tab — the
        // standard "back to where I was" shortcut. Skipped when
        // there's only one tab (or `last_active_tab` is the same).
        if ui
            .ctx()
            .input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(egui::Key::Tab))
        {
            let target = self.canvas.last_active_tab;
            if target != self.canvas.active_tab && target < self.canvas.tabs.len() {
                self.set_active_tab(target);
            }
        }

        let available = ui.available_size();
        let (canvas_rect, response) =
            ui.allocate_exact_size(available, egui::Sense::click_and_drag());
        self.canvas.rect_last = canvas_rect;

        // Drain any layout that was deferred from the welcome panel
        // (template card click, File → New from Preset). The
        // viewport-pin path inside `auto_layout_selection` reads
        // `canvas_rect_last`, which is now fresh — so the layout
        // lands on screen instead of off-canvas.
        if self.canvas.pending_auto_layout_all {
            self.canvas.pending_auto_layout_all = false;
            self.auto_layout_selection();
        }

        let painter = ui.painter_at(canvas_rect);

        // Draw grid
        let grid_spacing = 30.0;
        let grid_color = ui
            .visuals()
            .widgets
            .noninteractive
            .bg_stroke
            .color
            .linear_multiply(0.2);

        let offset = self.canvas.offset;
        let grid_offset_x = offset.x % grid_spacing;
        let grid_offset_y = offset.y % grid_spacing;

        let mut x = canvas_rect.left() + grid_offset_x;
        while x <= canvas_rect.right() {
            painter.line_segment(
                [
                    egui::pos2(x, canvas_rect.top()),
                    egui::pos2(x, canvas_rect.bottom()),
                ],
                egui::Stroke::new(1.0, grid_color),
            );
            x += grid_spacing;
        }
        let mut y = canvas_rect.top() + grid_offset_y;
        while y <= canvas_rect.bottom() {
            painter.line_segment(
                [
                    egui::pos2(canvas_rect.left(), y),
                    egui::pos2(canvas_rect.right(), y),
                ],
                egui::Stroke::new(1.0, grid_color),
            );
            y += grid_spacing;
        }

        // Handle canvas panning with middle-click or right-click drag
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            self.canvas.offset += response.drag_delta();
        }

        // Click on empty space to deselect (suppress when palette drag
        // is in progress, and skip if the click landed on a group
        // header / body — handled separately below).
        let pointer = ui.ctx().pointer_latest_pos();
        let clicked_in_group = pointer.is_some_and(|p| {
            self.group_header_rects.values().any(|r| r.contains(p))
                || self.group_body_rects.values().any(|r| r.contains(p))
        });
        if response.clicked() && self.palette_drag.is_none() && !clicked_in_group {
            self.clear_selection();
        }

        // ── Marquee selection ──────────────────────────────────────────────
        // Primary-button drag on the empty canvas (i.e. not landing on
        // a node, group, or palette item) sweeps out a rectangle; on
        // release every node whose rect intersects the rectangle is
        // selected. Ctrl/Cmd makes the marquee additive (added to the
        // current selection rather than replacing it).
        if response.drag_started_by(egui::PointerButton::Primary)
            && self.palette_drag.is_none()
            && !clicked_in_group
        {
            if let Some(p) = pointer {
                self.canvas.marquee_start = Some(p);
            }
        }
        if let Some(anchor) = self.canvas.marquee_start {
            let cur = pointer.unwrap_or(anchor);
            let marquee = egui::Rect::from_two_pos(anchor, cur);
            painter.rect_filled(marquee, 2.0, tokens::NODE_BORDER_SEL.gamma_multiply(0.11));
            painter.rect_stroke(
                marquee,
                2.0,
                egui::Stroke::new(1.0, tokens::NODE_BORDER_SEL),
                egui::StrokeKind::Inside,
            );
            // On drag end, finalize the selection.
            if !ui
                .ctx()
                .input(|i| i.pointer.button_down(egui::PointerButton::Primary))
            {
                let additive = ui.ctx().input(|i| i.modifiers.ctrl || i.modifiers.command);
                let mut hits: Vec<NodeId> = Vec::new();
                // Subgraph membership set: nodes inside a is_subgraph group.
                // Used to prevent marquee from crossing the main/subgraph boundary.
                let subgraph_members: std::collections::HashSet<NodeId> = self
                    .groups
                    .values()
                    .filter(|g| g.is_subgraph)
                    .flat_map(|g| g.member_ids.iter().copied())
                    .collect();
                for (id, visual) in &self.node_visuals {
                    let in_scope = match self.current_view() {
                        CanvasView::Main => !subgraph_members.contains(id),
                        CanvasView::SubGraph(gid) => self
                            .groups
                            .get(&gid)
                            .is_some_and(|g| g.member_ids.contains(id)),
                    };
                    if !in_scope {
                        continue;
                    }
                    let r = egui::Rect::from_min_size(
                        egui::pos2(visual.position.x + offset.x, visual.position.y + offset.y),
                        visual.size,
                    );
                    if marquee.intersects(r) {
                        hits.push(*id);
                    }
                }

                // Also pick up subgraphs / groups whose visible rect
                // intersects the marquee. Collapsed subgraphs have
                // no member-node visuals to catch (their innards are
                // hidden behind the compact block) so without this
                // pass the marquee couldn't select them at all.
                // Expanded groups are caught here too — selecting a
                // group whose body the user dragged over reads as
                // "I want this whole chunk", regardless of whether
                // the marquee also clipped each individual member.
                let mut group_hits: Vec<u64> = Vec::new();
                // Collapsed subgraphs: cached rect IS the whole
                // visible footprint.
                for (gid, rect) in &self.collapsed_subgraph_rects {
                    if marquee.intersects(*rect) {
                        group_hits.push(*gid);
                    }
                }
                // Expanded groups: union of header + body rects from
                // the previous draw_groups frame.
                for gid in self.groups.keys() {
                    if self.collapsed_subgraph_rects.contains_key(gid) {
                        continue;
                    }
                    let header = self.group_header_rects.get(gid).copied();
                    let body = self.group_body_rects.get(gid).copied();
                    let group_rect = match (header, body) {
                        (Some(h), Some(b)) => Some(h.union(b)),
                        (Some(h), None) => Some(h),
                        (None, Some(b)) => Some(b),
                        (None, None) => None,
                    };
                    if let Some(r) = group_rect {
                        if marquee.intersects(r) {
                            group_hits.push(*gid);
                        }
                    }
                }

                if !additive {
                    self.selection.nodes.clear();
                    self.selection.node = None;
                }
                // Single-group-selection model: if the marquee hit
                // exactly one group, that's the active group; if it
                // hit several, leave selected_group cleared (the
                // properties popup can't disambiguate). Either way,
                // every hit group's members are added to the node
                // selection so subsequent operations (move, delete)
                // act on the whole chunk.
                self.selection.group = match group_hits.len() {
                    1 => Some(group_hits[0]),
                    _ => None,
                };
                for gid in &group_hits {
                    if let Some(group) = self.groups.get(gid) {
                        for id in &group.member_ids {
                            self.selection.nodes.insert(*id);
                            hits.push(*id);
                        }
                    }
                }
                for id in &hits {
                    self.selection.nodes.insert(*id);
                }
                if self.selection.node.is_none() {
                    self.selection.node = hits.first().copied();
                }
                self.canvas.marquee_start = None;
            }
        }

        // Draw group rectangles BEHIND connections + nodes so the
        // grouping reads as a backdrop, not a foreground decoration.
        self.draw_groups(&painter, offset);

        // Group hit-testing on the cached rects from the previous
        // draw_groups frame: clicking a group header selects the group;
        // clicking the body (only when the click didn't land on a
        // child node) also selects the group; dragging the header
        // moves every member.
        let group_hits: Vec<(u64, egui::Rect, egui::Rect)> = self
            .groups
            .keys()
            .filter_map(|gid| {
                let h = *self.group_header_rects.get(gid)?;
                let b = *self.group_body_rects.get(gid)?;
                Some((*gid, h, b))
            })
            .collect();
        for (gid, header_rect, body_rect) in group_hits {
            // Header is the priority hit zone — that's the title bar
            // the user grabs to drag. Body is a fallback for clicks on
            // empty space inside the group rect (we filter out clicks
            // that land on a child node by checking node rects later
            // in the node pass; here we treat any header/body click
            // as a group-claim).
            let header_resp = ui.interact(
                header_rect,
                egui::Id::new(("group_header", gid)),
                egui::Sense::click_and_drag(),
            );
            let body_resp = ui.interact(
                body_rect,
                egui::Id::new(("group_body", gid)),
                egui::Sense::click_and_drag(),
            );
            if header_resp.clicked() || body_resp.clicked() {
                self.select_group(gid);
                if let Some(p) = ui.ctx().pointer_latest_pos() {
                    self.dialog.pending_props_open = Some(PendingPropsOpen {
                        target: PropsTarget::Group(gid),
                        armed_at: Instant::now(),
                        armed_pos: p,
                    });
                }
            }
            // Drag either header or body to move every member node by
            // the same delta — analogous to dragging a folder in a
            // file manager.
            let drag_resp = if header_resp.dragged() {
                Some(&header_resp)
            } else if body_resp.dragged() {
                Some(&body_resp)
            } else {
                None
            };
            if let Some(r) = drag_resp {
                let delta = r.drag_delta();
                if let Some(g) = self.groups.get(&gid) {
                    let ids: Vec<NodeId> = g.member_ids.iter().copied().collect();
                    for id in ids {
                        if let Some(v) = self.node_visuals.get_mut(&id) {
                            v.position += delta;
                        }
                    }
                }
            }
            // Group-level context menu — delete with confirm.
            let is_sub = self
                .groups
                .get(&gid)
                .map(|g| g.is_subgraph)
                .unwrap_or(false);
            header_resp.context_menu(|ui| {
                let label = if is_sub {
                    "Delete subgraph"
                } else {
                    "Delete group…"
                };
                if ui.button(label).clicked() {
                    if is_sub {
                        self.delete_subgraph_with_contents(gid);
                    } else {
                        self.selection.pending_group_delete = Some(gid);
                    }
                    ui.close_menu();
                }
            });
        }

        // When the user is dragging a connection, compute which nodes
        // have at least one compatible input port so everything else
        // can be dimmed. The source node is always considered active.
        let drag_active_nodes: Option<std::collections::HashSet<NodeId>> =
            self.canvas.drag_connection.as_ref().and_then(|drag| {
                let drag_kind = self
                    .graph
                    .get_node(drag.from_node)?
                    .outputs
                    .iter()
                    .find(|p| p.name == drag.from_port)
                    .map(|p| p.kind)?;
                let mut active = std::collections::HashSet::new();
                active.insert(drag.from_node);
                for (nid, node) in self.graph.nodes() {
                    // IO boundary nodes accept any kind via the engine bypass.
                    let is_io = matches!(
                        node.node_type,
                        NodeType::SubgraphInput | NodeType::SubgraphOutput
                    );
                    if is_io
                        || node
                            .inputs
                            .iter()
                            .any(|p| drag_kind.compatible_with(p.kind))
                    {
                        active.insert(*nid);
                    }
                }
                Some(active)
            });

        // Draw connections + hit-test against the cursor for selection.
        // Compute the layout of every collapsed subgraph upfront so
        // wires can reroute through their external port handles when
        // an endpoint is on a hidden inner node that's bound to one.
        let (_collapsed_rects_pre, subgraph_handles) = self.collapsed_subgraph_layout(offset);
        let hidden_for_wires = self.hidden_nodes_this_frame();
        let connections_snapshot = self.graph.connections().to_vec();
        let mut wire_polylines: Vec<((PortId, PortId), Vec<egui::Pos2>)> = Vec::new();
        for conn in &connections_snapshot {
            // Pick the visible endpoint for each side. If the endpoint
            // is on a hidden inner node, look it up in the subgraph
            // handle map; if found, use the external port's handle
            // position. Skip the wire only when an endpoint is
            // genuinely invisible (hidden + not exposed by binding).
            let from_hidden = hidden_for_wires.contains(&conn.from.node_id);
            let to_hidden = hidden_for_wires.contains(&conn.to.node_id);
            let from_pos: Option<egui::Pos2> = if from_hidden {
                subgraph_handles
                    .get(&(conn.from.node_id, conn.from.port_name.clone()))
                    .copied()
            } else {
                self.node_visuals
                    .get(&conn.from.node_id)
                    .and_then(|visual| {
                        let node = self.graph.get_node(conn.from.node_id)?;
                        let out_idx = node
                            .outputs
                            .iter()
                            .position(|p| p.name == conn.from.port_name)?;
                        let node_rect = egui::Rect::from_min_size(
                            egui::pos2(visual.position.x + offset.x, visual.position.y + offset.y),
                            visual.size,
                        );
                        Some(node_port_pos(
                            &node.node_type,
                            node_rect,
                            PortPlacement::Right,
                            out_idx,
                        ))
                    })
            };
            // to_pos and to_placement computed together so control-point
            // axis can reflect which edge the input port sits on.
            let (to_pos, to_placement): (Option<egui::Pos2>, PortPlacement) = if to_hidden {
                (
                    subgraph_handles
                        .get(&(conn.to.node_id, conn.to.port_name.clone()))
                        .copied(),
                    PortPlacement::Left,
                )
            } else {
                let result = self.node_visuals.get(&conn.to.node_id).and_then(|visual| {
                    let node = self.graph.get_node(conn.to.node_id)?;
                    let port = node.inputs.iter().find(|p| p.name == conn.to.port_name)?;
                    let placement = PortPlacement::for_input(port.kind);
                    let side_idx = if matches!(placement, PortPlacement::Left) {
                        node.inputs
                            .iter()
                            .filter(|p| {
                                matches!(PortPlacement::for_input(p.kind), PortPlacement::Left)
                            })
                            .position(|p| p.name == conn.to.port_name)?
                    } else {
                        0
                    };
                    let node_rect = egui::Rect::from_min_size(
                        egui::pos2(visual.position.x + offset.x, visual.position.y + offset.y),
                        visual.size,
                    );
                    let pos = node_port_pos(&node.node_type, node_rect, placement, side_idx);
                    Some((pos, placement))
                });
                match result {
                    Some((pos, pl)) => (Some(pos), pl),
                    None => (None, PortPlacement::Left),
                }
            };
            let (Some(from_pos), Some(to_pos)) = (from_pos, to_pos) else {
                continue;
            };
            let to_axis = match to_placement {
                PortPlacement::Left => egui::vec2(-1.0, 0.0),
                PortPlacement::Right => egui::vec2(1.0, 0.0),
                PortPlacement::Top(_) => egui::vec2(0.0, -1.0),
                PortPlacement::Bottom => egui::vec2(0.0, 1.0),
            };
            let dist = (from_pos - to_pos).length().max(40.0);
            let strength = 0.4 * dist;
            let ctrl1 = from_pos + egui::vec2(1.0, 0.0) * strength;
            let ctrl2 = to_pos + to_axis * strength;
            let points: Vec<egui::Pos2> = (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    cubic_bezier(from_pos, ctrl1, ctrl2, to_pos, t)
                })
                .collect();
            wire_polylines.push(((conn.from.clone(), conn.to.clone()), points));
        }

        // Hit-test the cursor against every wire's polyline. The
        // closest wire within `hit_radius` is the candidate; if the
        // user clicked on empty canvas this frame, that candidate
        // becomes the new selection.
        let hit_radius = 6.0_f32;
        let mut wire_hit: Option<(PortId, PortId)> = None;
        if let Some(p) = pointer {
            let mut best: f32 = hit_radius;
            for (key, points) in &wire_polylines {
                let d = polyline_distance(p, points);
                if d < best {
                    best = d;
                    wire_hit = Some(key.clone());
                }
            }
        }
        // Plain primary-click on empty canvas (the canvas response
        // owns the click since no node took it) selects the wire if
        // one is under the cursor; otherwise the click was
        // already handled above as "deselect".
        if response.clicked() && self.palette_drag.is_none() && !clicked_in_group {
            if let Some(key) = wire_hit.clone() {
                self.select_connection(key.0, key.1);
            }
        }
        // Right-click on a wire offers a delete shortcut without
        // having to reach for the keyboard.
        let mut wire_to_delete: Option<(PortId, PortId)> = None;
        if let Some(key) = wire_hit.clone() {
            if response.secondary_clicked() {
                self.select_connection(key.0.clone(), key.1.clone());
            }
            response.clone().context_menu(|ui| {
                if ui.button("Delete connection").clicked() {
                    wire_to_delete = Some(key.clone());
                    ui.close_menu();
                }
            });
        }
        if let Some((from, to)) = wire_to_delete {
            self.push_undo("Delete connection");
            self.graph.disconnect(&from, &to);
            self.selection.connection = None;
        }

        // Stroke pass — the selected wire renders thicker + brighter
        // so the user can see what they've targeted before deleting.
        for ((from, to), points) in &wire_polylines {
            let is_selected = self
                .selection
                .connection
                .as_ref()
                .map(|(sf, st)| sf == from && st == to)
                .unwrap_or(false);
            let wire_active = drag_active_nodes.as_ref().is_none_or(|active| {
                active.contains(&from.node_id) || active.contains(&to.node_id)
            });
            let (w, color) = if is_selected {
                (3.0, tokens::WIRE_SELECTED)
            } else {
                (2.0, tokens::WIRE_DEFAULT)
            };
            let color = if wire_active {
                color
            } else {
                color.gamma_multiply(0.25)
            };
            let stroke = egui::Stroke::new(w, color);
            for i in 0..points.len() - 1 {
                painter.line_segment([points[i], points[i + 1]], stroke);
            }
        }

        // Draw in-progress connection
        if let Some(ref drag) = self.canvas.drag_connection {
            if let Some(pointer_pos) = ui.ctx().pointer_latest_pos() {
                let from = drag.from_pos;
                let dist = (pointer_pos - from).length().max(40.0);
                let strength = 0.4 * dist;
                // Outputs always emit rightward, so the source-end tangent is +X.
                let ctrl1 = egui::pos2(from.x + strength, from.y);
                let to_dir = (from - pointer_pos).normalized();
                let ctrl2 = pointer_pos + to_dir * strength;

                let points: Vec<egui::Pos2> = (0..=20)
                    .map(|i| {
                        let t = i as f32 / 20.0;
                        cubic_bezier(from, ctrl1, ctrl2, pointer_pos, t)
                    })
                    .collect();

                for i in 0..points.len() - 1 {
                    painter.line_segment(
                        [points[i], points[i + 1]],
                        egui::Stroke::new(2.0, tokens::WIRE_DRAG),
                    );
                }
            }
        }

        // Draw nodes
        let node_ids: Vec<NodeId> = self.node_visuals.keys().copied().collect();
        // (id, additive). additive=true means "Ctrl+click — toggle in
        // the multi-selection set"; false means "plain click — replace".
        let mut new_selection: Option<(NodeId, bool)> = None;
        // Nodes whose drag ended this frame — we'll resolve their
        // landing group (if any) below.
        let mut drag_drop_into_group: Vec<NodeId> = Vec::new();
        // Nodes that should not render or accept interaction this
        // frame (members of a collapsed subgraph, or anything outside
        // the current SubGraph tab's scope).
        let hidden_nodes = self.hidden_nodes_this_frame();
        // Contextual properties panel: arm when a node was clicked,
        // clear when a drag started or a tab was opened. Applied
        // after the per-node loop so the pending state is consistent
        // for the whole frame.
        let mut pending_props_arm: Option<PendingPropsOpen> = None;
        let mut pending_props_clear = false;
        let mut connection_start: Option<DragConnection> = None;
        let mut connection_end: Option<(NodeId, String)> = None;

        for node_id in &node_ids {
            if hidden_nodes.contains(node_id) {
                continue;
            }
            // Extract all owned data upfront so later `get_mut` calls don't conflict.
            let extracted = self.graph.get_node(*node_id).map(|n| {
                let passthrough_files = if n.node_type == NodeType::PassThrough {
                    let s = match n.params.get("files") {
                        Some(ParamValue::String(s)) => s.clone(),
                        _ => String::new(),
                    };
                    Some(parse_passthrough_files(&s))
                } else {
                    None
                };
                // For IO nodes, pull the user-supplied name and the
                // port kind. The bottom line of the tag shows the
                // user's name when set (and not equal to the default
                // "input"/"output" placeholder); otherwise it shows
                // the kind. The kind also colours the port circle.
                let (io_name, io_kind) = if matches!(
                    n.node_type,
                    NodeType::SubgraphInput | NodeType::SubgraphOutput
                ) {
                    let name = match n.params.get("name") {
                        Some(ParamValue::String(s)) if !s.is_empty() => Some(s.clone()),
                        _ => None,
                    };
                    let kind = match n.params.get("kind") {
                        Some(ParamValue::String(s)) if !s.is_empty() => Some(s.clone()),
                        _ => None,
                    };
                    (name, kind)
                } else {
                    (None, None)
                };
                (
                    n.node_type.clone(),
                    n.label.clone(),
                    n.inputs.clone(),
                    n.outputs.clone(),
                    passthrough_files,
                    io_name,
                    io_kind,
                )
            });
            let Some((
                node_type,
                node_label,
                node_inputs,
                node_outputs,
                passthrough_files,
                io_name,
                io_kind,
            )) = extracted
            else {
                continue;
            };
            let visual_data = self.node_visuals.get(node_id).map(|v| (v.position, v.size));
            let Some((node_pos_raw, node_size)) = visual_data else {
                continue;
            };
            // All borrows on self.graph and self.node_visuals released here.

            let node_pos = node_pos_raw + offset;
            let node_rect =
                egui::Rect::from_min_size(egui::pos2(node_pos.x, node_pos.y), node_size);

            if !canvas_rect.intersects(node_rect) {
                continue;
            }

            // Per-node minimum height from port count. IO nodes
            // render proportionally and don't stack ports, so they
            // get a smaller floor than regular nodes — the user can
            // shrink them below the 60-px default if they want to
            // pack a subgraph tightly.
            let is_primary = self.selection.node == Some(*node_id);
            let is_selected = is_primary || self.selection.nodes.contains(node_id);
            let is_io_input = matches!(node_type, NodeType::SubgraphInput);
            let is_io_output = matches!(node_type, NodeType::SubgraphOutput);
            let is_io = is_io_input || is_io_output;
            // Fade nodes that have no compatible port for the in-flight drag.
            let node_fade = drag_active_nodes.as_ref().map_or(1.0_f32, |active| {
                if active.contains(node_id) {
                    1.0
                } else {
                    0.3
                }
            });
            let left_input_count = node_inputs
                .iter()
                .filter(|p| matches!(PortPlacement::for_input(p.kind), PortPlacement::Left))
                .count();
            let n_ports = left_input_count.max(node_outputs.len());
            let footer_h_allowance = if node_type == NodeType::Preview {
                22.0
            } else {
                0.0
            };
            let (node_min_h, node_min_w) = if is_io {
                (28.0_f32, 90.0_f32)
            } else {
                (
                    (PORT_Y_BASE + n_ports as f32 * PORT_Y_STEP + 10.0 + footer_h_allowance)
                        .max(60.0),
                    100.0_f32,
                )
            };

            if is_io {
                // ── Subgraph IO node: directional tag ───────────────
                // The whole silhouette (rounded body on the far side,
                // chevron point on the port side) is built as one
                // convex polygon with arc samples on the rounded
                // corners. That keeps the fill and the border in
                // perfect register at any size — no chamfered
                // border-vs-curved-fill mismatch — and lets every
                // dimension scale proportionally with the node's
                // actual height. At the reference 60-px height the
                // numbers reproduce the design spec exactly.
                let h = node_rect.height();
                let scale = h / IO_REF_H;
                let chevron_w = h * 0.30;
                let body_radius = (h / 6.0).min(node_rect.width() / 4.0);
                // Vertical packing — earlier passes left ~20–28% of
                // the node height as empty space above and below
                // the icon/text stack. The fixes:
                //
                // - The icon now spans 80% of the node's height
                //   (was 60%). At 60-px height it's 48 px, leaving
                //   only 6 px above and below — about 10% per side.
                // - Text sizes scale up too so the text block
                //   doesn't look stranded next to the larger icon.
                //   Top 18 / bottom 15 at the reference height,
                //   stack ≈ 35 px = 58% of the node height.
                let inner_pad = 6.0 * scale;
                let icon_size = 48.0 * scale;
                let icon_text_gap = 8.0 * scale;
                let top_text_size = 18.0 * scale;
                let bottom_text_size = 15.0 * scale;

                let body_color = tokens::IO_BODY.gamma_multiply(node_fade);
                let border_color = if is_selected {
                    tokens::IO_BORDER_SEL
                } else {
                    tokens::IO_BORDER.gamma_multiply(node_fade)
                };
                let border_width = if is_selected { 2.0 } else { 1.5 };
                let mid_y = node_rect.center().y;

                let outline_pts = build_io_outline(node_rect, chevron_w, body_radius, is_io_input);
                painter.add(egui::Shape::convex_polygon(
                    outline_pts,
                    body_color,
                    egui::Stroke::new(border_width, border_color),
                ));

                // Icon container, anchored on the side opposite the
                // port, vertically centred.
                let icon_rect = if is_io_input {
                    egui::Rect::from_min_size(
                        egui::pos2(node_rect.left() + inner_pad, mid_y - icon_size / 2.0),
                        egui::vec2(icon_size, icon_size),
                    )
                } else {
                    egui::Rect::from_min_size(
                        egui::pos2(
                            node_rect.right() - inner_pad - icon_size,
                            mid_y - icon_size / 2.0,
                        ),
                        egui::vec2(icon_size, icon_size),
                    )
                };
                draw_io_icon(&painter, icon_rect, is_io_input);

                // Two-line text. Top line is the role
                // ("Input"/"Output"); bottom line is always the
                // port type. The node's `name` param drives the
                // *wrapper block*'s external port label and isn't
                // shown on the IO node itself — the IO node is a
                // marker, not a carrier of semantic identity.
                // `io_name` (extracted earlier) is therefore unused
                // in the visual; keeping the param around so the
                // wrapper render can read it.
                let _ = &io_name;
                let top_text = if is_io_input { "Input" } else { "Output" };
                let bottom_text = match io_kind.as_deref() {
                    Some(s) if !s.is_empty() => s,
                    _ => "Unknown",
                };
                let text_left = if is_io_input {
                    icon_rect.right() + icon_text_gap
                } else {
                    node_rect.left() + chevron_w + inner_pad
                };
                // 2 px was too tight — the "Input"/"Output" label
                // and the type label below it visually touched.
                // 6 px gives them room to breathe without breaking
                // the stack's visual unity.
                let line_gap = 6.0 * scale;
                let stack_h = top_text_size + line_gap + bottom_text_size;
                let text_top = mid_y - stack_h / 2.0;
                painter.text(
                    egui::pos2(text_left, text_top),
                    egui::Align2::LEFT_TOP,
                    top_text,
                    egui::FontId::proportional(top_text_size),
                    tokens::IO_LABEL_PRI,
                );
                painter.text(
                    egui::pos2(text_left, text_top + top_text_size + line_gap),
                    egui::Align2::LEFT_TOP,
                    bottom_text,
                    egui::FontId::proportional(bottom_text_size),
                    tokens::IO_LABEL_SEC,
                );
            } else {
                let ns = NodeStyle::default();
                // Node background — slightly lighter when in the multi-
                // selection, even lighter for the primary so the user can
                // tell which one's properties are showing.
                let bg_color = if is_primary {
                    ns.bg_pri
                } else if is_selected {
                    ns.bg_sel
                } else {
                    ns.bg
                };
                painter.rect_filled(node_rect, ns.rounding, bg_color.gamma_multiply(node_fade));

                // Node border
                let (border_color, bw) = if is_selected {
                    (ns.border_sel, ns.border_w_sel)
                } else {
                    (ns.border, ns.border_w)
                };
                painter.rect_stroke(
                    node_rect,
                    ns.rounding,
                    egui::Stroke::new(bw, border_color.gamma_multiply(node_fade)),
                    egui::StrokeKind::Outside,
                );

                // Node title bar -- inset by TITLE_Y_OFFSET so the top-port
                // circles (centered on node_rect.min.y) don't overlap the text.
                let title_rect = egui::Rect::from_min_size(
                    egui::pos2(node_rect.min.x, node_rect.min.y + TITLE_Y_OFFSET),
                    egui::vec2(node_rect.width(), ns.title_h),
                );
                let title_color = node_type_color(&node_type).gamma_multiply(node_fade);
                painter.rect_filled(title_rect, ns.title_rounding, title_color);
                painter.text(
                    title_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &node_label,
                    egui::FontId::proportional(12.0),
                    egui::Color32::WHITE,
                );
            }

            // Input + output port handles, registered as proper egui
            // interactions so their click/drag events are z-ordered
            // ahead of the canvas's marquee detection. Without this,
            // a click on a port handle (which sits on the node's
            // edge) would also fire `drag_started_by(Primary)` on
            // the canvas behind it — the user would see the marquee
            // and the connection-start fire simultaneously.
            let port_radius = 5.0;
            let hit_size = egui::vec2(14.0, 14.0);
            let mut left_port_idx = 0_usize;
            for (i, input) in node_inputs.iter().enumerate() {
                // SubgraphInput's input is the EXTERNAL side, only ever
                // reached from outside the subgraph (via the collapsed
                // block's port). Don't draw it on the IO pill.
                if is_io_input {
                    continue;
                }
                let placement = PortPlacement::for_input(input.kind);
                let port_pos = node_port_pos(&node_type, node_rect, placement, left_port_idx);
                if matches!(placement, PortPlacement::Left) {
                    left_port_idx += 1;
                }
                let port_color = port_kind_color(&input.kind);
                let hit_rect = egui::Rect::from_center_size(port_pos, hit_size);
                let port_resp = ui.interact(
                    hit_rect,
                    egui::Id::new(("port_in", node_id.0, i)),
                    egui::Sense::click_and_drag(),
                );
                draw_port_circle(
                    &painter,
                    port_pos,
                    port_radius,
                    port_color,
                    port_resp.hovered(),
                );
                if is_io {
                    // Spec: 1 px outline around the port circle in
                    // `#1F2933` so it reads against the chevron.
                    painter.circle_stroke(
                        port_pos,
                        port_radius,
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(0x1F, 0x29, 0x33)),
                    );
                }
                if !is_io {
                    if matches!(placement, PortPlacement::Left) {
                        painter.text(
                            egui::pos2(port_pos.x + 10.0, port_pos.y),
                            egui::Align2::LEFT_CENTER,
                            &input.label,
                            egui::FontId::proportional(10.0),
                            egui::Color32::LIGHT_GRAY,
                        );
                    } else {
                        // Top / Bottom ports skip the inline label; show tooltip
                        // instantly (no delay) so the user gets feedback without waiting.
                        if port_resp.hovered() {
                            egui::show_tooltip_text(
                                ui.ctx(),
                                ui.layer_id(),
                                egui::Id::new(("port_tip", node_id.0, i as u64)),
                                &input.label,
                            );
                        }
                    }
                }
                // End an in-flight connection when the user releases
                // primary while their cursor is over this input's
                // hit rect.
                if self.canvas.drag_connection.is_some()
                    && ui.input(|i| i.pointer.primary_released())
                    && port_resp.contains_pointer()
                {
                    connection_end = Some((*node_id, input.name.clone()));
                }
                // Disconnect-by-grabbing-input: if the user starts a
                // drag on an input port that already has a connection,
                // detach it and turn the drag into a re-wire from the
                // existing source's output. This makes "I want to
                // move this wire to a different input" a one-gesture
                // operation: grab the input end, drag to new target.
                if port_resp.drag_started_by(egui::PointerButton::Primary)
                    && self.canvas.drag_connection.is_none()
                {
                    let existing = self
                        .graph
                        .connections()
                        .iter()
                        .find(|c| c.to.node_id == *node_id && c.to.port_name == input.name)
                        .map(|c| {
                            (
                                c.from.node_id,
                                c.from.port_name.clone(),
                                c.to.node_id,
                                c.to.port_name.clone(),
                            )
                        });
                    if let Some((src_node, src_port, dst_node, dst_port)) = existing {
                        self.push_undo("Disconnect");
                        self.graph.disconnect(
                            &PortId {
                                node_id: src_node,
                                port_name: src_port.clone(),
                            },
                            &PortId {
                                node_id: dst_node,
                                port_name: dst_port,
                            },
                        );
                        if let Some(src_pos) =
                            self.output_port_screen_pos(src_node, &src_port, offset)
                        {
                            connection_start = Some(DragConnection {
                                from_node: src_node,
                                from_port: src_port,
                                from_pos: src_pos,
                            });
                        }
                    }
                }
            }

            for (i, output) in node_outputs.iter().enumerate() {
                // SubgraphOutput's output is the EXTERNAL side, only
                // ever reached from outside the subgraph. Don't draw
                // it on the IO pill.
                if is_io_output {
                    continue;
                }
                let port_pos = node_port_pos(&node_type, node_rect, PortPlacement::Right, i);
                let port_color = port_kind_color(&output.kind);
                let hit_rect = egui::Rect::from_center_size(port_pos, hit_size);
                let port_resp = ui.interact(
                    hit_rect,
                    egui::Id::new(("port_out", node_id.0, i)),
                    egui::Sense::click_and_drag(),
                );
                draw_port_circle(
                    &painter,
                    port_pos,
                    port_radius,
                    port_color,
                    port_resp.hovered(),
                );
                if !is_io {
                    painter.text(
                        egui::pos2(port_pos.x - 10.0, port_pos.y),
                        egui::Align2::RIGHT_CENTER,
                        &output.label,
                        egui::FontId::proportional(10.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                }
                // Begin a connection only when the port's own response
                // says a drag started on it. egui's z-order ensures
                // this fires only when the cursor is genuinely on the
                // port handle, not on the surrounding node body.
                if port_resp.drag_started_by(egui::PointerButton::Primary)
                    && self.canvas.drag_connection.is_none()
                {
                    connection_start = Some(DragConnection {
                        from_node: *node_id,
                        from_port: output.name.clone(),
                        from_pos: port_pos,
                    });
                }
            }

            // PassThrough: draw file hierarchy in node body
            if let Some(ref files) = passthrough_files {
                draw_passthrough_body(&painter, node_rect, files);
            }

            // Bundler layout:
            //   • Export button  — top-right, 44×36, same as toolbar
            //   • Preview footer — full-width bar at the bottom (styled like an
            // Bundler carries an Export button. Preview nodes carry
            // an "Open" footer that pops the 3D viewport. The two
            // are no longer co-located on the same node — Bundler is
            // for export only; the Preview node is the sole driver
            // of the 3D viewport (see Stage N).
            let has_open_footer = node_type == NodeType::Preview;
            let has_export_button = node_type == NodeType::Bundler;
            let export_rect: Option<egui::Rect> = if has_export_button {
                let btn = egui::vec2(44.0, 36.0);
                Some(egui::Rect::from_min_size(
                    egui::pos2(node_rect.max.x - 6.0 - btn.x, node_rect.min.y + 26.0),
                    btn,
                ))
            } else {
                None
            };
            let open_footer_rect: Option<egui::Rect> = if has_open_footer {
                let footer_h = 22.0_f32;
                Some(egui::Rect::from_min_size(
                    egui::pos2(node_rect.min.x, node_rect.max.y - footer_h),
                    egui::vec2(node_rect.width(), footer_h),
                ))
            } else {
                None
            };

            // Paint Bundler's Export button.
            if let Some(export_rect) = export_rect {
                let ptr = ui.ctx().pointer_latest_pos();
                let busy = self.preview.export_status.affects(*node_id);
                let any_running = self.preview.export_status.is_running();

                let export_hov = ptr.is_some_and(|p| export_rect.contains(p));
                let export_bg = if busy {
                    egui::Color32::from_rgb(80, 80, 30)
                } else if any_running {
                    egui::Color32::from_rgb(40, 60, 40)
                } else if export_hov {
                    egui::Color32::from_rgb(48, 132, 62)
                } else {
                    egui::Color32::from_rgb(35, 110, 50)
                };
                painter.rect_filled(export_rect, 5.0, export_bg);
                paint_export_icon(&painter, export_rect, egui::Color32::WHITE);
                if busy {
                    paint_busy_dot(&painter, export_rect, ui.input(|i| i.time));
                    ui.ctx().request_repaint();
                }
            }

            // Paint the Preview node's "Open" footer.
            if let Some(footer_rect) = open_footer_rect {
                let ptr = ui.ctx().pointer_latest_pos();
                let open_hov = ptr.is_some_and(|p| footer_rect.contains(p));
                let footer_bg = if open_hov {
                    egui::Color32::from_rgb(22, 148, 178)
                } else {
                    egui::Color32::from_rgb(15, 110, 140)
                };
                painter.rect_filled(
                    footer_rect,
                    egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 4,
                        se: 4,
                    },
                    footer_bg,
                );
                // "Open" reads as "open the viewport" — the Preview
                // node IS the preview, the footer just exposes the
                // viewport panel.
                painter.text(
                    footer_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Open",
                    egui::FontId::proportional(11.0),
                    egui::Color32::WHITE,
                );
            }

            // ── Interaction ──────────────────────────────────────────────────────────
            // Node interact is registered FIRST.  Button interacts are registered
            // SECOND so that egui gives them higher priority when clicks land on them.
            // Neither button click changes the node selection.

            // Node interaction (click / double-click / drag)
            let node_response = ui.interact(
                node_rect,
                egui::Id::new(("node", node_id.0)),
                egui::Sense::click_and_drag(),
            );

            // Button interactions — registered AFTER node so they
            // capture clicks first. Export and Open are independent
            // affordances on different node types now.
            let mut run_clicked = false;
            let mut open_clicked = false;
            if let Some(export_rect) = export_rect {
                let any_running = self.preview.export_status.is_running();
                let busy_self = self.preview.export_status.affects(*node_id);
                let run_tooltip = if busy_self {
                    "Exporting…"
                } else if any_running {
                    "Another export is running"
                } else {
                    "Export Bundle"
                };
                let run_sense = if any_running {
                    egui::Sense::hover()
                } else {
                    egui::Sense::click()
                };
                let run_resp = ui
                    .interact(
                        export_rect,
                        egui::Id::new(("bundler_run", node_id.0)),
                        run_sense,
                    )
                    .on_hover_text(run_tooltip);
                run_clicked = !any_running && run_resp.clicked();
            }
            if let Some(footer_rect) = open_footer_rect {
                let prev_resp = ui
                    .interact(
                        footer_rect,
                        egui::Id::new(("preview_open", node_id.0)),
                        egui::Sense::click(),
                    )
                    .on_hover_text("Open the 3D viewport");
                open_clicked = prev_resp.clicked();
            }

            // Node selection — only when neither action button was
            // clicked. Also arm the contextual Properties panel: the
            // click is the user's intent to "look at" this node;
            // the 100 ms hover gate filters out accidental clicks on
            // the way to dragging.
            if node_response.clicked() && !run_clicked && !open_clicked {
                let additive = ui.ctx().input(|i| i.modifiers.ctrl || i.modifiers.command);
                new_selection = Some((*node_id, additive));
                // Ctrl-click is "build a multi-selection", not "look
                // at this node" — don't pop a per-node panel for
                // each toggle; the user is composing a selection.
                if !additive {
                    if let Some(p) = ui.ctx().pointer_latest_pos() {
                        pending_props_arm = Some(PendingPropsOpen {
                            target: PropsTarget::Node(*node_id),
                            armed_at: Instant::now(),
                            armed_pos: p,
                        });
                    }
                }
            }
            // Drag-start on a node cancels any pending props popup —
            // the user wanted to drag, not inspect.
            if node_response.drag_started_by(egui::PointerButton::Primary) {
                pending_props_clear = true;
            }

            // Right-click context menu — group operations + delete.
            // Building the menu here keeps the per-node-id context tight.
            let this_id = *node_id;
            let in_group = self.node_to_group.get(&this_id).copied();
            let other_groups: Vec<(u64, String)> = self
                .groups
                .iter()
                .filter_map(|(gid, g)| {
                    if Some(*gid) == in_group {
                        None
                    } else {
                        let label = if g.label.is_empty() {
                            format!("Group {gid}")
                        } else {
                            g.label.clone()
                        };
                        Some((*gid, label))
                    }
                })
                .collect();
            let mut group_op: Option<GroupOp> = None;
            let mut auto_wire_targets: Option<Vec<NodeId>> = None;
            let mut auto_layout_requested = false;
            // Snapshot the set so the menu closure can iterate without
            // re-borrowing `self`.
            let multi: Vec<NodeId> = self.selection.nodes.iter().copied().collect();
            let multi_active = multi.len() > 1 && self.selection.nodes.contains(&this_id);
            // Pre-compute the unwired-input count so the menu item
            // can render disabled when there's nothing to wire.
            let auto_wire_pool: Vec<NodeId> = if multi_active {
                multi.clone()
            } else {
                vec![this_id]
            };
            let unwired_count = self.count_unwired_inputs(&auto_wire_pool);
            node_response.context_menu(|ui| {
                if multi_active {
                    let label = format!("Group {} selected nodes", multi.len());
                    if ui.button(label).clicked() {
                        group_op = Some(GroupOp::CreateFromSelection(multi.clone()));
                        ui.close_menu();
                    }
                    if !other_groups.is_empty() {
                        ui.menu_button("Move selection to group", |ui| {
                            for (gid, label) in &other_groups {
                                if ui.button(label).clicked() {
                                    group_op = Some(GroupOp::AddManyTo(multi.clone(), *gid));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                } else {
                    if ui.button("New group with this node").clicked() {
                        group_op = Some(GroupOp::CreateWith(this_id));
                        ui.close_menu();
                    }
                    if !other_groups.is_empty() {
                        ui.menu_button("Move to group", |ui| {
                            for (gid, label) in &other_groups {
                                if ui.button(label).clicked() {
                                    group_op = Some(GroupOp::AddTo(this_id, *gid));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    if in_group.is_some() && ui.button("Remove from group").clicked() {
                        group_op = Some(GroupOp::RemoveFrom(this_id));
                        ui.close_menu();
                    }
                }
                ui.separator();
                // Auto Wire: fill every unwired input on the
                // target(s) by looking left for compatible outputs.
                // Disabled when there's nothing to wire so the
                // menu doesn't pretend to have an action.
                let label = if multi_active {
                    format!("Auto Wire {} nodes...", multi.len())
                } else {
                    "Auto Wire...".to_string()
                };
                if ui
                    .add_enabled(unwired_count > 0, egui::Button::new(label))
                    .clicked()
                {
                    auto_wire_targets = Some(auto_wire_pool.clone());
                    ui.close_menu();
                }
                // Auto Layout: reflow the selection (or the
                // right-clicked node) into a left-to-right depth
                // layout. Subgraphs travel as one block.
                if ui.button(t!("editor.menu.auto_layout")).clicked() {
                    auto_layout_requested = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Delete node").clicked() {
                    if !multi_active {
                        self.select_only_node(this_id);
                    }
                    self.delete_selected_node();
                    ui.close_menu();
                }
            });
            if let Some(targets) = auto_wire_targets {
                self.push_undo("Auto wire");
                let made = self.auto_wire_nodes(&targets);
                self.dialog.status_message = Some(if made == 0 {
                    "Auto wire: no compatible sources to the left.".to_string()
                } else if made == 1 {
                    "Auto wire: 1 connection added.".to_string()
                } else {
                    format!("Auto wire: {made} connections added.")
                });
            }
            if auto_layout_requested {
                // If the user right-clicked a node that wasn't in
                // their existing selection, treat the click as
                // "select just this node" so Auto Layout acts on it
                // alone rather than on a stale selection.
                if !self.selection.nodes.contains(&this_id) {
                    self.select_only_node(this_id);
                }
                self.auto_layout_selection();
            }
            if let Some(op) = group_op {
                // Each user-visible group action is one undo step; we
                // push once before the helpers modify state.
                self.push_undo(match &op {
                    GroupOp::CreateWith(_) => "Create group",
                    GroupOp::CreateFromSelection(_) => "Group selection",
                    GroupOp::AddTo(_, _) | GroupOp::AddManyTo(_, _) => "Move to group",
                    GroupOp::RemoveFrom(_) => "Remove from group",
                });
                match op {
                    GroupOp::CreateWith(nid) => {
                        let gid = self.create_group(String::new());
                        self.add_node_to_group(nid, gid);
                        self.select_group(gid);
                    }
                    GroupOp::CreateFromSelection(ids) => {
                        let gid = self.create_group(String::new());
                        for id in ids {
                            self.add_node_to_group(id, gid);
                        }
                        self.select_group(gid);
                    }
                    GroupOp::AddTo(nid, gid) => self.add_node_to_group(nid, gid),
                    GroupOp::AddManyTo(ids, gid) => {
                        for id in ids {
                            self.add_node_to_group(id, gid);
                        }
                    }
                    GroupOp::RemoveFrom(nid) => self.remove_node_from_group(nid),
                }
            }

            if open_clicked {
                self.preview.open = true;
                // The Preview node IS the viewport target; clicking
                // its footer just opens the panel and re-targets it
                // in case multiple Preview nodes exist in the graph.
                self.preview.node = Some(*node_id);
            }
            if run_clicked && self.validate_before_export("Bundle") {
                self.preview.run_bundler_node = Some(*node_id);
            }

            // Resize corner handles (8 px squares; processed after node interact so they
            // are "on top" in egui's interaction stack)
            let handle_sz = 8.0_f32;
            let corners: [(i8, i8); 4] = [(-1, -1), (1, -1), (-1, 1), (1, 1)];
            let mut any_resize = false;
            for (cx, cy) in corners {
                let corner_pos = match (cx, cy) {
                    (-1, -1) => node_rect.left_top(),
                    (1, -1) => node_rect.right_top(),
                    (-1, 1) => node_rect.left_bottom(),
                    _ => node_rect.right_bottom(),
                };
                let handle_rect =
                    egui::Rect::from_center_size(corner_pos, egui::vec2(handle_sz, handle_sz));
                let handle_resp = ui.interact(
                    handle_rect,
                    egui::Id::new(("resize", node_id.0, cx, cy)),
                    egui::Sense::drag(),
                );
                let handle_active = handle_resp.hovered() || handle_resp.dragged();
                if handle_active {
                    let cursor = if cx == cy {
                        egui::CursorIcon::ResizeNwSe
                    } else {
                        egui::CursorIcon::ResizeNeSw
                    };
                    ui.ctx().set_cursor_icon(cursor);
                }
                if node_response.hovered() || handle_active {
                    painter.rect_filled(
                        handle_rect,
                        2.0,
                        if handle_active {
                            egui::Color32::from_rgb(160, 200, 255)
                        } else {
                            egui::Color32::from_rgb(70, 80, 100)
                        },
                    );
                }
                if handle_resp.dragged() {
                    any_resize = true;
                    let delta = handle_resp.drag_delta();
                    if let Some(v) = self.node_visuals.get_mut(node_id) {
                        if cx == 1 {
                            v.size.x = (v.size.x + delta.x).max(node_min_w);
                        } else {
                            let new_w = (v.size.x - delta.x).max(node_min_w);
                            let dw = new_w - v.size.x;
                            v.position.x -= dw;
                            v.size.x = new_w;
                        }
                        if cy == 1 {
                            v.size.y = (v.size.y + delta.y).max(node_min_h);
                        } else {
                            let new_h = (v.size.y - delta.y).max(node_min_h);
                            let dh = new_h - v.size.y;
                            v.position.y -= dh;
                            v.size.y = new_h;
                        }
                    }
                }
            }

            // Move node on drag (suppressed while a resize handle is
            // active). When the dragged node is part of a multi-
            // selection, every selected node moves by the same delta
            // — same shortcut as Photoshop / Figma / Blender.
            if node_response.dragged() && !any_resize {
                let delta = node_response.drag_delta();
                if self.selection.nodes.contains(node_id) && self.selection.nodes.len() > 1 {
                    let to_move: Vec<NodeId> = self.selection.nodes.iter().copied().collect();
                    for id in to_move {
                        if let Some(visual) = self.node_visuals.get_mut(&id) {
                            visual.position += delta;
                        }
                    }
                } else if let Some(visual) = self.node_visuals.get_mut(node_id) {
                    visual.position += delta;
                }
            }

            // On drag-end, fold a node into whatever group its centre
            // landed inside. Nodes already in that group are skipped.
            // Multi-selection drops together — the group accepts all
            // selected nodes whose centres landed in its rect.
            if node_response.drag_stopped() {
                let drop_targets: Vec<NodeId> =
                    if self.selection.nodes.contains(node_id) && self.selection.nodes.len() > 1 {
                        self.selection.nodes.iter().copied().collect()
                    } else {
                        vec![*node_id]
                    };
                drag_drop_into_group.extend(drop_targets);
            }
        }

        // Collapsed subgraphs render as compact node-like blocks AFTER
        // nodes (so they appear in the foreground). They handle their
        // own selection / double-click-to-enter-confined-mode.
        let (subgraph_block_rects, _subgraph_handle_positions, sg_conn_start, sg_conn_end) =
            self.draw_collapsed_subgraphs(ui, offset);
        if let Some(s) = sg_conn_start {
            connection_start = Some(s);
        }
        if let Some(e) = sg_conn_end {
            connection_end = Some(e);
        }
        for (gid, rect) in subgraph_block_rects {
            let resp = ui.interact(
                rect,
                egui::Id::new(("subgraph_block", gid)),
                egui::Sense::click_and_drag(),
            );
            if resp.clicked() {
                self.select_group(gid);
                if let Some(p) = ui.ctx().pointer_latest_pos() {
                    self.dialog.pending_props_open = Some(PendingPropsOpen {
                        target: PropsTarget::Group(gid),
                        armed_at: Instant::now(),
                        armed_pos: p,
                    });
                }
            }
            if resp.double_clicked() {
                self.open_or_activate_tab(CanvasView::SubGraph(gid));
                self.clear_selection();
            }
            // Drag the collapsed block to translate every member node
            // by the same delta — same affordance as dragging the
            // expanded group's title bar.
            if resp.dragged() {
                let delta = resp.drag_delta();
                if let Some(g) = self.groups.get(&gid) {
                    let ids: Vec<NodeId> = g.member_ids.iter().copied().collect();
                    for id in ids {
                        if let Some(v) = self.node_visuals.get_mut(&id) {
                            v.position += delta;
                        }
                    }
                }
            }
            resp.context_menu(|ui| {
                if ui.button("Enter (edit contents)").clicked() {
                    self.open_or_activate_tab(CanvasView::SubGraph(gid));
                    self.clear_selection();
                    ui.close_menu();
                }
                if ui.button("Expand").clicked() {
                    self.push_undo("Expand subgraph");
                    if let Some(g) = self.groups.get_mut(&gid) {
                        g.collapsed = false;
                    }
                    ui.close_menu();
                }
                if ui.button("Delete subgraph").clicked() {
                    self.delete_subgraph_with_contents(gid);
                    ui.close_menu();
                }
            });
        }

        // Double-click on an expanded subgraph's header / body also
        // enters confined-edit mode (parallel affordance to double-
        // clicking the collapsed block).
        let subgraph_double_click: Vec<u64> = self
            .group_header_rects
            .iter()
            .filter_map(|(gid, rect)| {
                let g = self.groups.get(gid)?;
                if g.is_subgraph
                    && pointer.is_some_and(|p| rect.contains(p))
                    && ui.ctx().input(|i| {
                        i.pointer
                            .button_double_clicked(egui::PointerButton::Primary)
                    })
                {
                    Some(*gid)
                } else {
                    None
                }
            })
            .collect();
        if let Some(gid) = subgraph_double_click.first().copied() {
            self.open_or_activate_tab(CanvasView::SubGraph(gid));
            self.clear_selection();
        }
        // Apply the pending-props state changes from the node loop.
        // Clear takes precedence over arm so a drag-start on the
        // same frame as a click reliably cancels the popup.
        if pending_props_clear {
            self.dialog.pending_props_open = None;
        } else if let Some(p) = pending_props_arm {
            self.dialog.pending_props_open = Some(p);
        }

        // Apply selection. Plain click replaces; Ctrl+click toggles.
        if let Some((id, additive)) = new_selection {
            if additive {
                self.toggle_select_node(id);
            } else {
                self.select_only_node(id);
            }
        }

        // Resolve drag-and-drop into groups: for each node that just
        // finished a drag, check whether its centre is inside any
        // group's rect. If so, add it to that group (or move it from
        // its old group). If it ended outside every group, remove it
        // from any group it had been in. We push a single undo entry
        // BEFORE applying any of these changes so the whole drag is
        // one undo step (matching typical editor behaviour).
        //
        // SubGraph view: the entire canvas IS the active subgraph,
        // so `draw_groups` deliberately doesn't paint the group's
        // backdrop. That leaves `group_header_rects` empty, which
        // would make the drop logic below conclude every node landed
        // "outside every group" and evict each one from the
        // subgraph it's a member of. Skip the whole pass — group
        // membership only changes via Main-view drags or the right-
        // click menu.
        let in_subgraph_view = matches!(self.current_view(), CanvasView::SubGraph(_));
        if in_subgraph_view {
            drag_drop_into_group.clear();
        }
        if !drag_drop_into_group.is_empty() {
            // Determine if the drag actually changes any membership;
            // if not, skip the undo push so trivial drags don't pile
            // up empty undo entries.
            let mut group_membership_changed = false;
            // Compute candidate landing without mutating yet.
            let group_rects: Vec<(u64, egui::Rect)> = self
                .group_header_rects
                .iter()
                .filter_map(|(gid, h)| {
                    let b = self.group_body_rects.get(gid)?;
                    Some((*gid, h.union(*b)))
                })
                .collect();
            for nid in &drag_drop_into_group {
                let Some(visual) = self.node_visuals.get(nid) else {
                    continue;
                };
                let centre = egui::pos2(
                    visual.position.x + visual.size.x * 0.5 + offset.x,
                    visual.position.y + visual.size.y * 0.5 + offset.y,
                );
                let landed_in = group_rects
                    .iter()
                    .find(|(_, r)| r.contains(centre))
                    .map(|(gid, _)| *gid);
                let prev = self.node_to_group.get(nid).copied();
                match (prev, landed_in) {
                    (Some(p), Some(n)) if p != n => group_membership_changed = true,
                    (None, Some(_)) | (Some(_), None) => group_membership_changed = true,
                    _ => {}
                }
            }
            if group_membership_changed {
                self.push_undo("Drag into/out of group");
            }
        }
        if !drag_drop_into_group.is_empty() {
            // Build (group_id, full_rect) snapshot. We use the cached
            // header + body rects to reconstruct the union.
            let group_rects: Vec<(u64, egui::Rect)> = self
                .group_header_rects
                .iter()
                .filter_map(|(gid, h)| {
                    let b = self.group_body_rects.get(gid)?;
                    Some((*gid, h.union(*b)))
                })
                .collect();
            for nid in drag_drop_into_group {
                let Some(visual) = self.node_visuals.get(&nid) else {
                    continue;
                };
                let centre = egui::pos2(
                    visual.position.x + visual.size.x * 0.5 + offset.x,
                    visual.position.y + visual.size.y * 0.5 + offset.y,
                );
                let landed_in = group_rects
                    .iter()
                    .find(|(_, r)| r.contains(centre))
                    .map(|(gid, _)| *gid);
                match (self.node_to_group.get(&nid).copied(), landed_in) {
                    (Some(prev), Some(new)) if prev != new => {
                        self.add_node_to_group(nid, new);
                    }
                    (None, Some(new)) => {
                        self.add_node_to_group(nid, new);
                    }
                    (Some(_), None) => {
                        // Dragged a member entirely outside every
                        // group rect → remove it from its group so
                        // the rect doesn't balloon to chase it.
                        self.remove_node_from_group(nid);
                    }
                    _ => {}
                }
            }
        }

        // Handle connection creation. If a connection drag started
        // this frame, cancel any marquee that started in the same
        // frame — the user wanted to wire a port, not select a region.
        // (Belt-and-braces guard; the ui.interact() registrations on
        // each port should already steal the click from the canvas's
        // marquee detection, but this catches edge cases where two
        // overlapping interactions both think they started.)
        if let Some(start) = connection_start {
            self.canvas.drag_connection = Some(start);
            self.canvas.marquee_start = None;
        }

        if let Some((to_node, to_port)) = connection_end {
            if let Some(drag) = self.canvas.drag_connection.clone() {
                self.push_undo("Connect nodes");
                let from = PortId {
                    node_id: drag.from_node,
                    port_name: drag.from_port,
                };
                let to = PortId {
                    node_id: to_node,
                    port_name: to_port,
                };
                let _ = self.graph.connect(from, to);
            }
            self.canvas.drag_connection = None;
        }

        // Cancel drag on release without target
        if ui.input(|i| i.pointer.any_released()) {
            self.canvas.drag_connection = None;
        }
    }
}
