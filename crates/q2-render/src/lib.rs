//! Qwasm2-rs: GL3/GLES3 renderer implementation using glow
//!
//! Contains the BSP loader, asset file parsers and the GL3 renderer backend.
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 39 (blocks, not calls: most blocks wrap a run of
//!   glow calls under one `// SAFETY:` comment)
//! - c2rust mechanical: 0
//! - FFI boundary: 39 (glow OpenGL calls in gl3). Invariant relied on by
//!   each: every GL object handle passed to glow was created by the same
//!   `Gl3Renderer::gl` context and has not been deleted (the context is
//!   fixed when `Gl3Renderer::new` is called).
//! - Performance: 0 (vertex floats are viewed as bytes with `bytemuck`)
//! - Inherent: 0

pub mod bsp;
pub mod files;
pub mod gl3;
