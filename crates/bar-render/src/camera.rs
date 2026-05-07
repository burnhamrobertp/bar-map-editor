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
        // right = cross(world_up, forward) for a right-handed frame.
        let right_xz = Vec3::new(forward_xz.z, 0.0, -forward_xz.x);
        self.target += right_xz * right + forward_xz * forward;
    }

    /// Legacy free-form pan kept for tests / non-orbital use cases.
    pub fn pan(&mut self, delta: Vec3) {
        self.target += delta;
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
    fn test_elevation_clamping() {
        let mut cam = Camera::default();
        cam.orbit(0.0, 100.0); // Try to go past 90 degrees
        assert!(cam.elevation < std::f32::consts::FRAC_PI_2);
    }
}
