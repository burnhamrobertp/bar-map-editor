//! Metal-spot overlay for the 3D viewport.
//!
//! Renders the same cyan circles + floating worth labels that
//! `bar-game/luaui/Widgets/gui_metalspots.lua` paints over each
//! detected metalmap cluster in-engine. The Rust side of the
//! spot-finder lives in `bar_project::metal_spots`; this module
//! owns the projection + paint.
//!
//! The widget's worth gate (`valueToText(worth * incomeMultiplier
//! / 1000) > 0.001 && < 15`) is mirrored here so a stray noise pixel
//! in a high-metal map doesn't draw a hundred labels at 0.0
//! metal/sec. Style intentionally diverges from the engine widget's
//! rotating animation (BME's overlay is static, no instance VBO);
//! it carries the same information at a glance.

use bar_project::MetalSpot;
use eframe::egui;
use glam::{Mat4, Vec3, Vec4};

/// Ring radius (elmos) the engine widget actually draws. The vertex
/// shader multiplies `outersize = 1.98` by `(12 + dir) * 2 * scale`,
/// so for a typical scale of ~0.77 the on-screen ring lands at
/// roughly 40 elmos. Spot scale only varies by a few percent for the
/// modal cluster sizes BAR maps ship, so a fixed radius reads the
/// same.
const RING_RADIUS_ELMOS: f32 = 40.0;

/// Engine widget renders 8 broken arc segments per direction; the
/// visible gaps between arcs are what make the ring read as
/// engine-style rather than a solid circle.
const RING_ARC_COUNT: usize = 8;
/// Fraction of each `360 / RING_ARC_COUNT` segment that's filled;
/// the remainder is the gap. 0.55 approximates the engine widget's
/// `circleSpaceUsage = 0.62` minus the counter-rotation overlap.
const RING_ARC_FRACTION: f32 = 0.55;
/// Line samples per arc. 6 is enough at typical zoom for the curve
/// to read smoothly.
const RING_ARC_SAMPLES: usize = 6;

/// Centre dot radius (elmos). Engine widget draws an inner "free /
/// occupied" indicator; BME has no unit state so it's always "free"
/// (cyan).
const CENTRE_DOT_ELMOS: f32 = 6.0;

/// Vertical offset (elmos) lifting the ring above the terrain to
/// avoid z-fighting at oblique angles. Engine widget uses
/// `groundHeight + 3` -- match that magnitude.
const GROUND_OFFSET_ELMOS: f32 = 3.0;

/// World-space lift (elmos) for the worth label so it floats clear
/// of the ring instead of sitting on top of it.
const LABEL_LIFT_ELMOS: f32 = 25.0;

/// Label glyph height in elmos. The engine widget billboards its
/// pre-rendered text atlas at a fraction of `TEXTHEIGHT` in world
/// units (`bbpos.y * 0.25` in the vert shader, with `TEXTHEIGHT =
/// fontfileSize + outline = ~122`), so the label scales with the
/// camera the same way the rings do. We pick a fixed elmo size and
/// derive the projected pixel size at paint time so BME's label
/// follows the same world-scaled feel.
const LABEL_GLYPH_HEIGHT_ELMOS: f32 = 18.0;
/// Pixel clamps applied to the projected label size so the text
/// stays legible at extreme zooms (matches the engine widget --
/// the atlas resolution gates how small the text can shrink before
/// it goes illegible, and a very close camera shouldn't blow the
/// badge up to fill the viewport).
const LABEL_FONT_MIN_PX: f32 = 8.0;
const LABEL_FONT_MAX_PX: f32 = 42.0;

/// Ray-march sample count for the camera-to-sample occlusion test.
/// Same density as the sun gizmo's rings -- 128 is plenty inside a
/// typical map AABB and stays cheap even with dozens of spots.
const OCCLUSION_STEPS: u32 = 128;

/// Lower display gate, in metal/sec (`worth * max_metal / 1000`).
/// Below this the label rounds to "0.0", which is just noise on
/// the screen.
const MIN_DISPLAY_VALUE: f32 = 0.05;
/// Upper gate (`maxValue = 15` in `gui_metalspots`). Above this the
/// widget assumes the whole surface is metal and skips drawing.
const MAX_DISPLAY_VALUE: f32 = 15.0;

