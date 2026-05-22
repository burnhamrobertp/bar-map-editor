//! Assemble Map wizard -- a guided flow that takes the discrete files
//! a map-maker has on disk (heightmap, diffuse texture, metalmap,
//! optional resources) and lands them in a ready-to-save `.barproj`
//! with the matching `MapSettings` populated.
//!
//! The wizard avoids ever asking the user to wire nodes by hand: on
//! Finish it builds the same graph shape an `.sd7` import produces
//! (FinalComposition + input nodes per provided file) and copies the
//! picked files into `passthrough/`. Everything optional defaults to
//! empty (engine fallback / auto-generate at bundle time).

pub mod build;
pub mod state;
mod wizard;

mod extras;
mod heightmap;
mod identity;
mod surface;

pub use state::AssembleMapState;
pub use wizard::draw;

use eframe::egui;

/// Single-line optional-file picker row used by the Surface and
/// Extras pages. Renders `label + (Browse/Clear) + filename hint`,
/// drives a native file dialog with the given extensions, and writes
/// the picked absolute path into `slot`. No validation -- the Finish
/// handler decodes everything once.
pub(super) fn file_row(
    ui: &mut egui::Ui,
    label: &str,
    extensions: &[&str],
    slot: &mut Option<std::path::PathBuf>,
    parent: Option<&crate::io::dialogs::ParentWindow>,
    dialog_title: &str,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        if ui.small_button("Browse...").clicked() {
            let mut dialog = rfd::FileDialog::new().set_title(dialog_title);
            if let Some(parent) = parent {
                dialog = dialog.set_parent(parent);
            }
            if !extensions.is_empty() {
                dialog = dialog.add_filter("Files", extensions);
            }
            if let Some(picked) = dialog.pick_file() {
                *slot = Some(picked);
            }
        }
        if slot.is_some() && ui.small_button("Clear").clicked() {
            *slot = None;
        }
        match slot.as_ref().and_then(|p| p.file_name()) {
            Some(name) => {
                ui.label(name.to_string_lossy().into_owned());
            }
            None => {
                ui.weak("(skip)");
            }
        }
    });
}
