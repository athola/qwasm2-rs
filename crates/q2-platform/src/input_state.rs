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
    /// The page's Keys button was pressed since the last frame.
    pub legend_requested: bool,
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

    /// Release every key and button, and drop taps not yet taken. Called
    /// when the page can no longer see the matching key-up or mouse-up
    /// (focus or pointer lock lost), so nothing stays held.
    pub fn release_all(&mut self) {
        self.keys = [false; 256];
        self.mouse_buttons = 0;
        self.pressed_keys = [false; 256];
        self.pressed_buttons = 0;
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
    }

    /// This frame's input: what is held or went down since the last call
    /// (C `CL_FinishMove` tests `state & 3`, then clears bit 2), the mouse
    /// motion and the legend request. The press latches, the motion and
    /// the legend request start over; held keys and buttons stay.
    pub fn take_frame(&mut self) -> Self {
        let mut frame = self.clone();
        for (key, pressed) in frame.keys.iter_mut().zip(self.pressed_keys) {
            *key |= pressed;
        }
        frame.mouse_buttons |= self.pressed_buttons;
        self.pressed_keys = [false; 256];
        self.pressed_buttons = 0;
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
        self.legend_requested = false;
        frame
    }

    /// Pointer lock was gained (`locked`) or lost. Motion from before is
    /// stale either way. Held keys and buttons are kept on gaining it,
    /// which browsers also report when the already locked canvas is
    /// clicked again; on losing it, a button or key may be released where
    /// the page cannot see it, so everything is released.
    pub fn pointer_lock_changed(&mut self, locked: bool) {
        if locked {
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        } else {
            self.release_all();
        }
    }
}

/// What a click on the game canvas asks the browser for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClickRequests {
    /// Fullscreen and keyboard lock (C `vid_fullscreen` 1), so browser
    /// shortcuts such as Ctrl+W reach the game.
    pub fullscreen: bool,
    pub pointer_lock: bool,
}

impl ClickRequests {
    /// Only what the page does not hold yet: asking for pointer lock again
    /// makes the browser fire pointerlockchange for nothing.
    pub fn for_click(vid_fullscreen: bool, fullscreen: bool, pointer_locked: bool) -> Self {
        Self {
            fullscreen: vid_fullscreen && !fullscreen,
            pointer_lock: !pointer_locked,
        }
    }
}

impl Default for WasmInputState {
    fn default() -> Self {
        Self {
            keys: [false; 256],
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            mouse_buttons: 0,
            legend_requested: false,
            pressed_keys: [false; 256],
            pressed_buttons: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held() -> WasmInputState {
        let mut state = WasmInputState::default();
        state.keys[b'w' as usize] = true;
        state.mouse_buttons = 1;
        state.mouse_dx = 40.0;
        state.mouse_dy = -3.0;
        state
    }

    #[test]
    fn gaining_pointer_lock_keeps_held_keys_and_drops_stale_mouse_motion() {
        // Browsers fire pointerlockchange again when the locked canvas is
        // clicked (each mouse shot): held movement must survive it.
        let mut state = held();
        state.pointer_lock_changed(true);
        assert!(state.keys[b'w' as usize]);
        assert_eq!(state.mouse_buttons, 1);
        assert_eq!((state.mouse_dx, state.mouse_dy), (0.0, 0.0));
    }

    #[test]
    fn a_click_asks_only_for_what_the_page_does_not_have() {
        let ask = |vid_fullscreen, fullscreen, locked| {
            ClickRequests::for_click(vid_fullscreen, fullscreen, locked)
        };
        // vid_fullscreen 0: pointer lock only, and not again while held.
        assert_eq!(
            ask(false, false, false),
            ClickRequests {
                fullscreen: false,
                pointer_lock: true
            }
        );
        assert_eq!(ask(false, false, true), ClickRequests::default());
        // vid_fullscreen 1: fullscreen (with keyboard lock) as well.
        assert_eq!(
            ask(true, false, false),
            ClickRequests {
                fullscreen: true,
                pointer_lock: true
            }
        );
        assert_eq!(
            ask(true, false, true),
            ClickRequests {
                fullscreen: true,
                pointer_lock: false
            }
        );
        assert_eq!(ask(true, true, true), ClickRequests::default());
    }

    #[test]
    fn a_frame_takes_the_motion_and_legend_request_and_keeps_held_keys() {
        let mut state = held();
        state.legend_requested = true;
        let frame = state.take_frame();
        assert_eq!((frame.mouse_dx, frame.mouse_dy), (40.0, -3.0));
        assert!(frame.legend_requested);
        assert_eq!((state.mouse_dx, state.mouse_dy), (0.0, 0.0));
        assert!(!state.legend_requested, "one toggle per click");
        assert!(state.keys[b'w' as usize] && state.mouse_buttons == 1);
    }

    #[test]
    fn losing_pointer_lock_or_focus_releases_everything() {
        let mut state = held();
        state.pointer_lock_changed(false);
        assert!(!state.keys[b'w' as usize]);
        assert_eq!(state.mouse_buttons, 0);
        let mut state = held();
        state.release_all();
        assert!(!state.keys.iter().any(|&k| k));
        assert_eq!((state.mouse_buttons, state.mouse_dx), (0, 0.0));
    }

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
