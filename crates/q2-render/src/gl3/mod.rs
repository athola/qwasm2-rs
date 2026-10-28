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
//! World lightmaps blend up to four light styles each and texture animation
//! chains cycle, both as in C. MD2 models outside the view frustum are
//! skipped; the server only sends entities in the player's PVS. World faces
//! are culled by `RefDef::areabits` as C's leaf marking does: each batch
//! knows the areas of the non-solid leafs listing its faces and is drawn
//! when one of them is visible, so areas behind shut doors are not drawn.
//!
//! Deferred relative to C: dynamic lights on world and brush surfaces (MD2
//! shading does include them), back-to-front sorting of translucent surfaces,
//! PVS and frustum culling of world surfaces and brush entities (without
//! area bits the whole world is drawn every frame), subdivision of warp surfaces (the warp is per-fragment, so it is
//! not needed), `.sp2` sprites, `RF_SHELL_*` shells, `RF_BEAM` beams
//! (`GL3_DrawBeam`), the placeholder for entities without a model
//! (`GL3_DrawNullModel`), the weapon model's own FOV and depth hack, entity
//! shadows, particles and the `flushmap` cvar (the same map is always kept).
//! Of [`RefDef`], `x`/`y` (the viewport is always at
//! 0, 0), `blend` (screen flash), `rdflags` (`RDF_UNDERWATER`,
//! `RDF_NOWORLDMODEL`) and `r_fullbright` are not honoured.

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

/// Faces culled by the 3D pass. Quake faces wind clockwise seen from their
/// front and GL's default front face is CCW, so C culls `GL_FRONT`.
const CULL_FACE: u32 = glow::FRONT;

/// Parameters from the last `set_sky` call.
struct SkySetting {
    name: String,
    rotate: f32,
    axis: [f32; 3],
}

/// GL3 renderer state.
pub struct Gl3Renderer {
    /// `None` only in headless unit tests.
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
    file_loader: FileLoader,
    /// `maps/<map>.bsp` of the loaded world (C `mod_known[0].name`).
    world_name: Option<String>,
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
    /// C `R_Init`: the platform supplies the GL context and `FS_LoadFile`.
    /// The palette is read here, like C `GL3_InitImages`; without it world
    /// textures show as notexture.
    pub fn new(gl: glow::Context, file_loader: FileLoader) -> Self {
        Self::with_context(Some(gl), file_loader)
    }

    /// A renderer without GL, for testing the GL-free logic.
    #[cfg(test)]
    fn headless(file_loader: FileLoader) -> Self {
        Self::with_context(None, file_loader)
    }

    fn with_context(gl: Option<glow::Context>, file_loader: FileLoader) -> Self {
        let palette = load_palette(&file_loader);
        if palette.is_none() {
            tracing::error!("GL3: no palette, world textures will show as notexture");
        }
        Self {
            gl,
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
            file_loader,
            world_name: None,
            palette,
            extra: None,
            images: ImageStore::default(),
            models: ModelRegistry::default(),
            model_count: None,
            light: None,
            ui_queue: Vec::new(),
            canvas: (0, 0),
        }
    }

    /// C `Mod_Free(&mod_known[0])`: the world's GPU objects, lighting and
    /// inline models.
    fn free_world(&mut self) {
        if let (Some(gl), Some(old)) = (&self.gl, self.world.take()) {
            draw::free_world(gl, old);
        }
        self.world_name = None;
        self.light = None;
        self.model_count = None;
        self.models.clear_brush();
    }

