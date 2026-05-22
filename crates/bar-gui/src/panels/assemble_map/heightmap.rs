//! Heightmap page -- pick the source heightmap file, derive dimensions
//! from its resolution, and capture min / max height in elmos.
//!
//! BAR / SMF heightmaps are `(W*64 + 1)` x `(H*64 + 1)` for some
//! integer square count (W, H) ≤ 512. The wizard validates the picked
//! file decodes cleanly to one of these shapes before letting the user
//! advance. PNG / TIFF / 16-bit grayscale paths all flow through the
//! `image` crate's normal decoder; the pixel data is read at Finish
//! time, not here -- this page only commits to the resolution.

use eframe::egui;

use crate::app::BarEditorApp;

pub(super) fn draw(app: &mut BarEditorApp, ui: &mut egui::Ui) {
    ui.label(
        "Pick a heightmap image. Resolution must be (W*64+1) x (H*64+1) \
         for some 1 <= W,H <= 512 -- e.g. 4097x4097 for a 64x64-square \
         map. PNG, TIFF, or 16-bit grayscale are accepted.",
    );
    ui.add_space(8.0);

    let parent = app.parent_window();
    let picked_label = app
        .assemble_map
        .picks
        .heightmap_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "(none)".to_string());

    let mut picked_now: Option<std::path::PathBuf> = None;
    ui.horizontal(|ui| {
        ui.label("File");
        if ui.small_button("Browse...").clicked() {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Select heightmap")
                .add_filter("Heightmap", &["png", "tif", "tiff"]);
            if let Some(parent) = parent.as_ref() {
                dialog = dialog.set_parent(parent);
            }
            picked_now = dialog.pick_file();
        }
        if app.assemble_map.picks.heightmap_path.is_some() && ui.small_button("Clear").clicked() {
            app.assemble_map.picks.heightmap_path = None;
            app.assemble_map.picks.squares_x = 0;
            app.assemble_map.picks.squares_z = 0;
            app.assemble_map.heightmap_error = None;
        }
        ui.label(picked_label);
    });

    if let Some(path) = picked_now {
        match probe_heightmap(&path) {
            Ok((sx, sz, min_h, max_h)) => {
                app.assemble_map.picks.heightmap_path = Some(path);
                app.assemble_map.picks.squares_x = sx;
                app.assemble_map.picks.squares_z = sz;
                if app.assemble_map.picks.min_height == 0.0
                    && app.assemble_map.picks.max_height == 0.0
                {
                    app.assemble_map.picks.min_height = min_h;
                    app.assemble_map.picks.max_height = max_h;
                }
                app.assemble_map.heightmap_error = None;
            }
            Err(e) => {
                app.assemble_map.picks.heightmap_path = None;
                app.assemble_map.picks.squares_x = 0;
                app.assemble_map.picks.squares_z = 0;
                app.assemble_map.heightmap_error = Some(e);
            }
        }
    }

    if let Some(ref err) = app.assemble_map.heightmap_error {
        ui.colored_label(egui::Color32::from_rgb(220, 100, 100), err);
    }

    if app.assemble_map.picks.squares_x > 0 {
        ui.add_space(8.0);
        ui.label(format!(
            "Dimensions: {} x {} squares  (heightmap {} x {})",
            app.assemble_map.picks.squares_x,
            app.assemble_map.picks.squares_z,
            app.assemble_map.picks.squares_x * 64 + 1,
            app.assemble_map.picks.squares_z * 64 + 1,
        ));

        ui.add_space(8.0);
        egui::Grid::new("assemble_map_heightmap_range")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("Min height");
                ui.add(
                    egui::DragValue::new(&mut app.assemble_map.picks.min_height)
                        .range(-2000.0..=4000.0)
                        .speed(1.0),
                );
                ui.end_row();
                ui.label("Max height");
                ui.add(
                    egui::DragValue::new(&mut app.assemble_map.picks.max_height)
                        .range(-2000.0..=4000.0)
                        .speed(1.0),
                );
                ui.end_row();
            });
        ui.weak(
            "Min < max. These set the elmo range the heightmap pixel \
             values map across; you can tune this later in the \
             Dimensions modal.",
        );
    }
}

/// Decode the heightmap image header just enough to derive
/// `(squares_x, squares_z, min_h, max_h)`. Reads the full pixel grid
/// once to populate the height range; the Finish handler will re-read
/// it through the proper import path.
fn probe_heightmap(path: &std::path::Path) -> Result<(u32, u32, f32, f32), String> {
    let img = image::open(path).map_err(|e| format!("Failed to decode image: {e}"))?;
    let (w, h) = (img.width(), img.height());
    if !w.is_multiple_of(1)
        || w < 65
        || h < 65
        || !(w - 1).is_multiple_of(64)
        || !(h - 1).is_multiple_of(64)
    {
        return Err(format!(
            "Heightmap must be (W*64+1) x (H*64+1); got {w} x {h}."
        ));
    }
    let sx = (w - 1) / 64;
    let sz = (h - 1) / 64;
    if sx == 0 || sz == 0 || sx > 512 || sz > 512 {
        return Err(format!(
            "Square count out of range: {sx} x {sz} (1 <= W,H <= 512)."
        ));
    }

    // Sample min / max from the pixel data so the defaults read out
    // reasonable height ranges. 8-bit images map to 0..255 in raw
    // elmos; 16-bit maps to 0..65535 -- both are wild defaults but
    // the user revises them on this page.
    let (min_v, max_v) = if let Some(buf) = img.as_luma16() {
        let mut lo = u16::MAX;
        let mut hi = 0u16;
        for &px in buf.iter() {
            if px < lo {
                lo = px;
            }
            if px > hi {
                hi = px;
            }
        }
        (lo as f32 / 65535.0, hi as f32 / 65535.0)
    } else {
        let g = img.to_luma8();
        let mut lo = u8::MAX;
        let mut hi = 0u8;
        for &px in g.iter() {
            if px < lo {
                lo = px;
            }
            if px > hi {
                hi = px;
            }
        }
        (lo as f32 / 255.0, hi as f32 / 255.0)
    };
    // Display in elmos -- assume the user wants 0..(800 * normalized
    // pixel-range) as the default range; they can revise.
    let min_h = (min_v * 800.0).round();
    let max_h = (max_v * 800.0).round().max(min_h + 1.0);
    Ok((sx, sz, min_h, max_h))
}
