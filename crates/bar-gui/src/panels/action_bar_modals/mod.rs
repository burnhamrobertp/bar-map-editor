//! Action-bar modal panels.
//!
//! Each `pub mod` in this directory is a top-level modal opened by
//! one of the action-bar buttons (Identity / Dimensions / Physics /
//! Atmosphere / Lighting / Water / Resources / Grass / Map Edge /
//! Start Boxes). They share the same opening / commit / undo
//! semantics via the helpers in [`shared`]:
//!
//! * [`shared::modal_frame`] wraps the egui `Window` + `ScrollArea`
//!   + scrollbar clearance boilerplate every modal uses.
//! * [`shared::render_specs`] iterates a `FieldSpec` slice from the
//!   schema, inserting sub-section headings on group transitions
//!   and threading validation findings + intent dispatch through
//!   the generic `field_editor::render_field`.
//! * [`shared::drive_text_edit_intent`] / [`shared::drive_drag_intent`]
//!   give the bespoke widgets (text fields outside the schema,
//!   custom drag rows) the same atomic-commit + undo behaviour.
//!
//! Each modal owns its own `show_*_editor: bool` on `DialogState`;
//! `BarEditorApp::draw_action_bar_modals` calls them all and the
//! ones whose flag is `false` short-circuit immediately.

pub mod shared;

pub mod atmosphere;
pub mod dimensions;
pub mod grass;
pub mod identity;
pub mod lighting;
pub mod map_edge;
pub mod physics;
pub mod resources;
pub mod start_boxes;
pub mod water;
