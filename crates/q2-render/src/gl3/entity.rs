//! Model matrices for entities (C `GL3_RotateForEntity` and
//! `rotAroundAxisZYX` in `gl3_main.c`, plus the angle negations its callers
//! `GL3_DrawBrushModel` and `GL3_DrawAliasModel` apply). GL-free.

/// Column-major rotation: around X, then Y, then Z when applied to a column
/// vector, i.e. `R_z * R_y * R_x` (C `rotAroundAxisZYX`, degrees).
fn rot_around_axis_zyx(around_z: f32, around_y: f32, around_x: f32) -> [f32; 16] {
    let (sin_a, cos_a) = around_z.to_radians().sin_cos();
    let (sin_b, cos_b) = around_y.to_radians().sin_cos();
    let (sin_g, cos_g) = around_x.to_radians().sin_cos();
    #[rustfmt::skip]
    let m = [
        cos_a * cos_b,                      sin_a * cos_b,                      -sin_b,        0.0,
        cos_a * sin_b * sin_g - sin_a * cos_g, sin_a * sin_b * sin_g + cos_a * cos_g, cos_b * sin_g, 0.0,
        cos_a * sin_b * cos_g + sin_a * sin_g, sin_a * sin_b * cos_g - cos_a * sin_g, cos_b * cos_g, 0.0,
        0.0,                                0.0,                                0.0,           1.0,
    ];
    m
}

fn with_translation(mut matrix: [f32; 16], origin: [f32; 3]) -> [f32; 16] {
    matrix[12..15].copy_from_slice(&origin);
    matrix
}

/// Brush entities: `GL3_DrawBrushModel` negates pitch and roll and
/// `GL3_RotateForEntity` negates them again, so the net rotation is
/// `Z(yaw) * Y(pitch) * X(roll)`. `angles` are (pitch, yaw, roll) degrees.
pub fn brush_matrix(origin: [f32; 3], angles: [f32; 3]) -> [f32; 16] {
    with_translation(rot_around_axis_zyx(angles[1], angles[0], angles[2]), origin)
}

/// Alias entities: `GL3_DrawAliasModel` negates only pitch around
/// `GL3_RotateForEntity`, which negates pitch and roll, so roll alone ends up
/// negated: `Z(yaw) * Y(pitch) * X(-roll)`.
pub fn alias_matrix(origin: [f32; 3], angles: [f32; 3]) -> [f32; 16] {
    with_translation(
        rot_around_axis_zyx(angles[1], angles[0], -angles[2]),
        origin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]
    }

    fn near(actual: [f32; 3], expected: [f32; 3]) {
        for axis in 0..3 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 1e-5,
                "got {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn no_angles_is_a_pure_translation() {
        let m = brush_matrix([1.0, 2.0, 3.0], [0.0; 3]);
        near(apply(&m, [10.0, 20.0, 30.0]), [11.0, 22.0, 33.0]);
        assert_eq!(m, alias_matrix([1.0, 2.0, 3.0], [0.0; 3]));
    }

    #[test]
    fn yaw_turns_forward_toward_plus_y_about_the_entity_origin() {
        let m = alias_matrix([1.0, 2.0, 3.0], [0.0, 90.0, 0.0]);
        near(apply(&m, [1.0, 0.0, 0.0]), [1.0, 3.0, 3.0]);
        near(apply(&m, [0.0, 1.0, 0.0]), [0.0, 2.0, 3.0]);
    }

    #[test]
    fn positive_pitch_points_the_nose_down_for_both_kinds() {
        // Same as AngleVectors: forward at pitch 90 is (0, 0, -1).
        for m in [
            alias_matrix([0.0; 3], [90.0, 0.0, 0.0]),
            brush_matrix([0.0; 3], [90.0, 0.0, 0.0]),
        ] {
            near(apply(&m, [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]);
        }
    }

    #[test]
    fn roll_has_opposite_sense_for_alias_and_brush_models() {
        // C: brush roll is rotation about +X by +roll, alias by -roll.
        near(
            apply(&brush_matrix([0.0; 3], [0.0, 0.0, 90.0]), [0.0, 1.0, 0.0]),
            [0.0, 0.0, 1.0],
        );
        near(
            apply(&alias_matrix([0.0; 3], [0.0, 0.0, 90.0]), [0.0, 1.0, 0.0]),
            [0.0, 0.0, -1.0],
        );
    }

    #[test]
    fn yaw_is_applied_after_pitch() {
        // Pitch 90 sends +X to -Z; yaw then leaves the Z axis alone.
        let m = alias_matrix([0.0; 3], [90.0, 90.0, 0.0]);
        near(apply(&m, [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]);
        // +Y is untouched by pitch and then yawed to -X.
        near(apply(&m, [0.0, 1.0, 0.0]), [-1.0, 0.0, 0.0]);
    }

    #[test]
    fn rotation_matrix_is_orthonormal() {
        let m = brush_matrix([0.0; 3], [33.0, 71.0, -12.0]);
        for column in 0..3 {
            let c = &m[column * 4..column * 4 + 3];
            let length = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            assert!((length - 1.0).abs() < 1e-5);
        }
    }
}
