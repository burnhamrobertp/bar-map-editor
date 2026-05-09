//! Native file-dialog spawning, parented to the editor window.
//!
//! `ParentWindow` is the cross-thread wrapper that lets a worker
//! thread invoke `rfd::FileDialog::set_parent` without holding a
//! live borrow of `eframe::Frame`. `BarEditorApp` exposes builder
//! methods that all dialog call sites should go through, so dialogs
//! consistently belong to the editor's window (right icon, right
//! focus return) rather than whichever OS window happens to be
//! foreground when the dialog spawns.

use crate::app::BarEditorApp;
use bar_graph::NodeType;

/// Build a file-picker dialog appropriate for the given node type's
/// `path` param. The dialog is parented to the editor window when a
/// handle has been pushed via `BarEditorApp::set_parent_window_handles`.
pub(crate) fn make_path_dialog(app: &BarEditorApp, node_type: &NodeType) -> rfd::FileDialog {
    let base = app.make_dialog();
    match node_type {
        NodeType::SmfImport => base
            .set_title("Select .smf Map File")
            .add_filter("Spring Map File", &["smf"]),
        NodeType::SmtImport => base
            .set_title("Select .smt Tile File")
            .add_filter("Spring Map Tiles", &["smt"]),
        NodeType::FileInput => base
            .set_title("Select Image File")
            .add_filter("Image", &["png", "tiff", "tif", "jpg", "jpeg"]),
        _ => base.set_title("Select File"),
    }
}

/// Owned wrapper that re-implements `HasWindowHandle` +
/// `HasDisplayHandle` for previously captured raw handles. Lets us
/// pass a parent handle to rfd from a worker thread without holding
/// a live borrow of `eframe::Frame`. Public so `bar-app` can build
/// one for its own dialogs (folder picker in the export flow)
/// without re-implementing the same pattern.
pub struct ParentWindow {
    pub window: raw_window_handle::RawWindowHandle,
    pub display: raw_window_handle::RawDisplayHandle,
}

// SAFETY: The raw handle types are conservatively `!Send` because some
// of their variants embed raw pointers. We only ever use a handle as
// an opaque token passed to OS file-dialog APIs (which accept handles
// from any thread by contract), and never dereference the embedded
// pointer or read the underlying window memory. Crossing the
// `thread::spawn` boundary with the wrapper is therefore safe.
unsafe impl Send for ParentWindow {}
unsafe impl Sync for ParentWindow {}

impl ParentWindow {
    pub fn new(
        window: raw_window_handle::RawWindowHandle,
        display: raw_window_handle::RawDisplayHandle,
    ) -> Self {
        Self { window, display }
    }
}

impl raw_window_handle::HasWindowHandle for ParentWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: the underlying OS window must outlive any dialog
        // built with this parent. In our app, the editor's main
        // window exists for the entire process lifetime, and dialog
        // calls always complete before shutdown -- so any handle we
        // captured during update() is still valid here.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(self.window) })
    }
}

impl raw_window_handle::HasDisplayHandle for ParentWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: same lifetime argument as above -- the underlying
        // OS display connection lives for the process.
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(self.display) })
    }
}

impl BarEditorApp {
    /// Update the cached parent window + display handles. Called each
    /// frame by `bar-app` from `eframe::Frame`. Native file dialogs
    /// read this so the OS knows which window to attach to.
    pub fn set_parent_window_handles(
        &mut self,
        handles: Option<(
            raw_window_handle::RawWindowHandle,
            raw_window_handle::RawDisplayHandle,
        )>,
    ) {
        self.parent_window_handles = handles;
    }

    /// Build a `ParentWindow` for the current frame's handles, if any.
    /// Useful from `bar-app` and other crates that want to spawn their
    /// own dialogs while preserving correct parenting.
    pub fn parent_window(&self) -> Option<ParentWindow> {
        self.parent_window_handles
            .map(|(w, d)| ParentWindow::new(w, d))
    }

    /// Build an rfd::FileDialog already parented to the editor window
    /// (when handles are available). Use this everywhere instead of
    /// `rfd::FileDialog::new()` so dialogs don't latch onto whichever
    /// window happens to be foreground.
    pub(crate) fn make_dialog(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match self.parent_window() {
            Some(parent) => dialog.set_parent(&parent),
            None => dialog,
        }
    }

    /// Spawn the Open file dialog on a worker thread so the egui
    /// main loop can keep rendering while the OS dialog is up. The
    /// result lands in `project.pending_open_rx` which `update` polls
    /// each frame. No-op if a dialog is already in flight.
    pub(crate) fn open_file_dialog_async(&mut self) {
        if self.project.pending_open_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let parent = self.parent_window();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Open")
                .add_filter("Supported Files", &["barproj", "sd7"])
                .add_filter("BAR Map Editor Project", &["barproj"])
                .add_filter("Spring Map Archive", &["sd7"]);
            if let Some(parent) = &parent {
                dialog = dialog.set_parent(parent);
            }
            let path = dialog.pick_file();
            let _ = tx.send(path);
        });
        self.project.pending_open_rx = Some(rx);
    }
}
