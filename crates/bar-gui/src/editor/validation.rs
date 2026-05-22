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
    /// `ProjectState::commits` -- bumps on every `mark_dirty()`,
    /// which is the editor-wide signal for "an atomic field commit
    /// just happened" (text-input blur, drag-stop, colour pick,
    /// checkbox toggle). Including it means any committed recipe
    /// edit re-runs validation on the next frame without needing
    /// the fingerprint to enumerate every individual recipe field.
    pub commits: u64,
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
            commits: u64::MAX,
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
}

impl Default for ValidationState {
    fn default() -> Self {
        Self {
            findings: Vec::new(),
            filter: ValidationFilter::All,
            last_fingerprint: ValidationFingerprint::sentinel_initial(),
        }
    }
}

/// Top-level surface a finding belongs to. The mapping from
/// [`bar_project::Finding::category`] to `ModalId` is data-driven
/// via [`ModalId::for_category`] so the action bar can ask "how
/// many errors live in the Map Info modal?" without scanning the
/// finding list itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModalId {
    /// The map-settings modals: Identity / Dimensions / Physics /
    /// Atmosphere / Lighting / Water / Resources.
    MapInfo,
    /// The Fog modal (distance fog, height fog, volumetric clouds).
    Fog,
    /// The Map Edge modal (grass shading texture picker).
    MapEdge,
    /// The Grass modal (`custom.grassConfig` block).
    Grass,
    /// Findings that don't fit any modal -- graph-level issues,
    /// bundler-missing, file-not-found. The Map Info modal's
    /// "general" badge surfaces these as project-level issues.
    Project,
}

impl ModalId {
    /// Map a finding's category string to its owning modal.
    /// Categories not listed fall through to [`ModalId::Project`].
    pub fn for_category(category: &str) -> Self {
        use bar_project::field_schema::categories as c;
        match category {
            c::IDENTITY | c::DIMENSIONS | c::PHYSICS | c::ATMOSPHERE | c::LIGHTING | c::WATER => {
                ModalId::MapInfo
            }
            c::FOG | c::CLOUDS => ModalId::Fog,
            c::GRASS => ModalId::Grass,
            c::RESOURCES => ModalId::MapEdge,
            _ => ModalId::Project,
        }
    }
}

/// Export-side action that may be gated by validation errors. Each
/// variant maps to the corresponding button in the action bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockingAction {
    Compile,
    TestInBar,
    Bundle,
}

/// Per-surface severity counts. Drives badge rendering on modal
/// launchers and the action-bar buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationSummary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

impl ValidationSummary {
    pub fn is_clean(&self) -> bool {
        self.errors == 0 && self.warnings == 0 && self.infos == 0
    }
}

impl ValidationState {
    pub fn findings(&self) -> &[bar_project::Finding] {
        &self.findings
    }

    /// Total severity counts across all cached findings.
    pub fn summary(&self) -> ValidationSummary {
        let mut s = ValidationSummary::default();
        for f in &self.findings {
            match f.severity {
                bar_project::Severity::Error => s.errors += 1,
                bar_project::Severity::Warning => s.warnings += 1,
                bar_project::Severity::Info => s.infos += 1,
            }
        }
        s
    }

    /// Severity counts for findings tagged with one specific
    /// `Finding::category` string. Used by the per-tab Map Info
    /// action-bar buttons so each tab's button surfaces only its
    /// own findings (rather than the whole Map Info modal's sum).
    pub fn summary_for_category(&self, category: &str) -> ValidationSummary {
        let mut s = ValidationSummary::default();
        for f in &self.findings {
            if f.category != category {
                continue;
            }
            match f.severity {
                bar_project::Severity::Error => s.errors += 1,
                bar_project::Severity::Warning => s.warnings += 1,
                bar_project::Severity::Info => s.infos += 1,
            }
        }
        s
    }

    /// Severity counts for findings whose category routes to the
    /// given modal. Cheap; just walks the cached list.
    pub fn summary_for_modal(&self, modal: ModalId) -> ValidationSummary {
        let mut s = ValidationSummary::default();
        for f in &self.findings {
            if ModalId::for_category(&f.category) != modal {
                continue;
            }
            match f.severity {
                bar_project::Severity::Error => s.errors += 1,
                bar_project::Severity::Warning => s.warnings += 1,
                bar_project::Severity::Info => s.infos += 1,
            }
        }
        s
    }

