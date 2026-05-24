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
    /// Label shown in the UI -- archive filename, with `" (latest)"`
    /// suffixed on the newest-mtime entry.
    pub label: String,
    /// Value written to `GameType=` in the startscript. Sourced from the
    /// archive's `modinfo.lua` (`name + " " + version`) -- the filename
    /// alone won't satisfy Recoil's archive lookup.
    pub archive_name: String,
    /// Full path to the archive on disk.
    pub path: Option<std::path::PathBuf>,
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
    /// Locally installed game archives, newest-mtime first. The newest
    /// entry's label is suffixed with `" (latest)"` so the dropdown's
    /// default selection makes the "use the freshest install" intent
    /// obvious. Empty when no usable archive lives under
    /// `<install>/data/games/`.
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
    /// Load engine + game + maps state from an explicit BAR install
    /// root. Returns `None` when the path doesn't look like a BAR
    /// install (missing `Beyond-All-Reason.exe`, missing `data/maps`,
    /// or no engine binaries found). The editor never falls back to
    /// guessing the install location -- the only input is the
    /// `bar_install_path` setting, populated by
    /// `auto_detect_install_root` on first launch and overridable
    /// from Preferences.
    ///
    /// The configured path is normalised first: if the user pasted in
    /// `<root>/data` or `<root>/data/games` directly (a common slip
    /// when picking the folder from a file dialog), we walk back up to
    /// the actual install root so the rest of the resolution doesn't
    /// double-stack `data/data/...`.
    pub fn from_install_root(root: &Path) -> Option<Self> {
        let root = normalize_install_root(root);
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

        let games = collect_games(&data_dir.join("games"));

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
        map_internal_name: &str,
        map_squares: (u32, u32),
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
        // Skip the copy when the source is already inside `maps/`
        // (Test-in-BAR fast path writes the .sdd directory there
        // directly). Also handles repeat-launch where the previous
        // .sd7 is already at the destination.
        let already_in_place = sd7_path == dest.as_path();
        if !already_in_place {
            if sd7_path.is_dir() {
                if dest.exists() {
                    let _ = std::fs::remove_dir_all(&dest);
                }
                copy_dir_recursive(sd7_path, &dest).map_err(|e| {
                    LaunchError::CopyFailed(format!(
                        "{} -> {}: {e}",
                        sd7_path.display(),
                        dest.display()
                    ))
                })?;
            } else {
                std::fs::copy(sd7_path, &dest).map_err(|e| {
                    LaunchError::CopyFailed(format!(
                        "{} -> {}: {e}",
                        sd7_path.display(),
                        dest.display()
                    ))
                })?;
            }
        }

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

        // MapName must match the Spring archive identity: `name .. " " .. version`
        // from mapinfo.lua. SMFMapFile::Open also appends ".smf" to MapName
        // when searching inside the archive, so the name must NOT include an
        // extension (e.g. use "my_map 0.1", not "my_map.sd7").
        //
        // Map-testing setup -- the local player joins as a spectator
        // so they can fly the whole map without being constrained to
        // one team's view / commander. Two AI teams play each other
        // so neither team auto-wins (one team alive -> game over ->
        // post-game UI overlays the viewport). The spectator has free
        // camera + no LOS restrictions.
        //
        // When the BAR button grows a "choose how to test" modal
        // (gameplay vs spectator vs solo etc.), the script generator
        // will live behind a `LaunchMode` enum; for now this single
        // form covers the iteration use case.
        //
        // StartPosType=3 (ChooseBeforeGame) lets the script pin each
        // team's spawn via `StartPosX`/`StartPosZ`. Without explicit
        // positions, AI-only matches default to spawning every team
        // on top of each other at map center -- commanders mash into
        // melee on tick 1 and one team blinks out, ending the match
        // before the camera even settles. World units are spring map
        // squares * 8 (each square = 8x8 world units); we drop one
        // team near the (0.2, 0.2) corner and the other near (0.8,
        // 0.8) so they're on opposite ends regardless of map shape.
        //
        // TODO: parse mapinfo.lua's `teams = { [N] = { startPos } }`
        // table when present and use those instead -- some maps ship
        // hand-picked starts that should win over the corner default.
        // Also: once BAR's gameconfig start-box format is mapped,
        // honour `<gameconfig>/map_startboxes.lua` so the spawn
        // reflects the configured "start box centre" for that gametype.
        let (msq_x, msq_y) = map_squares;
        let world_x = (msq_x.max(1) * 8) as f32;
        let world_z = (msq_y.max(1) * 8) as f32;
        let t0_x = (world_x * 0.2) as i32;
        let t0_z = (world_z * 0.2) as i32;
        let t1_x = (world_x * 0.8) as i32;
        let t1_z = (world_z * 0.8) as i32;
        // MyPlayerNum + IsHost are required to initialise the local player slot.
        // TeamLeader in every [TEAMn] must be a valid player number (0 = host).
        let script = format!(
            "[GAME]\n{{\n\
            \tMapName={map_internal_name};\n\
            \tGameType={game};\n\
            \tStartPosType=3;\n\
            \tGameStartDelay=4;\n\
            \tMyPlayerNum=0;\n\
            \tMyPlayerName=MapTester;\n\
            \tIsHost=1;\n\
            \n\
            \t[PLAYER0]\n\t{{\n\t\tName=MapTester;\n\t\tSpectator=1;\n\t}}\n\
            \n\
            \t[ALLYTEAM0]\n\t{{\n\t\tNumAllies=0;\n\t}}\n\
            \t[TEAM0]\n\t{{\n\t\tAllyTeam=0;\n\t\tTeamLeader=0;\n\t\tStartPosX={t0_x};\n\t\tStartPosZ={t0_z};\n\t}}\n\
            \t[AI0]\n\t{{\n\t\tName=BARb0;\n\t\tShortName=BARb;\n\t\tTeam=0;\n\t\tHost=0;\n\t\tIsFromDemo=0;\n\t}}\n\
            \n\
            \t[ALLYTEAM1]\n\t{{\n\t\tNumAllies=0;\n\t}}\n\
            \t[TEAM1]\n\t{{\n\t\tAllyTeam=1;\n\t\tTeamLeader=0;\n\t\tStartPosX={t1_x};\n\t\tStartPosZ={t1_z};\n\t}}\n\
            \t[AI1]\n\t{{\n\t\tName=BARb1;\n\t\tShortName=BARb;\n\t\tTeam=1;\n\t\tHost=0;\n\t\tIsFromDemo=0;\n\t}}\n\
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

        Ok(LaunchOutcome::EngineStarted {
            map_name: map_internal_name.to_string(),
        })
    }
}