    /// Build the world from a parsed BSP: geometry, textures and lightmaps
    /// onto the GPU (C `Mod_LoadBrushModel` plus `GL3_BuildPolygonFromSurface`
    /// and lightmap creation).
    fn load_world(&mut self, bsp: &BspData) {
        // Entity lighting and `*n` validation need no GL context.
        self.light = Some(LightTree::from_bsp(bsp));
        self.model_count = Some(bsp.models.len());
        let Some(gl) = &self.gl else {
            return;
        };
        let images = load_world_images(&bsp.texinfo, &self.file_loader, self.palette.as_ref());
        let mesh = world::build_world_mesh(bsp, &images);
        tracing::info!(
            "GL3: uploading {} triangles, {} textures, {} lightmap pages",
            mesh.vertices.len() / (world::VERTEX_FLOATS * 3),
            images.images.len(),
            mesh.lightmaps.pages.len()
        );
        match draw::upload_world(gl, &images, &mesh) {
            Ok(uploaded) => self.world = Some(uploaded),
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
        let loader = &self.file_loader;
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
        // A repeated init starts the image cache afresh.
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
            gl.cull_face(CULL_FACE);
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

    /// C `GL3_BeginRegistration`: starts a registration sequence and loads
    /// `maps/<map_name>.bsp` through the file loader as the world, unless it
    /// is already the loaded world. Handles stay valid until
    /// `end_registration`, except inline models of a replaced world.
    fn begin_registration(&mut self, map_name: &str) -> Q2Result<()> {
        self.models.begin_sequence();
        self.images.begin_sequence();
        let fullname = format!("maps/{map_name}.bsp");
        if self.world_name.as_deref() == Some(fullname.as_str()) {
            return Ok(());
        }
        self.free_world();
        // C Mod_ForName(fullname, NULL, true): a missing world is ERR_DROP.
        let bytes = (self.file_loader)(&fullname).ok_or_else(|| {
            Q2Error::Drop(format!("GL3 begin_registration: {fullname} not found"))
        })?;
        self.load_world(&BspData::load(&bytes)?);
        self.world_name = Some(fullname);
        Ok(())
    }

    /// `*n` is inline submodel `n` of the loaded BSP; `*.md2` is loaded
    /// through the file loader with its skins. Anything else, or a failure,
    /// returns `ModelHandle::NONE` (logged once per name). A model found
    /// again has its skins marked as used, like C `Mod_ReLoadSkins`.
    fn register_model(&mut self, name: &str) -> ModelHandle {
        if name.starts_with('*') {
            return self.models.register_inline(name, self.model_count);
        }
        let (gl, palette) = (self.gl.as_ref(), self.palette.as_ref());
        let images = &mut self.images;
        let loader = &self.file_loader;
        let handle = self.models.register(name, || {
            let is_md2 = name.len() > 4 && name.to_ascii_lowercase().ends_with(".md2");
            if !is_md2 {
                return None;
            }
            entities::load_alias_model(gl, loader, palette, images, name).map(Model::Alias)
        });
        if let Some(Model::Alias(model)) = self.models.get(handle) {
            for &skin in model.skins.iter().flatten() {
                self.images.touch(skin);
            }
        }
        handle
    }

    /// `Skin` is C `RegisterSkin` (a path), `Pic` is C `Draw_FindPic` (a
    /// pic name). C registers no other type; they return `NONE`.
    fn register_image(&mut self, name: &str, img_type: ImageType) -> ImageHandle {
        let index = match img_type {
            ImageType::Skin => self.images.find(
                self.gl.as_ref(),
                &self.file_loader,
                self.palette.as_ref(),
                name,
                images::ImageKind::Skin,
            ),
            ImageType::Pic => self.find_pic(name),
            ImageType::Sprite | ImageType::Wall | ImageType::Sky => {
                tracing::warn!("GL3: register_image of {name} as {img_type:?} is not supported");
                None
            }
        };
        index.map_or(ImageHandle::NONE, images::image_handle)
    }

    /// C `GL3_EndRegistration`: frees MD2 models and skins not registered
    /// since `begin_registration`. Pics and the world stay. C (Yamagi) skips
    /// the sweep while its fixed model and image tables have room; these
    /// tables grow, so the sweep always runs, as in id's original.
    fn end_registration(&mut self) {
        self.models.free_unused();
        self.images.free_unused(self.gl.as_ref());
    }

    /// C `GL3_BeginFrame` ends in `GL3_SetGL2D`: the viewport and the 2D
    /// ortho cover the whole drawable every frame, so a frame drawing only
    /// 2D (a picture level) follows a resized canvas.
    fn begin_frame(&mut self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.canvas = (width, height);
        if let Some(gl) = &self.gl {
            // SAFETY: State-only call on the live context `gl`.
            unsafe { gl.viewport(0, 0, width, height) };
        }
    }

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
        // SAFETY: `uniforms.styles` belongs to the program bound above and
        // `lightstyles` holds exactly the 64 vec4s the shader declares.
        unsafe { gl.uniform_4_f32_slice(uniforms.styles.as_ref(), &fd.lightstyles) };

        if let Some(sky) = &self.sky {
            draw::draw_sky(gl, uniforms, sky, &self.sky_view_proj(fd));
        }
        let view_proj = build_view_projection(fd);
        let world_batches = |filter| {
            if let Some(world) = &self.world {
                let draw = BatchDraw {
                    view_proj: &view_proj,
                    time: fd.time,
                    // C GL3_DrawWorld animates world textures at 2 fps.
                    frame: (fd.time * 2.0) as i32,
                    filter,
                    translucent_entity: false,
                    areabits: fd.areabits.as_deref(),
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
            styles: &fd.lightstyles,
            frustum: alias::frustum(
                fd.vieworg.to_array(),
                fd.viewangles.to_array(),
                fd.fov_x,
                fd.fov_y,
            ),
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

        // SAFETY: Unbinding the program and reading the error flag are
        // state-only calls.
        unsafe {
            gl.use_program(None);
            // Debug builds surface a failed upload or invalid draw instead
            // of drawing nothing silently; release skips the sync point.
            if cfg!(debug_assertions) {
                let error = gl.get_error();
                if error != glow::NO_ERROR {
                    tracing::warn!("GL3: GL error 0x{error:04x} during the frame");
                }
            }
        }
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
            QuadImage::Pic(index) => images.get(index).and_then(|entry| entry.texture),
        });
    }
}

impl Gl3Renderer {
    /// Image-store index of the pic `name` resolves to (C `R_FindPic`).
    fn find_pic(&mut self, name: &str) -> Option<usize> {
        self.images.find_pic(
            self.gl.as_ref(),
            &self.file_loader,
            self.palette.as_ref(),
            name,
        )
    }

    fn pic_size(&mut self, name: &str) -> Option<(i32, i32)> {
        let index = self.find_pic(name)?;
        let entry = self.images.get(index)?;
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
    use std::cell::RefCell;
    use std::rc::Rc;

    fn no_files() -> FileLoader {
        Box::new(|_| None)
    }

    #[test]
    fn renderer_new_is_uninitialized() {
        let r = Gl3Renderer::headless(no_files());
        assert!(!r.initialized);
        assert_eq!(r.width, 0);
        assert_eq!(r.height, 0);
        // Also proves object safety: Gl3Renderer implements Renderer
        let _boxed: Box<dyn Renderer> = Box::new(r);
    }

    #[test]
    fn init_fails_without_context() {
        let mut r = Gl3Renderer::headless(no_files());
        assert!(r.init(800, 600).is_err());
    }

    #[test]
    fn init_without_context_is_recoverable_q2_drop() {
        // Renderer errors share Q2Error with the engine so the frame loop
        // and q2err_to_js handle them like any other ERR_DROP.
        let mut r = Gl3Renderer::headless(no_files());
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

    /// `files` plus `maps/<name>.bsp` with `models` submodels for each map,
    /// recording every path asked for.
    fn with_maps(
        files: FileLoader,
        maps: &[(&str, usize)],
        requests: Rc<RefCell<Vec<String>>>,
    ) -> FileLoader {
        let maps: Vec<(String, Vec<u8>)> = maps
            .iter()
            .map(|&(name, models)| {
                (
                    format!("maps/{name}.bsp"),
                    crate::bsp::test_bsp_bytes(models),
                )
            })
            .collect();
        Box::new(move |path| {
            requests.borrow_mut().push(path.to_string());
            maps.iter()
                .find(|(map, _)| map == path)
                .map(|(_, bytes)| bytes.clone())
                .or_else(|| files(path))
        })
    }

    fn map_renderer(maps: &[(&str, usize)]) -> (Gl3Renderer, Rc<RefCell<Vec<String>>>) {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let loader = with_maps(model_files(), maps, Rc::clone(&requests));
        (Gl3Renderer::headless(loader), requests)
    }

    fn map_loads(requests: &RefCell<Vec<String>>, path: &str) -> usize {
        requests.borrow().iter().filter(|p| *p == path).count()
    }

    #[test]
    fn begin_registration_without_a_gl_context_builds_no_gpu_world() {
        let (mut r, _) = map_renderer(&[("base1", 1)]);
        r.begin_registration("base1").unwrap();
        assert!(r.world.is_none());
        assert!(r.sky.is_none());
    }

    #[test]
    fn begin_registration_loads_the_world_from_maps_dir() {
        // C R_BeginRegistration: Mod_ForName("maps/%s.bsp", crash = true).
        let (mut r, requests) = map_renderer(&[("base1", 3)]);
        r.begin_registration("base1").unwrap();
        assert_eq!(map_loads(&requests, "maps/base1.bsp"), 1);
        assert_eq!(r.world_name.as_deref(), Some("maps/base1.bsp"));
        assert_eq!(r.model_count, Some(3));
        assert!(r.light.is_some());
    }

    #[test]
    fn begin_registration_of_a_missing_or_corrupt_map_is_a_recoverable_drop() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let corrupt: FileLoader = Box::new(|path| (path == "maps/bad.bsp").then(|| vec![1, 2, 3]));
        let loader = with_maps(corrupt, &[("base1", 3)], Rc::clone(&requests));
        let mut r = Gl3Renderer::headless(loader);
        r.begin_registration("base1").unwrap();
        for map in ["gone", "bad"] {
            let err = r.begin_registration(map).unwrap_err();
            assert!(matches!(err, Q2Error::Drop(_)), "{map}: {err:?}");
            assert!(err.is_recoverable());
            // C has freed the old world before Mod_ForName fails.
            assert!(r.world_name.is_none());
            assert_eq!(r.register_model("*1"), ModelHandle::NONE);
        }
        assert_eq!(map_loads(&requests, "maps/gone.bsp"), 1);
        // A later good map loads again.
        r.begin_registration("base1").unwrap();
        assert_ne!(r.register_model("*1"), ModelHandle::NONE);
    }

    #[test]
    fn begin_registration_of_a_new_map_replaces_the_world() {
        let (mut r, _) = map_renderer(&[("base1", 3), ("base2", 2)]);
        r.begin_registration("base1").unwrap();
        let two = r.register_model("*2");
        assert!(matches!(
            r.models.get(two),
            Some(Model::Brush { submodel: 2 })
        ));
        let sequence = r.models.sequence();
        r.begin_registration("base2").unwrap();
        assert_eq!(r.world_name.as_deref(), Some("maps/base2.bsp"));
        assert_eq!(r.model_count, Some(2));
        assert!(r.models.get(two).is_none());
        assert_eq!(r.register_model("*2"), ModelHandle::NONE);
        assert!(r.models.sequence() > sequence);
    }

    #[test]
    fn begin_registration_of_the_same_map_keeps_the_world() {
        // C frees mod_known[0] only `if (strcmp(mod_known[0].name, fullname))`.
        let (mut r, requests) = map_renderer(&[("base1", 3)]);
        r.begin_registration("base1").unwrap();
        let one = r.register_model("*1");
        let sequence = r.models.sequence();
        r.begin_registration("base1").unwrap();
        assert_eq!(map_loads(&requests, "maps/base1.bsp"), 1);
        assert!(r.models.get(one).is_some());
        assert_eq!(r.register_model("*1"), one);
        assert!(r.models.sequence() > sequence);
    }

    #[test]
    fn sky_defaults_to_unit1_until_set() {
        let mut r = Gl3Renderer::headless(no_files());
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
    fn constructor_reads_palette_from_colormap() {
        let mut pcx = vec![0u8; 200];
        pcx.extend((0..=255u8).flat_map(|i| [i, 0, 0]));
        let r = Gl3Renderer::headless(Box::new(move |path| {
            (path == "pics/colormap.pcx").then(|| pcx.clone())
        }));
        assert_eq!(r.palette.as_ref().unwrap().0[7], [7, 0, 0]);
    }

    #[test]
    fn missing_colormap_leaves_palette_unset() {
        let r = Gl3Renderer::headless(no_files());
        assert!(r.palette.is_none());
    }

    #[test]
    fn sky_view_proj_ignores_eye_position() {
        let r = Gl3Renderer::headless(no_files());
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

    /// Signed area of a world-space triangle after projection to NDC;
    /// negative means clockwise on screen.
    fn ndc_signed_area(view_proj: &[f32; 16], triangle: [[f32; 3]; 3]) -> f32 {
        let ndc = triangle.map(|p| {
            let clip = |row: usize| {
                view_proj[row] * p[0]
                    + view_proj[4 + row] * p[1]
                    + view_proj[8 + row] * p[2]
                    + view_proj[12 + row]
            };
            let w = clip(3);
            [clip(0) / w, clip(1) / w]
        });
        let (a, b, c) = (ndc[0], ndc[1], ndc[2]);
        (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
    }

    #[test]
    fn front_facing_quake_faces_project_clockwise_so_gl_front_is_culled() {
        // Quake faces wind clockwise seen from their front: a floor at z = 0
        // in surfedge order, seen from above, as the mesh fans it.
        let floor = [[0.0, 0.0, 0.0], [0.0, 64.0, 0.0], [64.0, 64.0, 0.0]];
        for (vieworg, viewangles) in [
            (Vec3f::new(32.0, 32.0, 100.0), Vec3f::new(89.0, 0.0, 0.0)),
            (Vec3f::new(-100.0, 20.0, 50.0), Vec3f::new(20.0, 10.0, 0.0)),
            (
                Vec3f::new(30.0, 200.0, 80.0),
                Vec3f::new(30.0, -100.0, 15.0),
            ),
        ] {
            let fd = RefDef {
                width: 640,
                height: 480,
                fov_y: 90.0,
                vieworg,
                viewangles,
                ..Default::default()
            };
            let area = ndc_signed_area(&build_view_projection(&fd), floor);
            assert!(area < 0.0, "{vieworg:?} {viewangles:?}: area {area}");
        }
        // GL's default front face is CCW, so clockwise faces are kept by
        // culling GL_FRONT, as C's glCullFace(GL_FRONT).
        assert_eq!(CULL_FACE, glow::FRONT);
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
            // Visible with `--nocapture` or on failure; CI has no game data.
            eprintln!("SKIPPED: {} not found", pak_path.display());
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

        // The clockwise-front convention the cull state relies on holds
        // for the real map: each face's fan winds clockwise about the
        // face's front normal (summed over the fan, so near-collinear
        // corners cannot flip the sign).
        let (mut clockwise, mut counter) = (0, 0);
        for face in &bsp.faces {
            let Some(p) = world::face_positions(&bsp, face) else {
                continue;
            };
            let plane = &bsp.planes[usize::from(face.plane_idx)];
            let sign = if face.side == 0 { 1.0 } else { -1.0 };
            let corner = |i: usize| Vec3f::new(p[i][0], p[i][1], p[i][2]);
            let area: Vec3f = (1..p.len() - 1)
                .map(|k| (corner(k) - corner(0)).cross(corner(k + 1) - corner(0)))
                .sum();
            let along_normal = area.dot(plane.normal) * sign;
            if along_normal < 0.0 {
                clockwise += 1;
            } else if along_normal > 0.0 {
                counter += 1;
            }
        }
        assert!(clockwise > 1000, "{clockwise} clockwise faces");
        assert_eq!(counter, 0, "{counter} counter-clockwise faces");

        let mesh = world::build_world_mesh(&bsp, &images);
        assert!(mesh.vertices.len() > 1000 * world::VERTEX_FLOATS);
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
            i32::try_from(mesh.vertices.len() / world::VERTEX_FLOATS).ok()
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
            let first = batch.first_vertex as usize * world::VERTEX_FLOATS;
            let count = batch.vertex_count as usize * world::VERTEX_FLOATS;
            for vertex in mesh.vertices[first..first + count].chunks(world::VERTEX_FLOATS) {
                assert!((0.0..=1.0).contains(&vertex[5]) && (0.0..=1.0).contains(&vertex[6]));
            }
        }
        // demo1 has switchable or flickering lights: some lit face blends a
        // style other than 0 into a lightmap layer.
        assert!(
            mesh.vertices
                .chunks(world::VERTEX_FLOATS)
                .any(|vertex| vertex[7..11].iter().any(|&style| style > 0.0)),
            "some face carries a non-default light style"
        );

        for suffix in SKY_SUFFIXES {
            let face = sky::load_sky_image(&loader, DEFAULT_SKY, suffix).expect("unit1 sky face");
            assert!(face.width > 0 && face.rgba.len() == face.width * face.height * 4);
        }
    }

    #[test]
    fn demo1_world_behind_its_shut_portal_door_is_not_drawn_from_area_2() {
        let Some(loader) = stock_loader() else {
            return;
        };
        let bsp = BspData::load(&loader("maps/demo1.bsp").expect("demo1 in pak0")).unwrap();
        let images = load_world_images(&bsp.texinfo, &loader, None);
        let mesh = world::build_world_mesh(&bsp, &images);
        let drawn = |bits: Option<&[u8]>| -> i32 {
            mesh.batches
                .iter()
                .filter(|batch| batch.in_visible_area(bits))
                .map(|batch| batch.vertex_count)
                .sum()
        };
        let everything = drawn(None);
        // demo1's door portal joins areas 1 and 2 (CM_WriteAreaBits of
        // area 2 while it is shut, then open).
        let shut = drawn(Some(&[0b0100]));
        let open = drawn(Some(&[0b0110]));
        assert!(
            0 < shut && shut < open && open <= everything,
            "{shut} {open} {everything}"
        );
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

    #[test]
    fn register_model_loads_an_md2_with_its_skin_once() {
        let mut r = Gl3Renderer::headless(model_files());
        let handle = r.register_model("models/t/tris.md2");
        assert_ne!(handle, ModelHandle::NONE);
        assert_eq!(r.register_model("models/t/tris.md2"), handle);
        let Some(Model::Alias(model)) = r.models.get(handle) else {
            panic!("expected an MD2 model");
        };
        assert_eq!(model.corners.len(), 3);
        // Skin 0 resolved through the image store to the 2x2 PCX.
        let skin = model.skins[0].expect("skin loaded");
        assert_eq!(r.images.get(skin).unwrap().width, 2);
    }

    #[test]
    fn register_model_rejects_missing_and_unsupported_names() {
        let mut r = Gl3Renderer::headless(no_files());
        assert_eq!(r.register_model("models/t/tris.md2"), ModelHandle::NONE);
        let mut r = Gl3Renderer::headless(model_files());
        assert_eq!(r.register_model("models/t/gone.md2"), ModelHandle::NONE);
        assert_eq!(r.register_model("sprites/s_explod.sp2"), ModelHandle::NONE);
        assert_eq!(r.register_model(""), ModelHandle::NONE);
        // A model whose skin is absent still registers; the skin slot is empty.
        let mut r = Gl3Renderer::headless(Box::new(|path| {
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
        let (mut r, _) = map_renderer(&[("base1", 3), ("base2", 2)]);
        assert_eq!(r.register_model("*1"), ModelHandle::NONE);
        r.begin_registration("base1").unwrap();
        let two = r.register_model("*2");
        assert!(matches!(
            r.models.get(two),
            Some(Model::Brush { submodel: 2 })
        ));
        assert_eq!(r.register_model("*3"), ModelHandle::NONE);
        assert_eq!(r.register_model("*0"), ModelHandle::NONE);

        // A new map drops inline models but keeps MD2s until the sweep.
        let md2 = r.register_model("models/t/tris.md2");
        r.begin_registration("base2").unwrap();
        assert!(r.models.get(two).is_none());
        assert!(r.models.get(md2).is_some());
        assert_eq!(r.register_model("*2"), ModelHandle::NONE);
    }

    #[test]
    fn end_registration_frees_models_not_registered_since_begin() {
        // C GL3_EndRegistration frees every model whose registration_sequence
        // is old, and GL3_FreeUnusedImages every image but pics.
        let (mut r, _) = map_renderer(&[("base1", 2)]);
        r.begin_registration("base1").unwrap();
        let md2 = r.register_model("models/t/tris.md2");
        let brush = r.register_model("*1");
        let Some(Model::Alias(model)) = r.models.get(md2) else {
            panic!("expected an MD2 model");
        };
        let skin = model.skins[0].unwrap();
        let pic = r.find_pic("num_0").unwrap();
        r.begin_registration("base1").unwrap();
        // Handles survive begin_registration, as in C.
        assert!(r.models.get(md2).is_some());
        r.end_registration();
        assert!(r.models.get(md2).is_none());
        assert!(r.images.get(skin).is_none());
        assert!(r.images.get(pic).is_some());
        // Inline models live as long as the world.
        assert!(r.models.get(brush).is_some());
    }

    #[test]
    fn re_registered_models_keep_their_skins_through_end_registration() {
        let (mut r, _) = map_renderer(&[("base1", 1)]);
        r.begin_registration("base1").unwrap();
        let md2 = r.register_model("models/t/tris.md2");
        r.begin_registration("base1").unwrap();
        // C Mod_ReLoadSkins touches the skins of a model found again.
        assert_eq!(r.register_model("models/t/tris.md2"), md2);
        r.end_registration();
        let Some(Model::Alias(model)) = r.models.get(md2) else {
            panic!("MD2 kept");
        };
        assert!(r.images.get(model.skins[0].unwrap()).is_some());
    }

    #[test]
    fn register_image_returns_handles_for_skins_and_pics() {
        let mut r = Gl3Renderer::headless(model_files());
        let skin = r.register_image("models/t/skin.pcx", ImageType::Skin);
        assert_ne!(skin, ImageHandle::NONE);
        assert_eq!(r.register_image("models/t/skin.pcx", ImageType::Skin), skin);
        let index = images::handle_index(skin).unwrap();
        assert_eq!(r.images.get(index).unwrap().width, 2);
        // Pics resolve like draw names (C RegisterPic / Draw_FindPic).
        let pic = r.register_image("num_0", ImageType::Pic);
        assert_eq!(images::handle_index(pic), r.find_pic("num_0"));
        assert_eq!(
            r.register_image("nope.pcx", ImageType::Skin),
            ImageHandle::NONE
        );
        assert_eq!(r.register_image("nope", ImageType::Pic), ImageHandle::NONE);
        // C exports no registration for walls, sprites or skies.
        assert_eq!(
            r.register_image("models/t/skin.pcx", ImageType::Wall),
            ImageHandle::NONE
        );
    }

    #[test]
    fn registered_skin_untouched_since_begin_is_freed_by_end_registration() {
        let (mut r, _) = map_renderer(&[("base1", 1)]);
        r.begin_registration("base1").unwrap();
        let kept = r.register_image("pics/num_0.pcx", ImageType::Skin);
        let dropped = r.register_image("models/t/skin.pcx", ImageType::Skin);
        r.begin_registration("base1").unwrap();
        assert_eq!(r.register_image("pics/num_0.pcx", ImageType::Skin), kept);
        r.end_registration();
        assert!(r.images.get(images::handle_index(kept).unwrap()).is_some());
        assert!(r
            .images
            .get(images::handle_index(dropped).unwrap())
            .is_none());
    }

    #[test]
    fn pic_size_comes_from_the_resolved_pcx_and_misses_are_none() {
        let mut r = Gl3Renderer::headless(no_files());
        assert_eq!(r.draw_get_pic_size("num_0"), None, "no files");
        let mut r = Gl3Renderer::headless(model_files());
        assert_eq!(r.draw_get_pic_size("num_0"), Some((2, 2)));
        assert_eq!(r.draw_get_pic_size("pics/num_0.pcx"), Some((2, 2)));
        assert_eq!(r.draw_get_pic_size("nope"), None);
    }

    #[test]
    fn two_d_draws_queue_in_call_order_with_c_geometry() {
        let mut r = Gl3Renderer::headless(model_files());
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
    fn begin_frame_sizes_the_2d_canvas_to_the_drawable_like_c_set_gl2d() {
        let mut r = Gl3Renderer::headless(no_files());
        r.begin_frame(1280, 720);
        assert_eq!(r.canvas, (1280, 720));
        // An empty drawable keeps the last usable size, as render_frame does.
        r.begin_frame(0, 720);
        assert_eq!(r.canvas, (1280, 720));
    }

    #[test]
    fn spaces_offscreen_chars_missing_pics_and_bad_fills_queue_nothing() {
        let mut r = Gl3Renderer::headless(model_files());
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
        let mut r = Gl3Renderer::headless(model_files());
        r.draw_fill(0, 0, 4, 4, 1);
        r.end_frame();
        assert!(r.ui_queue.is_empty());
    }

    #[test]
    fn light_level_is_150_even_where_the_lightmap_is_dark_like_c_gl3() {
        // C GL3_SetLightLevel calls GL3_LightPoint(NULL, vieworg, ...), and
        // GL3_LightPoint answers 1.0 for a NULL entity before reading the
        // lightmap, so r_lightlevel is 150 * 1.0 wherever the view is.
        let Some(loader) = stock_loader() else {
            return;
        };
        let bsp = BspData::load(&loader("maps/demo1.bsp").expect("demo1 in pak0")).unwrap();
        let tree = LightTree::from_bsp(&bsp);
        let mut r = Gl3Renderer::headless(loader);
        r.begin_registration("demo1").unwrap();
        let world = &bsp.models[0];
        let dark = (0..40 * 40)
            .map(|i| {
                let x = world.mins.x + (world.maxs.x - world.mins.x) * (i % 40) as f32 / 39.0;
                let y = world.mins.y + (world.maxs.y - world.mins.y) * (i / 40) as f32 / 39.0;
                [x, y, world.maxs.z - 1.0]
            })
            .find(|&p| {
                let c = tree.light_point(p, &[1.0; q2_shared::constants::MAX_LIGHTSTYLES], &[]);
                c.iter().all(|&v| v < 0.1)
            })
            .expect("demo1 has a dark spot");
        let fd = RefDef {
            vieworg: Vec3f::from_array(dark),
            ..RefDef::default()
        };
        r.render_frame(&fd, &[], &[], &[]);
        assert_eq!(r.light_level(), 150.0);
    }

    #[test]
    fn stock_models_pics_and_lighting_load_from_pak0() {
        let Some(loader) = stock_loader() else {
            return;
        };
        let bsp = BspData::load(&loader("maps/demo1.bsp").expect("demo1 in pak0")).unwrap();
        let mut r = Gl3Renderer::headless(loader);
        r.begin_registration("demo1")
            .expect("demo1 loads by its short name");

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

        // MD2 triangles wind like world faces (clockwise about the outward
        // vertex normals), so the world's cull state also suits them.
        let frame = &model.md2.frames[0];
        let (mut clockwise, mut counter) = (0, 0);
        for triangle in &model.md2.triangles {
            let corner = |k: usize| frame.vertex_position(usize::from(triangle.vertex[k]));
            let normal = |k: usize| {
                let index = frame.verts[usize::from(triangle.vertex[k])].normal_index;
                let [x, y, z] = crate::files::md2::VERTEX_NORMALS[usize::from(index)];
                Vec3f::new(x, y, z)
            };
            let winding = (corner(1) - corner(0)).cross(corner(2) - corner(0));
            let outward = normal(0) + normal(1) + normal(2);
            match winding.dot(outward) {
                d if d < 0.0 => clockwise += 1,
                d if d > 0.0 => counter += 1,
                _ => {}
            }
        }
        assert!(
            clockwise > 20 * counter,
            "{clockwise} clockwise, {counter} counter-clockwise soldier triangles"
        );

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
                colors.push(tree.light_point(
                    [x, y, world.maxs.z - 1.0],
                    &[1.0; q2_shared::constants::MAX_LIGHTSTYLES],
                    &[],
                ));
            }
        }
        assert!(
            colors.iter().any(|c| *c != [0.0; 3] && *c != [1.0; 3]),
            "some grid point lands on a lit lightmap texel"
        );
    }
}
