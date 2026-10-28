use q2_shared::types::*;

/// Default `cl_forwardspeed`, `cl_sidespeed` and `cl_upspeed` (cl_main.c),
/// in pmove units per second.
const CL_FORWARDSPEED: f32 = 200.0;
const CL_SIDESPEED: f32 = 200.0;
const CL_UPSPEED: f32 = 200.0;

/// `usercmd_t.buttons` bits (shared.h).
pub const BUTTON_ATTACK: u8 = 1;
pub const BUTTON_USE: u8 = 2;
/// Any key whatsoever.
pub const BUTTON_ANY: u8 = 128;

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
            // No weapon-select or inventory input yet (#16): C `in_impulse`
            // is not produced, and nothing sets `use_btn`.
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

#[cfg(test)]
mod tests {
    use super::*;

    // C CL_BaseMove: cl_forwardspeed/cl_sidespeed/cl_upspeed default to 200
    // (cl_main.c) and running doubles them.
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
