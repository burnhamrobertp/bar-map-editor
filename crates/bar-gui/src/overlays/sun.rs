//! Sun-direction gizmo for the Sculpt 3D viewport.
//!
//! The sun lives on a sphere of fixed radius around the map centre
//! (the radius is `widest_dim_elmos + GIZMO_PAD_ELMOS`; it is a UX
//! cue only, not tied to intensity or any mapinfo field). Dragging
//! the sun therefore rotates it -- it does NOT translate it -- so
//! the gizmo follows Blender's rotate-gizmo metaphor: three radial
//! guides, one per world axis, each tracing the full path the sun
//! would travel if rotated around that axis. The user clicks the
//! sun marker to "arm" the gizmo; once armed, the user clicks a
//! ring to lock the drag to rotation around that single axis.
//!
//! ### Math for the ring around world axis A
//!
//! - the sun's component along A is `sun_unit · A`,
//! - rotation around A preserves that component, so the path is
//!   the intersection of the gizmo sphere with the plane
//!   `dot(p, A) = (sun·A) * R`,
//! - that intersection is a circle centred at `A * (sun·A) * R`
//!   with radius `R * sqrt(1 - (sun·A)^2)` in the plane perpendicular
//!   to A.
//!
//! The ring degenerates to a point when the sun sits on the rotation
//! axis (e.g. sun on +Y, ring around Y); in that case the ring is
//! still drawn at a small visible radius so the user sees the axis
//! and can rotate around the OTHER two axes to push the sun off the
//! pole. Drag math handles the degenerate tangent gracefully (no
//! divide-by-zero; drag simply has no effect along the degenerate
//! axis until the sun moves off it).
//!
//! ### Visibility states
//!
//! Rings are always solid and always interactive (one click + drag on
//! a ring rotates the sun -- no two-step arm-then-drag).
//! - **default (revealed = false)**: each ring segment fades with
//!   *arc distance from the sun along that ring* (NOT 3D distance,
//!   and NOT screen distance). The fade pivot is the sun's `theta`
//!   on each ring; segments past `FADE_ARC_CUTOFF_RADIANS` of arc
//!   away are fully transparent. Using arc distance per-ring keeps
//!   the visible fraction consistent across rings of any size,
//!   including the tight rings that form when the sun sits near
//!   one of the world axes (where every point would otherwise be a
//!   short 3D distance from the sun and the previous sphere-angle
//!   fade did nothing useful).
//! - **revealed (user clicked the sun marker)**: full alpha across
//!   all three rings, no fade -- useful for inspecting the entire
//!   rotation path before committing to a drag.
//! - Occluded segments (line-of-sight blocked by the terrain)
//!   render at lower alpha in BOTH states so they read as "behind
//!   the map".
//!
//! Press / drag / release wiring lives at the call site
//! (`bar-app::viewport::handle_camera_input`); this module owns the
//! pure math + paint surface.

use bar_data::Heightmap;
use eframe::egui;
use glam::{Mat4, Vec3, Vec4};

/// Padding (BAR elmos) added to the widest map dimension to place
/// the gizmo sphere outside the playable area. Editor UX only;
/// engine has no equivalent.
pub const GIZMO_PAD_ELMOS: f32 = 80.0;

/// Number of sample points per ring. 96 keeps the polyline smooth
/// at typical zoom levels (no visible faceting) while staying cheap
/// to hit-test and occlusion-test against (each sample is one ray
/// march of [`OCCLUSION_STEPS`] heightmap lookups).
pub const RING_SAMPLES: usize = 96;

/// Per-sample ray-march resolution used by the occlusion test. The
/// march is clipped to the terrain AABB so 128 steps land at
/// ~`map_extent / 128` spacing -- fine enough to catch hill silhouettes
/// at any camera angle without re-marching the full camera-to-sample
/// distance (which can be huge when the camera pulls back).
pub const OCCLUSION_STEPS: u32 = 128;

/// Screen-space hit radius when distance-testing the cursor against
/// a ring polyline. Wide enough to be grab-able without precision
/// aiming; narrow enough that the three rings remain distinguishable
/// where they cross.
pub const RING_HIT_RADIUS: f32 = 9.0;

/// Clickable radius around the sun marker. Single click within this
/// distance arms or disarms the gizmo; clicks outside disarm.
pub const MARKER_HIT_RADIUS: f32 = 12.0;

/// Visible sun marker radius.
pub const MARKER_RADIUS: f32 = 5.0;

