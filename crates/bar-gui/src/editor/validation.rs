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

/// Active filter tab in the validation details window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationFilter {
    All,
    Error,
    Warning,
    Info,
}

/// Active section in the Map Settings modal -- replaces the
/// per-section CollapsingHeaders so only one section's controls are
/// on screen at a time, switched via a tab strip across the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapInfoTab {
    Identity,
    Dimensions,
    Physics,
    Atmosphere,
    Lighting,
    Water,
    Resources,
}

/// Snapshot of every input `validate_project` reads, in a form cheap
/// to compare. The editor recomputes this every frame; whenever it
/// differs from the cached value, validation re-runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidationFingerprint {
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
pub struct ValidationState {
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

use crate::app::BarEditorApp;

impl BarEditorApp {
    /// Re-run project validation and stash the findings for the panel.
    /// True iff the current cached validation has any blocking
    /// errors. Cheap — just scans the cached findings list.
    pub fn validation_has_errors(&self) -> bool {
        bar_project::has_errors(&self.validation.findings)
    }

    /// Count cached findings by severity for the sidebar display.
    pub fn validation_counts(&self) -> (usize, usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        let mut infos = 0;
        for f in &self.validation.findings {
            match f.severity {
                bar_project::Severity::Error => errors += 1,
                bar_project::Severity::Warning => warnings += 1,
                bar_project::Severity::Info => infos += 1,
            }
        }
        (errors, warnings, infos)
    }

    /// Re-run validation iff any input that feeds it has changed since
    /// the last run. Runs at the top of every frame so the sidebar
    /// counts and the export gate are always in sync with the editor
    /// state -- no manual click needed.
    pub(crate) fn refresh_validation_if_dirty(&mut self) {
        let fp = self.validation_inputs_fingerprint();
        if fp != self.validation.last_fingerprint {
            self.run_validation();
            self.validation.last_fingerprint = fp;
        }
    }

    /// Compact fingerprint of every input `validate_project` reads.
    /// Used to decide whether the cached findings are still valid.
    /// Cheap: small struct, cheap to compare.
    pub(crate) fn validation_inputs_fingerprint(&self) -> ValidationFingerprint {
        ValidationFingerprint {
            graph_revision: self.graph.revision(),
            map_width: self.map.width,
            map_height: self.map.height,
            min_h_bits: self.map.settings.min_height.to_bits(),
            max_h_bits: self.map.settings.max_height.to_bits(),
            n_spawns: self.map.settings.start_positions.len(),
        }
    }

    /// Compact "Validation" summary in the left sidebar: live error /
    /// warning / info counts plus a Details button that opens the
    /// findings panel. Replaces the "Nodes: N / Connections: N"
    /// stats that used to live in the status bar — the per-severity
    /// counts are far more actionable.
    ///
    /// Validation itself runs at the top of every frame from
    /// `update`'s `refresh_validation_if_dirty`; this method just
    /// reads the cached findings.
    /// Sidebar validation summary - see `crate::panels::validation`.
    pub(crate) fn draw_validation_summary(&mut self, ui: &mut egui::Ui) {
        crate::panels::validation::draw_summary(self, ui);
    }

    /// Validation gate for the export flow. Runs validation, then:
    /// - if there are errors, opens the panel and refuses to start
    ///   the export (returns `false`);
    /// - otherwise, the caller is cleared to set `run_requested` /
    ///   `run_export_node` (returns `true`).
    pub(crate) fn validate_before_export(&mut self, action_label: &str) -> bool {
        self.run_validation();
        if self.validation_has_errors() {
            self.dialog.show_validation_panel = true;
            self.dialog.status_message =
                Some(format!("{action_label}: fix validation errors first."));
            false
        } else {
            true
        }
    }

    pub(crate) fn run_validation(&mut self) {
        // We construct a temporary MapSettings with current min/max
        // height so the validator sees what the project will export
        // with. Other fields use defaults — full structured-mapinfo
        // editing comes in M1.1.
        let settings = bar_project::MapSettings {
            min_height: self.map.min_height,
            max_height: self.map.max_height,
            ..Default::default()
        };
        self.validation.findings =
            bar_project::validate_project(&self.graph, &settings, self.map.width, self.map.height);
    }
}
