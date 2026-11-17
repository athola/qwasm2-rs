use q2_shared::constants::{BUTTON_ANY, BUTTON_ATTACK, BUTTON_USE};
use q2_shared::types::*;

/// Default `cl_forwardspeed`, `cl_sidespeed` and `cl_upspeed` (cl_main.c),
/// in pmove units per second.
const CL_FORWARDSPEED: f32 = 200.0;
const CL_SIDESPEED: f32 = 200.0;
const CL_UPSPEED: f32 = 200.0;

/// Input state
#[derive(Debug)]
pub struct InputState {
    /// Key states (indexed by key code)
    pub keys: [bool; 256],
    /// Mouse delta this frame
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    /// View angles accumulated from mouse
    pub view_angles: Vec3f,
    /// Movement values
    pub forward_move: f32,
    pub side_move: f32,
    pub up_move: f32,
    /// Buttons. Jump and crouch are C +moveup/+movedown: they drive
    /// `upmove`, not button bits.
    pub attack: bool,
    pub jump: bool,
    pub crouch: bool,
    pub use_btn: bool,
    /// Speed key XOR `cl_run`: doubles every move (C `CL_BaseMove`).
    pub run: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            keys: [false; 256],
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            view_angles: Vec3f::ZERO,
            forward_move: 0.0,
            side_move: 0.0,
            up_move: 0.0,
            attack: false,
            jump: false,
            crouch: false,
            use_btn: false,
            run: false,
        }
    }
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a UserCmd from current input state, scaled like C
    /// `CL_BaseMove` + `CL_FinishMove`.
    pub fn build_cmd(&self, msec: u8) -> UserCmd {
        let speed = if self.run { 2.0 } else { 1.0 };
        let up = self.up_move + f32::from(u8::from(self.jump)) - f32::from(u8::from(self.crouch));
        UserCmd {
            msec,
            buttons: self.button_bits(),
            angles: [
                angle_to_short(self.view_angles.x),
                angle_to_short(self.view_angles.y),
                angle_to_short(self.view_angles.z),
            ],
            forwardmove: (self.forward_move.clamp(-1.0, 1.0) * CL_FORWARDSPEED * speed) as i16,
            sidemove: (self.side_move.clamp(-1.0, 1.0) * CL_SIDESPEED * speed) as i16,
            upmove: (up.clamp(-1.0, 1.0) * CL_UPSPEED * speed) as i16,
            // Weapon and inventory selection go through client commands
            // (`LocalGame::client_command`); C `in_impulse` is not
            // produced, and nothing sets `use_btn`.
            impulse: 0,
            lightlevel: 0,
        }
    }

    fn button_bits(&self) -> u8 {
        let mut bits = 0u8;
        if self.attack {
            bits |= BUTTON_ATTACK;
        }
        if self.use_btn {
            bits |= BUTTON_USE;
        }
        let any_key_down = self.keys.iter().any(|&down| down)
            || self.attack
            || self.use_btn
            || self.jump
            || self.crouch;
        if any_key_down {
            bits |= BUTTON_ANY;
        }
        bits
    }

    /// Clear per-frame deltas.
    pub fn clear_frame(&mut self) {
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
    }
}

fn angle_to_short(angle: f32) -> i16 {
    ((angle * 65536.0 / 360.0) as i32 & 0xFFFF) as i16
}

/// Mouse sensitivity (degrees per pixel of mouse movement). A choice of
/// this port, not C's default (`sensitivity 3` times `m_yaw 0.022` is
/// 0.066 deg/px); there is no sensitivity cvar yet.
pub const MOUSE_SENSITIVITY: f32 = 0.15;

/// C `cl_yawspeed` / `cl_pitchspeed` defaults: keyboard turning, deg/s.
pub const CL_YAWSPEED: f32 = 140.0;
pub const CL_PITCHSPEED: f32 = 150.0;
/// C `cl_anglespeedkey` default: the turn speed factor while +speed is held.
pub const CL_ANGLESPEEDKEY: f32 = 1.5;

/// Move time for a user command (C CL_FinishMove): the frame time in whole
/// milliseconds, or 100 when it is over 250 ("time was unreasonable").
pub fn cmd_msec(dt_seconds: f32) -> u8 {
    let ms = (dt_seconds * 1000.0) as i32;
    if ms > 250 {
        100
    } else {
        ms.max(0) as u8
    }
}

/// The light level a user command carries (C CL_FinishMove's
/// `(byte)cl_lightlevel->value`): truncated, keeping the low 8 bits as the
/// C conversion does on wasm and x86.
pub fn cmd_lightlevel(r_lightlevel: f32) -> u8 {
    r_lightlevel as i32 as u8
}

