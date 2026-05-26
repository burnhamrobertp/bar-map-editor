//! Shared 2D canvas widget for properties-panel item editing.
//!
//! Used by node types that author 2D data (`LayoutGenerator` shapes,
//! `SplineLayout` control points). The widget owns the canvas chrome
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

/// One draggable handle the calling panel wants the widget to know
/// about. Handles are listed every frame -- the panel computes their
/// positions from its own data, then hands the list over.
#[derive(Clone, Debug)]
pub struct HandleSpec {
    pub item: usize,
    pub id: HandleId,
    /// Normalised position. The widget hit-tests this in pixel space
    /// against the cursor with the configured radius.
    pub pos: [f32; 2],
    /// Pixel hit-test radius. Lets small handles (rotation arms) be
    /// distinguished from big handles (shape centres).
    pub px_radius: f32,
}

/// Gestures emitted by the widget. The panel matches on these and
/// mutates its own data accordingly. Several variants carry fields
/// the LayoutGenerator panel doesn't read but the SplineLayout
/// panel does (or will); the `dead_code` allow lets the public API
/// stay uniform without per-consumer noise.
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
    /// Right-click on a handle. Panel removes the item.
    HandleDeleted { item: usize },
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
pub fn draw<F>(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    state: &mut CanvasState,
    handles: &[HandleSpec],
    draw_items: F,
) -> Vec<CanvasGesture>
where
    F: FnOnce(&egui::Painter, &CanvasTransform),
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

    // Handles: paint each as a small disc with selection highlight.
    let selected_color = egui::Color32::from_rgb(255, 200, 60);
    let normal_color = ui.visuals().widgets.inactive.fg_stroke.color;
    for h in handles {
        let p = xform.to_pixel(h.pos);
        let selected_item = state.selected == Some(h.item);
        let drag_active = state
            .drag
            .as_ref()
            .map(|d| d.item == h.item && d.handle == h.id)
            .unwrap_or(false);
        let colour = if selected_item || drag_active {
            selected_color
        } else {
            normal_color
        };
        painter.circle_filled(p, h.px_radius, colour);
        painter.circle_stroke(
            p,
            h.px_radius,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(180)),
        );
    }

    // Helper: find the topmost handle whose hit-circle contains `p`.
    let hit_test = |p: egui::Pos2| -> Option<&HandleSpec> {
        handles.iter().rev().find(|h| {
            let hp = xform.to_pixel(h.pos);
            (hp - p).length() <= h.px_radius
        })
    };

    // Right-click delete (one-shot, requires a handle hit).
    if response.secondary_clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            if let Some(h) = hit_test(p) {
                gestures.push(CanvasGesture::HandleDeleted { item: h.item });
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
    if down_on_canvas && state.drag.is_none() {
        if let Some(p) = response.interact_pointer_pos() {
            if let Some(h) = hit_test(p) {
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

    // Left-click on empty canvas adds an item. Handle hits are
    // consumed by the press path above (which sets state.selected),
    // so AddAt only fires for clicks that miss every handle and land
    // inside the [0..1] frame.
    if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(p) = response.interact_pointer_pos() {
            if hit_test(p).is_none() {
                let pos = xform.to_norm(p);
                if (0.0..=1.0).contains(&pos[0]) && (0.0..=1.0).contains(&pos[1]) {
                    gestures.push(CanvasGesture::AddAt { pos });
                }
            }
        }
    }

    gestures
}
