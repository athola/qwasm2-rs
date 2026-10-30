//! The input the browser's event listeners collect between two frames.
//! Platform-neutral so the latching is tested natively; the listeners live
//! in `wasm::input`.

/// Shared input state that event listeners write to.
#[derive(Debug, Clone)]
pub struct WasmInputState {
    pub keys: [bool; 256],
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    pub mouse_buttons: u8,
    /// Keys and buttons that went down since the last `take_frame` (C
    /// `kbutton_t` state bit 2), so a tap released before the frame
    /// samples the input still counts once.
    pressed_keys: [bool; 256],
    pressed_buttons: u8,
}

impl WasmInputState {
    pub fn key_down(&mut self, key: u8) {
        self.keys[usize::from(key)] = true;
        self.pressed_keys[usize::from(key)] = true;
    }

    pub fn key_up(&mut self, key: u8) {
        self.keys[usize::from(key)] = false;
    }

    /// `mask` is the button's bit in `mouse_buttons`.
    pub fn button_down(&mut self, mask: u8) {
        self.mouse_buttons |= mask;
        self.pressed_buttons |= mask;
    }

    pub fn button_up(&mut self, mask: u8) {
        self.mouse_buttons &= !mask;
    }

    /// This frame's input: what is held or went down since the last call
    /// (C `CL_FinishMove` tests `state & 3`, then clears bit 2), and the
    /// mouse motion. The press latches and the motion are reset.
    pub fn take_frame(&mut self) -> WasmInputState {
        let mut frame = self.clone();
        for (key, pressed) in frame.keys.iter_mut().zip(self.pressed_keys) {
            *key |= pressed;
        }
        frame.mouse_buttons |= self.pressed_buttons;
        self.pressed_keys = [false; 256];
        self.pressed_buttons = 0;
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
        frame
    }

    /// Release every key and button. Called when the page can no longer
    /// see the matching key-up or mouse-up (focus lost, pointer lock
    /// changed), so nothing stays held.
    pub fn release_all(&mut self) {
        *self = Self::default();
    }
}

impl Default for WasmInputState {
    fn default() -> Self {
        Self {
            keys: [false; 256],
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            mouse_buttons: 0,
            pressed_keys: [false; 256],
            pressed_buttons: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_released_before_the_frame_counts_once() {
        let mut input = WasmInputState::default();
        input.key_down(b'w');
        input.key_up(b'w');
        input.button_down(1);
        input.button_up(1);
        let frame = input.take_frame();
        assert!(frame.keys[usize::from(b'w')]);
        assert_eq!(frame.mouse_buttons, 1);
        let next = input.take_frame();
        assert!(!next.keys[usize::from(b'w')]);
        assert_eq!(next.mouse_buttons, 0);
    }

    #[test]
    fn a_held_key_stays_down_across_frames() {
        let mut input = WasmInputState::default();
        input.key_down(b'w');
        input.take_frame();
        assert!(input.take_frame().keys[usize::from(b'w')]);
    }

    #[test]
    fn mouse_motion_is_taken_once() {
        let mut input = WasmInputState {
            mouse_dx: 3.0,
            mouse_dy: -2.0,
            ..Default::default()
        };
        let frame = input.take_frame();
        assert_eq!((frame.mouse_dx, frame.mouse_dy), (3.0, -2.0));
        let next = input.take_frame();
        assert_eq!((next.mouse_dx, next.mouse_dy), (0.0, 0.0));
    }

    #[test]
    fn release_all_drops_holds_and_pending_taps() {
        let mut input = WasmInputState::default();
        input.key_down(b'w');
        input.button_down(2);
        input.release_all();
        let frame = input.take_frame();
        assert!(!frame.keys[usize::from(b'w')]);
        assert_eq!(frame.mouse_buttons, 0);
    }
}