/// Recursively copy a directory's contents. Used when the Test-in-BAR
/// fast path produces a `.sdd` directory in a different filesystem
/// from `maps/` (cross-drive `rename` would fail, so we copy instead).
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Detection helpers
// ---------------------------------------------------------------------------

/// One-shot auto-detection of a BAR install. Used ONLY to seed the
/// `bar_install_path` setting on first launch -- once the setting
/// is populated (or the user clears it deliberately), this is never
/// consulted again. No env-var fallthrough at the
/// `BarVersions::from_install_root` callsite: that consumes the
/// configured path verbatim.
///
/// Probes (Windows): `%LOCALAPPDATA%\Programs\Beyond-All-Reason`,
/// `%PROGRAMFILES%\Beyond-All-Reason`, `%PROGRAMFILES(X86)%\
/// Beyond-All-Reason`. First candidate that contains
/// `Beyond-All-Reason.exe` wins; the actual install validity check
/// (engine binaries present, etc.) happens later in
/// `from_install_root`.
pub fn auto_detect_install_root() -> Option<PathBuf> {
    candidate_install_roots()
        .into_iter()
        .find(|root| root.join("Beyond-All-Reason.exe").exists())
}

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

/// Game archives under `<games_root>/`, newest mtime first.
/// Matches both the legacy `byar_*.sdz` format and the current `BAR*.sdd` format.
///
/// `archive_name` is populated from each archive's `modinfo.lua` identity
/// (`name + " " + version`) -- that's what Recoil matches `GameType=` against.
/// Archives without a readable modinfo are skipped because they can't be
/// reliably launched; presenting them in the dropdown would be the same
/// trap the synthetic `$VERSION` entry used to be.
///
/// The newest-mtime entry's label gets a `" (latest)"` suffix so the
/// default selection signals which archive will be used.
fn collect_games(games_root: &Path) -> Vec<BarGameVersion> {
    let Ok(entries) = std::fs::read_dir(games_root) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, BarGameVersion)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let full_path = entry.path();
            let ext = full_path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            if ext != "sdz" && ext != "sd7" && ext != "sdd" {
                return None;
            }
            let lc = name.to_lowercase();
            if !lc.starts_with("byar") && !lc.starts_with("bar") {
                return None;
            }
            let mtime = entry.metadata().ok()?.modified().ok()?;
            let identity = read_archive_identity(&full_path)?;
            Some((
                mtime,
                BarGameVersion {
                    label: name,
                    archive_name: identity,
                    path: Some(full_path),
                },
            ))
        })
        .collect();
    found.sort_by_key(|v| std::cmp::Reverse(v.0));
    let mut games: Vec<BarGameVersion> = found.into_iter().map(|(_, v)| v).collect();
    if let Some(newest) = games.first_mut() {
        newest.label = format!("{} (latest)", newest.label);
    }
    games
}

