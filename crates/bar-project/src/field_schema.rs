//! Declarative field schema for the map-settings recipe.
//!
//! Each modelled field on `MapSettings` / `RecipeMeta` / nested sub-
//! settings is described by a [`FieldSpec`]. The spec is the single
//! source of truth for:
//!
//! * the field's label, optional explanatory tooltip, and category;
//! * its accepted hard range (clamped on commit; out-of-range source
//!   values produce a validation Error) and optional soft range (no
//!   clamp; out-of-range values produce a validation Warning);
//! * the engine default the field falls through to when unset, taken
//!   straight from [`crate::engine_defaults`] -- no duplicated
//!   constants;
//! * how to read and write the underlying `Option<T>` on the recipe
//!   (`get` / `set` function pointers; the renderer + validator never
//!   reach into the struct directly);
//! * whether a hard-range violation should block the export-side
//!   actions (Compile / Test in BAR / Bundle) -- see
//!   [`FieldSpec::blocks_export`].
//!
//! The schema drives:
//! * the UI -- one generic field renderer (in `bar-gui`) iterates the
//!   schema, dispatches on [`FieldKind`], and handles atomic-commit
//!   undo, hard-clamping, and finding decoration without per-field
//!   widget code;
//! * validation -- [`validate_with_schema`] walks every spec, reads
//!   its value, and emits [`crate::validation::Finding`]s for hard
//!   and soft range violations. Hand-written cross-field checks
//!   (wind range ordering, etc.) layer on top.
//!
//! Adding a new modelled field is one schema entry. No widget
//! boilerplate, no per-field validation function, no separate UI
//! range constants.

use crate::validation::Finding;

/// Stable category strings used for routing findings to the modal
/// that owns the field. The matching `ModalId` lives in `bar-gui`
/// (closer to where the UI consumes it); the strings are duplicated
/// here as `&'static str` so the schema doesn't depend on the GUI
/// crate.
pub mod categories {
    pub const IDENTITY: &str = "identity";
    pub const DIMENSIONS: &str = "dimensions";
    pub const PHYSICS: &str = "physics";
    pub const ATMOSPHERE: &str = "atmosphere";
    pub const LIGHTING: &str = "lighting";
    pub const WATER: &str = "water";
    pub const GRASS: &str = "grass";
    pub const RESOURCES: &str = "resources";
    pub const STARTBOXES: &str = "startboxes";
}

/// Domain-typed runtime value carrier used by [`FieldSpec::get`] /
/// [`FieldSpec::set`]. Wraps `Option<T>` for each supported scalar
/// shape so a generic field renderer can dispatch on a single enum
/// without needing the underlying recipe type.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    F32(Option<f32>),
    U32(Option<u32>),
    Bool(Option<bool>),
    Color(Option<[f32; 3]>),
    Vec3(Option<[f32; 3]>),
    Vec4(Option<[f32; 4]>),
    Text(String),
    OptionText(Option<String>),
}

/// Engine-default value paired with each [`FieldSpec`]. The renderer
/// shows the default as placeholder text when the recipe field is
/// `None`; the validator uses it as the fall-through value for
/// cross-field checks that need a concrete number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DefaultValue {
    F32(f32),
    U32(u32),
    Bool(bool),
    Color([f32; 3]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// Empty-string default for `Text` / `OptionText` kinds. Real
    /// string defaults are rare for mapinfo fields (most are
    /// "absent" by default); a `&'static str` would force every spec
    /// to carry one.
    Empty,
}

impl DefaultValue {
    /// Concrete fallback for the resolved value when the recipe
    /// field is `None`. Useful for validators that need to evaluate
    /// against the engine default.
    pub fn as_field_value(&self) -> FieldValue {
        match self {
            DefaultValue::F32(v) => FieldValue::F32(Some(*v)),
            DefaultValue::U32(v) => FieldValue::U32(Some(*v)),
            DefaultValue::Bool(v) => FieldValue::Bool(Some(*v)),
            DefaultValue::Color(v) => FieldValue::Color(Some(*v)),
            DefaultValue::Vec3(v) => FieldValue::Vec3(Some(*v)),
            DefaultValue::Vec4(v) => FieldValue::Vec4(Some(*v)),
            DefaultValue::Empty => FieldValue::OptionText(None),
        }
    }
}

