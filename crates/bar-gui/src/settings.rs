//! User preferences and recent files, persisted as JSON in the platform's
//! per-user config directory.
//!
//! Layout (Windows): `%APPDATA%\BarEditor\BarEditor\config\settings.json`
//!   (Linux):        `~/.config/openmachine/settings.json`
//!   (macOS):        `~/Library/Application Support/BarEditor/settings.json`
//!
//! Writes are atomic (write-temp-then-rename) so a crash mid-save can't
//! produce a truncated config file.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const RECENT_FILES_MAX: usize = 10;
const SETTINGS_FILENAME: &str = "settings.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    #[serde(default)]
    pub recent_files: Vec<PathBuf>,

    #[serde(default = "default_autosave_enabled")]
    pub autosave_enabled: bool,

    #[serde(default = "default_autosave_interval")]
    pub autosave_interval_secs: u64,

    /// Number of rotating autosave slots. Each slot gets its own file
    /// (`<project>.autosave1` .. `<project>.autosave{n}`). Older slots
    /// are overwritten in round-robin order so the most recent N writes
    /// are always recoverable.
    #[serde(default = "default_autosave_slot_count")]
    pub autosave_slot_count: u32,

    /// Identifiers of confirmation modals the user has ticked
    /// "Don't ask again" on. The matching modal type just runs
    /// without prompting from then on. Reset via Preferences →
    /// "Clear suppressed confirmations".
    #[serde(default)]
    pub suppressed_confirmations: HashSet<String>,

    /// Reopen the last-loaded project on launch. Defaults to true so
    /// the user picks up where they left off; can be disabled if they
    /// want a clean canvas every time.
    #[serde(default = "default_restore_last_project")]
    pub restore_last_project: bool,

    #[serde(default)]
    pub window: Option<WindowState>,

    /// Last-active editor layout. The user's choice of layout is a
    /// UI preference, not a project property — it persists across
    /// projects, not with them. `Default` covers older settings
    /// files that pre-date this field.
    #[serde(default)]
    pub active_layout: crate::app::Layout,
}

fn default_autosave_enabled() -> bool {
    true
}
fn default_autosave_interval() -> u64 {
    120
}
fn default_autosave_slot_count() -> u32 {
    3
}
fn default_restore_last_project() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Whether the window was maximized at the time of save. Restored
    /// on next launch via `ViewportBuilder::with_maximized`. Older
    /// saved files that pre-date this field default to `false`.
    #[serde(default)]
    pub maximized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent_files: Vec::new(),
            autosave_enabled: default_autosave_enabled(),
            autosave_interval_secs: default_autosave_interval(),
            autosave_slot_count: default_autosave_slot_count(),
            suppressed_confirmations: HashSet::new(),
            restore_last_project: default_restore_last_project(),
            window: None,
            active_layout: crate::app::Layout::default(),
        }
    }
}

impl Settings {
    /// Load settings from disk. Returns defaults on any error (missing file,
    /// parse error, permission denied) — the editor must always be usable.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<Self>(&s).unwrap_or_else(|e| {
                tracing::warn!(?path, error = %e, "Failed to parse settings; using defaults");
                Self::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!(?path, error = %e, "Failed to read settings; using defaults");
                Self::default()
            }
        }
    }

    /// Atomically persist settings to disk. Errors are logged but not returned;
    /// settings persistence is best-effort and must never block app shutdown.
    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(?parent, error = %e, "Failed to create settings dir");
                return;
            }
        }
        let json = match serde_json::to_string_pretty(self) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to serialise settings");
                return;
            }
        };
        let tmp = path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, &json) {
            tracing::warn!(?tmp, error = %e, "Failed to write settings tmp");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            tracing::warn!(?path, error = %e, "Failed to rename settings tmp into place");
        }
    }

    /// Insert (or move-to-front) a recently-opened file path. Older duplicates
    /// are removed and the list is truncated to the maximum length.
    pub fn add_recent(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(RECENT_FILES_MAX);
    }

    /// Remove a recent file (e.g. after a failed open because the file no
    /// longer exists).
    pub fn remove_recent(&mut self, path: &Path) {
        self.recent_files.retain(|p| p != path);
    }

    /// Path to the settings JSON file, or None if no per-user config dir is
    /// resolvable on this platform.
    pub fn config_path() -> Option<PathBuf> {
        Self::project_dirs().map(|d| d.config_dir().join(SETTINGS_FILENAME))
    }

    /// Directory under which auto-saves are stored when no project file path
    /// is set yet (untitled projects).
    pub fn autosave_dir() -> Option<PathBuf> {
        Self::project_dirs().map(|d| d.data_local_dir().join("autosave"))
    }

    fn project_dirs() -> Option<ProjectDirs> {
        ProjectDirs::from("", "BarEditor", "BarEditor")
    }
}
