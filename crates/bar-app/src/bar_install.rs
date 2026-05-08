//! Detect a local Beyond All Reason install and locate the binaries +
//! maps directory the launcher needs.
//!
//! BAR ships as an Electron lobby ("Beyond All Reason") that downloads
//! engine versions and game data on demand. On Windows the lobby installs
//! to `%LOCALAPPDATA%\Programs\Beyond-All-Reason\` and game data lives
//! under `data/`:
//!
//! - `Beyond-All-Reason.exe` — the lobby launcher (top level)
//! - `data/engine/<version>/spring.exe` — the engine binary; multiple
//!   versions can coexist (we pick the most recently modified)
//! - `data/maps/` — where SD7 archives are dropped to be visible in BAR
//!
//! For the v0.2 "Test in BAR" launcher we invoke the engine binary
//! directly with a generated `start.lua` rather than going through the
//! lobby — the lobby is for casual play; iterative map development needs
//! a one-click "compile and run this skirmish" loop.

use std::path::{Path, PathBuf};

/// Resolved paths inside a BAR install.
#[derive(Debug, Clone)]
pub struct BarInstall {
    /// The Electron lobby launcher (`Beyond-All-Reason.exe`).
    pub lobby_exe: PathBuf,
    /// Directory where SD7 archives must be placed to appear in BAR's
    /// map list.
    pub maps_dir: PathBuf,
}

impl BarInstall {
    /// Try to detect a BAR install on this machine. Returns `None` when
    /// the lobby launcher isn't found in any of the canonical locations.
    pub fn detect() -> Option<Self> {
        for candidate in candidate_install_roots() {
            if let Some(install) = Self::from_root(&candidate) {
                return Some(install);
            }
        }
        None
    }

    /// Build a `BarInstall` from a candidate install-root directory.
    /// Returns `None` if the lobby exe, an engine, or the maps dir is
    /// missing — a half-installed BAR isn't usable.
    fn from_root(root: &Path) -> Option<Self> {
        let lobby_exe = root.join("Beyond-All-Reason.exe");
        if !lobby_exe.exists() {
            return None;
        }

        let engine_root = root.join("data").join("engine");
        // Validate an engine binary exists before declaring this a usable install.
        newest_engine(&engine_root)?;

        let maps_dir = root.join("data").join("maps");
        if !maps_dir.exists() {
            return None;
        }

        Some(Self {
            lobby_exe,
            maps_dir,
        })
    }
}

/// Yield candidate install-root directories in priority order. The first
/// one whose `Beyond-All-Reason.exe` exists wins.
fn candidate_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    // Per-user install (the default path for the BAR Electron lobby).
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Beyond-All-Reason"),
        );
    }

    // System-wide (less common).
    if let Ok(pf) = std::env::var("PROGRAMFILES") {
        roots.push(PathBuf::from(pf).join("Beyond-All-Reason"));
    }
    if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
        roots.push(PathBuf::from(pf86).join("Beyond-All-Reason"));
    }

    roots
}

/// Pick the most-recently-modified `spring.exe` under
/// `<root>/<version>/spring.exe`. Newest is a reasonable proxy for
/// "the engine BAR is currently using" — when the lobby updates, the
/// active engine version's mtime advances.
fn newest_engine(engine_root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(engine_root).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let exe = entry.path().join("spring.exe");
        let Ok(meta) = std::fs::metadata(&exe) else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        match &best {
            Some((best_mtime, _)) if *best_mtime >= mtime => {}
            _ => best = Some((mtime, exe)),
        }
    }
    best.map(|(_, path)| path)
}

/// Outcome of a "Test in BAR" launch.
#[derive(Debug)]
pub enum LaunchOutcome {
    /// SD7 was copied into BAR's maps dir and the lobby was spawned.
    /// User must navigate to "Skirmish" and pick the map; we surface a
    /// status message instructing this.
    LobbyOpened {
        /// The basename (stem) the map will appear under in the lobby's
        /// map picker.
        map_stem: String,
    },
}

/// Reasons a BAR launch can fail. We don't pull in `thiserror` for the
/// app crate — `Display` via the manual impl below is plenty for what
/// the GUI surfaces in a status bar.
#[derive(Debug)]
pub enum LaunchError {
    CopyFailed(String),
    SpawnFailed(String),
    Sd7Unreadable(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CopyFailed(s) => write!(f, "failed to copy SD7 to BAR maps dir: {s}"),
            Self::SpawnFailed(s) => write!(f, "failed to spawn BAR lobby: {s}"),
            Self::Sd7Unreadable(s) => write!(f, "the SD7 is missing or unreadable: {s}"),
        }
    }
}

impl std::error::Error for LaunchError {}

impl BarInstall {
    /// Copy `sd7_path` into the maps directory and open the BAR lobby.
    /// The user picks the map manually from the skirmish menu (the map
    /// will appear under the SD7's filename stem). This is the v0.2
    /// flow — reliable and BAR-version-agnostic. A direct-engine
    /// "instant skirmish" path can come later as an advanced option.
    pub fn launch_lobby_with_map(&self, sd7_path: &Path) -> Result<LaunchOutcome, LaunchError> {
        if !sd7_path.exists() {
            return Err(LaunchError::Sd7Unreadable(format!(
                "{} does not exist",
                sd7_path.display()
            )));
        }
        let file_name = sd7_path
            .file_name()
            .ok_or_else(|| LaunchError::Sd7Unreadable("path has no filename".into()))?;
        let dest = self.maps_dir.join(file_name);

        std::fs::copy(sd7_path, &dest).map_err(|e| {
            LaunchError::CopyFailed(format!("{} -> {}: {e}", sd7_path.display(), dest.display()))
        })?;

        // Spawn the lobby. We don't wait for it to exit — the user will
        // interact with the lobby UI; OM continues running.
        std::process::Command::new(&self.lobby_exe)
            .spawn()
            .map_err(|e| LaunchError::SpawnFailed(e.to_string()))?;

        let map_stem = sd7_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(LaunchOutcome::LobbyOpened { map_stem })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_some_or_none_without_panicking() {
        // We can't assert presence on every dev machine, but the call
        // must never panic (e.g. missing env vars must produce None).
        let _ = BarInstall::detect();
    }
}