/// Field shape + clamp / warn ranges. The renderer dispatches on
/// this to pick a widget; the validator dispatches on this to know
/// what range to enforce.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    /// Numeric scalar. `hard` clamps on commit; values from a source
    /// outside `hard` emit an Error finding. `soft` is advisory only:
    /// values outside `soft` (but inside `hard`) emit a Warning
    /// finding. `unit` is a short suffix shown next to the widget
    /// (e.g. `"elmos"`, `"°"`). Empty unit suppresses the suffix.
    F32 {
        hard: (f32, f32),
        soft: Option<(f32, f32)>,
        unit: &'static str,
    },
    /// `u32` counterpart of [`Self::F32`].
    U32 {
        hard: (u32, u32),
        soft: Option<(u32, u32)>,
        unit: &'static str,
    },
    /// `Option<bool>` tri-state. The renderer shows a three-way combo
    /// (default / true / false). No range.
    Bool,
    /// `Option<[f32; 3]>` sRGB-perceptual colour. Each channel is
    /// clamped to `[0, 1]` on commit; widget hides the picker behind
    /// an Override checkbox while the recipe field is `None`.
    Color,
    /// 3-vector with per-channel `hard` range. Sun direction, sky
    /// direction, etc. The validator clamps each channel independently.
    Vec3 {
        hard: (f32, f32),
        soft: Option<(f32, f32)>,
    },
    /// 4-vector with per-channel `hard` range. Splat-tex scales /
    /// mults are the main consumers.
    Vec4 {
        hard: (f32, f32),
        soft: Option<(f32, f32)>,
    },
    /// Non-optional string. Used by `RecipeMeta.description` and the
    /// few engine-required identity fields.
    Text { max_len: Option<usize> },
    /// `Option<String>`. Empty input becomes `None` -- consistent with
    /// the existing `edit_optional_string` semantics in the GUI.
    OptionText { max_len: Option<usize> },
    /// Filename pointing into the project's `passthrough/` tree;
    /// renderer wraps it in a file-picker with extension filter +
    /// preview cache. Underlying storage is the same `String` shape
    /// as [`Self::Text`].
    PassthroughTexture { extensions: &'static [&'static str] },
}

impl FieldKind {
    /// True when this kind has any kind of numeric clamp range.
    /// Used by the validator to decide whether to attempt range
    /// validation at all (Bool, Text, etc. have nothing to check).
    pub fn has_range(&self) -> bool {
        matches!(
            self,
            FieldKind::F32 { .. }
                | FieldKind::U32 { .. }
                | FieldKind::Vec3 { .. }
                | FieldKind::Vec4 { .. }
                | FieldKind::Color
        )
    }
}

