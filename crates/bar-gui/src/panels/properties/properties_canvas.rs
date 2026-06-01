//! Shared 2D canvas widget for properties-panel item editing.
//!
//! Used by node types that author 2D data (the `Layout` node's
//! primitive shapes and spline control points). The widget owns the
//! canvas chrome
//! -- background grid, pan/zoom, hit-testing, drag state -- and emits
//! typed gestures the calling panel translates into param mutations.
//!
//! Coords throughout this module are in normalised [0..1, 0..1] space
//! aligned with the canvas's logical content. The calling panel never
//! sees pixel coords; the widget projects to / from pixels internally
//! via the `CanvasTransform`.

use eframe::egui;

/// Persistent canvas state. Held by the calling panel (typically inside
/// an `egui::Id`-keyed temp slot) so pan / zoom / selection / drag
/// survive frames.
#[derive(Clone, Debug, Default)]
pub struct CanvasState {
    /// Canvas-pixel offset applied to the (0, 0) corner of the logical
    /// content. Positive values move content right / down.
    pub pan: egui::Vec2,
    /// Canvas pixels per normalised unit. Default `0.0` means
    /// "auto-fit on next frame" -- the widget computes a fit value and
    /// stores it.
    pub zoom: f32,
    /// Index into the panel's item list, if anything is selected.
    pub selected: Option<usize>,
    /// Drag state, set on a press and cleared on a release.
    pub drag: Option<DragInProgress>,
    /// Active "drag in empty space to create a new shape" gesture. Set
    /// when the user presses in an empty area (no handle, no item
    /// body), cleared on release. While set, the widget renders a
    /// preview rectangle from `press_pos` to `current_pos`.
    pub creation: Option<CreationDrag>,
    /// World-space position of the OPPOSITE corner when the user is
    /// dragging a corner-resize handle. Captured by the panel on
    /// HandlePressed, consumed each HandleDragged frame so the
    /// opposite corner stays anchored (the dragged corner moves to
    /// the cursor; the shape's centre moves with the cursor as a
    /// consequence). Cleared on release. `None` for non-corner drags.
    pub corner_anchor: Option<[f32; 2]>,
    /// World-space centre of the primitive at press time when the
    /// user pressed on its BODY (as opposed to grabbing the centre
    /// handle directly). Used so the move drag translates the
    /// primitive by the cursor delta -- the click point stays under
    /// the cursor -- instead of snapping the centre to the click
    /// point. `None` for handle-direct drags. Cleared on release.
    pub body_drag_origin: Option<[f32; 2]>,
}

/// Drag-rect / freehand-path state for creating a new shape. The
/// shape isn't created until release, and only if the cursor moved
/// past a small threshold from the press point -- a click without
/// drag does NOT create. `path` accumulates samples along the drag
/// (subsampled to one point every ~0.005 normalised units) so a
/// spline-tool freehand draw can reconstruct what the user traced;
/// primitive tools simply use `press_pos` + `current_pos`.
#[derive(Clone, Debug, Default)]
pub struct CreationDrag {
    pub press_pos: [f32; 2],
    pub current_pos: [f32; 2],
    pub path: Vec<[f32; 2]>,
    pub moved: bool,
}

/// Drag-state details. The panel inspects this to know which item /
/// handle is being moved without having to track it independently.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DragInProgress {
    pub item: usize,
    pub handle: HandleId,
    /// Where the drag started, in normalised coords. Used to measure
    /// movement so a press-without-drag (a plain click-to-select)
    /// doesn't get treated as an edit.
    pub press_pos: [f32; 2],
    /// Set once the cursor moves past a small threshold from
    /// `press_pos`. Distinguishes a real drag (commit an undo entry)
    /// from a click that merely selected the handle (no undo entry).
    pub moved: bool,
}

/// Generic handle identifier. Panels invent their own semantic kinds
/// via the inner `u8` (centre handle = 0, corner_0 = 1, rotation = 5,
/// etc.). The widget only uses it for equality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleId(pub u8);

/// Visual category for a handle. Drives the widget's per-handle
/// rendering -- each kind gets a distinct silhouette + colour so the
/// transformer reads as a set of differentiated controls rather than
/// a bag of identical dots, and so a handle never looks the same as
/// the shape outline it sits on top of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    /// Centre / move handle. Drawn as a filled disc in cool blue.
    Centre,
    /// Scale handle at a corner. Drawn as a filled square in green.
    Corner,
    /// Rotation handle. Drawn as a ring with an inner dot, in pink.
    Rotation,
    /// One control point of a spline. Drawn as a filled diamond in
    /// cyan -- visually unlike any primitive transformer handle.
    SplinePoint,
}

