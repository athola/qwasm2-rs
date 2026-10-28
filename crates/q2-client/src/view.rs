use q2_common::error::{Q2Error, Q2Result};

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