/// Angular sensitivity in radians per screen pixel of motion along
/// the ring's tangent at the sun. 0.005 rad/px puts a full 360°
/// sweep at ~1256 pixels of straight-line drag along the tangent --
/// roughly half the camera-orbit sensitivity so the sun feels
/// deliberately controllable rather than skittish.
pub const ANGULAR_SENSITIVITY: f32 = 0.005;

/// Arc-distance cutoff (along the ring itself, in radians) past
/// which segments fade to fully transparent. Set on a per-ring
/// basis -- NOT in sphere-angle terms -- so the fade behaves
/// uniformly regardless of how tight each ring is. When the sun
/// sits near a world axis the ring around that axis is a tight
/// loop and every point on it is within a small sphere-angle of
/// the sun, which made the previous sphere-angle fade do
/// approximately nothing. Arc-distance fade clips a consistent
/// fraction of each ring (here 1/3, i.e. 120deg total).
pub const FADE_ARC_CUTOFF_RADIANS: f32 = std::f32::consts::FRAC_PI_3;

/// Curve exponent applied to the linear arc-distance fade. Higher
/// values fall off faster near the sun and linger nearer the
/// cutoff; lower values stay bright longer near the sun and drop
/// sharply at the cutoff. 1.5 lands in the middle: visibly dimmed
/// at half the cutoff (35% alpha), gone at the cutoff.
pub const FADE_EXPONENT: f32 = 1.5;

/// Which world axis a sun-gizmo drag rotates around. Colour matches
/// the standard 3D-editor X = red, Y = green, Z = blue convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SunGizmoAxis {
    X,
    Y,
    Z,
}

impl SunGizmoAxis {
    pub fn world_dir(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            Self::X => egui::Color32::from_rgb(225, 75, 75),
            Self::Y => egui::Color32::from_rgb(95, 210, 110),
            Self::Z => egui::Color32::from_rgb(95, 155, 235),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }
}

/// Map-extent + render-extent inputs needed to position the gizmo
/// sphere. `map_w` / `map_h` are heightmap dims (cells = dim - 1, 8
/// elmos per cell); `x_extent` / `z_extent` are the renderer's
/// half-spans in render space.
#[derive(Clone, Copy)]
pub struct SunGizmoDims {
    pub map_w: u32,
    pub map_h: u32,
    pub x_extent: f32,
    pub z_extent: f32,
}

impl SunGizmoDims {
    pub fn map_w_elmos(&self) -> f32 {
        ((self.map_w.saturating_sub(1)).max(1) as f32) * 8.0
    }

    pub fn map_h_elmos(&self) -> f32 {
        ((self.map_h.saturating_sub(1)).max(1) as f32) * 8.0
    }

    /// Render-space units per elmo on the X axis. The renderer's
    /// X scale is `2*x_extent / map_w_elmos`; we use this to convert
    /// the elmo-space gizmo radius into render space.
    pub fn render_per_elmo_x(&self) -> f32 {
        2.0 * self.x_extent / self.map_w_elmos()
    }

    /// Sphere radius (render space) on which the sun gizmo sits.
    pub fn gizmo_radius_render(&self) -> f32 {
        let widest_elmos = self.map_w_elmos().max(self.map_h_elmos());
        (widest_elmos + GIZMO_PAD_ELMOS) * self.render_per_elmo_x()
    }
}

/// Terrain context passed to `compute_geometry` for ring-occlusion
/// testing. `None` disables occlusion entirely (every sample
/// rendered as visible) -- used when the heightmap hasn't loaded
/// yet and the gizmo just floats above an empty viewport.
#[derive(Clone, Copy)]
pub struct OcclusionData<'a> {
    pub camera_pos: Vec3,
    pub heightmap: &'a Heightmap,
    pub x_extent: f32,
    pub z_extent: f32,
    pub height_scale: f32,
}

/// World-space sun-gizmo position. The sphere is centred at the
/// world origin so the position is just `unit_dir * gizmo_radius`.
/// Falls back to +Y when the recipe stores a degenerate zero
/// vector.
pub fn gizmo_world_pos(sun_dir: [f32; 3], dims: &SunGizmoDims) -> Vec3 {
    let unit = unit_dir_or_default(sun_dir);
    unit * dims.gizmo_radius_render()
}

