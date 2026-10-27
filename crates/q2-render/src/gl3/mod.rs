//! GL3/GLES3 renderer backend using the `glow` crate.
//!
//! Draws the BSP world textured and lightmapped like the C GL3 renderer,
//! with a sky box behind it, then entities (inline BSP models and MD2s) and
//! queued 2D draws. Pure logic lives in submodules so it can be tested
//! without a GL context:
//! - [`texture`]: `.wal` loading, palette conversion, texture upload
//! - [`lightmap`]: surface extents and lightmap atlas packing
//! - [`world`]: face batching of the world and inline models into one buffer
//! - [`sky`]: sky box geometry and image lookup
//! - [`model`]: model handle registry; [`light`]: light under a point
//! - [`entity`]: entity matrices; [`alias`]: MD2 frame interpolation and shading
//! - [`images`] / [`pics`]: pic and skin cache, 2D quads and font cells
//! - [`shader`] / [`draw`] / [`entities`]: the GL programs and draw loops
//!
//! MD2 frames are interpolated on the CPU every draw, as C does, and streamed
//! to one shared buffer; so a registered MD2 owns no GPU buffers, only its
//! skin textures in the image cache. 2D draws are queued in call order and
//! drawn by `end_frame` on top of the 3D frame.
//!
//! Deferred relative to C: texture animation chains (`texinfo.next`), light
//! styles other than the first lightmap, dynamic lights on world and brush
//! surfaces (MD2 shading does include them), back-to-front sorting of translucent surfaces,
//! PVS/frustum culling (the whole world and every entity is drawn every
//! frame), subdivision of warp surfaces (the warp is per-fragment, so it is
//! not needed), `.sp2` sprites, `RF_SHELL_*` shells, the weapon model's own
//! FOV and depth hack, entity shadows, particles and `register_image`.

mod alias;
mod draw;
mod entities;
mod entity;
mod images;
mod light;
mod lightmap;
mod model;
mod pics;
mod shader;
mod sky;
mod texture;
mod world;

use glow::HasContext;
use q2_render_api::*;
use q2_shared::error::{Q2Error, Q2Result};
use q2_shared::types::Vec3f;

use crate::bsp::BspData;
use crate::files::palette::Palette;
use alias::AliasModel;
use draw::{draw_batches, draw_ui, BatchDraw, BatchFilter, SkyGpu, UiTarget, WorldGpu};
use entities::{create_extra, draw_entities, free_extra, EntityScene, ExtraGpu};
use images::ImageStore;
use light::LightTree;
use model::{Model, ModelRegistry};
use pics::{Quad, QuadImage};
use shader::WorldUniforms;
use sky::{DEFAULT_SKY, SKY_SUFFIXES};
use texture::{load_palette, load_world_images, notexture, FileLoader, Image};

/// Parameters from the last `set_sky` call.
struct SkySetting {
    name: String,
    rotate: f32,
    axis: [f32; 3],
}

/// GL3 renderer state.
pub struct Gl3Renderer {
    gl: Option<glow::Context>,
    width: i32,
    height: i32,
    initialized: bool,
    /// Shader program for world and sky.
    program: Option<glow::Program>,
    uniforms: Option<WorldUniforms>,
    world: Option<WorldGpu>,
    sky: Option<SkyGpu>,
    sky_setting: SkySetting,
    /// C `ri.FS_LoadFile`.
    file_loader: Option<FileLoader>,
    /// From `pics/colormap.pcx`; needed to convert `.wal` pixels.
    palette: Option<Palette>,
    /// MD2 and 2D programs, stream buffers and fallback textures.
    extra: Option<ExtraGpu>,
    /// 2D pics and model skins.
    images: ImageStore,
    /// Registered models; handles are valid until the next registration.
    models: ModelRegistry<AliasModel>,
    /// Number of models in the loaded BSP (`*n` must be below it).
    model_count: Option<usize>,
    /// Lightmap data for lighting entities, from the loaded BSP.
    light: Option<LightTree>,
    /// 2D draws since the last `end_frame`.
    ui_queue: Vec<Quad>,
    /// 2D coordinate space in pixels: the init size, then the last RefDef size.
    canvas: (i32, i32),
}

impl Gl3Renderer {
    pub fn new() -> Self {
        Self {
            gl: None,
            width: 0,
            height: 0,
            initialized: false,
            program: None,
            uniforms: None,
            world: None,
            sky: None,
            sky_setting: SkySetting {
                name: DEFAULT_SKY.to_string(),
                rotate: 0.0,
                axis: [0.0, 0.0, 1.0],
            },
            file_loader: None,
            palette: None,
            extra: None,
            images: ImageStore::default(),
            models: ModelRegistry::default(),
            model_count: None,
            light: None,
            ui_queue: Vec::new(),
            canvas: (0, 0),
        }
    }

    /// Provide the glow GL context. Must be called before `init`.
    pub fn set_gl_context(&mut self, gl: glow::Context) {
        self.gl = Some(gl);
    }

    /// Give the renderer access to game files (textures, sky boxes, palette).
    /// Call before [`Gl3Renderer::load_bsp`].
    pub fn set_file_loader(&mut self, loader: FileLoader) {
        self.palette = load_palette(&loader);
        if self.palette.is_none() {
            tracing::error!("GL3: no palette, world textures will show as notexture");
        }
        self.file_loader = Some(loader);
    }

