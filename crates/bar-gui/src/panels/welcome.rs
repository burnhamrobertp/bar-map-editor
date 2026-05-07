//! Welcome panel — replaces the blank canvas before any project is
//! loaded. Surfaces a curated set of preset macros as cards, plus
//! "Open project / SD7…" and "Empty graph" entry points and a
//! recent-files menu. Stateless: every interaction routes through
//! `BarEditorApp` methods (`start_with_macro`, `start_open_path`,
//! and the inline blank-project drop) so other layouts can call
//! `panels::welcome::draw` without coordination.

use eframe::egui;

use crate::app::BarEditorApp;
use crate::t;

/// Curated starter macros surfaced on the welcome screen — one per
/// category. Each entry is `(localization-stem, BUILTIN_MACROS name)`.
/// Clicking a card drops the macro into a fresh project. Display
/// strings are looked up at render time via
/// `t!("editor.templates.<stem>.{name,description}")` so they
/// localise alongside the rest of the UI.
const WELCOME_TEMPLATES: &[(&str, &str)] = &[
    ("plains", "Plains"),
    ("mountain_range", "Mountain Range"),
    ("archipelago", "Archipelago"),
    ("canyon", "Canyon"),
    ("dunes", "Dunes"),
];

/// Render the welcome panel into `ui`. Caller decides when to
/// display it (today: when `graph.nodes()` is empty AND no project
/// is loaded, in `BarEditorApp::draw_node_graph`).
pub(crate) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    let available = ui.available_size();
    let (rect, _resp) = ui.allocate_exact_size(available, egui::Sense::hover());
    // Match the regular canvas backdrop so the transition into a
    // loaded project doesn't flash a different colour.
    ui.painter().rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

    // Centred card-list. We measure heading + body + each card and
    // lay them out vertically inside a max-width rect so the
    // welcome stays readable on huge screens.
    let max_width: f32 = 720.0;
    let target_w = available.x.min(max_width);
    let panel_left = rect.left() + (rect.width() - target_w) * 0.5;
    let panel_top = rect.top() + 32.0;
    let panel_rect = egui::Rect::from_min_size(
        egui::pos2(panel_left, panel_top),
        egui::vec2(target_w, available.y - 64.0),
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(panel_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    child.heading(t!("editor.welcome.heading"));
    child.add_space(4.0);
    child.weak(t!("editor.welcome.tagline"));
    child.add_space(18.0);

    // Macro template cards — one per category. Clicking drops
    // the macro into a fresh canvas with a Bundler pre-wired so
    // the user can immediately tune knobs or export.
    let mut to_drop: Option<&'static str> = None;
    for &(stem, macro_name) in WELCOME_TEMPLATES {
        let title_key = format!("editor.templates.{stem}.name");
        let desc_key = format!("editor.templates.{stem}.description");
        let title = t!(&title_key).to_string();
        let desc = t!(&desc_key).to_string();
        let card = egui::Frame::group(child.style())
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(&mut child, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&title).strong().size(15.0));
                        ui.add_space(2.0);
                        ui.weak(&desc);
                    });
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button(t!("editor.welcome.use_this")).clicked() {
                                to_drop = Some(macro_name);
                            }
                        },
                    );
                });
            });
        if card.response.interact(egui::Sense::click()).clicked() {
            to_drop = Some(macro_name);
        }
        child.add_space(6.0);
    }

    // "Or" separator — slightly smaller than the buttons below,
    // visually pulls the two start-paths apart from the templates
    // above.
    child.add_space(20.0);
    child.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.label(
            egui::RichText::new(t!("editor.welcome.or"))
                .size(13.0)
                .weak(),
        );
    });
    child.add_space(10.0);

    // Two large side-by-side buttons. Sized to a fixed minimum so
    // they read as the primary affordances, centered as a pair.
    let btn_w = 200.0_f32;
    let btn_h = 44.0_f32;
    let gap = 16.0_f32;
    let pair_w = btn_w * 2.0 + gap;
    let avail = child.available_width();
    let lpad = ((avail - pair_w) * 0.5).max(0.0);
    let mut clicked_blank = false;
    let mut clicked_open = false;
    child.horizontal(|ui| {
        ui.add_space(lpad);
        if ui
            .add_sized(
                [btn_w, btn_h],
                egui::Button::new(
                    egui::RichText::new(t!("editor.welcome.blank_project")).size(15.0),
                ),
            )
            .clicked()
        {
            clicked_blank = true;
        }
        ui.add_space(gap);
        if ui
            .add_sized(
                [btn_w, btn_h],
                egui::Button::new(
                    egui::RichText::new(t!("editor.welcome.open_project")).size(15.0),
                ),
            )
            .clicked()
        {
            clicked_open = true;
        }
    });

    // Recent menu kept available but visually de-emphasised below
    // the primary buttons.
    if !app.settings().recent_files.is_empty() {
        child.add_space(12.0);
        child.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.menu_button(
                egui::RichText::new(t!("editor.welcome.recent"))
                    .size(12.0)
                    .weak(),
                |ui| {
                    let recents = app.settings().recent_files.clone();
                    for p in recents {
                        let label = p
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.display().to_string());
                        if ui.button(label).clicked() {
                            app.start_open_path_for_panel(p);
                            ui.close_menu();
                        }
                    }
                },
            );
        });
    }

    // Apply actions outside the layout closures (avoids
    // borrow-conflict on `app` inside `child.horizontal(...)`).
    if clicked_blank {
        app.welcome_blank_project();
    }
    if clicked_open {
        app.welcome_open_dialog();
    }
    if let Some(macro_name) = to_drop {
        app.start_with_macro(macro_name);
    }
}

// Cross-module shims (`welcome_blank_project`, `welcome_open_dialog`,
// `start_open_path_for_panel`) live in `crate::app` because they
// touch private fields. The panel only consumes them via
// `BarEditorApp`'s `pub(crate)` API — keeps panels stateless and
// app.rs the sole owner of `BarEditorApp` internals.
