//! Shared decoded-image preview cache for action-bar modals.
//!
//! Decodes a bitmap file (DDS or anything `image::ImageFormat` recognises)
//! exactly once per `(project_dir, filename)` key, downsamples it to
//! `max_side` before the GPU upload, and hands the resulting
//! `TextureHandle` back to the caller. The downsample keeps modal-open
//! latency bounded -- raw DXT decodes of 1024x1024+ source images
//! otherwise stall the UI thread on first paint.

use eframe::egui;

#[derive(Default)]
pub struct PreviewCache {
    key: Option<Key>,
    texture: Option<egui::TextureHandle>,
}

#[derive(PartialEq, Eq, Hash, Clone)]
struct Key {
    project_dir: std::path::PathBuf,
    filename: String,
    max_side: u32,
    label: &'static str,
}

impl PreviewCache {
    pub fn ensure<F>(
        &mut self,
        ctx: &egui::Context,
        project_dir: &std::path::Path,
        filename: &str,
        max_side: u32,
        label: &'static str,
        resolve: F,
    ) -> Option<&egui::TextureHandle>
    where
        F: FnOnce(&std::path::Path, &str) -> Option<std::path::PathBuf>,
    {
        let key = Key {
            project_dir: project_dir.to_path_buf(),
            filename: filename.to_string(),
            max_side,
            label,
        };
        if self.key.as_ref() == Some(&key) {
            return self.texture.as_ref();
        }
        self.key = Some(key);
        self.texture = resolve(project_dir, filename)
            .and_then(|p| decode_to_rgba(&p))
            .map(|(rgba, w, h)| {
                let (rgba, w, h) = downscale_to_max(rgba, w, h, max_side);
                let image =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                ctx.load_texture(label, image, egui::TextureOptions::LINEAR)
            });
        self.texture.as_ref()
    }

    pub fn invalidate(&mut self) {
        self.key = None;
        self.texture = None;
    }
}

fn decode_to_rgba(path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "dds" {
        if let Ok((rgba, w, h)) = bar_data::load_dds_2d(path) {
            return Some((rgba, w, h));
        }
    }
    let bytes = std::fs::read(path).ok()?;
    let fmt = image::ImageFormat::from_extension(&ext)?;
    let img = image::load_from_memory_with_format(&bytes, fmt).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some((rgba.into_raw(), w, h))
}

fn downscale_to_max(rgba: Vec<u8>, w: u32, h: u32, max_side: u32) -> (Vec<u8>, u32, u32) {
    let larger = w.max(h);
    if larger <= max_side || max_side == 0 {
        return (rgba, w, h);
    }
    let scale = max_side as f32 / larger as f32;
    let nw = (w as f32 * scale).round().max(1.0) as u32;
    let nh = (h as f32 * scale).round().max(1.0) as u32;
    let buf = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, rgba)
        .expect("rgba buffer matches w*h*4");
    let resized = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle);
    (resized.into_raw(), nw, nh)
}