/// Key and mouse-button state into movement and button input, with
/// Quake 2's default bindings (WASD and arrows, alt+arrows strafe, space
/// jump, c crouch, shift run, ctrl or mouse1 attack).
pub fn apply_keys(keys: &[bool; 256], mouse_buttons: u8, input: &mut InputState) {
    use q2_common::keys::*;
    let down = |k: u8| keys[k as usize];
    let axis = |pos: bool, neg: bool| f32::from(u8::from(pos)) - f32::from(u8::from(neg));
    let strafe = down(K_ALT);
    input.forward_move = axis(
        down(b'w') || down(K_UPARROW),
        down(b's') || down(K_DOWNARROW),
    );
    input.side_move = axis(
        down(b'd') || down(b'.') || (strafe && down(K_RIGHTARROW)),
        down(b'a') || down(b',') || (strafe && down(K_LEFTARROW)),
    );
    input.jump = down(K_SPACE);
    input.crouch = down(b'c');
    input.run = down(K_SHIFT);
    input.attack = down(K_CTRL) || mouse_buttons & 1 != 0;
    input.keys = *keys;
}

/// Turn the view by mouse movement and the turn keys (C CL_AdjustAngles and
/// the mouse move), then clamp pitch as C CL_ClampPitch does against the
/// player's `delta_pitch` (degrees, already in -180..180).
pub fn turn_view(
    input: &mut InputState,
    keys: &[bool; 256],
    mouse_dx: f32,
    mouse_dy: f32,
    dt: f32,
    delta_pitch: f32,
) {
    use q2_common::keys::*;
    let down = |k: u8| keys[k as usize];
    let angles = &mut input.view_angles;
    angles.y -= mouse_dx * MOUSE_SENSITIVITY;
    angles.x += mouse_dy * MOUSE_SENSITIVITY;
    // +speed (shift) turns faster too.
    let speed = if down(K_SHIFT) {
        dt * CL_ANGLESPEEDKEY
    } else {
        dt
    };
    if !down(K_ALT) {
        if down(K_LEFTARROW) {
            angles.y += CL_YAWSPEED * speed;
        }
        if down(K_RIGHTARROW) {
            angles.y -= CL_YAWSPEED * speed;
        }
    }
    if down(K_PGDN) {
        angles.x -= CL_PITCHSPEED * speed; // +lookup
    }
    if down(K_DEL) {
        angles.x += CL_PITCHSPEED * speed; // +lookdown
    }

    if angles.x + delta_pitch < -360.0 {
        angles.x += 360.0;
    }
    if angles.x + delta_pitch > 360.0 {
        angles.x -= 360.0;
    }
    if angles.x + delta_pitch > 89.0 {
        angles.x = 89.0 - delta_pitch;
    }
    if angles.x + delta_pitch < -89.0 {
        angles.x = -89.0 - delta_pitch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // C CL_BaseMove: cl_forwardspeed/cl_sidespeed/cl_upspeed default to 200
    // (cl_main.c) and running doubles them.
    #[test]
    fn cmd_lightlevel_is_the_renderers_light_level_cast_to_a_byte_like_c() {
        assert_eq!(cmd_lightlevel(150.0), 150);
        assert_eq!(cmd_lightlevel(5.9), 5, "truncated toward zero");
        assert_eq!(cmd_lightlevel(0.0), 0);
        // C `(byte)cl_lightlevel->value` keeps the low 8 bits.
        assert_eq!(cmd_lightlevel(300.0), 44);
    }

    #[test]
    fn cmd_msec_is_the_frame_time_with_c_unreasonable_time_rule() {
        assert_eq!(cmd_msec(0.016), 16);
        assert_eq!(cmd_msec(0.25), 250);
        // CL_FinishMove: over 250 ms "time was unreasonable", send 100.
        assert_eq!(cmd_msec(0.4), 100);
        assert_eq!(cmd_msec(-1.0), 0);
    }

    #[test]
    fn apply_keys_maps_wasd_jump_crouch_run_and_attack() {
        use q2_common::keys::{K_CTRL, K_SHIFT, K_SPACE};
        let mut keys = [false; 256];
        keys[b'w' as usize] = true;
        keys[b'd' as usize] = true;
        keys[K_SPACE as usize] = true;
        keys[K_SHIFT as usize] = true;
        let mut input = InputState::default();
        apply_keys(&keys, 0, &mut input);
        assert_eq!((input.forward_move, input.side_move), (1.0, 1.0));
        assert!(input.jump && input.run && !input.crouch && !input.attack);

        keys[K_CTRL as usize] = true;
        apply_keys(&keys, 0, &mut input);
        assert!(input.attack, "ctrl is +attack");
        apply_keys(&[false; 256], 1, &mut input);
        assert!(input.attack, "mouse1 is +attack");
        assert_eq!(input.forward_move, 0.0);
    }

    #[test]
    fn turn_view_follows_c_mouse_signs_and_clamps_pitch() {
        let mut input = InputState::default();
        // Mouse right turns right (yaw down), mouse down looks down (+pitch).
        turn_view(&mut input, &[false; 256], 10.0, 10.0, 0.0, 0.0);
        assert!(input.view_angles.y < 0.0 && input.view_angles.x > 0.0);

        // CL_ClampPitch: pitch + delta pitch stays within +-89.
        turn_view(&mut input, &[false; 256], 0.0, 100_000.0, 0.0, 0.0);
        assert_eq!(input.view_angles.x, 89.0);
        turn_view(&mut input, &[false; 256], 0.0, -100_000.0, 0.0, 10.0);
        assert_eq!(input.view_angles.x, -99.0);
    }

    #[test]
    fn turn_view_arrow_keys_turn_at_cl_yawspeed() {
        use q2_common::keys::K_LEFTARROW;
        let mut input = InputState::default();
        let mut keys = [false; 256];
        keys[K_LEFTARROW as usize] = true;
        turn_view(&mut input, &keys, 0.0, 0.0, 0.5, 0.0);
        assert!(
            (input.view_angles.y - 70.0).abs() < 1e-4,
            "140 deg/s for 0.5 s"
        );
    }

    #[test]
    fn turn_view_speed_key_turns_at_cl_anglespeedkey_times_the_speed() {
        use q2_common::keys::{K_LEFTARROW, K_SHIFT};
        let mut input = InputState::default();
        let mut keys = [false; 256];
        keys[K_LEFTARROW as usize] = true;
        keys[K_SHIFT as usize] = true;
        turn_view(&mut input, &keys, 0.0, 0.0, 0.5, 0.0);
        assert!(
            (input.view_angles.y - 105.0).abs() < 1e-4,
            "140 deg/s * 1.5 for 0.5 s, got {}",
            input.view_angles.y
        );
    }

    #[test]
    fn build_cmd_full_forward_is_200_and_run_is_400() {
        let mut input = InputState {
            forward_move: 1.0,
            side_move: -1.0,
            ..Default::default()
        };
        let walk = input.build_cmd(16);
        assert_eq!(walk.forwardmove, 200);
        assert_eq!(walk.sidemove, -200);

        input.run = true;
        let run = input.build_cmd(16);
        assert_eq!(run.forwardmove, 400);
        assert_eq!(run.sidemove, -400);
    }

    // Jump and crouch are +moveup / +movedown in C: they drive upmove,
    // which pmove reads (> 0 jump, < 0 duck). They are not buttons.
    #[test]
    fn jump_sets_upmove_not_button_use() {
        let input = InputState {
            jump: true,
            ..Default::default()
        };
        let cmd = input.build_cmd(16);
        assert_eq!(cmd.upmove, 200);
        assert_eq!(cmd.buttons & BUTTON_USE, 0);
    }

    #[test]
    fn crouch_sets_negative_upmove() {
        let input = InputState {
            crouch: true,
            run: true,
            ..Default::default()
        };
        assert_eq!(input.build_cmd(16).upmove, -400);
    }

    // shared.h: BUTTON_ATTACK 1, BUTTON_USE 2, BUTTON_ANY 128.
    #[test]
    fn use_sets_button_use_and_any() {
        let input = InputState {
            use_btn: true,
            ..Default::default()
        };
        assert_eq!(input.build_cmd(16).buttons, BUTTON_USE | BUTTON_ANY);
    }

    #[test]
    fn any_key_down_sets_button_any() {
        let mut input = InputState::new();
        assert_eq!(input.build_cmd(16).buttons, 0);
        input.keys[b'w' as usize] = true;
        assert_eq!(input.build_cmd(16).buttons, BUTTON_ANY);
    }

    #[test]
    fn angle_to_short_zero() {
        assert_eq!(angle_to_short(0.0), 0);
    }

    #[test]
    fn angle_to_short_90() {
        let result = angle_to_short(90.0);
        // 90 degrees = 65536/4 = 16384
        assert_eq!(result, 16384);
    }

    #[test]
    fn angle_to_short_180() {
        // 180 degrees = 65536/2 = 32768, which wraps to -32768 as i16
        assert_eq!(angle_to_short(180.0), -32768);
    }

    #[test]
    fn angle_to_short_270() {
        // 270 degrees = 65536 * 3/4 = 49152, wraps to -16384 as i16
        assert_eq!(angle_to_short(270.0), -16384);
    }

    #[test]
    fn angle_to_short_360_wraps() {
        // 360 degrees should wrap back to 0
        assert_eq!(angle_to_short(360.0), 0);
    }

    #[test]
    fn angle_to_short_negative() {
        // -90 degrees = same as 270 = -16384 as i16
        assert_eq!(angle_to_short(-90.0), -16384);
    }
}