/// Map-extent inputs needed to project an elmo-space metal-spot
/// position into world (render) space. Caller derives these from
/// the current preview frame + map settings.
#[derive(Clone, Copy)]
pub struct OverlayDims {
    /// Map width in heightmap samples (Spring `mapx + 1`).
    pub map_w: u32,
    /// Map height in heightmap samples.
    pub map_h: u32,
    /// Map's vertical range in elmos (used to convert terrain Y at
    /// the spot's XZ back to render space).
    pub min_height: f32,
    pub max_height: f32,
    /// Renderer's half-spans in render space.
    pub x_extent: f32,
    pub z_extent: f32,
    /// Renderer's Y scale (height_scale -- f32 heightmap [0,1] *
    /// height_scale = render-space Y).
    pub height_scale: f32,
    /// `mapinfo.smf.maxmetal`. The engine's `spGetGroundInfo`
    /// returns `byte * maxmetal`, and the widget displays
    /// `worth / 1000` where `worth` sums those scaled cells. BME's
    /// spot finder accumulates raw bytes, so we apply this scale
    /// here to match the in-engine label.
    pub max_metal: f32,
}

impl OverlayDims {
    /// Playable map width in elmos. Spring `mapx = samples - 1`,
    /// each cell 8 elmos wide.
    pub fn map_w_elmos(&self) -> f32 {
        ((self.map_w.saturating_sub(1)).max(1) as f32) * 8.0
    }
    pub fn map_h_elmos(&self) -> f32 {
        ((self.map_h.saturating_sub(1)).max(1) as f32) * 8.0
    }
}

fn spot_world_pos(
    spot: &MetalSpot,
    dims: &OverlayDims,
    heightmap: Option<&bar_data::Heightmap>,
) -> Vec3 {
    let rx = (spot.x_elmo / dims.map_w_elmos() - 0.5) * 2.0 * dims.x_extent;
    let rz = (spot.z_elmo / dims.map_h_elmos() - 0.5) * 2.0 * dims.z_extent;
    let height_range = (dims.max_height - dims.min_height).abs().max(1.0);
    let ground_y = heightmap
        .and_then(|hm| {
            bar_render::terrain_y_at_world_xz(
                rx,
                rz,
                hm,
                dims.x_extent,
                dims.z_extent,
                dims.height_scale,
            )
        })
        .unwrap_or_else(|| {
            ((dims.min_height + dims.max_height) * 0.5 - dims.min_height) / height_range
                * dims.height_scale
        });
    let offset_render = GROUND_OFFSET_ELMOS * dims.height_scale / height_range;
    Vec3::new(rx, ground_y + offset_render, rz)
}

/// Project a world-space point to a viewport-relative pixel
/// position. `None` when the point is behind the camera or outside
/// the NDC box.
fn project_world_point(
    world: Vec3,
    view_projection: Mat4,
    viewport_rect: egui::Rect,
) -> Option<egui::Pos2> {
    let clip = view_projection * Vec4::new(world.x, world.y, world.z, 1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;
    let ndc_z = clip.z / clip.w;
    if !(0.0..=1.0).contains(&ndc_z)
        || !(-1.0..=1.0).contains(&ndc_x)
        || !(-1.0..=1.0).contains(&ndc_y)
    {
        return None;
    }
    let sx = (ndc_x * 0.5 + 0.5) * viewport_rect.width() + viewport_rect.left();
    let sy = (1.0 - (ndc_y * 0.5 + 0.5)) * viewport_rect.height() + viewport_rect.top();
    Some(egui::pos2(sx, sy))
}

/// Camera context used to discard ring / dot / label samples that
/// sit behind the terrain from the active camera's POV. `None`
/// disables occlusion (no heightmap loaded -- every sample renders
/// as visible).
#[derive(Clone, Copy)]
pub struct OcclusionData<'a> {
    pub camera_pos: Vec3,
    pub heightmap: &'a bar_data::Heightmap,
}