fn unit_dir_or_default(sun_dir: [f32; 3]) -> Vec3 {
    let v = Vec3::from(sun_dir);
    let n = v.length();
    if n.is_finite() && n > 1e-6 {
        v / n
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    }
}

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
    if !(0.0..=1.0).contains(&ndc_z) {
        return None;
    }
    let sx = (ndc_x * 0.5 + 0.5) * viewport_rect.width() + viewport_rect.left();
    let sy = (1.0 - (ndc_y * 0.5 + 0.5)) * viewport_rect.height() + viewport_rect.top();
    Some(egui::pos2(sx, sy))
}

/// Heightmap occlusion test for one world-space point. Delegates to
/// [`bar_render::ray_terrain_occludes`], which clips the
/// camera-to-point ray to the terrain AABB and marches with
/// `OCCLUSION_STEPS` samples inside it -- much denser per-elmo than
/// the previous walk-the-whole-camera-ray approach, so terrain
/// silhouettes block the rings at any camera angle.
fn world_point_occluded(p: Vec3, occ: &OcclusionData<'_>) -> bool {
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
        occ.x_extent,
        occ.z_extent,
        occ.height_scale,
        OCCLUSION_STEPS,
    )
}

/// One ring sample. `theta` is the sample's angle on the ring (in
/// radians, range `[0, TAU]` matching the parameterisation used to
/// build the polyline) and lets the fade pivot on arc distance from
/// the sun's position on that ring -- which is what the user
/// perceives as "near the sun" regardless of how tight the ring is.
#[derive(Clone, Copy)]
pub struct RingSample {
    pub world: Vec3,
    pub screen: Option<egui::Pos2>,
    pub occluded: bool,
    pub theta: f32,
}

pub type RingPolyline = Vec<RingSample>;

/// Computed gizmo geometry for one render frame.
pub struct GizmoGeometry {
    pub center_screen: egui::Pos2,
    /// Sun position in render-world space. Kept for callers that
    /// need to project additional world-space points consistently
    /// with the rings (e.g. axis labels) without re-deriving it.
    pub sun_world: Vec3,
    /// Sphere radius (render units) on which the rings live.
    pub gizmo_radius: f32,
    pub rings: [RingPolyline; 3],
    /// Sun's angular position (theta in radians, `[0, TAU)`) on each
    /// ring. Indexed by `SunGizmoAxis as usize`. Used by the
    /// arc-distance fade to know where on each ring the sun sits.
    pub sun_thetas: [f32; 3],
    pub tangents: [Option<egui::Vec2>; 3],
}

impl GizmoGeometry {
    pub fn ring(&self, axis: SunGizmoAxis) -> &RingPolyline {
        &self.rings[axis as usize]
    }

    pub fn tangent(&self, axis: SunGizmoAxis) -> Option<egui::Vec2> {
        self.tangents[axis as usize]
    }

    pub fn sun_theta(&self, axis: SunGizmoAxis) -> f32 {
        self.sun_thetas[axis as usize]
    }
}

/// Build the three rotation rings for the current sun direction and
/// camera. Each ring is a closed polyline of `RING_SAMPLES + 1`
/// samples; when `occlusion` is `Some`, each sample's `occluded`
/// flag reflects whether the heightmap blocks the camera's line of
/// sight to that point.
pub fn compute_geometry(
    sun_dir: [f32; 3],
    dims: &SunGizmoDims,
    view_projection: Mat4,
    viewport_rect: egui::Rect,
    occlusion: Option<OcclusionData<'_>>,
) -> Option<GizmoGeometry> {
    let radius = dims.gizmo_radius_render();
    if radius < 1e-3 {
        return None;
    }
    let sun_unit = unit_dir_or_default(sun_dir);
    let sun_world = sun_unit * radius;
    let center_screen = project_world_point(sun_world, view_projection, viewport_rect)?;

    let (ring_x, sun_theta_x) = sample_ring(
        SunGizmoAxis::X,
        sun_unit,
        sun_world,
        radius,
        view_projection,
        viewport_rect,
        occlusion.as_ref(),
    );
    let (ring_y, sun_theta_y) = sample_ring(
        SunGizmoAxis::Y,
        sun_unit,
        sun_world,
        radius,
        view_projection,
        viewport_rect,
        occlusion.as_ref(),
    );
    let (ring_z, sun_theta_z) = sample_ring(
        SunGizmoAxis::Z,
        sun_unit,
        sun_world,
        radius,
        view_projection,
        viewport_rect,
        occlusion.as_ref(),
    );
    let tangents = [
        tangent_for(
            SunGizmoAxis::X,
            sun_unit,
            sun_world,
            view_projection,
            viewport_rect,
        ),
        tangent_for(
            SunGizmoAxis::Y,
            sun_unit,
            sun_world,
            view_projection,
            viewport_rect,
        ),
        tangent_for(
            SunGizmoAxis::Z,
            sun_unit,
            sun_world,
            view_projection,
            viewport_rect,
        ),
    ];

    Some(GizmoGeometry {
        center_screen,
        sun_world,
        gizmo_radius: radius,
        rings: [ring_x, ring_y, ring_z],
        sun_thetas: [sun_theta_x, sun_theta_y, sun_theta_z],
        tangents,
    })
}

