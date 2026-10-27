//! GL3/GLES3 renderer backend using the `glow` crate.
//!
//! Draws the BSP world textured and lightmapped like the C GL3 renderer,
//! with a sky box behind it. Pure logic lives in submodules so it can be
//! tested without a GL context:
//! - [`texture`]: `.wal` loading, palette conversion, texture upload
//! - [`lightmap`]: surface extents and lightmap atlas packing
//! - [`world`]: face batching into one vertex buffer
//! - [`sky`]: sky box geometry and image lookup
//! - [`shader`] / [`draw`]: the GL program and draw loops
//!
//! Deferred relative to C: texture animation chains (`texinfo.next`), light
//! styles other than the first lightmap, dynamic lights, back-to-front
//! sorting of translucent surfaces, PVS/frustum culling (the whole world is
//! drawn every frame), and subdivision of warp surfaces (the warp is
//! per-fragment, so it is not needed).

mod draw;
mod lightmap;
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
use draw::{SkyGpu, WorldGpu};
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

    fn begin_registration(&mut self, _map_name: &str) {}
    fn register_model(&mut self, _name: &str) -> ModelHandle {
        ModelHandle::NONE
    }
    fn register_image(&mut self, _name: &str, _img_type: ImageType) -> ImageHandle {
        ImageHandle::NONE
    }
    fn end_registration(&mut self) {}

    fn render_frame(
        &mut self,
        fd: &RefDef,
        _entities: &[RenderEntity],
        _dlights: &[DLight],
        _particles: &[Particle],
    ) {
        let gl = match &self.gl {
            Some(gl) => gl,
            None => return,
        };

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
        if let Some(world) = &self.world {
            draw::draw_world(gl, uniforms, world, &build_view_projection(fd), fd.time);
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
    fn draw_pic(&mut self, _x: i32, _y: i32, _name: &str) {}
    fn draw_stretch_pic(&mut self, _x: i32, _y: i32, _w: i32, _h: i32, _name: &str) {}
    fn draw_char(&mut self, _x: i32, _y: i32, _ch: i32) {}
    fn draw_fill(&mut self, _x: i32, _y: i32, _w: i32, _h: i32, _color: i32) {}

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

    fn end_frame(&mut self) {}
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
        let end = mesh.batches.last().map(|b| b.first_vertex + b.vertex_count);
        assert_eq!(
            end,
            i32::try_from(mesh.vertices.len() / sky::VERTEX_FLOATS).ok()
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
}
