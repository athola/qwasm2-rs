//! Light style brightness for the renderer. C ref: `CL_SetLightstyle` and
//! `CL_RunLightStyles` (client/cl_lights.c).

use q2_shared::constants::MAX_LIGHTSTYLES;

/// One style's brightness this frame for `pattern` (a `CS_LIGHTS` string):
/// a letter per 100 ms, 'a' = 0, 'm' = 1, 'z' ≈ 2, and below 'a' negative
/// as in C. An empty pattern is full brightness.
pub fn style_value(pattern: &str, time_ms: u32) -> f32 {
    let bytes = pattern.as_bytes();
    if bytes.is_empty() {
        return 1.0;
    }
    let ofs = (time_ms / 100) as usize;
    let ch = bytes[ofs % bytes.len()];
    (f32::from(ch) - f32::from(b'a')) / f32::from(b'm' - b'a')
}

/// Every style's brightness this frame, from `configstring(CS_LIGHTS + i)`.
pub fn style_values<'a>(
    pattern: impl Fn(usize) -> &'a str,
    time_ms: u32,
) -> [f32; MAX_LIGHTSTYLES] {
    std::array::from_fn(|i| style_value(pattern(i), time_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m_is_normal_a_is_dark_and_empty_is_full() {
        assert_eq!(style_value("m", 0), 1.0);
        assert_eq!(style_value("a", 12_345), 0.0);
        assert_eq!(style_value("", 0), 1.0);
        assert_eq!(style_value("z", 0), 25.0 / 12.0);
    }

    #[test]
    fn characters_below_a_are_negative_like_c() {
        // C computes `s[k] - 'a'` in int, not as an unsigned byte.
        assert_eq!(style_value("A", 0), -32.0 / 12.0);
        assert_eq!(style_value("0", 0), -49.0 / 12.0);
    }

    #[test]
    fn patterns_step_a_letter_every_tenth_of_a_second() {
        assert_eq!(style_value("am", 99), 0.0);
        assert_eq!(style_value("am", 100), 1.0);
        assert_eq!(style_value("am", 200), 0.0, "wraps");
    }

    #[test]
    fn style_values_covers_every_style() {
        let values: [f32; MAX_LIGHTSTYLES] = style_values(|i| if i == 1 { "a" } else { "" }, 0);
        assert_eq!((values[0], values[1]), (1.0, 0.0));
    }
}
