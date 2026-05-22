//! Reusable file-picker row.
//!
//! A `FilePickerField` is a label + filename display + Browse/Clear
//! buttons that mediates a `String` value representing a filename
//! inside the project's `.barproj` directory. When the user picks a
//! file via the native dialog, the file is copied into a configurable
//! subdirectory of the project (so the asset is part of the .barproj
//! on next save) and the basename is stored in the mediated string.
//!
//! There's no text input for typing a path directly: a typed-by-hand
//! string isn't reachable through Browse / Clear so it would drift
//! from the actual file on disk. The filename is shown as a plain
//! label; users edit it via Browse / Clear or by hand-editing the
//! project's `passthrough/` directory.
//!
//! Each call site configures:
//!   * `label`: the form-field label rendered to the left.
//!   * `subdir`: the path *inside the project* to copy chosen files to
//!     (e.g. `"passthrough"`, or `"passthrough/maps"`).
//!   * `extensions`: filter for the native file dialog (without dots,
//!     e.g. `["dds", "tga", "png"]`).
//!   * `dialog_title`: the OS file-dialog title.
//!   * `allow_clear`: whether to show a "Clear" button that empties
//!     the value (useful for optional textures where empty means
//!     "fall back to engine default").
//!   * `hint_text`: placeholder shown in the text input when empty.
//!
//! `show()` returns `true` when the value changed (text edit OR file
//! pick OR clear) so callers can drive their own dirty-tracking /
//! undo bookkeeping.
//!
//! The filesystem half (computing the destination path, copying the
//! file when needed) lives in [`copy_into_project`] -- a pure function
//! with no egui dependency that the unit tests cover directly.

use std::path::Path;

use eframe::egui;

use crate::io::dialogs::ParentWindow;

/// Builder describing one file-picker row's appearance and behaviour.
/// Cheap to construct per-frame; holds no state of its own.
pub(crate) struct FilePickerField<'a> {
    label: &'a str,
    subdir: &'a str,
    extensions: &'a [&'a str],
    dialog_title: &'a str,
    allow_clear: bool,
    hint_text: &'a str,
}

impl<'a> FilePickerField<'a> {
    /// Build a new field with the given label and project-relative
    /// subdirectory. Other knobs default to "any image type, clearing
    /// not allowed, generic dialog title".
    pub(crate) fn new(label: &'a str, subdir: &'a str) -> Self {
        Self {
            label,
            subdir,
            extensions: &[],
            dialog_title: "Select file",
            allow_clear: false,
            hint_text: "(unset)",
        }
    }

    pub(crate) fn extensions(mut self, exts: &'a [&'a str]) -> Self {
        self.extensions = exts;
        self
    }

    pub(crate) fn title(mut self, title: &'a str) -> Self {
        self.dialog_title = title;
        self
    }

    pub(crate) fn allow_clear(mut self, allow: bool) -> Self {
        self.allow_clear = allow;
        self
    }

    pub(crate) fn hint(mut self, hint: &'a str) -> Self {
        self.hint_text = hint;
        self
    }

    /// Render the row. Returns true when Browse or Clear changed
    /// `value`.
    ///
    /// `parent` is the editor's main-window handle; supplying it parents
    /// the OS file dialog to BME so it behaves as a child window
    /// instead of latching onto whichever process happens to be
    /// foreground (e.g. the terminal that launched `cargo watch`).
    /// Callers should pass `app.parent_window().as_ref()`.
    pub(crate) fn show(
        &self,
        ui: &mut egui::Ui,
        value: &mut String,
        project_dir: Option<&Path>,
        parent: Option<&ParentWindow>,
    ) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(self.label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.allow_clear && !value.is_empty() {
                    let resp = ui
                        .small_button("Clear")
                        .on_hover_text("Reset to engine default");
                    if resp.clicked() {
                        value.clear();
                        changed = true;
                    }
                }
                if ui.small_button("Browse...").clicked() {
                    let mut dialog = rfd::FileDialog::new().set_title(self.dialog_title);
                    if let Some(parent) = parent {
                        dialog = dialog.set_parent(parent);
                    }
                    if !self.extensions.is_empty() {
                        dialog = dialog.add_filter("Files", self.extensions);
                    }
                    if let Some(picked) = dialog.pick_file() {
                        if let Some(project) = project_dir {
                            match copy_into_project(&picked, project, self.subdir) {
                                Ok(basename) => {
                                    *value = basename;
                                    changed = true;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        err = %e,
                                        src = %picked.display(),
                                        subdir = %self.subdir,
                                        "FilePickerField: failed to copy into project"
                                    );
                                }
                            }
                        } else if let Some(name) = picked
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|s| s.to_string())
                        {
                            // No project dir resolved yet (pre-save
                            // edge case): store the basename
                            // optimistically; the next save will
                            // notice the file isn't in passthrough/
                            // and either pick it up or surface a
                            // validation warning.
                            *value = name;
                            changed = true;
                        }
                    }
                }
                // Filename display. Italic / weak when unset so the
                // hint reads as placeholder; strong when a real
                // value is stored.
                if value.is_empty() {
                    ui.label(egui::RichText::new(self.hint_text).weak().italics());
                } else {
                    ui.label(value.as_str()).on_hover_text(value.as_str());
                }
            });
        });
        changed
    }
}