/// Extract `"{name} {version}"` from an archive's `modinfo.lua`. This is
/// the archive identity Recoil uses when matching `GameType=` and
/// `depend = { ... }` entries -- the filename is irrelevant to lookup.
/// Returns `None` if `modinfo.lua` is absent or doesn't declare both
/// fields.
fn read_archive_identity(archive: &Path) -> Option<String> {
    let bytes = bar_engine::read_file_from_archive(archive, "modinfo.lua")?;
    let lua = String::from_utf8_lossy(&bytes);
    let name = bar_project::parse_mapinfo_string(&lua, "name")?;
    let version = bar_project::parse_mapinfo_string(&lua, "version")?;
    Some(format!("{name} {version}"))
}

/// Strip a trailing `data` or `data/games` from the configured install
/// path so the rest of the resolution can rely on `root.join("data")`
/// without doubling up. Triggers when the user pastes (or picks via
/// folder dialog) a path one or two levels deeper than the install
/// root, which is easy to do because `data/games/` is what's most
/// visible when browsing the install.
fn normalize_install_root(p: &Path) -> PathBuf {
    let leaf = p
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_ascii_lowercase());
    match leaf.as_deref() {
        Some("games") => {
            let parent = p.parent();
            let grandparent = parent.and_then(|d| d.parent());
            if parent.map(|d| d.file_name().and_then(|n| n.to_str())) == Some(Some("data")) {
                if let Some(gp) = grandparent {
                    return gp.to_path_buf();
                }
            }
        }
        Some("data") => {
            if let Some(parent) = p.parent() {
                return parent.to_path_buf();
            }
        }
        _ => {}
    }
    p.to_path_buf()
}

// ---------------------------------------------------------------------------
// Outcome / error types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LaunchOutcome {
    EngineStarted { map_name: String },
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
    fn auto_detect_does_not_panic() {
        let _ = auto_detect_install_root();
    }

    #[test]
    fn from_install_root_rejects_unrelated_path() {
        let tmp = std::env::temp_dir();
        // A temp dir is never a BAR install; we just want to confirm
        // the validator returns None cleanly rather than panicking.
        assert!(BarVersions::from_install_root(&tmp).is_none());
    }

    #[test]
    fn normalize_leaves_install_root_alone() {
        let p = PathBuf::from("C:/Beyond-All-Reason");
        assert_eq!(normalize_install_root(&p), p);
    }

    #[test]
    fn normalize_strips_trailing_data() {
        let root = PathBuf::from("C:/Beyond-All-Reason");
        let with_data = root.join("data");
        assert_eq!(normalize_install_root(&with_data), root);
    }

    #[test]
    fn normalize_strips_trailing_data_games() {
        let root = PathBuf::from("C:/Beyond-All-Reason");
        let with_data_games = root.join("data").join("games");
        assert_eq!(normalize_install_root(&with_data_games), root);
    }

    #[test]
    fn normalize_does_not_strip_unrelated_games_dir() {
        // Path ending in `games` but not under a `data` parent shouldn't
        // be touched -- it's not the BAR layout, walking up would point
        // at the wrong place.
        let weird = PathBuf::from("C:/Some/Other/games");
        assert_eq!(normalize_install_root(&weird), weird);
    }
}
