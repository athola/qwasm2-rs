use q2_shared::types::*;

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
    /// Buttons
    pub attack: bool,
    pub jump: bool,
    pub crouch: bool,
    pub use_btn: bool,
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
        }
    }
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a UserCmd from current input state.
    pub fn build_cmd(&self, msec: u8) -> UserCmd {
        UserCmd {
            msec,
            buttons: self.button_bits(),
            angles: [
                angle_to_short(self.view_angles.x),
                angle_to_short(self.view_angles.y),
                angle_to_short(self.view_angles.z),
            ],
            forwardmove: (self.forward_move.clamp(-1.0, 1.0) * 127.0) as i16,
            sidemove: (self.side_move.clamp(-1.0, 1.0) * 127.0) as i16,
            upmove: (self.up_move.clamp(-1.0, 1.0) * 127.0) as i16,
            impulse: 0,
            lightlevel: 0,
        }
    }

    fn button_bits(&self) -> u8 {
        let mut bits = 0u8;
        if self.attack {
            bits |= 1;
        }
        if self.jump {
            bits |= 2;
        }
        if self.crouch {
            bits |= 4;
        }
        if self.use_btn {
            bits |= 8;
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
