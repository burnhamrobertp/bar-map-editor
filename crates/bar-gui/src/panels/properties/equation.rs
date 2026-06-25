//! Contextual properties for the Equation node.
//!
//! A monospace formula editor bound to `params["formula"]`, a red parse-error
//! label when the expression doesn't compile, and a one-line variable legend.

use bar_graph::{NodeId, ParamValue};
use eframe::egui;

use crate::app::BarEditorApp;

impl BarEditorApp {
    pub(crate) fn draw_equation_properties(
        &mut self,
        ui: &mut egui::Ui,
        node_id: NodeId,
        params: &std::collections::HashMap<String, ParamValue>,
    ) {
        let mut formula = match params.get("formula") {
            Some(ParamValue::String(s)) => s.clone(),
            _ => "a".to_string(),
        };

        ui.label("Formula");
        let resp = ui.add(
            egui::TextEdit::multiline(&mut formula)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .font(egui::TextStyle::Monospace),
        );

        if let Err(e) = evalexpr::build_operator_tree::<evalexpr::DefaultNumericTypes>(&formula) {
            ui.colored_label(crate::panels::tokens::SEVERITY_ERROR, e.to_string());
        }

        ui.add_space(2.0);
        ui.label(egui::RichText::new("vars: a b c d, x y, h").weak());

        if resp.changed() {
            self.push_undo("Change parameter");
            if let Some(node) = self.graph.get_node_mut(node_id) {
                node.params.insert("formula".to_string(), ParamValue::String(formula));
                node.mark_dirty();
            }
        }
    }
}
