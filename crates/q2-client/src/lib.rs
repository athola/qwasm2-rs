//! Qwasm2-rs: Client — input, server message parsing, client state, view math
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod entities;
pub mod input;
pub mod layout;
pub mod lightstyles;
pub mod parse;
pub mod state;
pub mod view;

#[cfg(test)]
mod tests {
    use super::input::*;
    use super::state::*;
    use super::view::*;
    use q2_shared::types::*;

    #[test]
    fn client_state_default() {
        let state = ConnState::default();
        assert_eq!(state, ConnState::Disconnected);
    }

    #[test]
    fn input_build_cmd() {
        let mut input = InputState::new();
        input.forward_move = 1.0;
        input.side_move = -0.5;
        input.view_angles = Vec3f::new(10.0, 20.0, 0.0);

        let cmd = input.build_cmd(16);
        assert_eq!(cmd.msec, 16);
        // cl_forwardspeed / cl_sidespeed default to 200 in C.
        assert_eq!(cmd.forwardmove, 200);
        assert_eq!(cmd.sidemove, -100);
    }

    #[test]
    fn input_button_bits() {
        let mut input = InputState::new();
        input.attack = true;
        input.jump = true;
        input.crouch = true;
        let cmd = input.build_cmd(16);
        // BUTTON_ATTACK (1) | BUTTON_ANY (128); jump and crouch go to upmove.
        assert_eq!(cmd.buttons, 129);
    }

    #[test]
    fn calc_fov_90() {
        // At 4:3 ratio (640x480), horizontal FOV 90 -> vertical FOV ~73.74
        let fov_y = calc_fov(90.0, 640.0, 480.0).unwrap();
        assert!((fov_y - 73.74).abs() < 0.1, "fov_y was {}", fov_y);
    }

    #[test]
    fn calc_fov_rejects_fov_outside_1_to_179_like_c() {
        // C CalcFov raises ERR_DROP for fov_x < 1 or > 179.
        assert!(calc_fov(0.5, 640.0, 480.0).is_err());
        assert!(calc_fov(179.5, 640.0, 480.0).is_err());
        assert!(calc_fov(1.0, 640.0, 480.0).is_ok());
        assert!(calc_fov(179.0, 640.0, 480.0).is_ok());
    }

    #[test]
    fn lerp_entity_midpoint() {
        let prev = EntityState {
            origin: Vec3f::new(0.0, 0.0, 0.0),
            angles: Vec3f::new(0.0, 0.0, 0.0),
            ..Default::default()
        };
        let current = EntityState {
            origin: Vec3f::new(10.0, 20.0, 30.0),
            angles: Vec3f::new(90.0, 180.0, 0.0),
            ..Default::default()
        };
        let (origin, angles) = lerp_entity(&prev, &current, 0.5);
        assert!((origin.x - 5.0).abs() < 0.001);
        assert!((origin.y - 10.0).abs() < 0.001);
        assert!((origin.z - 15.0).abs() < 0.001);
        assert!((angles.x - 45.0).abs() < 0.001);
        assert!((angles.y - 90.0).abs() < 0.001);
    }

    #[test]
    fn lerp_entity_angle_wrapping() {
        // 350° → 10° should interpolate through 0°, not through 180°
        let prev = EntityState {
            angles: Vec3f::new(0.0, 350.0, 0.0),
            ..Default::default()
        };
        let current = EntityState {
            angles: Vec3f::new(0.0, 10.0, 0.0),
            ..Default::default()
        };
        let (_, angles) = lerp_entity(&prev, &current, 0.5);
        // Midpoint of 350→10 (short path) is 0 (or 360)
        assert!(
            angles.y.abs() < 0.001 || (angles.y - 360.0).abs() < 0.001,
            "expected ~0 or ~360, got {}",
            angles.y
        );
    }
}
