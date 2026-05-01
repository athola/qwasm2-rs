//! Browser key-code to Quake 2 key index mapping.
//!
//! Maps browser KeyboardEvent.code values to Q2 key indices matching
//! the default.cfg bindings from pak0.pak.

/// Q2 special key codes (matching the C engine's K_* constants)
pub const K_TAB: u8 = 9;
pub const K_ENTER: u8 = 13;
pub const K_ESCAPE: u8 = 27;
pub const K_SPACE: u8 = 32;
pub const K_BACKSPACE: u8 = 127;
pub const K_UPARROW: u8 = 128;
pub const K_DOWNARROW: u8 = 129;
pub const K_LEFTARROW: u8 = 130;
pub const K_RIGHTARROW: u8 = 131;
pub const K_ALT: u8 = 132;
pub const K_CTRL: u8 = 133;
pub const K_SHIFT: u8 = 134;
pub const K_F1: u8 = 135;
pub const K_F2: u8 = 136;
pub const K_F3: u8 = 137;
pub const K_F4: u8 = 138;
pub const K_F5: u8 = 139;
pub const K_F6: u8 = 140;
pub const K_F7: u8 = 141;
pub const K_F8: u8 = 142;
pub const K_F9: u8 = 143;
pub const K_F10: u8 = 144;
pub const K_F11: u8 = 145;
pub const K_F12: u8 = 146;
pub const K_INS: u8 = 147;
pub const K_DEL: u8 = 148;
pub const K_PGDN: u8 = 149;
pub const K_PGUP: u8 = 150;
pub const K_HOME: u8 = 151;
pub const K_END: u8 = 152;
pub const K_PAUSE: u8 = 153;

pub const K_MOUSE1: u8 = 200;
pub const K_MOUSE2: u8 = 201;
pub const K_MOUSE3: u8 = 202;

/// Maps browser KeyboardEvent.code to Q2 key index.
pub fn key_code_to_q2(code: &str) -> Option<u8> {
    match code {
        // Letters (Q2 uses lowercase ASCII)
        "KeyA" => Some(b'a'),
        "KeyB" => Some(b'b'),
        "KeyC" => Some(b'c'),
        "KeyD" => Some(b'd'),
        "KeyE" => Some(b'e'),
        "KeyF" => Some(b'f'),
        "KeyG" => Some(b'g'),
        "KeyH" => Some(b'h'),
        "KeyI" => Some(b'i'),
        "KeyJ" => Some(b'j'),
        "KeyK" => Some(b'k'),
        "KeyL" => Some(b'l'),
        "KeyM" => Some(b'm'),
        "KeyN" => Some(b'n'),
        "KeyO" => Some(b'o'),
        "KeyP" => Some(b'p'),
        "KeyQ" => Some(b'q'),
        "KeyR" => Some(b'r'),
        "KeyS" => Some(b's'),
        "KeyT" => Some(b't'),
        "KeyU" => Some(b'u'),
        "KeyV" => Some(b'v'),
        "KeyW" => Some(b'w'),
        "KeyX" => Some(b'x'),
        "KeyY" => Some(b'y'),
        "KeyZ" => Some(b'z'),

        // Digits
        "Digit0" => Some(b'0'),
        "Digit1" => Some(b'1'),
        "Digit2" => Some(b'2'),
        "Digit3" => Some(b'3'),
        "Digit4" => Some(b'4'),
        "Digit5" => Some(b'5'),
        "Digit6" => Some(b'6'),
        "Digit7" => Some(b'7'),
        "Digit8" => Some(b'8'),
        "Digit9" => Some(b'9'),

        // Special keys
        "Space" => Some(K_SPACE),
        "Enter" => Some(K_ENTER),
        "Tab" => Some(K_TAB),
        "Escape" => Some(K_ESCAPE),
        "Backspace" => Some(K_BACKSPACE),
        "Backquote" => Some(b'`'),
        "Pause" => Some(K_PAUSE),

        // Modifiers
        "ShiftLeft" | "ShiftRight" => Some(K_SHIFT),
        "ControlLeft" | "ControlRight" => Some(K_CTRL),
        "AltLeft" | "AltRight" => Some(K_ALT),

        // Arrow keys
        "ArrowUp" => Some(K_UPARROW),
        "ArrowDown" => Some(K_DOWNARROW),
        "ArrowLeft" => Some(K_LEFTARROW),
        "ArrowRight" => Some(K_RIGHTARROW),

        // Navigation
        "Insert" => Some(K_INS),
        "Delete" => Some(K_DEL),
        "Home" => Some(K_HOME),
        "End" => Some(K_END),
        "PageUp" => Some(K_PGUP),
        "PageDown" => Some(K_PGDN),

        // Function keys
        "F1" => Some(K_F1),
        "F2" => Some(K_F2),
        "F3" => Some(K_F3),
        "F4" => Some(K_F4),
        "F5" => Some(K_F5),
        "F6" => Some(K_F6),
        "F7" => Some(K_F7),
        "F8" => Some(K_F8),
        "F9" => Some(K_F9),
        "F10" => Some(K_F10),
        "F11" => Some(K_F11),
        "F12" => Some(K_F12),

        // Punctuation (matching Q2 ASCII)
        "Comma" => Some(b','),
        "Period" => Some(b'.'),
        "Slash" => Some(b'/'),
        "Backslash" => Some(b'\\'),
        "BracketLeft" => Some(b'['),
        "BracketRight" => Some(b']'),
        "Minus" => Some(b'-'),
        "Equal" => Some(b'='),
        "Semicolon" => Some(b';'),
        "Quote" => Some(b'\''),

        _ => None,
    }
}

