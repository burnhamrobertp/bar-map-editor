//! `FieldModel` + descriptor mapping that renders node parameters
//! through the same `render_field` the settings modals use.
//!
//! Node params are always concrete (no tri-state `Option`), so "at
//! default" means the stored value equals the registry default, and a
//! revert (a `None`-variant write) resets to that default rather than
//! clearing to `None`. Everything else -- control choice, commit timing,
//! clamp, tooltip, undo -- is the shared renderer's job.

use bar_graph::{NodeId, NodeType, ParamValue};
use bar_project::field_schema::{clamp_value, DefaultValue, FieldKind, FieldValue};

use crate::app::BarEditorApp;
use crate::panels::field_editor::{FieldDesc, FieldModel};

/// Build a render descriptor for one node param from the registry, or
/// `None` for kinds the generic grid skips (Vec2 / Spline). `default`
/// is the registry default value; its variant selects the numeric kind
/// and, with the registry's range/choice/colour lookups, the widget.
pub(crate) fn param_desc<'a>(
    node_type: &NodeType,
    key: &'a str,
    default: &ParamValue,
) -> Option<FieldDesc<'a>> {
    let kind = kind_for(node_type, key, default)?;
    let default = default_value(default, &kind);
    Some(FieldDesc {
        id: key,
        label: key,
        description: bar_graph::param_description(node_type, key),
        kind,
        default,
    })
}

fn kind_for(node_type: &NodeType, key: &str, sample: &ParamValue) -> Option<FieldKind> {
    Some(match sample {
        ParamValue::Float(_) => match bar_graph::param_float_range(node_type, key) {
            Some((mn, mx)) => FieldKind::F32 {
                hard: (mn, mx),
                soft: None,
                unit: "",
            },
            None => FieldKind::FloatFree,
        },
        ParamValue::UInt(_) => match bar_graph::param_uint_range(node_type, key) {
            Some((mn, mx)) => FieldKind::U32 {
                hard: (mn, mx),
                soft: None,
                unit: "",
            },
            None => FieldKind::UIntFree,
        },
        ParamValue::Int(_) => FieldKind::IntFree,
        ParamValue::Bool(_) => FieldKind::Bool,
        ParamValue::String(_) => {
            if let Some(opts) = bar_graph::param_choices(node_type, key) {
                FieldKind::Choices(opts)
            } else if bar_graph::param_is_color(node_type, key) {
                FieldKind::Color
            } else {
                FieldKind::Text { max_len: None }
            }
        }
        ParamValue::Vec2(_) | ParamValue::Spline(_) => return None,
    })
}

fn default_value(default: &ParamValue, kind: &FieldKind) -> DefaultValue {
    match default {
        ParamValue::Float(f) => DefaultValue::F32(*f),
        ParamValue::UInt(u) => DefaultValue::U32(*u),
        ParamValue::Int(i) => DefaultValue::I32(*i),
        ParamValue::Bool(b) => DefaultValue::Bool(*b),
        ParamValue::String(s) if matches!(kind, FieldKind::Color) => {
            DefaultValue::Color(hex_to_srgb(s))
        }
        // Choices / free text have no numeric default tick; revert picks
        // the default option from the dropdown instead.
        _ => DefaultValue::Empty,
    }
}

/// [`FieldModel`] over one node param. Reads/writes `node.params[key]`;
/// `commit` applies registry side effects, marks the node dirty (the
/// re-eval trigger), and pushes the pre-edit snapshot.
pub(crate) struct NodeParamField<'a> {
    app: &'a mut BarEditorApp,
    node_id: NodeId,
    node_type: NodeType,
    key: String,
    kind: FieldKind,
    default: ParamValue,
    wired: bool,
}

impl<'a> NodeParamField<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        app: &'a mut BarEditorApp,
        node_id: NodeId,
        node_type: NodeType,
        key: String,
        kind: FieldKind,
        default: ParamValue,
        wired: bool,
    ) -> Self {
        Self {
            app,
            node_id,
            node_type,
            key,
            kind,
            default,
            wired,
        }
    }

    fn current(&self) -> Option<ParamValue> {
        self.app
            .graph
            .get_node(self.node_id)
            .and_then(|n| n.params.get(&self.key).cloned())
    }
}

