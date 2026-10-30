//! What the browser's input listeners record between frames. Kept out of
//! the wasm32-only module so its rules run in native tests.

/// Shared input state that event listeners write to.
#[derive(Debug, Clone)]
pub struct WasmInputState {
    pub keys: [bool; 256],
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    pub mouse_buttons: u8,
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

impl Default for WasmInputState {
    fn default() -> Self {
        Self {
            keys: [false; 256],
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            mouse_buttons: 0,
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