fn sample_occluded(p: Vec3, dims: &OverlayDims, occ: &OcclusionData<'_>) -> bool {
    let ray = p - occ.camera_pos;
    let len = ray.length();
    if len < 1e-3 {
        return false;
    }
    let dir = ray / len;
    bar_render::ray_terrain_occludes(
        occ.camera_pos,
        dir,
        len,
        occ.heightmap,
        dims.x_extent,
        dims.z_extent,
        dims.height_scale,
        OCCLUSION_STEPS,
    )
}

/// Paint all visible metal-spot markers. Each spot becomes a flat
/// white broken-arc ring at terrain level, a cyan-green centre
/// dot, and an outlined billboarded worth label floating above the
/// spot. Samples whose camera-ray is blocked by intervening terrain
/// are skipped so rings / labels don't bleed through hills.
pub fn paint(
    painter: &egui::Painter,
    spots: &[MetalSpot],
    dims: &OverlayDims,
    heightmap: Option<&bar_data::Heightmap>,
    occlusion: Option<OcclusionData<'_>>,
    view_projection: Mat4,
    viewport_rect: egui::Rect,
) {
    let map_w_elmos = dims.map_w_elmos();
    let map_h_elmos = dims.map_h_elmos();
    let elmo_to_render_x = if map_w_elmos > 0.0 {
        2.0 * dims.x_extent / map_w_elmos
    } else {
        0.0
    };
    let elmo_to_render_z = if map_h_elmos > 0.0 {
        2.0 * dims.z_extent / map_h_elmos
    } else {
        0.0
    };
    let ring_radius_x = RING_RADIUS_ELMOS * elmo_to_render_x;
    let ring_radius_z = RING_RADIUS_ELMOS * elmo_to_render_z;
    let dot_radius_x = CENTRE_DOT_ELMOS * elmo_to_render_x;
    let dot_radius_z = CENTRE_DOT_ELMOS * elmo_to_render_z;
    let height_range = (dims.max_height - dims.min_height).abs().max(1.0);
    let label_lift_render = LABEL_LIFT_ELMOS * dims.height_scale / height_range;
    let label_height_render = LABEL_GLYPH_HEIGHT_ELMOS * dims.height_scale / height_range;

    for spot in spots {
        let value = spot.worth as f32 * dims.max_metal / 1000.0;
        if !(MIN_DISPLAY_VALUE..MAX_DISPLAY_VALUE).contains(&value) {
            continue;
        }
        let world = spot_world_pos(spot, dims, heightmap);
        let Some(centre_screen) = project_world_point(world, view_projection, viewport_rect) else {
            continue;
        };

        let ring_screen_radius = paint_ring(
            painter,
            world,
            ring_radius_x,
            ring_radius_z,
            dims,
            occlusion.as_ref(),
            view_projection,
            viewport_rect,
        );

        if !occlusion
            .as_ref()
            .is_some_and(|occ| sample_occluded(world, dims, occ))
        {
            paint_filled_disc(
                painter,
                world,
                dot_radius_x,
                dot_radius_z,
                view_projection,
                viewport_rect,
            );
        }

        // Project a world-space lift above the spot. The text height
        // comes from a second projected point above that, so the
        // label scales with the camera the same way the rings do.
        let label_world = world + Vec3::new(0.0, label_lift_render, 0.0);
        // Skip the label entirely if the lift point sits behind
        // intervening terrain. Without this the badge floats over
        // hills the spot is hidden behind.
        if occlusion
            .as_ref()
            .is_some_and(|occ| sample_occluded(label_world, dims, occ))
        {
            continue;
        }
        let label_world_top = label_world + Vec3::new(0.0, label_height_render, 0.0);
        let (pointer_anchor, label_px) = match (
            project_world_point(label_world, view_projection, viewport_rect),
            project_world_point(label_world_top, view_projection, viewport_rect),
        ) {
            (Some(anchor), Some(top)) => {
                let px = (anchor.y - top.y)
                    .abs()
                    .clamp(LABEL_FONT_MIN_PX, LABEL_FONT_MAX_PX);
                (anchor, px)
            }
            _ => {
                let lift = ring_screen_radius.max(14.0) + 10.0;
                let anchor = egui::pos2(centre_screen.x, centre_screen.y - lift);
                let px = ring_screen_radius.clamp(LABEL_FONT_MIN_PX, LABEL_FONT_MAX_PX);
                (anchor, px)
            }
        };
        paint_worth_label(painter, pointer_anchor, label_px, value);
    }
}