/// One draggable handle the calling panel wants the widget to know
/// about. Handles are listed every frame -- the panel computes their
/// positions from its own data, then hands the list over.
#[derive(Clone, Debug)]
pub struct HandleSpec {
    pub item: usize,
    pub id: HandleId,
    pub kind: HandleKind,
    /// Normalised position. The widget hit-tests this in pixel space
    /// against the cursor with the configured radius.
    pub pos: [f32; 2],
    /// Pixel hit-test radius. Lets small handles (rotation arms) be
    /// distinguished from big handles (shape centres).
    pub px_radius: f32,
    /// Cursor the widget shows when this specific handle is hovered.
    /// Panels pick per-handle (e.g. NwseResize vs NeswResize for
    /// opposite corner pairs, Move for the centre, Crosshair for the
    /// rotation arm).
    pub cursor: egui::CursorIcon,
}

/// Gestures emitted by the widget. The panel matches on these and
/// mutates its own data accordingly. Some variants carry fields only
/// one item kind reads (a spline reads `handle` on delete; a primitive
/// ignores it); the `dead_code` allow lets the public API stay uniform
/// without per-consumer noise.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum CanvasGesture {
    /// Left-click on empty canvas. Panels typically translate this to
    /// "append a new item at `pos`".
    AddAt { pos: [f32; 2] },
    /// Left-press on a handle. Panel snapshots its state for undo and
    /// marks the item selected.
    HandlePressed {
        item: usize,
        handle: HandleId,
        pos: [f32; 2],
    },
    /// Cursor moved while a handle drag is active. `pos` is the new
    /// normalised position the panel should map to its data.
    HandleDragged {
        item: usize,
        handle: HandleId,
        pos: [f32; 2],
    },
    /// Drag released. `moved` is true if the cursor actually dragged
    /// the handle (commit the held undo snapshot) and false if it was
    /// a click that only selected the handle (discard the snapshot --
    /// no edit happened).
    HandleReleased {
        item: usize,
        handle: HandleId,
        moved: bool,
    },
    /// Right-click on a handle. The panel decides what to remove --
    /// for a primitive item it's the whole item; for a spline item
    /// the `handle` identifies which control point to drop.
    HandleDeleted { item: usize, handle: HandleId },
    /// Left-press on a shape's body (no handle hit). Fired on press
    /// (not click), so the panel can immediately both select the
    /// shape and start a "move the whole shape" drag without the
    /// user having to click once to select and again to drag.
    /// `pos` is the press position in normalised coords; the panel
    /// uses it to compute the drag's delta-from-press origin.
    ItemPressed { item: usize, pos: [f32; 2] },
    /// Drag-create: the user pressed in empty canvas, dragged past the
    /// click threshold, and released. The panel creates a new shape
    /// from these inputs. Primitive tools use `from` + `to` as the
    /// rectangle's two corners; the spline tool uses the full `path`
    /// (subsampled along the drag) as control points.
    CreateAt {
        from: [f32; 2],
        to: [f32; 2],
        path: Vec<[f32; 2]>,
    },
}

/// Coord transform between normalised [0..1] and canvas-pixel space.
/// The user draw callback receives one and uses it to project points
/// for painting.
#[derive(Clone, Copy, Debug)]
pub struct CanvasTransform {
    /// Canvas-rect origin in screen pixels.
    pub origin: egui::Pos2,
    /// Pan offset, in canvas pixels.
    pub pan: egui::Vec2,
    /// Canvas pixels per normalised unit.
    pub zoom: f32,
}

impl CanvasTransform {
    pub fn to_pixel(self, norm: [f32; 2]) -> egui::Pos2 {
        self.origin + self.pan + egui::vec2(norm[0] * self.zoom, norm[1] * self.zoom)
    }

    pub fn to_norm(self, pixel: egui::Pos2) -> [f32; 2] {
        let dx = pixel.x - self.origin.x - self.pan.x;
        let dy = pixel.y - self.origin.y - self.pan.y;
        [dx / self.zoom.max(1e-6), dy / self.zoom.max(1e-6)]
    }
}

