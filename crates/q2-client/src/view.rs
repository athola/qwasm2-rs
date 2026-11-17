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
    Ok((f64::from(a * 360.0) / std::f64::consts::PI) as f32)
}

/// Horizontal fov the view is fitted to (C `fov` cvar default).
pub const FOV_X: f32 = 90.0;

/// Render size and the vertical fov that fits it, refreshed whenever the
/// platform resizes the canvas so the picture is not stretched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSize {
    pub width: i32,
    pub height: i32,
    pub fov_y: f32,
}

impl ViewSize {
    /// `Ok(None)` when the canvas has no usable area (hidden, collapsed or
    /// too large for GL's i32 viewport): there is no aspect ratio to fit.
    pub fn for_canvas(width: u32, height: u32) -> Q2Result<Option<Self>> {
        let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) else {
            return Ok(None);
        };
        if width == 0 || height == 0 {
            return Ok(None);
        }
        let fov_y = calc_fov(FOV_X, width as f32, height as f32)?;
        Ok(Some(Self {
            width,
            height,
            fov_y,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_size_fits_vertical_fov_to_canvas_aspect() {
        let view = ViewSize::for_canvas(640, 480).unwrap().unwrap();
        assert_eq!((view.width, view.height), (640, 480));
        assert!((view.fov_y - 73.74).abs() < 0.01, "fov_y {}", view.fov_y);

        // A wider canvas keeps fov_x 90 and narrows fov_y, as C CalcFov does.
        let wide = ViewSize::for_canvas(1920, 1080).unwrap().unwrap();
        assert!((wide.fov_y - 58.72).abs() < 0.01, "fov_y {}", wide.fov_y);
    }

    #[test]
    fn view_size_has_no_fit_for_zero_or_unrepresentable_canvas() {
        // A hidden or collapsed canvas has no aspect ratio to fit.
        assert_eq!(ViewSize::for_canvas(0, 480).unwrap(), None);
        assert_eq!(ViewSize::for_canvas(640, 0).unwrap(), None);
        assert_eq!(ViewSize::for_canvas(u32::MAX, 480).unwrap(), None);
    }
}
