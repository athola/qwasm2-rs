use q2_shared::types::*;

/// Calculate vertical FOV from horizontal FOV and aspect ratio.
pub fn calc_fov(fov_x: f32, width: f32, height: f32) -> f32 {
    let x = width / (fov_x / 2.0).to_radians().tan();
    (height / x).atan().to_degrees() * 2.0
}

/// Interpolate between two entity states.
pub fn lerp_entity(prev: &EntityState, current: &EntityState, frac: f32) -> (Vec3f, Vec3f) {
    let origin = prev.origin + (current.origin - prev.origin) * frac;
    let angles = prev.angles + (current.angles - prev.angles) * frac;
    (origin, angles)
}
