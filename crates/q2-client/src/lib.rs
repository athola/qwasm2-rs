//! Qwasm2-rs: Client — input, sound, menu, console, screen, prediction
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod state;
pub mod parse;
pub mod input;
pub mod view;

#[cfg(test)]
mod tests {
    use super::state::*;
    use super::input::*;
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
        assert_eq!(cmd.forwardmove, 127);
        // -0.5 * 127.0 = -63.5, truncated to -63
        assert_eq!(cmd.sidemove, -63);
    }

    #[test]
    fn input_button_bits() {
        let mut input = InputState::new();
        input.attack = true;
        input.jump = true;
        input.crouch = true;
        let cmd = input.build_cmd(16);
        // attack=1, jump=2, crouch=4 => 7
        assert_eq!(cmd.buttons, 7);
    }

    #[test]
    fn calc_fov_90() {
        // At 4:3 ratio (640x480), horizontal FOV 90 -> vertical FOV ~73.74
        let fov_y = calc_fov(90.0, 640.0, 480.0);
        assert!((fov_y - 73.74).abs() < 0.1, "fov_y was {}", fov_y);
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
        assert!(angles.y.abs() < 0.001 || (angles.y - 360.0).abs() < 0.001,
            "expected ~0 or ~360, got {}", angles.y);
    }
}