    /// Load BSP world geometry, textures and lightmaps onto the GPU.
    pub fn load_bsp(&mut self, bsp: &BspData) {
        // Entity lighting and `*n` validation need no GL context. A new map
        // changes what `*n` means, so inline models are forgotten; MD2s stay.
        self.light = Some(LightTree::from_bsp(bsp));
        self.model_count = Some(bsp.models.len());
        self.models.clear_brush();
        let Some(gl) = &self.gl else {
            return;
        };
        let no_files: FileLoader = Box::new(|_| None);
        let loader = self.file_loader.as_ref().unwrap_or(&no_files);

        let images = load_world_images(&bsp.texinfo, loader, self.palette.as_ref());
        let mesh = world::build_world_mesh(bsp, &images);
        tracing::info!(
            "GL3: uploading {} triangles, {} textures, {} lightmap pages",
            mesh.vertices.len() / (sky::VERTEX_FLOATS * 3),
            images.images.len(),
            mesh.lightmaps.pages.len()
        );
        match draw::upload_world(gl, &images, &mesh) {
            Ok(uploaded) => {
                // A map change replaces the world; free the previous map's objects.
                if let Some(old) = self.world.replace(uploaded) {
                    draw::free_world(gl, old);
                }
            }
            Err(error) => tracing::error!("GL3: failed to upload world: {error}"),
        }
        if self.sky.is_none() {
            self.reload_sky();
        }
    }

    /// Load the six faces of the current sky; missing faces use notexture.
    fn reload_sky(&mut self) {
        let Some(gl) = &self.gl else {
            return;
        };
        let no_files: FileLoader = Box::new(|_| None);
        let loader = self.file_loader.as_ref().unwrap_or(&no_files);
        let name = self.sky_setting.name.as_str();
        let faces: [Image; 6] = SKY_SUFFIXES.map(|suffix| {
            sky::load_sky_image(loader, name, suffix).unwrap_or_else(|| {
                tracing::warn!("GL3: can't load sky {name}{suffix}");
                notexture()
            })
        });
        match draw::upload_sky(gl, &faces) {
            Ok(uploaded) => {
                if let Some(old) = self.sky.replace(uploaded) {
                    draw::free_sky(gl, old);
                }
            }
            Err(error) => tracing::error!("GL3: failed to upload sky: {error}"),
        }
    }

    /// Sky box transform: view without translation, then the sky's own spin.
    fn sky_view_proj(&self, fd: &RefDef) -> [f32; 16] {
        let proj = perspective_matrix(fd);
        let view = view_matrix_at(Vec3f::ZERO, fd.viewangles);
        let spin = sky::rotation_matrix(fd.time * self.sky_setting.rotate, self.sky_setting.axis);
        mat4_mul(&mat4_mul(&proj, &view), &spin)
    }
}