fn sample_ring(
    axis: SunGizmoAxis,
    sun_unit: Vec3,
    sun_world: Vec3,
    radius: f32,
    view_projection: Mat4,
    viewport_rect: egui::Rect,
    occlusion: Option<&OcclusionData<'_>>,
) -> (RingPolyline, f32) {
    let a = axis.world_dir();
    let axis_comp = sun_unit.dot(a);
    let ring_center = a * (axis_comp * radius);
    let in_plane = radius * (1.0 - axis_comp * axis_comp).max(0.0).sqrt();
    let visible_radius = in_plane.max(radius * 0.04);
    let helper = if a.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = a.cross(helper).normalize();
    let v = a.cross(u);

    // Sun's angular position on this ring, in the same (u, v) basis
    // used to parameterise the samples. The sun is mathematically on
    // the ring (it's the intersection of all three rings); even when
    // the ring degenerates to a point (sun on the rotation axis),
    // `sun_in_plane` is near-zero and atan2 returns 0 -- the fade
    // then treats the whole tiny visible ring as "right next to the
    // sun", which is fine since rotating around the degenerate axis
    // has no effect anyway.
    let sun_in_plane = sun_world - ring_center;
    let sun_theta = sun_in_plane
        .dot(v)
        .atan2(sun_in_plane.dot(u))
        .rem_euclid(std::f32::consts::TAU);

    let mut points = Vec::with_capacity(RING_SAMPLES + 1);
    for i in 0..=RING_SAMPLES {
        let theta = (i as f32 / RING_SAMPLES as f32) * std::f32::consts::TAU;
        let p = ring_center + (u * theta.cos() + v * theta.sin()) * visible_radius;
        let screen = project_world_point(p, view_projection, viewport_rect);
        let occluded = match occlusion {
            Some(occ) => world_point_occluded(p, occ),
            None => false,
        };
        points.push(RingSample {
            world: p,
            screen,
            occluded,
            theta,
        });
    }
    (points, sun_theta)
}

fn tangent_for(
    axis: SunGizmoAxis,
    sun_unit: Vec3,
    sun_world: Vec3,
    view_projection: Mat4,
    viewport_rect: egui::Rect,
) -> Option<egui::Vec2> {
    let a = axis.world_dir();
    let t = a.cross(sun_unit);
    let t_len = t.length();
    if t_len < 1e-4 {
        return None;
    }
    let t_unit = t / t_len;
    let eps = (sun_world.length() * 1e-3).max(1e-3);
    let head = project_world_point(sun_world + t_unit * eps, view_projection, viewport_rect)?;
    let tail = project_world_point(sun_world, view_projection, viewport_rect)?;
    let raw = head - tail;
    let len = raw.length();
    if len < 1e-3 {
        return None;
    }
    Some(raw / len)
}

/// Hit-test cursor against the three ring polylines. Skips occluded
/// segments so the user can't grab a ring through the terrain.
pub fn hit_test_axis(geometry: &GizmoGeometry, cursor: egui::Pos2) -> Option<SunGizmoAxis> {
    let mut best: Option<(SunGizmoAxis, f32)> = None;
    for axis in [SunGizmoAxis::X, SunGizmoAxis::Y, SunGizmoAxis::Z] {
        let ring = geometry.ring(axis);
        let d = nearest_visible_distance_to_polyline(cursor, ring);
        if let Some(d) = d {
            if d <= RING_HIT_RADIUS && best.is_none_or(|(_, prev)| d < prev) {
                best = Some((axis, d));
            }
        }
    }
    best.map(|(a, _)| a)
}

/// True iff `cursor` is within [`MARKER_HIT_RADIUS`] of the sun marker.
pub fn cursor_on_marker(geometry: &GizmoGeometry, cursor: egui::Pos2) -> bool {
    cursor.distance(geometry.center_screen) <= MARKER_HIT_RADIUS
}

