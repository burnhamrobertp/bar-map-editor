use glam::{Mat4, Vec3};

/// Orbital camera for the 3D terrain viewport.
#[derive(Debug, Clone)]
pub struct Camera {
    /// Camera target (look-at point)
    pub target: Vec3,
    /// Distance from target
    pub distance: f32,
    /// Horizontal angle in radians
    pub azimuth: f32,
    /// Vertical angle in radians (clamped to avoid gimbal lock)
    pub elevation: f32,
    /// Field of view in radians
    pub fov: f32,
    /// Near clipping plane
    pub near: f32,
    /// Far clipping plane
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        // Mesh spans roughly [-0.5, +0.5] in X/Z. distance ≈ 1.6 frames the
        // mesh tightly with a 45° FOV. Elevation π/8 (22.5°) is low enough
        // that terrain heights read dramatically — closer to the in-game
        // first-person view than the bird's-eye 30° we used to default to.
        // The orbital camera lets the user adjust if they want a different
        // angle.
        Self {
            target: Vec3::ZERO,
            distance: 1.6,
            azimuth: std::f32::consts::FRAC_PI_4,
            elevation: std::f32::consts::FRAC_PI_8,
            fov: std::f32::consts::FRAC_PI_4,
            near: 0.01,
            far: 1000.0,
        }
    }
}

impl Camera {
    /// Get the camera position in world space.
    pub fn position(&self) -> Vec3 {
        let x = self.distance * self.elevation.cos() * self.azimuth.cos();
        let y = self.distance * self.elevation.sin();
        let z = self.distance * self.elevation.cos() * self.azimuth.sin();
        self.target + Vec3::new(x, y, z)
    }