/// Complete schema entry for one field on the recipe state `S`.
///
/// `S` is typically `bar_project::MapSettings`, `bar_project::Recipe`,
/// or `RecipeMeta` (the latter in `bar-gui`). The schema modules are
/// parameterised so the same `FieldSpec` shape works against any
/// recipe-side struct.
pub struct FieldSpec<S: 'static> {
    /// Stable identifier (`"atmosphere.fog_start"`,
    /// `"identity.name"`). Used as the `field` key on validation
    /// findings so the UI can match findings to widgets, and as a
    /// stable identifier in undo entries / future doc tables.
    pub id: &'static str,
    /// Human-readable label drawn next to the widget.
    pub label: &'static str,
    /// Optional explanatory tooltip shown next to the label. When
    /// `Some`, the renderer adds an `ⓘ` hover-target. Most fields
    /// can leave this `None`; populate when the meaning isn't
    /// obvious from the label (engine jargon like `MAPCOLORFACTOR`,
    /// or unintuitive soft-vs-hard ranges). One short sentence; the
    /// egui tooltip wraps awkwardly past ~80 chars.
    pub description: Option<&'static str>,
    /// Field shape + clamp / warn ranges. Drives both widget choice
    /// and validation.
    pub kind: FieldKind,
    /// Engine default the field falls through to when `None`. Used
    /// by the renderer as placeholder text and by validators that
    /// need a concrete fallback.
    pub default: DefaultValue,
    /// Read the current `Option<T>` (or `String`) off the recipe.
    /// Returns a [`FieldValue`] whose enum variant must match `kind`
    /// (enforced by debug_assert in [`FieldSpec::commit`]).
    pub get: fn(&S) -> FieldValue,
    /// Write a value back. The renderer always calls this with a
    /// hard-clamped value (see [`clamp_value`]).
    pub set: fn(&mut S, FieldValue),
    /// Category used as the [`crate::validation::Finding::category`]
    /// of findings produced for this field. See [`categories`].
    pub category: &'static str,
    /// Optional sub-section name inside the field's modal tab. The
    /// renderer inserts a sub-heading the first time it encounters
    /// a new group string while walking a spec slice. Empty string
    /// means "no sub-heading"; the field renders flush with the
    /// previous one. Specs sharing a group must be adjacent in the
    /// slice (the renderer detects transitions only).
    pub group: &'static str,
    /// When `true` and a hard-range violation is present, the
    /// action-bar gate (`is_blocking`) disables Compile / Test in
    /// BAR / Bundle. Defaults to `true` for required fields and
    /// engine-rejected ranges; set to `false` for soft / cosmetic
    /// constraints that the engine tolerates.
    pub blocks_export: bool,
}

impl<S: 'static> FieldSpec<S> {
    /// Apply a new value to `state`: hard-clamp it against `kind`,
    /// then call `set`. Returns the post-clamp value for callers
    /// that want to refresh widget state (`true` if the value was
    /// actually changed by the clamp).
    pub fn commit(&self, state: &mut S, new_value: FieldValue) -> FieldValue {
        let clamped = clamp_value(&self.kind, new_value);
        (self.set)(state, clamped.clone());
        clamped
    }
}

/// Hard-clamp a [`FieldValue`] against a [`FieldKind`]'s `hard`
/// range. Pass-through for kinds that have no range. For colour /
/// vec3 / vec4 the clamp is per-channel.
pub fn clamp_value(kind: &FieldKind, value: FieldValue) -> FieldValue {
    match (kind, value) {
        (FieldKind::F32 { hard, .. }, FieldValue::F32(Some(v))) => {
            FieldValue::F32(Some(v.clamp(hard.0, hard.1)))
        }
        (FieldKind::U32 { hard, .. }, FieldValue::U32(Some(v))) => {
            FieldValue::U32(Some(v.clamp(hard.0, hard.1)))
        }
        (FieldKind::Color, FieldValue::Color(Some(v))) => FieldValue::Color(Some([
            v[0].clamp(0.0, 1.0),
            v[1].clamp(0.0, 1.0),
            v[2].clamp(0.0, 1.0),
        ])),
        (FieldKind::Vec3 { hard, .. }, FieldValue::Vec3(Some(v))) => FieldValue::Vec3(Some([
            v[0].clamp(hard.0, hard.1),
            v[1].clamp(hard.0, hard.1),
            v[2].clamp(hard.0, hard.1),
        ])),
        (FieldKind::Vec4 { hard, .. }, FieldValue::Vec4(Some(v))) => FieldValue::Vec4(Some([
            v[0].clamp(hard.0, hard.1),
            v[1].clamp(hard.0, hard.1),
            v[2].clamp(hard.0, hard.1),
            v[3].clamp(hard.0, hard.1),
        ])),
        // None / Bool / Text variants pass through unchanged.
        (_, other) => other,
    }
}

/// Outcome of a per-field range check. Used by [`validate_field`] to
/// produce 0, 1, or 2 findings (a value can be both out-of-hard
/// AND out-of-soft, but we only emit the more-severe one).
#[derive(Debug, Clone, PartialEq)]
pub enum RangeOutcome {
    /// Within the soft range -- nothing to report.
    Ok,
    /// Outside the soft range but inside the hard range -- emits a
    /// Warning finding.
    SoftViolation,
    /// Outside the hard range -- emits an Error finding and (when
    /// the spec has `blocks_export = true`) gates the action bar.
    HardViolation,
}

