//! Platform abstraction for qwasm2-rs.
//! Currently WASM-only; native SDL2 backend planned.
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod keymap;

#[cfg(target_arch = "wasm32")]
pub mod wasm;
