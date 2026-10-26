//! Qwasm2-rs: GL3/GLES3 renderer implementation using glow
//!
//! Contains the BSP loader and the GL3 renderer backend.
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 3
//! - c2rust mechanical: 0
//! - FFI boundary: 3 (glow OpenGL calls in gl3)
//! - Performance: 0
//! - Inherent: 0

pub mod bsp;
pub mod gl3;
