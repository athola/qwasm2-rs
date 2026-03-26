//! Platform abstraction for qwasm2-rs.
//! Currently WASM-only; native SDL2 backend planned.

pub mod keymap;

#[cfg(target_arch = "wasm32")]
pub mod wasm;