    /// Severity counts contributing to the gate on a given action.
    /// All three actions currently share the same gate (any Error
    /// blocks) but the structure is here so future per-action
    /// nuances can be added without changing the call sites.
    pub fn summary_for_action(&self, _action: BlockingAction) -> ValidationSummary {
        self.summary()
    }

    /// Return true when `action` should be disabled because of one
    /// or more blocking findings (Error severity). Warning-only
    /// states never block.
    pub fn is_blocking(&self, action: BlockingAction) -> bool {
        self.summary_for_action(action).errors > 0
    }

    /// Concatenated short labels of the first few errors that are
    /// blocking `action`. Used as a hover tooltip on disabled
    /// action-bar buttons so the user knows what to fix.
    pub fn blocking_summary(&self, _action: BlockingAction, max_items: usize) -> String {
        let mut items: Vec<&str> = Vec::new();
        for f in &self.findings {
            if f.severity == bar_project::Severity::Error {
                items.push(&f.message);
                if items.len() >= max_items {
                    break;
                }
            }
        }
        if items.is_empty() {
            String::new()
        } else {
            items.join("; ")
        }
    }

    pub fn filter(&self) -> ValidationFilter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: ValidationFilter) {
        self.filter = filter;
    }

    /// Reset filter + findings to defaults. Called by
    /// `BarEditorApp::reset_session_state` on project switch. The
    /// fingerprint stays at sentinel so the first frame after reset
    /// re-validates the new project.
    pub fn reset(&mut self) {
        self.findings.clear();
        self.filter = ValidationFilter::All;
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
        // settings.{min,max}_height are now Option<f32> so the field
        // captures "user explicitly set vs inherit from source".
        // Fingerprint the bit-pattern of the effective value (resolved
        // against engine defaults) so an unset value still produces a
        // stable hash that changes when the resolved value changes.
        let rs = self.map.settings.resolved();
        ValidationFingerprint {
            graph_revision: self.graph.revision(),
            map_width: self.map.width,
            map_height: self.map.height,
            min_h_bits: rs.min_height.to_bits(),
            max_h_bits: rs.max_height.to_bits(),
            n_spawns: self.map.settings.start_positions.len(),
            commits: self.project.commits,
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
        // Pass the LIVE settings (cloned, since validate_project
        // borrows immutably) so the schema-driven validators see
        // the user's actual edits. The min/max shadow fields on
        // MapState are kept in sync with settings.{min,max}_height
        // by the bind layer; treat the live settings as source of
        // truth.
        let mut settings = self.map.settings.clone();
        // Mirror MapState's shadowed heights back onto the settings
        // copy used for validation -- catches the case where the
        // shadow was edited but the binding hasn't flushed yet.
        settings.min_height = Some(self.map.min_height);
        settings.max_height = Some(self.map.max_height);
        // Schema-driven per-field validation. Runs against every
        // FieldSpec, emits Error / Warning findings for out-of-
        // hard-range / out-of-soft-range values respectively. The
        // hand-rolled cross-field checks in `validate_project`
        // (wind range ordering, sun-dir non-zero, etc.) layer on
        // top so both surfaces produce findings into the same list.
        let mut findings =
            bar_project::validate_project(&self.graph, &settings, self.map.width, self.map.height);
        findings.extend(bar_project::field_schema::validate_with_schema(
            bar_project::recipe_fields::PHYSICS_SPECS,
            &settings,
        ));
        findings.extend(bar_project::field_schema::validate_with_schema(
            bar_project::recipe_fields::ATMOSPHERE_SPECS,
            &settings,
        ));
        findings.extend(bar_project::field_schema::validate_with_schema(
            bar_project::recipe_fields::FOG_SPECS,
            &settings,
        ));
        findings.extend(bar_project::field_schema::validate_with_schema(
            bar_project::recipe_fields::CLOUDS_SPECS,
            &settings,
        ));
        findings.extend(bar_project::field_schema::validate_with_schema(
            bar_project::recipe_fields::LIGHTING_SPECS,
            &settings,
        ));
        findings.extend(bar_project::field_schema::validate_with_schema(
            bar_project::recipe_fields::WATER_SPECS,
            &settings,
        ));
        findings.extend(bar_project::field_schema::validate_with_schema(
            bar_project::recipe_fields::GRASS_SPECS,
            &settings,
        ));
        self.validation.findings = findings;
    }
}
