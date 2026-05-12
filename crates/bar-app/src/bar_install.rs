//! Detect a local Beyond All Reason install and locate the binaries +
//! maps directory the launcher needs.
//!
//! BAR ships as an Electron lobby ("Beyond All Reason") that downloads
//! engine versions and game data on demand. On Windows the lobby installs
//! to `%LOCALAPPDATA%\Programs\Beyond-All-Reason\` and game data lives
//! under `data/`:
//!
//! - `Beyond-All-Reason.exe` -- the lobby launcher (top level)
//! - `data/engine/<version>/spring.exe` -- engine binary; multiple versions
//!   can coexist (we pick the newest by mtime for the default)
//! - `data/games/byar_<version>.sdz` -- game archive; multiple versions
//!   can coexist
//! - `data/maps/` -- where SD7 archives are dropped to appear in BAR

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Version descriptors
// ---------------------------------------------------------------------------

/// One available game version shown in the version picker.
#[derive(Debug, Clone)]
pub struct BarGameVersion {
    /// Label shown in the UI (e.g. "byar:stable (rapid)" or "byar_1234.sdz").
    pub label: String,
    /// Value written to `GameType=` in the startscript.
    pub archive_name: String,
}

/// One available engine version shown in the version picker.
#[derive(Debug, Clone)]
pub struct BarEngineVersion {
    /// Label shown in the UI -- the version folder name.
    pub label: String,
    /// Path to `spring.exe`.
    pub exe: PathBuf,
}

// ---------------------------------------------------------------------------
// BarVersions
// ---------------------------------------------------------------------------

/// All detected game and engine versions for a BAR install.
#[derive(Debug, Clone)]
pub struct BarVersions {
    /// Available game archives. `games[0]` is always `byar:stable` (rapid
    /// tag fallback); subsequent entries are locally installed archives,
    /// newest first.
    pub games: Vec<BarGameVersion>,
    /// Available engine binaries, newest-mtime first.
    pub engines: Vec<BarEngineVersion>,
    /// Directory where SD7 archives must be placed to appear in BAR.
    pub maps_dir: PathBuf,
    /// BAR's data root (`<install>/data/`). Passed as `--write-dir` to the
    /// engine so it resolves maps and game archives relative to it.
    pub data_dir: PathBuf,
}

impl BarVersions {
    /// Detect a BAR install. Returns `None` when no usable install is found.
    pub fn detect() -> Option<Self> {
        for root in candidate_install_roots() {
            if let Some(v) = Self::from_root(&root) {
                return Some(v);
            }
        }
        None
    }

    fn from_root(root: &Path) -> Option<Self> {
        let lobby_exe = root.join("Beyond-All-Reason.exe");
        if !lobby_exe.exists() {
            return None;
        }

        let data_dir = root.join("data");
        let maps_dir = data_dir.join("maps");
        if !maps_dir.exists() {
            return None;
        }

        let engines = collect_engines(&data_dir.join("engine"));
        if engines.is_empty() {
            return None;
        }

        let mut games = vec![BarGameVersion {
            label: "byar:stable (rapid)".to_string(),
            archive_name: "byar:stable".to_string(),
        }];
        games.extend(collect_games(&data_dir.join("games")));

        Some(Self {
            games,
            engines,
            maps_dir,
            data_dir,
        })
    }

