//! External I/O integration for the editor.
//!
//! - `png` -- heightmap and color buffer PNG load/save.
//! - `dialogs` -- native file-dialog spawning + parent-window handling
//!   (lands in Stage 2 of the architecture refactor).
//!
//! Pure I/O. Anything that touches `BarEditorApp` state lives elsewhere
//! (see `crate::project::lifecycle`).

pub(crate) mod dialogs;
pub(crate) mod png;

pub use dialogs::ParentWindow;

/// Quick filename-extension check used by the inline file editor and
/// project-asset packing to decide whether a path is "text-like" and
/// should be embedded verbatim vs. copied as a binary asset.
pub(crate) fn is_text_file(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "lua"
            | "cfg"
            | "txt"
            | "md"
            | "json"
            | "toml"
            | "ini"
            | "conf"
            | "xml"
            | "yaml"
            | "yml"
            | "sh"
            | "py"
    )
}
