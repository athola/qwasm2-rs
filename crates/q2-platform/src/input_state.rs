//! What the browser's input listeners record between frames. Kept out of
//! the wasm32-only module so its rules run in native tests.

/// Shared input state that event listeners write to.
#[derive(Debug, Clone)]
pub struct WasmInputState {
    pub keys: [bool; 256],
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    pub mouse_buttons: u8,
    /// The page's Keys button was pressed since the last frame.
    pub legend_requested: bool,
}

impl WasmInputState {
    /// Release every key and button. Called when the page can no longer
    /// see the matching key-up or mouse-up (focus or pointer lock lost),
    /// so nothing stays held.
    pub fn release_all(&mut self) {
        self.keys = [false; 256];
        self.mouse_buttons = 0;
        self.mouse_dx = 0.0;
        self.mouse_dy = 0.0;
    }

    /// This frame's input: a copy, after which the motion and the legend
    /// request start over (held keys and buttons stay).
    pub fn take_frame(&mut self) -> Self {
        let frame = self.clone();
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
}