impl Default for Gl3Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Gl3Renderer {
    /// Release GPU objects even if the caller never called `shutdown`.
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Renderer for Gl3Renderer {
    fn init(&mut self, width: i32, height: i32) -> Q2Result<()> {
        let gl = match &self.gl {
            Some(gl) => gl,
            None => {
                return Err(Q2Error::Drop(
                    "GL3Renderer::init called without a GL context".into(),
                ));
            }
        };

        self.width = width;
        self.height = height;

        let (prog, uniforms) = shader::compile_world_program(gl)
            .map_err(|e| Q2Error::Drop(format!("Failed to compile world shaders: {e}")))?;

        // SAFETY: `prog` and any previous program were created by this same
        // `gl` context in `compile_world_program`, and `old` is no longer
        // referenced once replaced.
        unsafe {
            // A repeated init replaces the program; free the old one.
            if let Some(old) = self.program.replace(prog) {
                gl.delete_program(old);
            }
        }
        self.uniforms = Some(uniforms);

        let extra = create_extra(gl)
            .map_err(|e| Q2Error::Drop(format!("Failed to set up entity and 2D drawing: {e}")))?;
        if let Some(old) = self.extra.replace(extra) {
            free_extra(gl, old);
        }
        // Textures made before this context (init may repeat) are gone.
        self.images.free(gl);
        self.models.clear();
        self.canvas = (width, height);

        // SAFETY: State-only calls on the live context `gl`; no object
        // handles are passed.
        unsafe {
            gl.viewport(0, 0, width, height);
            gl.clear_color(0.05, 0.05, 0.1, 1.0);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
        }

        self.initialized = true;
        tracing::info!("GL3 renderer initialized ({}x{})", width, height);
        Ok(())
    }

    fn shutdown(&mut self) {
        if let Some(gl) = &self.gl {
            if let Some(world) = self.world.take() {
                draw::free_world(gl, world);
            }
            if let Some(sky) = self.sky.take() {
                draw::free_sky(gl, sky);
            }
            if let Some(extra) = self.extra.take() {
                free_extra(gl, extra);
            }
            self.images.free(gl);
            self.models.clear();
            self.ui_queue.clear();
            // SAFETY: The program was created on this same `gl` by init and is
            // taken out of `self` before deletion, so no deleted handle is
            // used again.
            unsafe {
                if let Some(prog) = self.program.take() {
                    gl.delete_program(prog);
                }
            }
        }
        self.uniforms = None;
        self.initialized = false;
    }

    /// Frees the previous map's models; handles from before this call are
    /// invalid. Images (pics, skins) stay cached across maps.
    fn begin_registration(&mut self, _map_name: &str) {
        self.models.clear();
    }

    /// `*n` is inline submodel `n` of the loaded BSP; `*.md2` is loaded
    /// through the file loader with its skins. Anything else, or a failure,
    /// returns `ModelHandle::NONE` (logged once per name).
    fn register_model(&mut self, name: &str) -> ModelHandle {
        if name.starts_with('*') {
            return self.models.register_inline(name, self.model_count);
        }
        let (gl, palette) = (self.gl.as_ref(), self.palette.as_ref());
        let images = &mut self.images;
        let loader = self.file_loader.as_ref();
        self.models.register(name, || {
            let is_md2 = name.len() > 4 && name.to_ascii_lowercase().ends_with(".md2");
            if !is_md2 {
                return None;
            }
            entities::load_alias_model(gl, loader?, palette, images, name).map(Model::Alias)
        })
    }
    fn register_image(&mut self, _name: &str, _img_type: ImageType) -> ImageHandle {
        ImageHandle::NONE
    }
    fn end_registration(&mut self) {}

    /// Draw order: sky, opaque world, opaque entities, translucent world,
    /// translucent entities. 2D draws are queued until `end_frame`.
    fn render_frame(
        &mut self,
        fd: &RefDef,
        entities: &[RenderEntity],
        dlights: &[DLight],
        _particles: &[Particle],
    ) {
        let gl = match &self.gl {
            Some(gl) => gl,
            None => return,
        };
        if fd.width > 0 && fd.height > 0 {
            self.canvas = (fd.width, fd.height);
        }

        // SAFETY: State-only calls on the live context `gl`; no object
        // handles are passed.
        unsafe {
            gl.viewport(0, 0, fd.width, fd.height);
            gl.clear_color(0.05, 0.05, 0.1, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }

        let (Some(program), Some(uniforms)) = (&self.program, &self.uniforms) else {
            tracing::trace!("GL3: skipping draw, renderer not initialised");
            return;
        };
        // SAFETY: `program` was created on this same `gl` by init and is
        // still owned by `self`.
        unsafe { gl.use_program(Some(*program)) };

        if let Some(sky) = &self.sky {
            draw::draw_sky(gl, uniforms, sky, &self.sky_view_proj(fd));
        }
        let view_proj = build_view_projection(fd);
        let world_batches = |filter| {
            if let Some(world) = &self.world {
                let draw = BatchDraw {
                    view_proj: &view_proj,
                    time: fd.time,
                    filter,
                };
                if let Some(batches) = world.model_batches(0) {
                    draw_batches(gl, uniforms, world, batches, &draw);
                }
            }
        };
        let scene = self.extra.as_ref().map(|extra| EntityScene {
            gl,
            world: self.world.as_ref(),
            world_program: *program,
            world_uniforms: uniforms,
            extra,
            models: &self.models,
            images: &self.images,
            light: self.light.as_ref(),
            refdef: fd,
            dlights,
            view_proj,
        });

        world_batches(BatchFilter::Opaque);
        if let Some(scene) = &scene {
            draw_entities(scene, entities, false);
        }
        world_batches(BatchFilter::Translucent);
        if let Some(scene) = &scene {
            draw_entities(scene, entities, true);
        }

        // SAFETY: Unbinding the program is a state-only call.
        unsafe { gl.use_program(None) };
    }

    fn set_sky(&mut self, name: &str, rotate: f32, axis: Vec3f) {
        self.sky_setting = SkySetting {
            name: if name.is_empty() {
                DEFAULT_SKY.to_string()
            } else {
                name.to_string()
            },
            rotate,
            axis: [axis.x, axis.y, axis.z],
        };
        self.reload_sky();
    }
    // 2D draws are queued in call order and drawn by `end_frame`, on top of
    // the 3D frame in canvas pixels (origin top-left), one batch per run of
    // quads that share a texture.
    fn draw_pic(&mut self, x: i32, y: i32, name: &str) {
        if let Some((w, h)) = self.pic_size(name) {
            self.draw_stretch_pic(x, y, w, h, name);
        }
    }

    fn draw_get_pic_size(&mut self, name: &str) -> Option<(i32, i32)> {
        self.pic_size(name)
    }

    fn draw_stretch_pic(&mut self, x: i32, y: i32, w: i32, h: i32, name: &str) {
        if let Some(index) = self.find_pic(name) {
            self.ui_queue.push(pics::quad(
                QuadImage::Pic(index),
                (x, y, w, h),
                [0.0, 0.0, 1.0, 1.0],
                [1.0; 4],
            ));
        }
    }

    fn draw_char(&mut self, x: i32, y: i32, ch: i32) {
        // C skips spaces and characters entirely above the screen.
        let Some(uv) = pics::char_uv(ch).filter(|_| y > -pics::CHAR_SIZE) else {
            return;
        };
        if let Some(index) = self.find_pic(pics::CONCHARS) {
            let rect = (x, y, pics::CHAR_SIZE, pics::CHAR_SIZE);
            self.ui_queue
                .push(pics::quad(QuadImage::Pic(index), rect, uv, [1.0; 4]));
        }
    }

    fn draw_fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: i32) {
        let palette = self.palette.as_ref();
        let rgb = |index: u8| palette.map_or([index; 3], |p| p.0[usize::from(index)]);
        match pics::fill_quad((x, y, w, h), color, rgb) {
            Some(quad) => self.ui_queue.push(quad),
            None => tracing::warn!("GL3: draw_fill with bad palette index {color}"),
        }
    }

    fn clear_screen(&mut self) {
        if let Some(gl) = &self.gl {
            // SAFETY: State-only calls on the live context `gl`; no object
            // handles are passed.
            unsafe {
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
        }
    }

    fn end_frame(&mut self) {
        let queue = std::mem::take(&mut self.ui_queue);
        let (Some(gl), Some(extra)) = (&self.gl, &self.extra) else {
            return;
        };
        if queue.is_empty() {
            return;
        }
        let target = UiTarget {
            program: extra.ui_program,
            uniforms: &extra.ui_uniforms,
            stream: &extra.ui_stream,
            ortho: &pics::ortho_matrix(self.canvas.0, self.canvas.1),
            canvas: self.canvas,
        };
        let images = &self.images;
        draw_ui(gl, &target, &queue, |image| match image {
            QuadImage::White => Some(extra.white),
            QuadImage::Pic(index) => images.entries.get(index).and_then(|entry| entry.texture),
        });
    }
}

impl Gl3Renderer {
    /// Image-store index of the pic `name` resolves to (C `R_FindPic`).
    fn find_pic(&mut self, name: &str) -> Option<usize> {
        let loader = self.file_loader.as_ref()?;
        let paths = pics::pic_paths(name);
        self.images
            .find_first(self.gl.as_ref(), loader, self.palette.as_ref(), &paths)
    }

    fn pic_size(&mut self, name: &str) -> Option<(i32, i32)> {
        let index = self.find_pic(name)?;
        let entry = self.images.entries.get(index)?;
        Some((
            i32::try_from(entry.width).ok()?,
            i32::try_from(entry.height).ok()?,
        ))
    }
}

/// Build a combined view-projection matrix from RefDef.
fn build_view_projection(fd: &RefDef) -> [f32; 16] {
    let proj = perspective_matrix(fd);
    let view = view_matrix(fd);
    mat4_mul(&proj, &view)
}

/// Standard perspective projection matrix.
fn perspective_matrix(fd: &RefDef) -> [f32; 16] {
    let aspect = fd.width as f32 / fd.height.max(1) as f32;
    let fov_y_rad = (fd.fov_y.max(1.0)).to_radians();
    let near = 4.0;
    let far = 4096.0;

    let f = 1.0 / (fov_y_rad / 2.0).tan();
    let nf = 1.0 / (near - far);

    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        (far + near) * nf,
        -1.0,
        0.0,
        0.0,
        2.0 * far * near * nf,
        0.0,
    ]
}

/// View matrix converting Q2 world coords to OpenGL eye coords.
/// Q2: +X forward, +Y left, +Z up. OpenGL: -Z forward, +X right, +Y up.
///
/// Port of the view setup in gl3_main.c SetupGL (:1467-1487):
/// `Z_UP_TO_GL * rotAroundAxisXYZ(-roll, -pitch, -yaw) *
/// translate(-vieworg)`. `viewangles` use the Q2 convention (pitch > 0
/// looks down).
fn view_matrix(fd: &RefDef) -> [f32; 16] {
    view_matrix_at(fd.vieworg, fd.viewangles)
}

/// [`view_matrix`] for an explicit eye position and angles; the sky box uses
/// a zero position so it stays centred on the eye.
fn view_matrix_at(origin: Vec3f, angles: Vec3f) -> [f32; 16] {
    // Column-major: first put the Z axis going up.
    #[rustfmt::skip]
    const Z_UP_TO_GL: [f32; 16] = [
         0.0, 0.0, -1.0, 0.0,
        -1.0, 0.0,  0.0, 0.0,
         0.0, 1.0,  0.0, 0.0,
         0.0, 0.0,  0.0, 1.0,
    ];
    let rot = rot_around_axis_xyz(-angles.z, -angles.x, -angles.y);
    let o = origin;
    #[rustfmt::skip]
    let translate = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -o.x, -o.y, -o.z, 1.0,
    ];
    mat4_mul(&mat4_mul(&Z_UP_TO_GL, &rot), &translate)
}