impl FieldModel for NodeParamField<'_> {
    fn value(&self) -> FieldValue {
        let pv = self.current().unwrap_or_else(|| self.default.clone());
        param_to_field(&pv, &self.kind)
    }

    fn set_value(&mut self, v: FieldValue) {
        if self.wired {
            return;
        }
        let clamped = clamp_value(&self.kind, v);
        // A `None` write is the renderer's revert -> fall back to default.
        let pv = field_to_param(clamped).unwrap_or_else(|| self.default.clone());
        if self.app.dialog.field_edit_in_progress.is_none() {
            let snap = self.app.snapshot(&format!("Edit {}", self.key));
            self.app.dialog.field_edit_in_progress = Some(snap);
        }
        if let Some(node) = self.app.graph.get_node_mut(self.node_id) {
            node.params.insert(self.key.clone(), pv);
        }
    }

    fn commit(&mut self) {
        if self.wired {
            return;
        }
        if let Some(snap) = self.app.dialog.field_edit_in_progress.take() {
            self.app.history.push(snap);
        }
        // Registry side effects (e.g. biome -> rock_color) ride on commit
        // so a drag doesn't reset siblings every frame.
        if let Some(pv) = self.current() {
            let effects = bar_graph::param_side_effects(&self.node_type, &self.key, &pv);
            if let Some(node) = self.app.graph.get_node_mut(self.node_id) {
                for (k, ev) in effects {
                    node.params.insert(k, ev);
                }
                node.mark_dirty();
            }
        }
        self.app.mark_dirty();
    }

    fn is_at_default(&self) -> bool {
        match self.current() {
            Some(pv) => pv == self.default,
            None => true,
        }
    }
}

fn param_to_field(pv: &ParamValue, kind: &FieldKind) -> FieldValue {
    match pv {
        ParamValue::Float(f) => FieldValue::F32(Some(*f)),
        ParamValue::UInt(u) => FieldValue::U32(Some(*u)),
        ParamValue::Int(i) => FieldValue::I32(Some(*i)),
        ParamValue::Bool(b) => FieldValue::Bool(Some(*b)),
        ParamValue::String(s) if matches!(kind, FieldKind::Color) => {
            FieldValue::Color(Some(hex_to_srgb(s)))
        }
        ParamValue::String(s) => FieldValue::Text(s.clone()),
        // Skipped by `param_desc`; never rendered.
        ParamValue::Vec2(_) | ParamValue::Spline(_) => FieldValue::Text(String::new()),
    }
}

fn field_to_param(fv: FieldValue) -> Option<ParamValue> {
    Some(match fv {
        FieldValue::F32(Some(f)) => ParamValue::Float(f),
        FieldValue::U32(Some(u)) => ParamValue::UInt(u),
        FieldValue::I32(Some(i)) => ParamValue::Int(i),
        FieldValue::Bool(Some(b)) => ParamValue::Bool(b),
        FieldValue::Color(Some(rgb)) => ParamValue::String(srgb_to_hex(rgb)),
        FieldValue::Text(s) => ParamValue::String(s),
        // `None` variants are the renderer's revert signal.
        _ => return None,
    })
}

fn hex_to_srgb(s: &str) -> [f32; 3] {
    let s = s.trim_start_matches('#');
    if s.len() < 6 {
        return [0.5, 0.5, 0.5];
    }
    let ch = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(128) as f32 / 255.0;
    [ch(0), ch(2), ch(4)]
}

