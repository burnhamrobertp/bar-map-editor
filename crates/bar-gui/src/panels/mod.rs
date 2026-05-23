//! UI panels -- stateless renderers over `BarEditorApp`.
//!
//! A panel is a self-contained piece of UI: the welcome screen, the
//! node palette, the properties popup, the validation sidebar, the
//! 2D inspector, and so on. Panels never own state of their own --
//! they read and write `BarEditorApp` fields. That keeps the
//! ownership story simple (one struct, one lifetime) and means a
//! panel can be reused across multiple `Layout`s without any
//! coordination.
//!
//! The action-bar modals (Identity / Dimensions / Physics /
//! Atmosphere / Lighting / Water / Resources / Grass / Map Edge /
//! Start Boxes) live under [`action_bar_modals`] -- they share an
//! opening / commit / undo pipeline and only the action-bar
//! buttons toggle them. Everything else here is a layout panel
//! (welcome / palette / properties / canvas / etc.) or a shared
//! widget primitive (icons / tokens / file picker / field editor).

pub mod action_bar_modals;
pub mod assemble_map;

pub mod canvas;
pub mod dialogs;
pub mod feature_library;
pub mod feature_popover;
pub mod field_editor;
pub mod file_picker;
pub mod icons;
pub mod image_preview;
pub mod inspector;
pub mod log;
pub mod opt_field;
pub mod palette;
pub mod properties;
pub mod tokens;
pub mod validation;
pub mod welcome;
pub(crate) mod widgets;
