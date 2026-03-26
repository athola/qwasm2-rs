//! GL3/GLES3 renderer backend using the `glow` crate.
//!
//! This is a skeleton implementation: `init` / `shutdown` / `clear_screen` / `end_frame`
//! are functional, and all other methods are placeholders that log and return defaults.

use glow::HasContext;
use q2_render_api::*;
use q2_shared::types::Vec3f;
use tracing::warn;

/// GL3 renderer state.
pub struct Gl3Renderer {
    /// The glow GL context, set by the platform layer before `init`.
    gl: Option<glow::Context>,
    width: i32,
    height: i32,
    initialized: bool,
}

impl Gl3Renderer {
    /// Create an uninitialized GL3 renderer.
    pub fn new() -> Self {
        Self {
            gl: None,
            width: 0,
            height: 0,
            initialized: false,
        }
    }

    /// Provide the glow GL context. Must be called before `init`.
    ///
    /// The context is created by the platform layer (SDL/web-sys) and handed
    /// to the renderer so it can make OpenGL calls.
    pub fn set_gl_context(&mut self, gl: glow::Context) {
        self.gl = Some(gl);
    }
}

impl Default for Gl3Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for Gl3Renderer {
    fn init(&mut self, width: i32, height: i32) -> bool {
        if self.gl.is_none() {
            warn!("GL3Renderer::init called without a GL context");
            return false;
        }
        self.width = width;
        self.height = height;

        if let Some(gl) = &self.gl {
            // SAFETY: glow wraps raw OpenGL calls which are inherently unsafe.
            // We only call well-defined GL state-setup functions here.
            unsafe {
                gl.viewport(0, 0, width, height);
                gl.clear_color(0.1, 0.1, 0.2, 1.0);
                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::LEQUAL);
                gl.enable(glow::CULL_FACE);
                gl.cull_face(glow::BACK);
            }
        }

        self.initialized = true;
        true
    }

    fn shutdown(&mut self) {
        // Future: delete VAOs, VBOs, shaders, textures, etc.
        self.initialized = false;
    }

    fn begin_registration(&mut self, _map_name: &str) {
        // TODO: bump registration sequence, load BSP world model
    }

    fn register_model(&mut self, _name: &str) -> ModelHandle {
        // TODO: load or find cached model
        ModelHandle(0)
    }

    fn register_image(&mut self, _name: &str, _img_type: ImageType) -> ImageHandle {
        // TODO: load or find cached image
        ImageHandle(0)
    }

    fn end_registration(&mut self) {
        // TODO: free unreferenced models/images
    }

    fn render_frame(
        &mut self,
        _fd: &RefDef,
        _entities: &[RenderEntity],
        _dlights: &[DLight],
        _particles: &[Particle],
    ) {
        if let Some(gl) = &self.gl {
            // SAFETY: glow wraps raw OpenGL calls which are inherently unsafe.
            // We clear the colour and depth buffers to produce a visible frame.
            unsafe {
                gl.clear_color(0.1, 0.1, 0.2, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            }
            // TODO: world BSP rendering, entity rendering, particle rendering, etc.
        }
    }

    fn set_sky(&mut self, _name: &str, _rotate: f32, _axis: Vec3f) {
        // TODO: load sky textures (cubemap or sphere)
    }

    fn draw_pic(&mut self, _x: i32, _y: i32, _name: &str) {
        // TODO: 2D image drawing (HUD/menu)
    }

    fn draw_stretch_pic(&mut self, _x: i32, _y: i32, _w: i32, _h: i32, _name: &str) {
        // TODO: stretched 2D image drawing
    }

    fn draw_char(&mut self, _x: i32, _y: i32, _ch: i32) {
        // TODO: console font character drawing
    }

    fn draw_fill(&mut self, _x: i32, _y: i32, _w: i32, _h: i32, _color: i32) {
        // TODO: fill rectangle with palette colour
    }

    fn clear_screen(&mut self) {
        if let Some(gl) = &self.gl {
            // SAFETY: glow wraps raw OpenGL calls which are inherently unsafe.
            unsafe {
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
        }
    }

    fn end_frame(&mut self) {
        // Buffer swap / present is handled by the platform layer (SDL / web-sys).
        // Nothing to do here in the glow backend.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_trait_object() {
        // Verify that Gl3Renderer can be used as a trait object.
        let renderer = Gl3Renderer::new();
        let _boxed: Box<dyn Renderer> = Box::new(renderer);
    }

    #[test]
    fn renderer_default() {
        let r = Gl3Renderer::default();
        assert!(!r.initialized);
        assert_eq!(r.width, 0);
        assert_eq!(r.height, 0);
    }

    #[test]
    fn init_fails_without_context() {
        let mut r = Gl3Renderer::new();
        assert!(!r.init(800, 600));
        assert!(!r.initialized);
    }
}