fn srgb_to_hex(rgb: [f32; 3]) -> String {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{:02X}{:02X}{:02X}", c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_graph::{Node, NodeId};

    fn app_with_node() -> (BarEditorApp, NodeId) {
        let mut app = BarEditorApp::default();
        let id = app
            .graph
            .add_node(Node::new(NodeId(0), NodeType::PerlinNoise, "n"));
        (app, id)
    }

    // D: every param of every registered node maps to a descriptor or is
    // intentionally skipped -- never panics. Fails the build the day a new
    // param/kind the renderer can't handle is added.
    #[test]
    fn every_node_param_maps_or_skips() {
        for def in bar_graph::nodes::all_defs() {
            let nt = def.node_type.clone();
            for (key, dv) in bar_graph::default_params(&nt) {
                let _ = param_desc(&nt, &key, &dv);
            }
        }
    }

    // A: ParamValue <-> FieldValue is total for the rendered variants.
    #[test]
    fn value_conversions_round_trip() {
        let cases = [
            (ParamValue::Float(1.5), FieldKind::FloatFree),
            (ParamValue::UInt(7), FieldKind::UIntFree),
            (ParamValue::Int(-3), FieldKind::IntFree),
            (ParamValue::Bool(true), FieldKind::Bool),
            (
                ParamValue::String("hi".into()),
                FieldKind::Text { max_len: None },
            ),
            (ParamValue::String("FF8000".into()), FieldKind::Color),
        ];
        for (pv, kind) in cases {
            let fv = param_to_field(&pv, &kind);
            assert_eq!(
                field_to_param(fv),
                Some(pv.clone()),
                "round-trip failed for {pv:?}"
            );
        }
    }

    // B: a node-param edit writes the graph, pushes exactly one undo entry,
    // and undo restores the pre-edit value.
    #[test]
    fn node_param_commit_undo_round_trips() {
        let (mut app, id) = app_with_node();
        app.graph
            .get_node_mut(id)
            .unwrap()
            .params
            .insert("frequency".into(), ParamValue::Float(2.5));
        let depth = app.history.undo_depth();
        {
            let mut m = NodeParamField::new(
                &mut app,
                id,
                NodeType::PerlinNoise,
                "frequency".into(),
                FieldKind::FloatFree,
                ParamValue::Float(2.5),
                false,
            );
            m.set_value(FieldValue::F32(Some(9.9)));
            m.commit();
        }
        let stored = app
            .graph
            .get_node(id)
            .unwrap()
            .params
            .get("frequency")
            .cloned();
        assert_eq!(stored, Some(ParamValue::Float(9.9)));
        assert_eq!(app.history.undo_depth(), depth + 1);
        app.undo();
        let restored = app
            .graph
            .get_node(id)
            .unwrap()
            .params
            .get("frequency")
            .cloned();
        assert_eq!(
            restored,
            Some(ParamValue::Float(2.5)),
            "undo restores value"
        );
    }

    // B: a None write (the renderer's revert) resets to the registry default.
    #[test]
    fn node_param_revert_writes_default() {
        let (mut app, id) = app_with_node();
        app.graph
            .get_node_mut(id)
            .unwrap()
            .params
            .insert("frequency".into(), ParamValue::Float(9.9));
        {
            let mut m = NodeParamField::new(
                &mut app,
                id,
                NodeType::PerlinNoise,
                "frequency".into(),
                FieldKind::FloatFree,
                ParamValue::Float(2.5),
                false,
            );
            assert!(!m.is_at_default());
            m.set_value(FieldValue::F32(None)); // revert
            m.commit();
            assert!(m.is_at_default());
        }
        let stored = app
            .graph
            .get_node(id)
            .unwrap()
            .params
            .get("frequency")
            .cloned();
        assert_eq!(stored, Some(ParamValue::Float(2.5)));
    }

    // B: a wired param is read-only -- writes and commits are no-ops.
    #[test]
    fn wired_param_is_read_only() {
        let (mut app, id) = app_with_node();
        app.graph
            .get_node_mut(id)
            .unwrap()
            .params
            .insert("frequency".into(), ParamValue::Float(2.5));
        let depth = app.history.undo_depth();
        {
            let mut m = NodeParamField::new(
                &mut app,
                id,
                NodeType::PerlinNoise,
                "frequency".into(),
                FieldKind::FloatFree,
                ParamValue::Float(2.5),
                true, // wired
            );
            m.set_value(FieldValue::F32(Some(9.9)));
            m.commit();
        }
        let stored = app
            .graph
            .get_node(id)
            .unwrap()
            .params
            .get("frequency")
            .cloned();
        assert_eq!(stored, Some(ParamValue::Float(2.5)), "wired write ignored");
        assert_eq!(app.history.undo_depth(), depth, "wired pushes no undo");
    }
}