/// Paint the worth label as the engine widget renders it: white
/// Exo2 text with a thick black per-glyph outline (the widget bakes
/// this from `gl.LoadFont(..., outlineSize=12, outlineStrength=20)`
/// into an atlas). `anchor` is the screen position the bottom-centre
/// of the text sits at. `text_height_px` controls the font size so
/// the label scales with the camera.
///
/// The outline is approximated by stamping the glyph in black at 8
/// compass offsets around the anchor, then drawing the white fill
/// on top. The offset radius is a fraction of the font height so
/// the outline thickness tracks zoom.
fn paint_worth_label(painter: &egui::Painter, anchor: egui::Pos2, text_height_px: f32, value: f32) {
    let label = format!("{value:.1}");
    let font = egui::FontId::new(text_height_px, egui::FontFamily::Name("bar".into()));
    let outline_radius = (text_height_px * 0.11).max(1.2);
    let outline = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 235);
    let fill = egui::Color32::WHITE;

    for (dx, dy) in [
        (-1.0, -1.0),
        (-1.0, 0.0),
        (-1.0, 1.0),
        (0.0, -1.0),
        (0.0, 1.0),
        (1.0, -1.0),
        (1.0, 0.0),
        (1.0, 1.0),
    ] {
        painter.text(
            anchor + egui::vec2(dx * outline_radius, dy * outline_radius),
            egui::Align2::CENTER_BOTTOM,
            &label,
            font.clone(),
            outline,
        );
    }
    painter.text(anchor, egui::Align2::CENTER_BOTTOM, &label, font, fill);
}

/// Paint the broken-arc ring on the XZ ground plane and return its
/// average projected screen radius. Stroke widths scale with the
/// projected radius so the ring's relative thickness stays
/// consistent across zoom levels; clamped to a minimum so it
/// doesn't disappear at extreme zoom-outs.
fn paint_ring(
    painter: &egui::Painter,
    centre_world: Vec3,
    radius_x_world: f32,
    radius_z_world: f32,
    dims: &OverlayDims,
    occlusion: Option<&OcclusionData<'_>>,
    view_projection: Mat4,
    viewport_rect: egui::Rect,
) -> f32 {
    let tau = std::f32::consts::TAU;
    let segment_angle = tau / RING_ARC_COUNT as f32;
    let arc_angle = segment_angle * RING_ARC_FRACTION;

    let centre_screen = project_world_point(centre_world, view_projection, viewport_rect);
    let sample_offsets = [
        Vec3::new(radius_x_world, 0.0, 0.0),
        Vec3::new(-radius_x_world, 0.0, 0.0),
        Vec3::new(0.0, 0.0, radius_z_world),
        Vec3::new(0.0, 0.0, -radius_z_world),
    ];
    let screen_radius = if let Some(c) = centre_screen {
        let mut sum = 0.0;
        let mut count = 0;
        for off in sample_offsets {
            if let Some(p) = project_world_point(centre_world + off, view_projection, viewport_rect)
            {
                sum += (p - c).length();
                count += 1;
            }
        }
        if count > 0 {
            sum / count as f32
        } else {
            16.0
        }
    } else {
        16.0
    };

    let scale = (screen_radius / 32.0).clamp(0.45, 1.7);
    let inner_w = (2.0 * scale).max(1.2);
    let glow_w = (4.5 * scale).max(2.5);
    let outline_w = (0.7 * scale).max(0.5);

    // Engine widget renders ring vertices as `vec4(vec3(1), 0.5)`
    // -- white at alpha 0.5. Match that, with a faint dark outline
    // so the ring stays visible against bright snow / pale terrain.
    let ring = egui::Color32::from_rgba_unmultiplied(245, 245, 245, 180);
    let ring_glow = egui::Color32::from_rgba_unmultiplied(245, 245, 245, 50);
    let outline = egui::Color32::from_rgba_unmultiplied(10, 10, 10, 180);

    for arc_index in 0..RING_ARC_COUNT {
        let start = arc_index as f32 * segment_angle;
        let end = start + arc_angle;
        let mut points: [Option<egui::Pos2>; RING_ARC_SAMPLES + 1] = [None; RING_ARC_SAMPLES + 1];
        let mut occluded: [bool; RING_ARC_SAMPLES + 1] = [false; RING_ARC_SAMPLES + 1];
        for (i, slot) in points.iter_mut().enumerate() {
            let t = i as f32 / RING_ARC_SAMPLES as f32;
            let theta = start + (end - start) * t;
            let p = centre_world
                + Vec3::new(
                    radius_x_world * theta.cos(),
                    0.0,
                    radius_z_world * theta.sin(),
                );
            *slot = project_world_point(p, view_projection, viewport_rect);
            occluded[i] = occlusion.is_some_and(|occ| sample_occluded(p, dims, occ));
        }
        for (i, pair) in points.windows(2).enumerate() {
            let (Some(a), Some(b)) = (pair[0], pair[1]) else {
                continue;
            };
            // Drop segments that have a terrain silhouette anywhere
            // along their span -- both endpoints occluded means the
            // arc is behind a hill; one endpoint occluded means it
            // crosses the silhouette, easier to just hide than to
            // clip.
            if occluded[i] || occluded[i + 1] {
                continue;
            }
            painter.line_segment([a, b], egui::Stroke::new(glow_w, ring_glow));
            painter.line_segment([a, b], egui::Stroke::new(outline_w, outline));
            painter.line_segment([a, b], egui::Stroke::new(inner_w, ring));
        }
    }
    screen_radius
}