/// Paint the canvas and handles, process input, return any gestures
/// the caller should apply this frame. The widget allocates its own
/// rect and interaction Response (with `Sense::click_and_drag`) so
/// pointer events for the canvas live on a single Response. An earlier
/// version of this API had the caller allocate first and the widget
/// allocate again; egui routed drag events inconsistently across the
/// two Responses, which broke drag detection on every handle.
pub fn draw<F, H>(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    state: &mut CanvasState,
    handles: &[HandleSpec],
    draw_items: F,
    item_hit_test: H,
) -> Vec<CanvasGesture>
where
    F: FnOnce(&egui::Painter, &CanvasTransform),
    H: Fn([f32; 2]) -> Option<usize>,
{
    let mut gestures = Vec::new();
    let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
    let rect = response.rect;

    // Auto-fit zoom on first frame.
    if state.zoom <= 0.0 {
        state.zoom = rect.width().min(rect.height());
        state.pan = egui::vec2(
            (rect.width() - state.zoom) * 0.5,
            (rect.height() - state.zoom) * 0.5,
        );
    }

    // ── Pan + zoom navigation ──────────────────────────────────────
    // Middle-mouse drag pans the canvas. Scroll wheel zooms with the
    // cursor as the anchor so authoring fine detail on large maps
    // doesn't fling the area you're looking at off-screen. Performed
    // before xform is captured so the rest of the frame sees the
    // updated pan / zoom immediately.
    if response.dragged_by(egui::PointerButton::Middle) {
        state.pan += response.drag_delta();
    }
    if let Some(cursor) = response.hover_pos() {
        let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            // Cursor position in normalised canvas coords before zoom.
            let dx = cursor.x - rect.min.x - state.pan.x;
            let dy = cursor.y - rect.min.y - state.pan.y;
            let cursor_norm = [dx / state.zoom.max(1e-6), dy / state.zoom.max(1e-6)];
            let factor = (1.0 + scroll * 0.0015).clamp(0.7, 1.4);
            // Bounds: don't zoom out below the auto-fit size, don't
            // zoom in beyond ~50x.
            let min_zoom = (rect.width().min(rect.height())) * 0.4;
            let max_zoom = (rect.width().min(rect.height())) * 50.0;
            let new_zoom = (state.zoom * factor).clamp(min_zoom, max_zoom);
            // Adjust pan so cursor_norm stays under the cursor pixel.
            state.pan += egui::vec2(cursor_norm[0], cursor_norm[1]) * (state.zoom - new_zoom);
            state.zoom = new_zoom;
        }
    }

    let xform = CanvasTransform {
        origin: rect.min,
        pan: state.pan,
        zoom: state.zoom,
    };

    // Background.
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

    // Logical [0..1] frame -- the canvas content area. Drawn with a
    // mid-tone stroke so the author can see where the map edge sits
    // regardless of zoom / pan.
    let frame_min = xform.to_pixel([0.0, 0.0]);
    let frame_max = xform.to_pixel([1.0, 1.0]);
    let frame_rect = egui::Rect::from_min_max(frame_min, frame_max);
    painter.rect_filled(frame_rect, 0.0, ui.visuals().panel_fill);
    painter.rect_stroke(
        frame_rect,
        0.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color),
        egui::epaint::StrokeKind::Inside,
    );

    // Grid lines at every 0.1; thicker at 0.5.
    let grid_minor = egui::Stroke::new(
        0.5,
        ui.visuals()
            .widgets
            .noninteractive
            .fg_stroke
            .color
            .gamma_multiply(0.35),
    );
    let grid_major = egui::Stroke::new(
        1.0,
        ui.visuals()
            .widgets
            .noninteractive
            .fg_stroke
            .color
            .gamma_multiply(0.55),
    );
    for i in 1..10 {
        let n = i as f32 / 10.0;
        let x = xform.to_pixel([n, 0.0]).x;
        painter.line_segment(
            [egui::pos2(x, frame_min.y), egui::pos2(x, frame_max.y)],
            if i == 5 { grid_major } else { grid_minor },
        );
        let y = xform.to_pixel([0.0, n]).y;
        painter.line_segment(
            [egui::pos2(frame_min.x, y), egui::pos2(frame_max.x, y)],
            if i == 5 { grid_major } else { grid_minor },
        );
    }

    // Caller paints its items now -- before handles so handles sit on
    // top regardless of caller paint order.
    draw_items(&painter, &xform);

    // Handle hit-test helper (used by both rendering for hover state
    // and the interaction logic below).
    let handle_hit_test = |p: egui::Pos2| -> Option<&HandleSpec> {
        handles.iter().rev().find(|h| {
            let hp = xform.to_pixel(h.pos);
            (hp - p).length() <= h.px_radius
        })
    };

    // Pointer position over the canvas this frame (None if outside).
    let pointer_inside = response.hover_pos();
    let hovered_handle = pointer_inside.and_then(handle_hit_test);
    let hovered_item_from_body: Option<usize> = match (pointer_inside, hovered_handle) {
        (Some(p), None) => item_hit_test(xform.to_norm(p)),
        _ => None,
    };

    // Render handles. Each `HandleKind` has its own silhouette + base
    // colour. A drag-in-progress or hover overlays an outer ring so the
    // active / about-to-act handle reads at a glance.
    for h in handles {
        let p = xform.to_pixel(h.pos);
        let drag_active = state
            .drag
            .as_ref()
            .map(|d| d.item == h.item && d.handle == h.id)
            .unwrap_or(false);
        let is_hovered = hovered_handle
            .map(|hh| hh.item == h.item && hh.id == h.id)
            .unwrap_or(false);
        draw_handle(&painter, p, h.kind, h.px_radius, drag_active, is_hovered);
    }

    // Cursor feedback. While dragging: Grabbing. Hovering a specific
    // handle: that handle's declared cursor (so resize corners show
    // resize cursors, rotation shows a crosshair, etc.). Hovering an
    // unselected shape's body: PointingHand to advertise the click
    // selects. Otherwise: default.
    if pointer_inside.is_some() {
        let cursor = if state.drag.is_some() {
            egui::CursorIcon::Grabbing
        } else if let Some(h) = hovered_handle {
            h.cursor
        } else if hovered_item_from_body.is_some() {
            egui::CursorIcon::PointingHand
        } else {
            egui::CursorIcon::Default
        };
        if cursor != egui::CursorIcon::Default {
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    // Right-click delete (one-shot, requires a handle hit).
    if response.secondary_clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            if let Some(h) = handle_hit_test(p) {
                gestures.push(CanvasGesture::HandleDeleted {
                    item: h.item,
                    handle: h.id,
                });
            }
        }
    }

    // Press-based interaction. The press itself selects the handle and
    // arms a potential drag -- exactly the "press grabs the shape"
    // behaviour an author expects, rather than waiting for mouseup to
    // select. egui's `drag_started` fires only after the cursor
    // crosses a movement threshold, by which point it has drifted off
    // the handle and the hit-test misses; detecting the press at the
    // first "button down on this response" frame catches it precisely.
    let down_on_canvas = response.is_pointer_button_down_on();
    if down_on_canvas && state.drag.is_none() && state.creation.is_none() {
        if let Some(p) = response.interact_pointer_pos() {
            if let Some(h) = handle_hit_test(p) {
                let pos = xform.to_norm(p);
                state.drag = Some(DragInProgress {
                    item: h.item,
                    handle: h.id,
                    press_pos: pos,
                    moved: false,
                });
                state.selected = Some(h.item);
                gestures.push(CanvasGesture::HandlePressed {
                    item: h.item,
                    handle: h.id,
                    pos,
                });
            } else {
                let pos = xform.to_norm(p);
                let inside_frame = (0.0..=1.0).contains(&pos[0]) && (0.0..=1.0).contains(&pos[1]);
                if let Some(item) = item_hit_test(pos) {
                    // Press on an item body. Emit `ItemPressed` -- the
                    // panel selects + (for primitives) sets up a
                    // body-press centre drag so the user can press +
                    // immediately drag without a separate select click.
                    gestures.push(CanvasGesture::ItemPressed { item, pos });
                } else if inside_frame {
                    // Empty inside the frame: drag-to-create.
                    state.creation = Some(CreationDrag {
                        press_pos: pos,
                        current_pos: pos,
                        path: vec![pos],
                        moved: false,
                    });
                }
            }
        }
    }

    if let Some(mut drag) = state.drag.clone() {
        if down_on_canvas {
            if let Some(p) = response.interact_pointer_pos() {
                let pos = xform.to_norm(p);
                // Only treat it as a drag once the cursor leaves a
                // small dead-zone around the press point -- otherwise
                // a slightly-jittery click would register as an edit.
                let dx = pos[0] - drag.press_pos[0];
                let dy = pos[1] - drag.press_pos[1];
                if !drag.moved && (dx * dx + dy * dy) > 0.002 * 0.002 {
                    drag.moved = true;
                    state.drag = Some(drag.clone());
                }
                if drag.moved {
                    gestures.push(CanvasGesture::HandleDragged {
                        item: drag.item,
                        handle: drag.handle,
                        pos,
                    });
                }
            }
        } else {
            // Released. `moved` tells the panel whether to keep the
            // edit (commit undo) or treat it as a select-only click
            // (discard the stashed snapshot).
            gestures.push(CanvasGesture::HandleReleased {
                item: drag.item,
                handle: drag.handle,
                moved: drag.moved,
            });
            state.drag = None;
        }
    }

    // Drag-to-create lifecycle. Started in the empty-area branch of
    // the press logic above. Tracks the cursor while held and renders
    // a translucent preview rect once the press has moved past a
    // small dead-zone. On release, emits CreateAt (with rect) if the
    // gesture was a real drag, or AddAt (no rect) if it was a click
    // without movement -- the panel uses AddAt for spline-point
    // addition and ignores it otherwise.
    if let Some(mut creation) = state.creation.clone() {
        if down_on_canvas {
            if let Some(p) = response.interact_pointer_pos() {
                let pos = xform.to_norm(p);
                creation.current_pos = pos;
                let dx = pos[0] - creation.press_pos[0];
                let dy = pos[1] - creation.press_pos[1];
                if !creation.moved && (dx * dx + dy * dy) > 0.01 * 0.01 {
                    creation.moved = true;
                }
                // Subsample the cursor path so freehand spline draws
                // get a usable polyline without ballooning to one
                // point per frame. ~0.005 normalised units between
                // samples reads as smooth at typical canvas sizes.
                let last = creation.path.last().copied().unwrap_or(creation.press_pos);
                let dx2 = pos[0] - last[0];
                let dy2 = pos[1] - last[1];
                if (dx2 * dx2 + dy2 * dy2) > 0.005 * 0.005 {
                    creation.path.push(pos);
                }
                state.creation = Some(creation.clone());
            }
            // The shape-specific preview silhouette is painted by the
            // caller's draw_items closure, which knows what kind it
            // will create. The widget itself is shape-agnostic.
        } else {
            // Released. A real drag emits CreateAt with the rect + the
            // full path; a press-without-drag emits AddAt at the press
            // point (the panel only acts on AddAt for spline-point
            // addition).
            if creation.moved {
                // Ensure the path ends at the release position.
                let last = creation.path.last().copied().unwrap_or(creation.press_pos);
                if last != creation.current_pos {
                    creation.path.push(creation.current_pos);
                }
                gestures.push(CanvasGesture::CreateAt {
                    from: creation.press_pos,
                    to: creation.current_pos,
                    path: creation.path,
                });
            } else {
                gestures.push(CanvasGesture::AddAt {
                    pos: creation.press_pos,
                });
            }
            state.creation = None;
        }
    }

    // All left-click resolution is now handled by the press path:
    // handle hits start handle drags, item-body presses emit
    // `ItemPressed` (panel selects + sets up a body drag), empty
    // presses start a creation drag. `clicked_by(Primary)` would
    // double-fire after a press-without-drag, so it's deliberately
    // not used here.

    gestures
}