/// Column-major rotation around X, then Y, then Z (degrees); C
/// `rotAroundAxisXYZ` in gl3_main.c:1304-1325.
fn rot_around_axis_xyz(around_x: f32, around_y: f32, around_z: f32) -> [f32; 16] {
    let (sin_a, cos_a) = around_x.to_radians().sin_cos();
    let (sin_b, cos_b) = around_y.to_radians().sin_cos();
    let (sin_g, cos_g) = around_z.to_radians().sin_cos();
    #[rustfmt::skip]
    let m = [
        cos_b * cos_g,  sin_a * sin_b * cos_g + cos_a * sin_g, -cos_a * sin_b * cos_g + sin_a * sin_g, 0.0,
       -cos_b * sin_g, -sin_a * sin_b * sin_g + cos_a * cos_g,  cos_a * sin_b * sin_g + sin_a * cos_g, 0.0,
        sin_b,         -sin_a * cos_b,                          cos_a * cos_b,                         0.0,
        0.0,            0.0,                                    0.0,                                   1.0,
    ];
    m
}

fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut r = [0.0f32; 16];
    for i in 0..4 {
        for j in 0..4 {
            r[j * 4 + i] = a[i] * b[j * 4]
                + a[4 + i] * b[j * 4 + 1]
                + a[8 + i] * b[j * 4 + 2]
                + a[12 + i] * b[j * 4 + 3];
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_new_is_uninitialized() {
        let r = Gl3Renderer::new();
        assert!(!r.initialized);
        assert_eq!(r.width, 0);
        assert_eq!(r.height, 0);
        // Also proves object safety: Gl3Renderer implements Renderer
        let _boxed: Box<dyn Renderer> = Box::new(r);
    }

    #[test]
    fn init_fails_without_context() {
        let mut r = Gl3Renderer::new();
        assert!(r.init(800, 600).is_err());
    }

    #[test]
    fn init_without_context_is_recoverable_q2_drop() {
        // Renderer errors share Q2Error with the engine so the frame loop
        // and q2err_to_js handle them like any other ERR_DROP.
        let mut r = Gl3Renderer::new();
        let err = r.init(800, 600).unwrap_err();
        assert!(matches!(err, q2_shared::error::Q2Error::Drop(_)));
        assert!(err.is_recoverable());
    }

    #[test]
    fn mat4_identity_mul() {
        let id = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let r = mat4_mul(&id, &id);
        assert_eq!(r, id);
    }

    #[test]
    fn perspective_matrix_basic_properties() {
        let fd = RefDef {
            width: 800,
            height: 600,
            fov_y: 73.74,
            ..Default::default()
        };
        let p = perspective_matrix(&fd);
        // Column-major: p[0] is m00 (x-scale), p[5] is m11 (y-scale)
        assert!(p[0] > 0.0, "x-scale must be positive");
        assert!(p[5] > 0.0, "y-scale must be positive");
        // p[15] should be 0 for a perspective projection (w-component)
        assert_eq!(p[15], 0.0);
        // p[11] should be -1 for perspective divide
        assert_eq!(p[11], -1.0);
    }

    #[test]
    fn view_matrix_identity_at_origin() {
        let fd = RefDef {
            vieworg: Vec3f::ZERO,
            viewangles: Vec3f::new(0.0, 0.0, 0.0),
            ..Default::default()
        };
        let v = view_matrix(&fd);
        // With zero position, the translation column (indices 12,13,14) should be ~0
        assert!(v[12].abs() < 1e-6, "tx={}", v[12]);
        assert!(v[13].abs() < 1e-6, "ty={}", v[13]);
        assert!(v[14].abs() < 1e-6, "tz={}", v[14]);
        // v[15] should be 1.0
        assert!((v[15] - 1.0).abs() < 1e-6);
    }

    /// Multiply a column-major 4x4 matrix by a vec4, returning vec4.
    fn mat4_mul_vec4(m: &[f32; 16], v: [f32; 4]) -> [f32; 4] {
        [
            m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12] * v[3],
            m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13] * v[3],
            m[2] * v[0] + m[6] * v[1] + m[10] * v[2] + m[14] * v[3],
            m[3] * v[0] + m[7] * v[1] + m[11] * v[2] + m[15] * v[3],
        ]
    }

    #[test]
    fn view_matrix_yaw90_maps_world_to_eye_correctly() {
        // At yaw=90, pitch=0, player at origin. A point at (0, 100, 0) — directly ahead in Q2 when facing +Y (yaw=90)
        // should map to eye-space (0, 0, -100): forward in OpenGL is -Z.
        let fd = RefDef {
            vieworg: Vec3f::ZERO,
            viewangles: Vec3f::new(0.0, 90.0, 0.0),
            ..Default::default()
        };
        let v = view_matrix(&fd);
        let eps = 1e-4;

        // World point directly "ahead" when facing yaw=90 (i.e. +Y direction)
        let ahead = mat4_mul_vec4(&v, [0.0, 100.0, 0.0, 1.0]);
        assert!(
            ahead[0].abs() < eps,
            "ahead eye-x should be 0, got {}",
            ahead[0]
        );
        assert!(
            ahead[1].abs() < eps,
            "ahead eye-y should be 0, got {}",
            ahead[1]
        );
        assert!(
            (ahead[2] - (-100.0)).abs() < eps,
            "ahead eye-z should be -100, got {}",
            ahead[2]
        );

        // World point at (100, 0, 0) — to the right when facing +Y
        // Should map to eye-space (100, 0, 0): +X is right in OpenGL.
        let right = mat4_mul_vec4(&v, [100.0, 0.0, 0.0, 1.0]);
        assert!(
            (right[0] - 100.0).abs() < eps,
            "right eye-x should be 100, got {}",
            right[0]
        );
        assert!(
            right[1].abs() < eps,
            "right eye-y should be 0, got {}",
            right[1]
        );
        assert!(
            right[2].abs() < eps,
            "right eye-z should be 0, got {}",
            right[2]
        );
    }

    fn assert_vec3_near(actual: [f32; 4], expected: [f32; 3], what: &str) {
        for axis in 0..3 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 1e-3,
                "{what}: got {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn view_matrix_positive_pitch_looks_down_like_q2_anglevectors() {
        // Q2 AngleVectors: forward = (cp*cy, cp*sy, -sp), so pitch > 0 looks
        // down. That direction must land straight ahead in eye space (-Z).
        let (pitch, yaw) = (10.0f32, 45.0f32);
        let fd = RefDef {
            viewangles: Vec3f::new(pitch, yaw, 0.0),
            ..Default::default()
        };
        let (sp, cp) = pitch.to_radians().sin_cos();
        let (sy, cy) = yaw.to_radians().sin_cos();
        let eye = mat4_mul_vec4(&view_matrix(&fd), [cp * cy, cp * sy, -sp, 0.0]);
        assert_vec3_near(eye, [0.0, 0.0, -1.0], "forward");
    }

    #[test]
    fn view_matrix_roll90_maps_world_up_to_eye_left() {
        // gl3_main.c rotates by -roll: at roll 90 world up (+Z) appears on
        // the eye's left (-X) and forward (+X) stays straight ahead.
        let fd = RefDef {
            viewangles: Vec3f::new(0.0, 0.0, 90.0),
            ..Default::default()
        };
        let v = view_matrix(&fd);
        assert_vec3_near(
            mat4_mul_vec4(&v, [0.0, 0.0, 100.0, 1.0]),
            [-100.0, 0.0, 0.0],
            "world up",
        );
        assert_vec3_near(
            mat4_mul_vec4(&v, [100.0, 0.0, 0.0, 1.0]),
            [0.0, 0.0, -100.0],
            "world forward",
        );
    }

    #[test]
    fn build_view_projection_produces_valid_matrix() {
        let fd = RefDef {
            width: 800,
            height: 600,
            fov_x: 90.0,
            fov_y: 73.74,
            vieworg: Vec3f::new(100.0, 200.0, 50.0),
            viewangles: Vec3f::new(10.0, 45.0, 0.0),
            ..Default::default()
        };
        let vp = build_view_projection(&fd);
        // Should not contain NaN or infinity
        for (i, &val) in vp.iter().enumerate() {
            assert!(val.is_finite(), "vp[{i}]={val} is not finite");
        }
    }

    fn empty_bsp() -> BspData {
        BspData {
            vertices: vec![],
            edges: vec![],
            surface_edges: vec![],
            faces: vec![],
            planes: vec![],
            texinfo: vec![],
            models: vec![],
            visibility: vec![],
            nodes: vec![],
            lightmap_data: vec![],
            leafs: vec![],
            leaf_faces: vec![],
        }
    }

    #[test]
    fn load_bsp_no_gl_returns_early() {
        let mut r = Gl3Renderer::new();
        // No GL context set: load_bsp should return without panicking
        r.load_bsp(&empty_bsp());
        assert!(r.world.is_none());
        assert!(r.sky.is_none());
    }

    #[test]
    fn set_gl_context_stores_context_presence() {
        let mut r = Gl3Renderer::new();
        assert!(r.gl.is_none());
        // We can't create a real glow::Context in unit tests without a
        // GL backend, but we verify the initial state and that init fails
        // without one — which implicitly validates set_gl_context's role.
        assert!(r.init(800, 600).is_err());
    }

    #[test]
    fn sky_defaults_to_unit1_until_set() {
        let mut r = Gl3Renderer::new();
        assert_eq!(r.sky_setting.name, "unit1_");
        r.set_sky("city_", 5.0, Vec3f::new(0.0, 1.0, 0.0));
        assert_eq!(r.sky_setting.name, "city_");
        assert_eq!(r.sky_setting.rotate, 5.0);
        assert_eq!(r.sky_setting.axis, [0.0, 1.0, 0.0]);
        // An empty name (map without a sky key) restores the default.
        r.set_sky("", 0.0, Vec3f::ZERO);
        assert_eq!(r.sky_setting.name, "unit1_");
    }

    #[test]
    fn file_loader_reads_palette_from_colormap() {
        let mut pcx = vec![0u8; 200];
        pcx.extend((0..=255u8).flat_map(|i| [i, 0, 0]));
        let mut r = Gl3Renderer::new();
        assert!(r.palette.is_none());
        r.set_file_loader(Box::new(move |path| {
            (path == "pics/colormap.pcx").then(|| pcx.clone())
        }));
        assert_eq!(r.palette.as_ref().unwrap().0[7], [7, 0, 0]);
        assert!(r.file_loader.is_some());
    }

    #[test]
    fn missing_colormap_leaves_palette_unset_but_keeps_loader() {
        let mut r = Gl3Renderer::new();
        r.set_file_loader(Box::new(|_| None));
        assert!(r.palette.is_none());
        assert!(r.file_loader.is_some());
    }

    #[test]
    fn sky_view_proj_ignores_eye_position() {
        let r = Gl3Renderer::new();
        let at = |x| RefDef {
            width: 640,
            height: 480,
            fov_y: 90.0,
            vieworg: Vec3f::new(x, 0.0, 0.0),
            viewangles: Vec3f::new(0.0, 30.0, 0.0),
            ..Default::default()
        };
        assert_eq!(r.sky_view_proj(&at(0.0)), r.sky_view_proj(&at(500.0)));
    }

    /// Skips (returns `None`) when `pak0.pak` is not available.
    fn stock_loader() -> Option<FileLoader> {
        use q2_common::filesystem::{FileSystem, Pack};
        let root = std::env::var_os("Q2_GAMEDATA_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../gamedata")
            });
        let pak_path = root.join("baseq2/pak0.pak");
        if !pak_path.is_file() {
            return None;
        }
        let mut filesystem = FileSystem::new("baseq2");
        filesystem.add_pack(Pack::load(&pak_path).expect("pak0.pak parses"));
        Some(Box::new(move |path| filesystem.load_file(path).ok()))
    }

    #[test]
    fn stock_map_builds_textured_lit_mesh_and_sky() {
        let Some(loader) = stock_loader() else {
            return;
        };
        let palette = load_palette(&loader).expect("colormap loads");
        let bsp = BspData::load(&loader("maps/demo1.bsp").expect("demo1 in pak0")).unwrap();

        let images = load_world_images(&bsp.texinfo, &loader, Some(&palette));
        let missing = images.texinfo_image.iter().filter(|&&i| i == 0).count();
        assert_eq!(missing, 0, "every demo1 texture resolves to a real .wal");

        let mesh = world::build_world_mesh(&bsp, &images);
        assert!(mesh.vertices.len() > 1000 * sky::VERTEX_FLOATS);
        assert!(!mesh.lightmaps.pages.is_empty());
        // Submodel batches follow the world's in the same buffer.
        assert_eq!(mesh.submodels.len(), bsp.models.len() - 1);
        let end = mesh
            .submodels
            .iter()
            .flatten()
            .chain(&mesh.batches)
            .map(|b| b.first_vertex + b.vertex_count)
            .max();
        assert_eq!(
            end,
            i32::try_from(mesh.vertices.len() / sky::VERTEX_FLOATS).ok()
        );
        assert!(
            mesh.submodels.iter().any(|batches| !batches.is_empty()),
            "demo1 has doors or platforms with faces"
        );
        // Lightmap UVs of lit faces stay inside the atlas page.
        let lit_ranges = mesh
            .batches
            .iter()
            .filter(|b| b.shading == world::Shading::Lit);
        for batch in lit_ranges {
            let first = batch.first_vertex as usize * sky::VERTEX_FLOATS;
            let count = batch.vertex_count as usize * sky::VERTEX_FLOATS;
            for vertex in mesh.vertices[first..first + count].chunks(sky::VERTEX_FLOATS) {
                assert!((0.0..=1.0).contains(&vertex[5]) && (0.0..=1.0).contains(&vertex[6]));
            }
        }

        for suffix in SKY_SUFFIXES {
            let face = sky::load_sky_image(&loader, DEFAULT_SKY, suffix).expect("unit1 sky face");
            assert!(face.width > 0 && face.rgba.len() == face.width * face.height * 4);
        }
    }

    /// One triangle, one 2x2 skin named `skin`, one frame.
    fn synthetic_md2(skin: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        let header: [i32; 17] = [
            0x3250_4449, // "IDP2"
            8,
            2,  // skin width
            2,  // skin height
            52, // frame size: 40 + 3 vertices * 4
            1,  // skins
            3,  // xyz
            3,  // st
            1,  // triangles
            0,  // glcmds
            1,  // frames
            68, // ofs skins
            132,
            144,
            156,
            0,
            208,
        ];
        for value in header {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut name = [0u8; 64];
        name[..skin.len()].copy_from_slice(skin.as_bytes());
        bytes.extend_from_slice(&name);
        for st in [[0i16, 0], [2, 0], [0, 2]] {
            bytes.extend_from_slice(&st[0].to_le_bytes());
            bytes.extend_from_slice(&st[1].to_le_bytes());
        }
        for index in [0u16, 1, 2, 0, 1, 2] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        for value in [1.0f32, 1.0, 1.0, 0.0, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0u8; 16]);
        for vertex in [[0u8, 0, 0, 0], [8, 0, 0, 0], [0, 8, 0, 0]] {
            bytes.extend_from_slice(&vertex);
        }
        assert_eq!(bytes.len(), 208);
        bytes
    }

    fn model_files() -> FileLoader {
        let pcx = images::tiny_pcx();
        Box::new(move |path| match path {
            "models/t/tris.md2" => Some(synthetic_md2("models/t/skin.pcx")),
            "models/t/skin.pcx" | "pics/num_0.pcx" | "pics/conchars.pcx" => Some(pcx.clone()),
            _ => None,
        })
    }

    fn bsp_with_models(count: usize) -> BspData {
        let mut bsp = empty_bsp();
        bsp.models = (0..count)
            .map(|_| crate::bsp::BspModel {
                mins: Vec3f::ZERO,
                maxs: Vec3f::ZERO,
                origin: Vec3f::ZERO,
                head_node: 0,
                first_face: 0,
                num_faces: 0,
            })
            .collect();
        bsp
    }

    #[test]
    fn register_model_loads_an_md2_with_its_skin_once() {
        let mut r = Gl3Renderer::new();
        r.set_file_loader(model_files());
        let handle = r.register_model("models/t/tris.md2");
        assert_ne!(handle, ModelHandle::NONE);
        assert_eq!(r.register_model("models/t/tris.md2"), handle);
        let Some(Model::Alias(model)) = r.models.get(handle) else {
            panic!("expected an MD2 model");
        };
        assert_eq!(model.corners.len(), 3);
        // Skin 0 resolved through the image store to the 2x2 PCX.
        let skin = model.skins[0].expect("skin loaded");
        assert_eq!(r.images.entries[skin].width, 2);
    }

    #[test]
    fn register_model_rejects_missing_and_unsupported_names() {
        let mut r = Gl3Renderer::new();
        assert_eq!(r.register_model("models/t/tris.md2"), ModelHandle::NONE);
        r.set_file_loader(model_files());
        assert_eq!(r.register_model("models/t/gone.md2"), ModelHandle::NONE);
        assert_eq!(r.register_model("sprites/s_explod.sp2"), ModelHandle::NONE);
        assert_eq!(r.register_model(""), ModelHandle::NONE);
        // A model whose skin is absent still registers; the skin slot is empty.
        let mut r = Gl3Renderer::new();
        r.set_file_loader(Box::new(|path| {
            (path == "m.md2").then(|| synthetic_md2("missing.pcx"))
        }));
        let handle = r.register_model("m.md2");
        let Some(Model::Alias(model)) = r.models.get(handle) else {
            panic!("expected an MD2 model");
        };
        assert_eq!(model.skins, vec![None]);
    }

    #[test]
    fn inline_models_are_valid_only_for_the_loaded_bsp() {
        let mut r = Gl3Renderer::new();
        assert_eq!(r.register_model("*1"), ModelHandle::NONE);
        r.load_bsp(&bsp_with_models(3));
        let two = r.register_model("*2");
        assert!(matches!(
            r.models.get(two),
            Some(Model::Brush { submodel: 2 })
        ));
        assert_eq!(r.register_model("*3"), ModelHandle::NONE);
        assert_eq!(r.register_model("*0"), ModelHandle::NONE);

        // A new map drops inline models but keeps MD2s.
        r.set_file_loader(model_files());
        let md2 = r.register_model("models/t/tris.md2");
        r.load_bsp(&bsp_with_models(2));
        assert!(r.models.get(two).is_none());
        assert!(r.models.get(md2).is_some());
        assert_eq!(r.register_model("*2"), ModelHandle::NONE);
    }

    #[test]
    fn begin_registration_invalidates_every_handle() {
        let mut r = Gl3Renderer::new();
        r.set_file_loader(model_files());
        r.load_bsp(&bsp_with_models(2));
        let md2 = r.register_model("models/t/tris.md2");
        let brush = r.register_model("*1");
        r.begin_registration("maps/next.bsp");
        assert!(r.models.get(md2).is_none());
        assert!(r.models.get(brush).is_none());
    }

    #[test]
    fn pic_size_comes_from_the_resolved_pcx_and_misses_are_none() {
        let mut r = Gl3Renderer::new();
        assert_eq!(r.draw_get_pic_size("num_0"), None, "no file loader");
        r.set_file_loader(model_files());
        assert_eq!(r.draw_get_pic_size("num_0"), Some((2, 2)));
        assert_eq!(r.draw_get_pic_size("pics/num_0.pcx"), Some((2, 2)));
        assert_eq!(r.draw_get_pic_size("nope"), None);
    }

    #[test]
    fn two_d_draws_queue_in_call_order_with_c_geometry() {
        let mut r = Gl3Renderer::new();
        r.set_file_loader(model_files());
        r.draw_fill(1, 2, 3, 4, 7);
        r.draw_pic(10, 20, "num_0");
        r.draw_stretch_pic(0, 0, 64, 32, "num_0");
        r.draw_char(8, 16, i32::from(b'A'));
        assert_eq!(r.ui_queue.len(), 4);
        assert_eq!(r.ui_queue[0].image, QuadImage::White);
        // draw_pic uses the pic's own 2x2 size: top-left (10,20), bottom-right (12,22).
        assert_eq!(&r.ui_queue[1].vertices[0..2], &[10.0, 20.0]);
        assert_eq!(&r.ui_queue[1].vertices[40..42], &[12.0, 22.0]);
        assert_eq!(&r.ui_queue[2].vertices[40..42], &[64.0, 32.0]);
        // 'A' is cell (1, 4) of the 16x16 font, drawn 8x8 pixels.
        assert_eq!(&r.ui_queue[3].vertices[2..4], &[0.0625, 0.25]);
        assert_eq!(&r.ui_queue[3].vertices[40..42], &[16.0, 24.0]);
    }

    #[test]
    fn spaces_offscreen_chars_missing_pics_and_bad_fills_queue_nothing() {
        let mut r = Gl3Renderer::new();
        r.set_file_loader(model_files());
        r.draw_char(0, 0, 32);
        r.draw_char(0, -8, i32::from(b'A'));
        r.draw_pic(0, 0, "nope");
        r.draw_stretch_pic(0, 0, 4, 4, "nope");
        r.draw_fill(0, 0, 4, 4, 300);
        assert!(r.ui_queue.is_empty());
        r.draw_char(0, -7, i32::from(b'A'));
        assert_eq!(r.ui_queue.len(), 1);
    }

    #[test]
    fn end_frame_without_a_gl_context_still_drops_the_queue() {
        let mut r = Gl3Renderer::new();
        r.set_file_loader(model_files());
        r.draw_fill(0, 0, 4, 4, 1);
        r.end_frame();
        assert!(r.ui_queue.is_empty());
    }

    #[test]
    fn stock_models_pics_and_lighting_load_from_pak0() {
        let Some(loader) = stock_loader() else {
            return;
        };
        let bsp = BspData::load(&loader("maps/demo1.bsp").expect("demo1 in pak0")).unwrap();
        let mut r = Gl3Renderer::new();
        r.set_file_loader(stock_loader().unwrap());
        r.load_bsp(&bsp);

        let soldier = r.register_model("models/monsters/soldier/tris.md2");
        let Some(Model::Alias(model)) = r.models.get(soldier) else {
            panic!("soldier registers as an MD2 model");
        };
        assert!(model.skins[0].is_some(), "soldier skin resolves to a PCX");
        let entity = RenderEntity {
            model: soldier,
            frame: 1,
            oldframe: 0,
            backlerp: 0.5,
            ..Default::default()
        };
        let stream = alias::entity_vertices(model, &entity, || [0.5; 3], 0.0).unwrap();
        assert_eq!(
            stream.len(),
            model.corners.len() * alias::ALIAS_VERTEX_FLOATS
        );
        assert!(stream.iter().all(|value| value.is_finite()));

        assert_eq!(r.draw_get_pic_size("conchars"), Some((128, 128)));
        assert!(r.draw_get_pic_size("num_0").is_some());

        // Inline doors and platforms are in demo1's model lump.
        assert!(bsp.models.len() > 1);
        let last = format!("*{}", bsp.models.len() - 1);
        assert_ne!(r.register_model(&last), ModelHandle::NONE);

        // Sample the lightmap on a grid dropped from the top of the world:
        // lit floors give neither black nor fullbright everywhere.
        let tree = LightTree::from_bsp(&bsp);
        let world = &bsp.models[0];
        let mut colors = Vec::new();
        for ix in 0..40 {
            for iy in 0..40 {
                let x = world.mins.x + (world.maxs.x - world.mins.x) * ix as f32 / 39.0;
                let y = world.mins.y + (world.maxs.y - world.mins.y) * iy as f32 / 39.0;
                colors.push(tree.light_point([x, y, world.maxs.z - 1.0], &[]));
            }
        }
        assert!(
            colors.iter().any(|c| *c != [0.0; 3] && *c != [1.0; 3]),
            "some grid point lands on a lit lightmap texel"
        );
    }
}
