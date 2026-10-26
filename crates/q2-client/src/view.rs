use q2_common::error::{Q2Error, Q2Result};
use q2_shared::types::*;

/// Calculate vertical FOV from horizontal FOV and aspect ratio.
///
/// Mirrors C `CalcFov`: trig runs in double and is narrowed to float at the
/// same points, and an out-of-range fov is an ERR_DROP.
pub fn calc_fov(fov_x: f32, width: f32, height: f32) -> Q2Result<f32> {
    if !(1.0..=179.0).contains(&fov_x) {
        return Err(Q2Error::Drop(format!("Bad fov: {fov_x}")));
    }
    let x = width / (f64::from(fov_x / 360.0) * std::f64::consts::PI).tan() as f32;
    let a = f64::from(height / x).atan() as f32;
    Ok((f64::from(a) * 360.0 / std::f64::consts::PI) as f32)
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
pub fn lerp_entity(prev: &EntityState, current: &EntityState, frac: f32) -> (Vec3f, Vec3f) {
    let origin = prev.origin + (current.origin - prev.origin) * frac;
    let angles = Vec3f::new(
        lerp_angle(prev.angles.x, current.angles.x, frac),
        lerp_angle(prev.angles.y, current.angles.y, frac),
        lerp_angle(prev.angles.z, current.angles.z, frac),
    );
    (origin, angles)
}
