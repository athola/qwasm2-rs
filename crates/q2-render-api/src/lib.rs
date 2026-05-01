//! Qwasm2-rs: Renderer trait definitions — shared interface between platform and renderer
//!
//! Defines the `Renderer` trait that any renderer backend (GL3, software, etc.) must implement,
//! plus the types passed across that boundary (handles, view definitions, entities, lights, etc.).
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

use q2_shared::types::*;
use std::fmt;

// ---------------------------------------------------------------------------
// Handles — opaque indices into renderer-internal tables
// ---------------------------------------------------------------------------

/// A handle to a registered model (BSP, MD2, SP2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelHandle(i32);

impl ModelHandle {
    /// The null/default handle (no model).
    pub const NONE: Self = Self(0);

    /// Create a handle from a raw renderer-internal index.
    pub fn new(index: i32) -> Self {
        Self(index)
    }

    /// Get the raw renderer-internal index.
    pub fn raw(self) -> i32 {
        self.0
    }
}

impl Default for ModelHandle {
    fn default() -> Self {
        Self::NONE
    }
}

impl fmt::Display for ModelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Model({})", self.0)
    }
}

/// A handle to a registered image/texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageHandle(i32);

impl ImageHandle {
    /// The null/default handle (no image).
    pub const NONE: Self = Self(0);

    /// Create a handle from a raw renderer-internal index.
    pub fn new(index: i32) -> Self {
        Self(index)
    }

    /// Get the raw renderer-internal index.
    pub fn raw(self) -> i32 {
        self.0
    }
}

impl Default for ImageHandle {
    fn default() -> Self {
        Self::NONE
    }
}

impl fmt::Display for ImageHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({})", self.0)
    }
}

// ---------------------------------------------------------------------------
// Image type — mirrors `imagetype_t` in the C code
// ---------------------------------------------------------------------------

/// Image type for registration — determines search path and processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Skin,
    Sprite,
    Wall,
    Pic, // 2D image (HUD, menu)
    Sky,
}

// ---------------------------------------------------------------------------
// Per-frame rendering inputs
// ---------------------------------------------------------------------------

/// Renderer view definition — describes what to render this frame.
///
/// Corresponds to `refdef_t` in the C code (ref.h).
#[derive(Debug, Clone, Default)]
pub struct RefDef {
    /// Viewport x-offset in virtual screen coordinates.
    pub x: i32,
    /// Viewport y-offset in virtual screen coordinates.
    pub y: i32,
    /// Viewport width in virtual screen coordinates.
    pub width: i32,
    /// Viewport height in virtual screen coordinates.
    pub height: i32,
    /// Horizontal field of view in degrees.
    pub fov_x: f32,
    /// Vertical field of view in degrees.
    pub fov_y: f32,
    /// Camera position in world space.
    pub vieworg: Vec3f,
    /// Camera angles (pitch, yaw, roll).
    pub viewangles: Vec3f,
    /// Full-screen colour blend (damage flash, powerups, etc.). RGBA 0-1.
    pub blend: [f32; 4],
    /// Time value used for auto-animating textures.
    pub time: f32,
    /// Render-definition flags (RDF_UNDERWATER, etc.).
    pub rdflags: i32,
}

/// An entity to render — one model instance in the world.
///
/// Corresponds to `entity_t` in the C code (ref.h).
#[derive(Debug, Clone, Default)]
pub struct RenderEntity {
    pub model: ModelHandle,
    pub frame: i32,
    pub oldframe: i32,
    /// Interpolation fraction: 0.0 = current frame, 1.0 = old frame.
    pub backlerp: f32,
    pub origin: Vec3f,
    pub oldorigin: Vec3f,
    pub angles: Vec3f,
    /// Transparency. Only used when RF_TRANSLUCENT is set in `flags`.
    pub alpha: f32,
    /// Explicit skin image. Default (0) means use inline skin.
    pub skin: ImageHandle,
    /// RF_* flags (RF_TRANSLUCENT, RF_SHELL_RED, etc.).
    pub flags: i32,
    /// Skin number — also used as palette index for RF_BEAM.
    pub skinnum: i32,
}

/// A dynamic light source.
///
/// Corresponds to `dlight_t` in the C code (ref.h).
#[derive(Debug, Clone, Default)]
pub struct DLight {
    pub origin: Vec3f,
    pub color: Vec3f,
    pub intensity: f32,
}

/// A particle effect.
///
/// Corresponds to `particle_t` in the C code (ref.h).
#[derive(Debug, Clone, Default)]
pub struct Particle {
    pub origin: Vec3f,
    /// Palette index.
    pub color: i32,
    pub alpha: f32,
}