fn nearest_visible_distance_to_polyline(p: egui::Pos2, points: &[RingSample]) -> Option<f32> {
    let mut best: Option<f32> = None;
    for win in points.windows(2) {
        // Skip segments where either endpoint is off-screen OR
        // occluded -- a click that lands "on" a hidden segment of
        // the ring should NOT grab that ring; the user has to
        // rotate the camera to expose it first.
        let (a, b) = match (win[0].screen, win[1].screen) {
            (Some(a), Some(b)) if !win[0].occluded && !win[1].occluded => (a, b),
            _ => continue,
        };
        let d = point_to_segment_distance(p, a, b);
        if best.is_none_or(|prev| d < prev) {
            best = Some(d);
        }
    }
    best
}

fn point_to_segment_distance(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-6 {
        return ap.length();
    }
    let t = (ap.dot(ab) / len_sq).clamp(0.0, 1.0);
    let nearest = a + ab * t;
    (p - nearest).length()
}

/// Rotate `sun_dir` around `axis` by the angular delta produced by
/// projecting `drag_delta` onto the ring's screen tangent.
pub fn rotate_around_axis(
    sun_dir: [f32; 3],
    axis: SunGizmoAxis,
    drag_delta: egui::Vec2,
    geometry: &GizmoGeometry,
) -> [f32; 3] {
    let Some(tangent) = geometry.tangent(axis) else {
        return sun_dir;
    };
    let signed_pixels = drag_delta.dot(tangent);
    let angle = signed_pixels * ANGULAR_SENSITIVITY;
    if angle.abs() < 1e-6 {
        return sun_dir;
    }
    let a = axis.world_dir();
    let s = unit_dir_or_default(sun_dir);
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let rotated = s * cos_a + a.cross(s) * sin_a + a * (a.dot(s)) * (1.0 - cos_a);
    let n = rotated.length();
    let final_unit = if n.is_finite() && n > 1e-6 {
        rotated / n
    } else {
        s
    };
    [final_unit.x, final_unit.y, final_unit.z]
}

/// Per-axis visual treatment for one paint pass. Selects base
/// alpha + stroke width given hover / active state. Distance fade
/// is applied per-segment on top of these base values.
struct RingStyle {
    visible_alpha: u8,
    occluded_alpha: u8,
    visible_width: f32,
    occluded_width: f32,
}

fn ring_style(
    axis: SunGizmoAxis,
    hovered_axis: Option<SunGizmoAxis>,
    active_axis: Option<SunGizmoAxis>,
) -> RingStyle {
    let is_active = active_axis == Some(axis);
    let is_hovered = active_axis.is_none() && hovered_axis == Some(axis);
    let dim_others = active_axis.is_some() && !is_active;
    if is_active {
        RingStyle {
            visible_alpha: 255,
            occluded_alpha: 90,
            visible_width: 3.0,
            occluded_width: 1.4,
        }
    } else if is_hovered {
        RingStyle {
            visible_alpha: 240,
            occluded_alpha: 80,
            visible_width: 2.5,
            occluded_width: 1.2,
        }
    } else if dim_others {
        RingStyle {
            visible_alpha: 100,
            occluded_alpha: 40,
            visible_width: 1.4,
            occluded_width: 1.0,
        }
    } else {
        RingStyle {
            visible_alpha: 210,
            occluded_alpha: 80,
            visible_width: 1.7,
            occluded_width: 1.0,
        }
    }
}