/// Paint the centre dot as a filled disc on the XZ ground plane so
/// it foreshortens with the camera the same way the outer ring
/// does.
fn paint_filled_disc(
    painter: &egui::Painter,
    centre_world: Vec3,
    radius_x_world: f32,
    radius_z_world: f32,
    view_projection: Mat4,
    viewport_rect: egui::Rect,
) {
    const SAMPLES: usize = 16;
    let tau = std::f32::consts::TAU;
    let mut polygon: Vec<egui::Pos2> = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let theta = i as f32 / SAMPLES as f32 * tau;
        let p = centre_world
            + Vec3::new(
                radius_x_world * theta.cos(),
                0.0,
                radius_z_world * theta.sin(),
            );
        match project_world_point(p, view_projection, viewport_rect) {
            Some(s) => polygon.push(s),
            None => return,
        }
    }
    // Engine widget's "free" centre indicator is `vec4(0, 1, 0.5, 1)`
    // -- a bright cyan-green. Match that here.
    let fill = egui::Color32::from_rgba_unmultiplied(0, 235, 128, 235);
    let stroke = egui::Stroke::new(0.8, egui::Color32::from_rgba_unmultiplied(10, 40, 25, 220));
    painter.add(egui::epaint::Shape::convex_polygon(polygon, fill, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_dims() -> OverlayDims {
        OverlayDims {
            map_w: 9,
            map_h: 9,
            min_height: 0.0,
            max_height: 100.0,
            x_extent: 0.5,
            z_extent: 0.5,
            height_scale: 0.3,
            max_metal: 1.0,
        }
    }

    #[test]
    fn map_w_elmos_uses_spring_convention() {
        let dims = dummy_dims();
        assert!((dims.map_w_elmos() - 64.0).abs() < 1e-3);
        assert!((dims.map_h_elmos() - 64.0).abs() < 1e-3);
    }

    #[test]
    fn spot_at_origin_projects_to_centre_of_render_xz() {
        let dims = dummy_dims();
        let spot = MetalSpot {
            x_elmo: dims.map_w_elmos() * 0.5,
            z_elmo: dims.map_h_elmos() * 0.5,
            worth: 50,
        };
        let world = spot_world_pos(&spot, &dims, None);
        assert!(world.x.abs() < 1e-4);
        assert!(world.z.abs() < 1e-4);
    }

    #[test]
    fn spot_at_corner_projects_to_render_extent() {
        let dims = dummy_dims();
        let spot = MetalSpot {
            x_elmo: 0.0,
            z_elmo: 0.0,
            worth: 50,
        };
        let world = spot_world_pos(&spot, &dims, None);
        assert!((world.x + dims.x_extent).abs() < 1e-4);
        assert!((world.z + dims.z_extent).abs() < 1e-4);
    }
}
