//! Quake 2 key numbers (C `keyboard.h`): what key binds and the client's
//! input read. Platforms map their own key events onto them.

/// Q2 special key codes. The arrows through `K_END` follow id's original
/// Quake 2 numbering (`K_UPARROW` = 128); `K_PAUSE` is this port's own
/// value. The Qwasm2 reference renumbers them (keyboard.h puts `K_COMMAND`,
/// `K_CAPSLOCK`, `K_POWER` and `K_PAUSE` first, so its `K_UPARROW` is 132).
/// Only this engine's tables use the numbers, which matters only once
/// bindings or configs are loaded by key number.
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
