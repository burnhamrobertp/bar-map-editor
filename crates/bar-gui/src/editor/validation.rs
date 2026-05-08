//! Validation cache and panel-tab state.
//!
//! Validation findings come from `bar_project::validate_project`. We
//! re-run that whenever its inputs change (graph revision, map
//! dimensions, height range, spawn count) -- the `ValidationFingerprint`
//! captures those inputs cheaply so the per-frame cache check is just
//! a struct equality.
//!
//! `ValidationState` also owns the active filter tab (All / Error /
//! Warning / Info) for the validation details window and the active
//! tab in the Map Info modal -- both are session-scoped UI state that
//! resets across project boundaries.

use crate::app::{MapInfoTab, ValidationFilter};

/// Snapshot of every input `validate_project` reads, in a form cheap
/// to compare. The editor recomputes this every frame; whenever it
/// differs from the cached value, validation re-runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ValidationFingerprint {
    pub graph_revision: u64,
    pub map_width: u32,
    pub map_height: u32,
    /// `f32::to_bits` so `Eq` works without bringing in approximate
    /// comparisons. Validation only re-runs on exact value changes,
    /// which matches what the user thinks of as "I changed this".
    pub min_h_bits: u32,
    pub max_h_bits: u32,
    pub n_spawns: usize,
}

impl ValidationFingerprint {
    /// Initial value used at startup -- guaranteed to compare unequal
    /// to any real fingerprint, so the first frame's
    /// `refresh_validation_if_dirty` check fires the first run.
    pub fn sentinel_initial() -> Self {
        Self {
            graph_revision: u64::MAX,
            map_width: 0,
            map_height: 0,
            min_h_bits: 0,
            max_h_bits: 0,
            n_spawns: usize::MAX,
        }
    }
}

/// Grouped validation cache + panel state. See module docs.
#[derive(Debug, Clone)]
pub(crate) struct ValidationState {
    /// Cached findings from the last `validate_project` run.
    pub findings: Vec<bar_project::Finding>,
    /// Active severity filter in the validation details window.
    pub filter: ValidationFilter,
    /// Cache key for the findings -- when the editor's per-frame
    /// fingerprint differs, validation re-runs.
    pub last_fingerprint: ValidationFingerprint,
    /// Active section in the Map Info modal (Identity / Dimensions /
    /// Physics / Atmosphere / Lighting / Water).
    pub mapinfo_tab: MapInfoTab,
}

impl Default for ValidationState {
    fn default() -> Self {
        Self {
            findings: Vec::new(),
            filter: ValidationFilter::All,
            last_fingerprint: ValidationFingerprint::sentinel_initial(),
            mapinfo_tab: MapInfoTab::Identity,
        }
    }
}

impl ValidationState {
    pub fn findings(&self) -> &[bar_project::Finding] {
        &self.findings
    }

    pub fn filter(&self) -> ValidationFilter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: ValidationFilter) {
        self.filter = filter;
    }

    pub fn mapinfo_tab(&self) -> MapInfoTab {
        self.mapinfo_tab
    }

    pub fn set_mapinfo_tab(&mut self, tab: MapInfoTab) {
        self.mapinfo_tab = tab;
    }

    /// Reset filter, findings, and mapinfo tab to defaults. Called by
    /// `BarEditorApp::reset_session_state` on project switch. The
    /// fingerprint stays at sentinel so the first frame after reset
    /// re-validates the new project.
    pub fn reset(&mut self) {
        self.findings.clear();
        self.filter = ValidationFilter::All;
        self.mapinfo_tab = MapInfoTab::Identity;
        self.last_fingerprint = ValidationFingerprint::sentinel_initial();
    }
}
