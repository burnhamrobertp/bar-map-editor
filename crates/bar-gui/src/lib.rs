//! # bar-gui
//!
//! Node-based editor GUI for BAR map editor, built on egui.
//! Provides the node graph editor, property panels, and viewport integration.

pub mod app;
pub(crate) mod dialog;
pub mod editor;
pub mod i18n;
pub mod io;
pub mod layouts;
pub mod macros;
pub(crate) mod paint;
pub mod panels;
pub mod project;
pub mod settings;
pub mod state;
pub mod undo;

pub use app::{BarEditorApp, BrushTarget, ExportStatus, Layout, SmfLightingSnapshot};
pub use io::ParentWindow;
pub use settings::{Settings, WindowState};
pub use undo::{Snapshot, UndoHistory};