    /// Get the view matrix.
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.position(), self.target, Vec3::Y)
    }

    /// Get the projection matrix for a given aspect ratio.
    pub fn projection_matrix(&self, aspect_ratio: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov, aspect_ratio, self.near, self.far)
    }

    /// Get the combined view-projection matrix.
    pub fn view_projection(&self, aspect_ratio: f32) -> Mat4 {
        self.projection_matrix(aspect_ratio) * self.view_matrix()
    }

    /// Orbit the camera horizontally and vertically.
    pub fn orbit(&mut self, delta_azimuth: f32, delta_elevation: f32) {
        self.azimuth += delta_azimuth;
        self.elevation = (self.elevation + delta_elevation).clamp(
            -std::f32::consts::FRAC_PI_2 + 0.01,
            std::f32::consts::FRAC_PI_2 - 0.01,
        );
    }

    /// Zoom the camera (change distance to target). `factor` is a
    /// multiplier — 0.1 means "10% closer", -0.1 means "10% farther".
    /// Multiplicative scaling keeps perceived zoom speed consistent across
    /// distances; additive scaling felt linear at far range and explosive
    /// at close range.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * (1.0 + factor)).clamp(0.05, 1000.0);
    }

    /// Snap the look-at target to `new_target` while preserving the camera's
    /// world position (and therefore the visual frame). Recomputes
    /// `distance`, `azimuth`, `elevation` so the camera looks at the new
    /// target from the same point in space.
    ///
    /// Used at the start of an orbit drag to put the rotation pivot under
    /// the cursor: without this, orbit spins around the map-centre regardless
    /// of zoom level, which makes close-up rotation feel like the camera is
    /// flying through the scene.
    pub fn snap_target_preserving_position(&mut self, new_target: Vec3) {
        let pos = self.position();
        let view_vec = pos - new_target;
        let new_distance = view_vec.length();
        if new_distance < 1e-4 {
            // Degenerate -- new target coincides with camera. Leave camera
            // untouched.
            return;
        }
        let dir = view_vec / new_distance;
        let elev = dir.y.clamp(-1.0, 1.0).asin().clamp(
            -std::f32::consts::FRAC_PI_2 + 0.01,
            std::f32::consts::FRAC_PI_2 - 0.01,
        );
        let azim = dir.z.atan2(dir.x);
        self.elevation = elev;
        self.azimuth = azim;
        self.distance = new_distance.clamp(0.05, 1000.0);
        self.target = new_target;
    }

    /// Pan the camera target along the camera's apparent ground-plane axes.
    /// `right` moves along the camera's screen-right axis (projected onto
    /// the XZ plane); `forward` moves into/out of the scene along the
    /// camera's view direction (also XZ-projected). Both deltas are
    /// expressed in world units; callers scale by drag delta * sensitivity.
    ///
    /// Y is intentionally untouched — orbital cameras pan parallel to the
    /// ground, never up/down. Use `orbit` for elevation changes.
    pub fn pan_xz(&mut self, right: f32, forward: f32) {
        let view_dir = self.target - self.position();
        let forward_xz = Vec3::new(view_dir.x, 0.0, view_dir.z).normalize_or_zero();
        // Camera-right in a right-handed Y-up frame is `cross(view_dir, up)`,
        // not `cross(up, view_dir)` -- the previous formula gave LEFT, which
        // is why middle-mouse drag right was moving the target LEFT (so the
        // scene slid right while forward/back panned with the cursor).
        // cross(forward_xz, Y) = (forward.z*0 - 0*1, 0*0 - forward.x*0,
        //                          forward.x*1 - 0*0) = (-forward.z, 0, forward.x).
        let right_xz = Vec3::new(-forward_xz.z, 0.0, forward_xz.x);
        self.target += right_xz * right + forward_xz * forward;
    }

    /// Legacy free-form pan kept for tests / non-orbital use cases.
    pub fn pan(&mut self, delta: Vec3) {
        self.target += delta;
    }

    /// Lift `target.y` so the camera position is at least `min_y` in
    /// world space. No-op when the camera is already above `min_y`.
    /// The view direction is preserved (target and position rise by
    /// the same delta), so the framing shifts upward by the lift
    /// amount but the angle and content stay the same. Used by the
    /// viewport code to keep the camera from clipping through
    /// terrain.
    pub fn clamp_position_above_y(&mut self, min_y: f32) {
        let pos = self.position();
        if pos.y < min_y {
            self.target.y += min_y - pos.y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_camera() {
        let cam = Camera::default();
        let pos = cam.position();
        // Camera should be above and to the side
        assert!(pos.y > 0.0);
        assert!(cam.distance > 0.0);
    }

    #[test]
    fn test_orbit() {
        let mut cam = Camera::default();
        let pos_before = cam.position();
        cam.orbit(0.5, 0.0);
        let pos_after = cam.position();
        // Position should change after orbit
        assert!((pos_before - pos_after).length() > 0.01);
    }

    #[test]
    fn clamp_above_lifts_target_when_position_below_floor() {
        let mut cam = Camera::default();
        let original_pos_y = cam.position().y;
        // Pick a floor well above current position.y; clamp should
        // lift target.y by (floor - pos.y).
        let floor = original_pos_y + 0.5;
        let original_target_y = cam.target.y;
        cam.clamp_position_above_y(floor);
        assert!((cam.target.y - (original_target_y + 0.5)).abs() < 1e-5);
        // Camera position should now be exactly at the floor.
        assert!((cam.position().y - floor).abs() < 1e-5);
    }

    #[test]
    fn clamp_above_is_noop_when_already_above_floor() {
        let mut cam = Camera::default();
        let pos_before = cam.position();
        let target_before = cam.target;
        // Floor well below current position.
        cam.clamp_position_above_y(pos_before.y - 0.5);
        assert_eq!(cam.target, target_before, "target should not move");
        assert_eq!(cam.position(), pos_before, "position should not move");
    }

    #[test]
    fn clamp_above_preserves_view_direction() {
        let mut cam = Camera::default();
        let original_dir = (cam.target - cam.position()).normalize();
        cam.clamp_position_above_y(cam.position().y + 0.3);
        let new_dir = (cam.target - cam.position()).normalize();
        // Lift moves both target and position by the same Y delta
        // so the look vector direction stays the same.
        assert!((new_dir - original_dir).length() < 1e-5);
    }

    #[test]
    fn test_elevation_clamping() {
        let mut cam = Camera::default();
        cam.orbit(0.0, 100.0); // Try to go past 90 degrees
        assert!(cam.elevation < std::f32::consts::FRAC_PI_2);
    }
}
