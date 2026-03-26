//! Qwasm2-rs: GL3/GLES3 renderer implementation using glow
//!
//! Contains the BSP loader, GL3 renderer backend, and re-exports the renderer API trait.
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 3
//! - c2rust mechanical: 0
//! - FFI boundary: 3 (glow OpenGL calls in gl3)
//! - Performance: 0
//! - Inherent: 0

pub mod bsp;
pub mod gl3;

pub use q2_render_api::*;