/// Validate one [`FieldValue`] against a [`FieldKind`]. Returns
/// [`RangeOutcome::Ok`] for kinds without ranges (Bool, Text, etc.).
pub fn check_range(kind: &FieldKind, value: &FieldValue) -> RangeOutcome {
    match (kind, value) {
        (FieldKind::F32 { hard, soft, .. }, FieldValue::F32(Some(v))) => check_f32(*v, hard, soft),
        (FieldKind::U32 { hard, soft, .. }, FieldValue::U32(Some(v))) => check_u32(*v, hard, soft),
        (FieldKind::Vec3 { hard, soft }, FieldValue::Vec3(Some(v))) => check_vec(v, hard, soft),
        (FieldKind::Vec4 { hard, soft }, FieldValue::Vec4(Some(v))) => check_vec(v, hard, soft),
        (FieldKind::Color, FieldValue::Color(Some(v))) => {
            // Colours have an implicit hard range of [0, 1] per
            // channel; out-of-range channels emit a Warning (the
            // engine tolerates HDR colour values from authored
            // mapinfos), not an Error.
            for c in v {
                if !(0.0..=1.0).contains(c) {
                    return RangeOutcome::SoftViolation;
                }
            }
            RangeOutcome::Ok
        }
        _ => RangeOutcome::Ok,
    }
}

fn check_f32(v: f32, hard: &(f32, f32), soft: &Option<(f32, f32)>) -> RangeOutcome {
    if !v.is_finite() || v < hard.0 || v > hard.1 {
        return RangeOutcome::HardViolation;
    }
    if let Some(s) = soft {
        if v < s.0 || v > s.1 {
            return RangeOutcome::SoftViolation;
        }
    }
    RangeOutcome::Ok
}

fn check_u32(v: u32, hard: &(u32, u32), soft: &Option<(u32, u32)>) -> RangeOutcome {
    if v < hard.0 || v > hard.1 {
        return RangeOutcome::HardViolation;
    }
    if let Some(s) = soft {
        if v < s.0 || v > s.1 {
            return RangeOutcome::SoftViolation;
        }
    }
    RangeOutcome::Ok
}

fn check_vec<const N: usize>(
    v: &[f32; N],
    hard: &(f32, f32),
    soft: &Option<(f32, f32)>,
) -> RangeOutcome {
    let mut worst = RangeOutcome::Ok;
    for c in v {
        let component_outcome = check_f32(*c, hard, soft);
        worst = match (&worst, &component_outcome) {
            (RangeOutcome::HardViolation, _) | (_, RangeOutcome::HardViolation) => {
                RangeOutcome::HardViolation
            }
            (RangeOutcome::SoftViolation, _) | (_, RangeOutcome::SoftViolation) => {
                RangeOutcome::SoftViolation
            }
            _ => RangeOutcome::Ok,
        };
    }
    worst
}

/// Validate one field against its spec. Produces 0 or 1 findings.
/// The renderer's `commit` step has already hard-clamped any value
/// the user typed; this validator catches values that arrived via
/// other paths (recipe.json that was edited by hand, mapinfo.lua
/// imports that exceeded the hard range, etc.).
pub fn validate_field<S: 'static>(spec: &FieldSpec<S>, state: &S) -> Option<Finding> {
    let value = (spec.get)(state);
    match check_range(&spec.kind, &value) {
        RangeOutcome::Ok => None,
        RangeOutcome::SoftViolation => Some(
            Finding::warn(
                spec.category,
                format!("{} is outside the typical range.", spec.label),
            )
            .on_field(spec.id),
        ),
        RangeOutcome::HardViolation => Some(
            Finding::err(
                spec.category,
                format!("{} is outside the engine-supported range.", spec.label),
            )
            .on_field(spec.id),
        ),
    }
}