/// Paint the gizmo. Rings are always solid; when `revealed` is
/// false they fade with *arc distance* from the sun along each ring
/// itself, so a consistent fraction of every ring stays visible no
/// matter how tight or wide the individual ring is. Clicking the sun
/// marker flips `revealed`, which removes the fade so the user can
/// inspect the entire rotation path before committing to a drag.
/// The active (dragging) axis also bypasses fade so the in-flight
/// rotation path always reads cleanly.
pub fn paint(
    painter: &egui::Painter,
    geometry: &GizmoGeometry,
    hovered_axis: Option<SunGizmoAxis>,
    active_axis: Option<SunGizmoAxis>,
    revealed: bool,
) {
    let marker_pos = geometry.center_screen;
    for axis in [SunGizmoAxis::X, SunGizmoAxis::Y, SunGizmoAxis::Z] {
        let style = ring_style(axis, hovered_axis, active_axis);
        // Per-axis fade pivot: `None` disables fade for that axis.
        // We disable it on the active axis (full path visible
        // during a drag) and on every axis when `revealed`.
        let fade_sun_theta = if revealed || Some(axis) == active_axis {
            None
        } else {
            Some(geometry.sun_theta(axis))
        };
        paint_ring(
            painter,
            geometry.ring(axis),
            axis.color(),
            &style,
            fade_sun_theta,
        );
    }

    let halo_alpha = if revealed { 170 } else { 110 };
    painter.circle_filled(
        marker_pos,
        MARKER_RADIUS + 2.0,
        egui::Color32::from_rgba_unmultiplied(255, 220, 120, halo_alpha),
    );
    painter.circle_filled(
        marker_pos,
        MARKER_RADIUS,
        egui::Color32::from_rgba_unmultiplied(255, 230, 130, 240),
    );
    painter.circle_stroke(
        marker_pos,
        MARKER_RADIUS,
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(60, 40, 20, 220)),
    );

    if let Some(axis) = active_axis {
        painter.text(
            marker_pos + egui::vec2(MARKER_RADIUS + 8.0, -MARKER_RADIUS - 8.0),
            egui::Align2::LEFT_BOTTOM,
            axis.label(),
            egui::FontId::monospace(13.0),
            axis.color(),
        );
    }
}

/// Shortest absolute arc distance (in radians, `[0, PI]`) between
/// two angles on a ring. Wraps so `delta(0.01, TAU - 0.01) ~= 0.02`.
fn arc_delta(theta_a: f32, theta_b: f32) -> f32 {
    let raw = (theta_a - theta_b).rem_euclid(std::f32::consts::TAU);
    if raw > std::f32::consts::PI {
        std::f32::consts::TAU - raw
    } else {
        raw
    }
}

/// Arc-distance fade. `factor = (1 - delta / FADE_ARC_CUTOFF) ^
/// FADE_EXPONENT` for `delta < cutoff`, else `0`. The cutoff is in
/// ring-arc radians, NOT sphere-angle radians, so the fade clips a
/// uniform fraction of each ring regardless of how tight or wide
/// that ring is.
fn fade_factor_arc(seg_theta: f32, sun_theta: f32) -> f32 {
    let delta = arc_delta(seg_theta, sun_theta);
    if delta >= FADE_ARC_CUTOFF_RADIANS {
        return 0.0;
    }
    (1.0 - delta / FADE_ARC_CUTOFF_RADIANS).powf(FADE_EXPONENT)
}

fn scale_alpha(base: u8, factor: f32) -> u8 {
    ((base as f32) * factor).clamp(0.0, 255.0) as u8
}

