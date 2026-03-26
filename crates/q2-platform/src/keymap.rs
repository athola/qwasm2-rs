//! Browser key-code to Quake 2 key index mapping.
//!
//! This module is not gated behind `cfg(target_arch = "wasm32")` because the
//! mapping is pure logic and can be tested on any platform.

/// Maps browser key codes (e.g. `"KeyW"`, `"Space"`) to Q2 key indices.
pub fn key_code_to_q2(code: &str) -> Option<u8> {
    match code {
        "KeyW" => Some(b'w'),
        "KeyA" => Some(b'a'),
        "KeyS" => Some(b's'),
        "KeyD" => Some(b'd'),
        "Space" => Some(b' '),
        "ShiftLeft" | "ShiftRight" => Some(0x80), // K_SHIFT
        "ControlLeft" | "ControlRight" => Some(0x81), // K_CTRL
        "Escape" => Some(0x1B),
        "Enter" => Some(0x0D),
        "Tab" => Some(0x09),
        "Backquote" => Some(b'`'), // console toggle
        "Digit1" => Some(b'1'),
        "Digit2" => Some(b'2'),
        "Digit3" => Some(b'3'),
        "Digit4" => Some(b'4'),
        "Digit5" => Some(b'5'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_code_mapping() {
        assert_eq!(key_code_to_q2("KeyW"), Some(b'w'));
        assert_eq!(key_code_to_q2("KeyA"), Some(b'a'));
        assert_eq!(key_code_to_q2("KeyS"), Some(b's'));
        assert_eq!(key_code_to_q2("KeyD"), Some(b'd'));
        assert_eq!(key_code_to_q2("Space"), Some(b' '));
        assert_eq!(key_code_to_q2("Escape"), Some(0x1B));
        assert_eq!(key_code_to_q2("Enter"), Some(0x0D));
        assert_eq!(key_code_to_q2("ShiftLeft"), Some(0x80));
        assert_eq!(key_code_to_q2("ControlRight"), Some(0x81));
        assert_eq!(key_code_to_q2("Backquote"), Some(b'`'));
        assert_eq!(key_code_to_q2("Digit3"), Some(b'3'));
        assert_eq!(key_code_to_q2("UnknownKey"), None);
    }
}