    /// Copy `sd7_path` into the maps directory and spawn the engine directly
    /// into a skirmish using the versions at `game_idx` / `engine_idx`.
    /// Out-of-bounds indices fall back to index 0.
    pub fn launch_skirmish(
        &self,
        sd7_path: &Path,
        game_idx: usize,
        engine_idx: usize,
    ) -> Result<LaunchOutcome, LaunchError> {
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

        let game = self
            .games
            .get(game_idx)
            .or_else(|| self.games.first())
            .ok_or_else(|| LaunchError::SpawnFailed("no game version available".into()))?;
        let engine = self
            .engines
            .get(engine_idx)
            .or_else(|| self.engines.first())
            .ok_or_else(|| LaunchError::SpawnFailed("no engine version available".into()))?;

        let map_name = file_name.to_string_lossy();
        let map_stem = sd7_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        // StartPosType=2: players click to place on the minimap during the
        // loading screen -- always works even if the map has no defined
        // start positions yet.
        //
        // MyPlayerNum + IsHost are required by the engine to initialise
        // the local player slot; without them the game Lua crashes once
        // loading finishes.  TeamLeader in every [TEAMn] must be a valid
        // player number (0 = the host).
        let script = format!(
            "[GAME]\n{{\n\
            \tMapName={map_name};\n\
            \tGameType={game};\n\
            \tStartPosType=2;\n\
            \tGameStartDelay=4;\n\
            \tMyPlayerNum=0;\n\
            \tMyPlayerName=MapTester;\n\
            \tIsHost=1;\n\
            \n\
            \t[ALLYTEAM0]\n\t{{\n\t\tNumAllies=0;\n\t}}\n\
            \t[TEAM0]\n\t{{\n\t\tAllyTeam=0;\n\t\tTeamLeader=0;\n\t}}\n\
            \t[PLAYER0]\n\t{{\n\t\tName=MapTester;\n\t\tTeam=0;\n\t}}\n\
            \n\
            \t[ALLYTEAM1]\n\t{{\n\t\tNumAllies=0;\n\t}}\n\
            \t[TEAM1]\n\t{{\n\t\tAllyTeam=1;\n\t\tTeamLeader=0;\n\t}}\n\
            \t[AI0]\n\t{{\n\t\tName=BARb;\n\t\tShortName=BARb;\n\t\tTeam=1;\n\t\tHost=0;\n\t\tIsFromDemo=0;\n\t}}\n\
            }}\n",
            game = game.archive_name,
        );

        let script_path = std::env::temp_dir().join("om_test_script.txt");
        std::fs::write(&script_path, &script)
            .map_err(|e| LaunchError::SpawnFailed(format!("write startscript: {e}")))?;

        // Run the engine from its own directory. Spring/Recoil resolves
        // package paths (packages/, pool/) relative to the executable
        // location; launching from a foreign CWD causes the VFS to miss
        // game content archives (e.g. "beyond all reason 0.01"), which
        // manifests as a content_error crash at game-start.
        let engine_dir = engine
            .exe
            .parent()
            .ok_or_else(|| LaunchError::SpawnFailed("engine exe has no parent dir".into()))?;

        std::process::Command::new(&engine.exe)
            .current_dir(engine_dir)
            .arg("--write-dir")
            .arg(&self.data_dir)
            .arg(&script_path)
            .spawn()
            .map_err(|e| LaunchError::SpawnFailed(e.to_string()))?;

        Ok(LaunchOutcome::EngineStarted { map_stem })
    }
}

// ---------------------------------------------------------------------------
// Detection helpers
// ---------------------------------------------------------------------------

fn candidate_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Beyond-All-Reason"),
        );
    }
    if let Ok(pf) = std::env::var("PROGRAMFILES") {
        roots.push(PathBuf::from(pf).join("Beyond-All-Reason"));
    }
    if let Ok(pf86) = std::env::var("PROGRAMFILES(X86)") {
        roots.push(PathBuf::from(pf86).join("Beyond-All-Reason"));
    }
    roots
}

/// All `spring.exe` binaries under `<engine_root>/<version>/`, newest mtime first.
fn collect_engines(engine_root: &Path) -> Vec<BarEngineVersion> {
    let Ok(entries) = std::fs::read_dir(engine_root) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, BarEngineVersion)> = entries
        .flatten()
        .filter_map(|entry| {
            let exe = entry.path().join("spring.exe");
            let meta = std::fs::metadata(&exe).ok()?;
            let mtime = meta.modified().ok()?;
            let label = entry
                .path()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown".to_string());
            Some((mtime, BarEngineVersion { label, exe }))
        })
        .collect();
    found.sort_by_key(|v| std::cmp::Reverse(v.0));
    found.into_iter().map(|(_, v)| v).collect()
}

/// All `byar_*.sd?` archives under `<games_root>/`, newest mtime first.
fn collect_games(games_root: &Path) -> Vec<BarGameVersion> {
    let Ok(entries) = std::fs::read_dir(games_root) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, BarGameVersion)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("byar") {
                return None;
            }
            let ext = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            if ext != "sdz" && ext != "sd7" && ext != "sdd" {
                return None;
            }
            let mtime = entry.metadata().ok()?.modified().ok()?;
            Some((
                mtime,
                BarGameVersion {
                    label: name.clone(),
                    archive_name: name,
                },
            ))
        })
        .collect();
    found.sort_by_key(|v| std::cmp::Reverse(v.0));
    found.into_iter().map(|(_, v)| v).collect()
}

// ---------------------------------------------------------------------------
// Outcome / error types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LaunchOutcome {
    EngineStarted { map_stem: String },
}

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
            Self::SpawnFailed(s) => write!(f, "failed to launch BAR engine: {s}"),
            Self::Sd7Unreadable(s) => write!(f, "SD7 missing or unreadable: {s}"),
        }
    }
}

impl std::error::Error for LaunchError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_does_not_panic() {
        let _ = BarVersions::detect();
    }
}