/// Pure-function half of [`FilePickerField::show`]'s Browse handler:
/// given a source file picked from anywhere on disk, the project root,
/// and the project-relative subdirectory the file should land in,
/// copy the source into the project (creating the subdir if needed)
/// and return the basename.
///
/// Idempotent: if the source IS the destination, no copy happens. The
/// returned basename is what callers store in mapinfo / recipe fields
/// (BAR mapinfo strings are always basenames, resolved by the loader
/// under the appropriate subdirectory).
///
/// Errors:
///   * source has no filename component (e.g. ends in `..`).
///   * IO error creating the destination subdir or copying the file.
pub(crate) fn copy_into_project(
    source: &Path,
    project_dir: &Path,
    subdir: &str,
) -> std::io::Result<String> {
    let basename = source
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "source path has no filename component",
            )
        })?
        .to_string();
    let dst_dir = project_dir.join(subdir);
    let dst = dst_dir.join(&basename);
    if source != dst {
        std::fs::create_dir_all(&dst_dir)?;
        std::fs::copy(source, &dst)?;
    }
    Ok(basename)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: unique scratch dir under the system temp root. Each test
    /// gets its own so they don't race when run in parallel.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bme-file-picker-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_external_file_into_project_subdir() {
        let workspace = scratch("external");
        let src_dir = workspace.join("downloads");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("my_grass.tga");
        std::fs::write(&src, b"fake grass mask payload").unwrap();
        let project = workspace.join("project.barproj");
        std::fs::create_dir_all(&project).unwrap();

        let basename = copy_into_project(&src, &project, "passthrough").unwrap();
        assert_eq!(basename, "my_grass.tga");
        let copied = project.join("passthrough").join("my_grass.tga");
        assert!(copied.is_file(), "expected file at {copied:?}");
        // Source is left untouched.
        assert!(src.is_file());
        // Destination contents match the source.
        assert_eq!(std::fs::read(&copied).unwrap(), b"fake grass mask payload");
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn creates_subdir_when_missing() {
        let workspace = scratch("subdir-missing");
        let src = workspace.join("src.dds");
        std::fs::write(&src, b"x").unwrap();
        let project = workspace.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        // Note: passthrough/ does NOT exist yet.
        assert!(!project.join("passthrough").exists());

        let basename = copy_into_project(&src, &project, "passthrough").unwrap();
        assert_eq!(basename, "src.dds");
        assert!(project.join("passthrough").is_dir());
        assert!(project.join("passthrough").join("src.dds").is_file());
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn supports_nested_subdir() {
        let workspace = scratch("nested");
        let src = workspace.join("foo.png");
        std::fs::write(&src, b"x").unwrap();
        let project = workspace.join("proj");
        std::fs::create_dir_all(&project).unwrap();

        // e.g., the "maps/textures" convention some grass widgets use.
        let basename = copy_into_project(&src, &project, "passthrough/maps").unwrap();
        assert_eq!(basename, "foo.png");
        assert!(project
            .join("passthrough")
            .join("maps")
            .join("foo.png")
            .is_file());
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn idempotent_when_source_equals_destination() {
        let workspace = scratch("idempotent");
        let project = workspace.join("proj");
        let dst_dir = project.join("passthrough");
        std::fs::create_dir_all(&dst_dir).unwrap();
        let dst = dst_dir.join("already_here.dds");
        std::fs::write(&dst, b"pre-existing content").unwrap();
        // Capture mtime so we can verify we didn't touch the file.
        let mtime_before = std::fs::metadata(&dst).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        let basename = copy_into_project(&dst, &project, "passthrough").unwrap();
        assert_eq!(basename, "already_here.dds");
        let mtime_after = std::fs::metadata(&dst).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "destination was rewritten when it should have been skipped"
        );
        assert_eq!(std::fs::read(&dst).unwrap(), b"pre-existing content");
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn overwrites_destination_if_different_source() {
        let workspace = scratch("overwrite");
        let project = workspace.join("proj");
        let dst_dir = project.join("passthrough");
        std::fs::create_dir_all(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("foo.tga"), b"OLD").unwrap();
        let src_dir = workspace.join("elsewhere");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("foo.tga");
        std::fs::write(&src, b"NEW").unwrap();

        let basename = copy_into_project(&src, &project, "passthrough").unwrap();
        assert_eq!(basename, "foo.tga");
        assert_eq!(
            std::fs::read(dst_dir.join("foo.tga")).unwrap(),
            b"NEW",
            "expected destination to be overwritten with picked file's contents"
        );
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn errors_when_source_has_no_filename() {
        let workspace = scratch("no-filename");
        let project = workspace.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        // `..` has no filename component.
        let bad = std::path::Path::new("..");
        let err = copy_into_project(bad, &project, "passthrough").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn errors_when_source_missing() {
        let workspace = scratch("source-missing");
        let project = workspace.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let missing = workspace.join("does_not_exist.dds");
        let err = copy_into_project(&missing, &project, "passthrough").unwrap_err();
        // Platform-specific: NotFound on most systems.
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        let _ = std::fs::remove_dir_all(&workspace);
    }
}