fn paint_ring(
    painter: &egui::Painter,
    polyline: &[RingSample],
    base_color: egui::Color32,
    style: &RingStyle,
    fade_sun_theta: Option<f32>,
) {
    for win in polyline.windows(2) {
        let (Some(a), Some(b)) = (win[0].screen, win[1].screen) else {
            continue;
        };
        // A segment counts as occluded if either endpoint is hidden;
        // keeps the joint between visible/occluded runs from flickering
        // at the sample boundary.
        let occluded = win[0].occluded || win[1].occluded;
        let (base_alpha, width) = if occluded {
            (style.occluded_alpha, style.occluded_width)
        } else {
            (style.visible_alpha, style.visible_width)
        };
        let alpha = match fade_sun_theta {
            Some(sun_theta) => {
                // Adjacent samples on the polyline are guaranteed to
                // be at most `TAU / RING_SAMPLES` apart and never
                // wrap around (we don't draw the [N, 0] seam), so a
                // plain average is the right segment midpoint
                // without wrap handling.
                let mid_theta = (win[0].theta + win[1].theta) * 0.5;
                scale_alpha(base_alpha, fade_factor_arc(mid_theta, sun_theta))
            }
            None => base_alpha,
        };
        if alpha == 0 {
            continue;
        }
        let color = egui::Color32::from_rgba_unmultiplied(
            base_color.r(),
            base_color.g(),
            base_color.b(),
            alpha,
        );
        painter.line_segment([a, b], egui::Stroke::new(width, color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_unit(v: [f32; 3]) {
        let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((mag - 1.0).abs() < 1e-4, "expected unit, mag = {mag}");
    }

    #[test]
    fn sphere_radius_uses_widest_dim_plus_pad() {
        let dims = SunGizmoDims {
            map_w: 12,
            map_h: 8,
            x_extent: 0.5,
            z_extent: 0.5 * 56.0 / 88.0,
        };
        let r = dims.gizmo_radius_render();
        let expected = (88.0 + GIZMO_PAD_ELMOS) / 88.0;
        assert!((r - expected).abs() < 1e-3);
    }

    #[test]
    fn unnormalised_input_renormalises() {
        let pos = gizmo_world_pos(
            [0.0, 5.0, 0.0],
            &SunGizmoDims {
                map_w: 9,
                map_h: 9,
                x_extent: 0.5,
                z_extent: 0.5,
            },
        );
        assert!(pos.y > 0.0);
        assert!(pos.x.abs() < 1e-6 && pos.z.abs() < 1e-6);
    }

    fn ring_with_samples(samples: Vec<RingSample>) -> RingPolyline {
        samples
    }

    fn geometry_with_tangents(
        tx: Option<egui::Vec2>,
        ty: Option<egui::Vec2>,
        tz: Option<egui::Vec2>,
    ) -> GizmoGeometry {
        GizmoGeometry {
            center_screen: egui::pos2(0.0, 0.0),
            sun_world: Vec3::new(1.0, 0.0, 0.0),
            gizmo_radius: 1.0,
            rings: [Vec::new(), Vec::new(), Vec::new()],
            sun_thetas: [0.0, 0.0, 0.0],
            tangents: [tx, ty, tz],
        }
    }

    fn dummy_sample(screen: egui::Pos2, occluded: bool) -> RingSample {
        RingSample {
            world: Vec3::ZERO,
            screen: Some(screen),
            occluded,
            theta: 0.0,
        }
    }

    #[test]
    fn rotation_around_y_preserves_y_component() {
        let geo = geometry_with_tangents(None, Some(egui::vec2(1.0, 0.0)), None);
        let prev = [1.0, 0.5, 0.0];
        let unit_prev = unit_dir_or_default(prev);
        let next = rotate_around_axis(
            [unit_prev.x, unit_prev.y, unit_prev.z],
            SunGizmoAxis::Y,
            egui::vec2(200.0, 0.0),
            &geo,
        );
        approx_unit(next);
        assert!(
            (next[1] - unit_prev.y).abs() < 1e-4,
            "Y component changed: prev {} -> {}",
            unit_prev.y,
            next[1],
        );
    }

    #[test]
    fn rotation_preserves_unit_length() {
        let geo = geometry_with_tangents(
            Some(egui::vec2(1.0, 0.0)),
            Some(egui::vec2(0.0, -1.0)),
            Some(egui::vec2(0.707, -0.707)),
        );
        for axis in [SunGizmoAxis::X, SunGizmoAxis::Y, SunGizmoAxis::Z] {
            let next = rotate_around_axis([0.5, 0.5, 0.707], axis, egui::vec2(150.0, 80.0), &geo);
            approx_unit(next);
        }
    }

    #[test]
    fn drag_perpendicular_to_tangent_is_noop() {
        let geo = geometry_with_tangents(Some(egui::vec2(1.0, 0.0)), None, None);
        let prev = [1.0, 0.0, 0.0];
        let next = rotate_around_axis(prev, SunGizmoAxis::X, egui::vec2(0.0, 80.0), &geo);
        for i in 0..3 {
            assert!(
                (next[i] - prev[i]).abs() < 1e-4,
                "axis {i}: expected no change",
            );
        }
    }

    #[test]
    fn degenerate_tangent_is_noop() {
        let geo = geometry_with_tangents(None, None, None);
        let prev = [0.0, 1.0, 0.0];
        let next = rotate_around_axis(prev, SunGizmoAxis::Y, egui::vec2(500.0, 500.0), &geo);
        for i in 0..3 {
            assert!((next[i] - prev[i]).abs() < 1e-6);
        }
    }

    #[test]
    fn rotation_around_y_with_unit_tangent_matches_expected_angle() {
        let geo = geometry_with_tangents(None, Some(egui::vec2(1.0, 0.0)), None);
        let next = rotate_around_axis(
            [1.0, 0.0, 0.0],
            SunGizmoAxis::Y,
            egui::vec2(100.0, 0.0),
            &geo,
        );
        let theta = 100.0 * ANGULAR_SENSITIVITY;
        let expected = [theta.cos(), 0.0, -theta.sin()];
        for i in 0..3 {
            assert!(
                (next[i] - expected[i]).abs() < 1e-4,
                "axis {i}: expected {} got {}",
                expected[i],
                next[i],
            );
        }
    }

    #[test]
    fn hit_test_ignores_occluded_segments() {
        let mut geo = geometry_with_tangents(None, None, None);
        // X ring: a single segment from (-50, 0) to (50, 0) but
        // marked occluded -- cursor "on" it should NOT pick X.
        geo.rings[0] = ring_with_samples(vec![
            dummy_sample(egui::pos2(-50.0, 0.0), true),
            dummy_sample(egui::pos2(50.0, 0.0), true),
        ]);
        // Y ring: same shape but VISIBLE, on a vertical line so the
        // hit-test has to pick the one within radius.
        geo.rings[1] = ring_with_samples(vec![
            dummy_sample(egui::pos2(0.0, -50.0), false),
            dummy_sample(egui::pos2(0.0, 50.0), false),
        ]);
        // Cursor near the X ring centre. X is occluded so the
        // hit-test must skip it -- nothing else is close, so the
        // result is None (not a wrong Y-axis match).
        let hit = hit_test_axis(&geo, egui::pos2(0.0, 2.0));
        assert_eq!(hit, Some(SunGizmoAxis::Y));
        // Far from any ring -> None.
        assert_eq!(hit_test_axis(&geo, egui::pos2(500.0, 500.0)), None);
    }

    #[test]
    fn fade_arc_cutoff_is_per_ring_uniform() {
        // The fade pivots on arc distance from the sun along the
        // ring itself, NOT the sphere -- so a uniform fraction of
        // each ring fades regardless of how tight or wide the ring
        // is. Sun at theta = 0; cutoff = FADE_ARC_CUTOFF_RADIANS.
        let at_sun = fade_factor_arc(0.0, 0.0);
        assert!((at_sun - 1.0).abs() < 1e-4);
        // Half-cutoff (one third of the way to the cutoff if
        // exponent > 1): dimmed but visible.
        let at_half = fade_factor_arc(FADE_ARC_CUTOFF_RADIANS * 0.5, 0.0);
        assert!(
            at_half > 0.1 && at_half < 0.6,
            "half cutoff should be partial, got {at_half}",
        );
        // AT cutoff: zero.
        let at_cutoff = fade_factor_arc(FADE_ARC_CUTOFF_RADIANS, 0.0);
        assert!(
            at_cutoff < 1e-4,
            "at cutoff should be zero, got {at_cutoff}"
        );
        // PAST cutoff: zero.
        let past = fade_factor_arc(FADE_ARC_CUTOFF_RADIANS + 0.5, 0.0);
        assert!(past < 1e-4, "past cutoff should be zero, got {past}");
        // Opposite side of the ring (theta = PI): zero.
        let opposite = fade_factor_arc(std::f32::consts::PI, 0.0);
        assert!(opposite < 1e-4, "opposite should be zero, got {opposite}");
        // Wraps correctly: small angle on the "wrong side" of zero
        // should still count as nearby.
        let near_wrap = fade_factor_arc(0.1, std::f32::consts::TAU - 0.1);
        assert!(
            near_wrap > 0.5,
            "wrap-around delta of 0.2 rad should be near-full, got {near_wrap}",
        );
    }

    #[test]
    fn arc_delta_wraps_around_tau() {
        let pi = std::f32::consts::PI;
        let tau = std::f32::consts::TAU;
        // Same angle -> 0.
        assert!(arc_delta(1.5, 1.5).abs() < 1e-4);
        // Quarter-turn either direction -> pi/2.
        assert!((arc_delta(0.0, pi * 0.5) - pi * 0.5).abs() < 1e-4);
        assert!((arc_delta(pi * 0.5, 0.0) - pi * 0.5).abs() < 1e-4);
        // Wrap: 0.1 vs TAU - 0.1 is 0.2 apart, not TAU - 0.2.
        assert!((arc_delta(0.1, tau - 0.1) - 0.2).abs() < 1e-3);
        // Antipode: pi.
        assert!((arc_delta(0.0, pi) - pi).abs() < 1e-4);
    }

    #[test]
    fn cursor_on_marker_within_radius() {
        let geo = geometry_with_tangents(None, None, None);
        // Marker at the origin (center_screen default).
        assert!(cursor_on_marker(&geo, egui::pos2(0.0, 0.0)));
        // Inside the hit radius.
        assert!(cursor_on_marker(
            &geo,
            egui::pos2(MARKER_HIT_RADIUS - 1.0, 0.0)
        ));
        // Just outside.
        assert!(!cursor_on_marker(
            &geo,
            egui::pos2(MARKER_HIT_RADIUS + 5.0, 0.0)
        ));
    }
}