/// Maps browser MouseEvent.button to Q2 mouse key.
pub fn mouse_button_to_q2(button: i16) -> Option<u8> {
    match button {
        0 => Some(K_MOUSE1), // left
        1 => Some(K_MOUSE3), // middle
        2 => Some(K_MOUSE2), // right
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_code_mapping() {
        // WASD
        assert_eq!(key_code_to_q2("KeyW"), Some(b'w'));
        assert_eq!(key_code_to_q2("KeyA"), Some(b'a'));
        assert_eq!(key_code_to_q2("KeyS"), Some(b's'));
        assert_eq!(key_code_to_q2("KeyD"), Some(b'd'));
        // Movement keys from default.cfg
        assert_eq!(key_code_to_q2("KeyC"), Some(b'c')); // +movedown
        assert_eq!(key_code_to_q2("Space"), Some(K_SPACE)); // +moveup
        assert_eq!(key_code_to_q2("KeyZ"), Some(b'z')); // +lookdown
                                                        // Modifiers
        assert_eq!(key_code_to_q2("ControlLeft"), Some(K_CTRL)); // +attack
        assert_eq!(key_code_to_q2("ShiftLeft"), Some(K_SHIFT)); // +speed
        assert_eq!(key_code_to_q2("AltLeft"), Some(K_ALT)); // +strafe
                                                            // Arrows
        assert_eq!(key_code_to_q2("ArrowUp"), Some(K_UPARROW)); // +forward
        assert_eq!(key_code_to_q2("ArrowDown"), Some(K_DOWNARROW)); // +back
        assert_eq!(key_code_to_q2("ArrowLeft"), Some(K_LEFTARROW)); // +left
        assert_eq!(key_code_to_q2("ArrowRight"), Some(K_RIGHTARROW)); // +right
                                                                      // Special
        assert_eq!(key_code_to_q2("Escape"), Some(K_ESCAPE));
        assert_eq!(key_code_to_q2("Backquote"), Some(b'`'));
        assert_eq!(key_code_to_q2("Tab"), Some(K_TAB)); // inven
                                                        // Weapons
        assert_eq!(key_code_to_q2("Digit1"), Some(b'1'));
        assert_eq!(key_code_to_q2("Digit9"), Some(b'9'));
        // Punctuation
        assert_eq!(key_code_to_q2("Comma"), Some(b',')); // +moveleft
        assert_eq!(key_code_to_q2("Period"), Some(b'.')); // +moveright
        assert_eq!(key_code_to_q2("BracketLeft"), Some(b'[')); // invprev
        assert_eq!(key_code_to_q2("BracketRight"), Some(b']')); // invnext
                                                                // Unknown
        assert_eq!(key_code_to_q2("UnknownKey"), None);
    }

    #[test]
    fn mouse_button_mapping() {
        assert_eq!(mouse_button_to_q2(0), Some(K_MOUSE1)); // +attack
        assert_eq!(mouse_button_to_q2(1), Some(K_MOUSE3)); // +forward
        assert_eq!(mouse_button_to_q2(2), Some(K_MOUSE2)); // +strafe
        assert_eq!(mouse_button_to_q2(5), None);
    }
}
