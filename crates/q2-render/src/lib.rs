//! Qwasm2-rs: GL3/GLES3 renderer implementation using glow
//!
//! Contains the BSP loader, asset file parsers and the GL3 renderer backend.
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 17
//! - c2rust mechanical: 0
//! - FFI boundary: 17 (glow OpenGL calls in gl3). Invariant relied on by
//!   each: every GL object handle passed to glow was created by the same
//!   `Gl3Renderer::gl` context and has not been deleted.
//! - Performance: 0
//! - Inherent: 0

pub mod bsp;
pub mod files;
pub mod gl3;
