use q2_shared::types::*;

/// Calculate vertical FOV from horizontal FOV and aspect ratio.
pub fn calc_fov(fov_x: f32, width: f32, height: f32) -> f32 {
    let x = width / (fov_x / 2.0).to_radians().tan();
    (height / x).atan().to_degrees() * 2.0
}

/// Interpolate an angle, handling the 360° wrapping boundary correctly.
///
/// Mirrors Q2's `LerpAngle()`. Without this, interpolating from 350° to 10°
/// would sweep through 180° instead of taking the short 20° path.
// Will be used for entity interpolation in the client render loop.
#[allow(dead_code)]
fn lerp_angle(a: f32, b: f32, frac: f32) -> f32 {
    let mut delta = b - a;
    if delta > 180.0 {
        delta -= 360.0;
    }
    if delta < -180.0 {
        delta += 360.0;
    }
    a + delta * frac
}

/// Interpolate between two entity states.
// Will be used for entity interpolation in the client render loop.
#[allow(dead_code)]
pub fn lerp_entity(prev: &EntityState, current: &EntityState, frac: f32) -> (Vec3f, Vec3f) {
    let origin = prev.origin + (current.origin - prev.origin) * frac;
    let angles = Vec3f::new(
        lerp_angle(prev.angles.x, current.angles.x, frac),
        lerp_angle(prev.angles.y, current.angles.y, frac),
        lerp_angle(prev.angles.z, current.angles.z, frac),
    );
    (origin, angles)
}
