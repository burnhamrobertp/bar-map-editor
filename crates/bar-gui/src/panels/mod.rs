//! UI panels — stateless renderers over `BarEditorApp`.
//!
//! A panel is a self-contained piece of UI: the welcome screen, the
//! node palette, the properties popup, the validation sidebar, the
//! 2D inspector, and so on. Panels never own state of their own —
//! they read and write `BarEditorApp` fields. That keeps the
//! ownership story simple (one struct, one lifetime) and means a
//! panel can be reused across multiple `Layout`s without any
//! coordination.
//!
//! New panels go here as `pub mod foo;` plus their entry point —
//! either a free `pub(crate) fn draw(app: &mut BarEditorApp, ...)`
//! or an `impl BarEditorApp { pub(crate) fn draw_foo(...) }` block.
//! Either pattern is fine; pick whichever makes the call sites
//! cleaner.

pub mod dialogs;
pub mod icons;
pub mod inspector;
pub mod mapinfo_editor;
pub mod canvas;
pub mod palette;
pub mod properties;
pub mod tokens;
pub mod validation;
pub mod welcome;