// ---------------------------------------------------------------------------
// Renderer trait
// ---------------------------------------------------------------------------

/// The renderer trait — implemented by GL3, software, etc.
///
/// Corresponds to `refexport_t` in the C code (ref.h).
pub trait Renderer: Send {
    /// Initialize the renderer with the given window dimensions.
    /// Returns `Err` with a description on failure.
    fn init(&mut self, width: i32, height: i32) -> Result<(), String>;

    /// Shut down the renderer, releasing all GPU resources.
    fn shutdown(&mut self);

    /// Begin registration of models/images for a new map.
    ///
    /// Called at the start of a level load. All assets registered between
    /// `begin_registration` and `end_registration` are kept; unreferenced
    /// assets are freed in `end_registration`.
    fn begin_registration(&mut self, map_name: &str);

    /// Register a model (BSP, MD2, SP2). Returns a handle.
    fn register_model(&mut self, name: &str) -> ModelHandle;

    /// Register an image/texture. Returns a handle.
    fn register_image(&mut self, name: &str, img_type: ImageType) -> ImageHandle;

    /// End registration — free any models/images not referenced since `begin_registration`.
    fn end_registration(&mut self);

    /// Render a complete frame.
    fn render_frame(
        &mut self,
        fd: &RefDef,
        entities: &[RenderEntity],
        dlights: &[DLight],
        particles: &[Particle],
    );

    /// Set the sky (cubemap or sphere).
    fn set_sky(&mut self, name: &str, rotate: f32, axis: Vec3f);

    /// Draw a 2D image at screen position (for HUD/menu).
    fn draw_pic(&mut self, x: i32, y: i32, name: &str);

    /// Draw a stretched 2D image.
    fn draw_stretch_pic(&mut self, x: i32, y: i32, w: i32, h: i32, name: &str);

    /// Draw a character from the console font.
    fn draw_char(&mut self, x: i32, y: i32, ch: i32);

    /// Fill a rectangle with a palette colour.
    fn draw_fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: i32);

    /// Clear the screen.
    fn clear_screen(&mut self);

    /// Swap buffers / present the rendered frame.
    fn end_frame(&mut self);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_handle_default_is_zero() {
        let h = ModelHandle::default();
        assert_eq!(h.raw(), 0);
    }

    #[test]
    fn image_handle_default_is_zero() {
        let h = ImageHandle::default();
        assert_eq!(h.raw(), 0);
    }

    #[test]
    fn model_handle_none_constant() {
        assert_eq!(ModelHandle::NONE, ModelHandle::default());
        assert_eq!(ModelHandle::NONE.raw(), 0);
    }

    #[test]
    fn image_handle_none_constant() {
        assert_eq!(ImageHandle::NONE, ImageHandle::default());
        assert_eq!(ImageHandle::NONE.raw(), 0);
    }

    #[test]
    fn model_handle_display() {
        let h = ModelHandle::new(42);
        assert_eq!(format!("{h}"), "Model(42)");
    }

    #[test]
    fn image_handle_display() {
        let h = ImageHandle::new(7);
        assert_eq!(format!("{h}"), "Image(7)");
    }

    #[test]
    fn handle_roundtrip() {
        let m = ModelHandle::new(99);
        assert_eq!(m.raw(), 99);
        let i = ImageHandle::new(-1);
        assert_eq!(i.raw(), -1);
    }

    #[test]
    fn refdef_default() {
        let rd = RefDef::default();
        assert_eq!(rd.x, 0);
        assert_eq!(rd.y, 0);
        assert_eq!(rd.width, 0);
        assert_eq!(rd.height, 0);
        assert_eq!(rd.fov_x, 0.0);
        assert_eq!(rd.fov_y, 0.0);
        assert_eq!(rd.vieworg, Vec3f::ZERO);
        assert_eq!(rd.viewangles, Vec3f::ZERO);
        assert_eq!(rd.blend, [0.0; 4]);
        assert_eq!(rd.time, 0.0);
        assert_eq!(rd.rdflags, 0);
    }

    #[test]
    fn render_entity_default() {
        let ent = RenderEntity::default();
        assert_eq!(ent.model, ModelHandle::default());
        assert_eq!(ent.alpha, 0.0);
        assert_eq!(ent.frame, 0);
    }

    #[test]
    fn dlight_default() {
        let dl = DLight::default();
        assert_eq!(dl.intensity, 0.0);
        assert_eq!(dl.origin, Vec3f::ZERO);
    }

    #[test]
    fn particle_default() {
        let p = Particle::default();
        assert_eq!(p.color, 0);
        assert_eq!(p.alpha, 0.0);
    }
}