/// Draw one handle in its kind's signature silhouette. The drag /
/// hover overlay is a yellow outer ring -- the same affordance every
/// kind uses so authors learn one "this is the active one" cue.
fn draw_handle(
    painter: &egui::Painter,
    p: egui::Pos2,
    kind: HandleKind,
    radius: f32,
    drag_active: bool,
    hovered: bool,
) {
    let outline = egui::Stroke::new(1.0, egui::Color32::from_black_alpha(200));
    match kind {
        HandleKind::Centre => {
            let fill = egui::Color32::from_rgb(90, 170, 255);
            painter.circle_filled(p, radius, fill);
            painter.circle_stroke(p, radius, outline);
        }
        HandleKind::Corner => {
            let fill = egui::Color32::from_rgb(140, 220, 140);
            let side = radius * 1.7;
            let rect = egui::Rect::from_center_size(p, egui::vec2(side, side));
            painter.rect_filled(rect, 1.0, fill);
            painter.rect_stroke(rect, 1.0, outline, egui::epaint::StrokeKind::Inside);
        }
        HandleKind::Rotation => {
            let ring = egui::Color32::from_rgb(255, 130, 200);
            painter.circle_stroke(p, radius, egui::Stroke::new(2.0, ring));
            painter.circle_filled(p, radius * 0.35, ring);
        }
        HandleKind::SplinePoint => {
            let fill = egui::Color32::from_rgb(160, 225, 225);
            let d = radius;
            let pts = vec![
                egui::pos2(p.x, p.y - d),
                egui::pos2(p.x + d, p.y),
                egui::pos2(p.x, p.y + d),
                egui::pos2(p.x - d, p.y),
            ];
            painter.add(egui::Shape::convex_polygon(pts, fill, outline));
        }
    }
    if drag_active || hovered {
        let ring = egui::Color32::from_rgb(255, 200, 60);
        painter.circle_stroke(p, radius + 3.0, egui::Stroke::new(1.5, ring));
    }
}