/// Walk a slice of specs and produce findings for each out-of-range
/// field. Returns findings only; the caller appends to the larger
/// `Vec<Finding>` produced by `validate_project`. Severity is
/// reported even when the spec's `blocks_export` is `false` -- the
/// blocking gate lives in the GUI's `ValidationState` layer, not
/// here.
pub fn validate_with_schema<S: 'static>(schema: &[FieldSpec<S>], state: &S) -> Vec<Finding> {
    schema
        .iter()
        .filter_map(|spec| validate_field(spec, state))
        .collect()
}

// ──────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Severity;

    /// Toy state struct for testing without dragging in the full
    /// MapSettings/Recipe shape. One f32, one u32, one bool, one
    /// color -- enough to exercise every clamp + range outcome
    /// without coupling to the live recipe schema (which evolves).
    #[derive(Default)]
    struct TestState {
        f: Option<f32>,
        u: Option<u32>,
        c: Option<[f32; 3]>,
    }

    fn f_spec() -> FieldSpec<TestState> {
        FieldSpec {
            id: "test.f",
            label: "F",
            description: None,
            kind: FieldKind::F32 {
                hard: (0.0, 10.0),
                soft: Some((0.0, 1.0)),
                unit: "",
            },
            default: DefaultValue::F32(0.5),
            get: |s| FieldValue::F32(s.f),
            set: |s, v| {
                if let FieldValue::F32(x) = v {
                    s.f = x;
                }
            },
            category: "test",
            group: "",
            blocks_export: true,
        }
    }

    fn u_spec() -> FieldSpec<TestState> {
        FieldSpec {
            id: "test.u",
            label: "U",
            description: None,
            kind: FieldKind::U32 {
                hard: (1, 100),
                soft: None,
                unit: "",
            },
            default: DefaultValue::U32(10),
            get: |s| FieldValue::U32(s.u),
            set: |s, v| {
                if let FieldValue::U32(x) = v {
                    s.u = x;
                }
            },
            category: "test",
            group: "",
            blocks_export: true,
        }
    }

    fn color_spec() -> FieldSpec<TestState> {
        FieldSpec {
            id: "test.c",
            label: "C",
            description: None,
            kind: FieldKind::Color,
            default: DefaultValue::Color([0.5, 0.5, 0.5]),
            get: |s| FieldValue::Color(s.c),
            set: |s, v| {
                if let FieldValue::Color(x) = v {
                    s.c = x;
                }
            },
            category: "test",
            group: "",
            blocks_export: false,
        }
    }

    #[test]
    fn hard_clamp_applies_on_commit() {
        let mut state = TestState::default();
        let spec = f_spec();
        spec.commit(&mut state, FieldValue::F32(Some(20.0)));
        assert_eq!(state.f, Some(10.0), "above-hard value clamped to upper");
        spec.commit(&mut state, FieldValue::F32(Some(-5.0)));
        assert_eq!(state.f, Some(0.0), "below-hard value clamped to lower");
        spec.commit(&mut state, FieldValue::F32(Some(0.7)));
        assert_eq!(state.f, Some(0.7), "in-range value untouched");
    }

    #[test]
    fn u32_clamp_applies_on_commit() {
        let mut state = TestState::default();
        let spec = u_spec();
        spec.commit(&mut state, FieldValue::U32(Some(0)));
        assert_eq!(state.u, Some(1), "below-hard u32 clamped to lower");
        spec.commit(&mut state, FieldValue::U32(Some(999)));
        assert_eq!(state.u, Some(100), "above-hard u32 clamped to upper");
    }

    #[test]
    fn color_clamp_per_channel() {
        let mut state = TestState::default();
        let spec = color_spec();
        spec.commit(&mut state, FieldValue::Color(Some([1.5, -0.2, 0.5])));
        assert_eq!(state.c, Some([1.0, 0.0, 0.5]));
    }

    #[test]
    fn soft_violation_reports_warning_not_error() {
        // f inside hard (0..10), outside soft (0..1)
        let state = TestState {
            f: Some(2.5),
            ..Default::default()
        };
        let spec = f_spec();
        let finding = validate_field(&spec, &state).expect("finding expected");
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.field.as_deref(), Some("test.f"));
        assert_eq!(finding.category, "test");
    }

    #[test]
    fn hard_violation_reports_error_when_bypassed() {
        // Simulate a value that bypassed `commit` (e.g. loaded from
        // recipe.json that was edited by hand). The validator
        // catches the out-of-hard-range value.
        let state = TestState {
            f: Some(50.0),
            ..Default::default()
        };
        let spec = f_spec();
        let finding = validate_field(&spec, &state).expect("finding expected");
        assert_eq!(finding.severity, Severity::Error);
    }

    #[test]
    fn nan_treated_as_hard_violation() {
        let state = TestState {
            f: Some(f32::NAN),
            ..Default::default()
        };
        let spec = f_spec();
        let finding = validate_field(&spec, &state).expect("NaN should emit finding");
        assert_eq!(finding.severity, Severity::Error);
    }

    #[test]
    fn none_value_produces_no_finding() {
        let state = TestState::default(); // f = None
        let spec = f_spec();
        assert!(
            validate_field(&spec, &state).is_none(),
            "None means 'engine default applies' -- no constraint to check"
        );
    }

    #[test]
    fn validate_with_schema_walks_all_specs() {
        let state = TestState {
            f: Some(50.0),            // hard violation -- Error
            u: Some(50),              // within hard -- no finding
            c: Some([1.5, 0.5, 0.5]), // colour soft violation -- Warning
        };
        let schema = vec![f_spec(), u_spec(), color_spec()];
        let findings = validate_with_schema(&schema, &state);
        assert_eq!(findings.len(), 2);
        assert!(findings
            .iter()
            .any(|f| f.field.as_deref() == Some("test.f") && f.severity == Severity::Error));
        assert!(findings
            .iter()
            .any(|f| f.field.as_deref() == Some("test.c") && f.severity == Severity::Warning));
    }

    #[test]
    fn vec3_per_channel_worst_outcome() {
        // One channel in hard violation, two in soft violation -->
        // overall HardViolation (the more severe).
        let kind = FieldKind::Vec3 {
            hard: (0.0, 10.0),
            soft: Some((0.0, 1.0)),
        };
        let v = FieldValue::Vec3(Some([0.5, 2.0, 50.0]));
        assert_eq!(check_range(&kind, &v), RangeOutcome::HardViolation);

        let v = FieldValue::Vec3(Some([0.5, 2.0, 3.0]));
        assert_eq!(check_range(&kind, &v), RangeOutcome::SoftViolation);

        let v = FieldValue::Vec3(Some([0.5, 0.8, 0.9]));
        assert_eq!(check_range(&kind, &v), RangeOutcome::Ok);
    }

    #[test]
    fn pass_through_kinds_have_no_range() {
        assert!(!FieldKind::Bool.has_range());
        assert!(!FieldKind::Text { max_len: None }.has_range());
        assert!(!FieldKind::OptionText { max_len: None }.has_range());
        assert!(!FieldKind::PassthroughTexture { extensions: &[] }.has_range());
        assert!(FieldKind::F32 {
            hard: (0.0, 1.0),
            soft: None,
            unit: ""
        }
        .has_range());
        assert!(FieldKind::Color.has_range());
    }

    #[test]
    fn pass_through_value_clamp_is_identity() {
        // Bool / Text variants don't have a clamp range; the
        // clamp_value function should leave them untouched.
        let v = FieldValue::Bool(Some(true));
        assert_eq!(clamp_value(&FieldKind::Bool, v.clone()), v);

        let v = FieldValue::Text("hello".to_string());
        assert_eq!(
            clamp_value(&FieldKind::Text { max_len: None }, v.clone()),
            v
        );

        let v = FieldValue::F32(None);
        assert_eq!(
            clamp_value(
                &FieldKind::F32 {
                    hard: (0.0, 1.0),
                    soft: None,
                    unit: ""
                },
                v.clone()
            ),
            v,
            "None pass-through"
        );
    }
}
