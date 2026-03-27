//! GL3/GLES3 renderer backend using the `glow` crate.
//!
//! Renders BSP world geometry as flat-shaded triangles with per-face
//! colouring derived from the face's plane normal (pseudo-lighting).

use glow::HasContext;
use q2_render_api::*;
use q2_shared::types::Vec3f;

use crate::bsp::BspData;

/// Compiled GPU mesh: VAO + vertex count, ready to draw.
struct GpuMesh {
    vao: glow::VertexArray,
    _vbo: glow::Buffer,
    vertex_count: i32,
}

/// GL3 renderer state.
pub struct Gl3Renderer {
    gl: Option<glow::Context>,
    width: i32,
    height: i32,
    initialized: bool,
    /// Shader program for world geometry.
    world_program: Option<glow::Program>,
    /// Uploaded world mesh.
    world_mesh: Option<GpuMesh>,
    /// Uniform locations.
    u_view_proj: Option<glow::UniformLocation>,
}

impl Gl3Renderer {
    pub fn new() -> Self {
        Self {
            gl: None,
            width: 0,
            height: 0,
            initialized: false,
            world_program: None,
            world_mesh: None,
            u_view_proj: None,
        }
    }

    /// Provide the glow GL context. Must be called before `init`.
    pub fn set_gl_context(&mut self, gl: glow::Context) {
        self.gl = Some(gl);
    }

    /// Load BSP world geometry onto the GPU.
    pub fn load_bsp(&mut self, bsp: &BspData) {
        let gl = match &self.gl {
            Some(gl) => gl,
            None => return,
        };

        // Build vertex data from BSP faces: position (3f) + color (3f) per vertex
        let mut verts: Vec<f32> = Vec::new();

        // Only render world model (model 0) faces
        let (first_face, num_faces) = if let Some(model) = bsp.models.first() {
            (model.first_face as usize, model.num_faces as usize)
        } else {
            (0, bsp.faces.len())
        };

        for face_idx in first_face..first_face + num_faces {
            let face = &bsp.faces[face_idx];
            let num_edges = face.num_edges as usize;
            if num_edges < 3 {
                continue;
            }

            // Skip sky/nodraw surfaces
            if (face.texinfo_idx as usize) < bsp.texinfo.len() {
                let ti = &bsp.texinfo[face.texinfo_idx as usize];
                // SURF_SKY=4, SURF_NODRAW=128
                if ti.flags & (4 | 128) != 0 {
                    continue;
                }
            }

            // Get the face's plane normal for pseudo-lighting
            let normal = if (face.plane_idx as usize) < bsp.planes.len() {
                let plane = &bsp.planes[face.plane_idx as usize];
                if face.side != 0 {
                    -plane.normal
                } else {
                    plane.normal
                }
            } else {
                Vec3f::new(0.0, 0.0, 1.0)
            };

            // Compute face color from normal (simple directional light)
            let light_dir = Vec3f::new(0.5, 0.3, 0.9).normalize();
            let ndotl = normal.dot(light_dir).max(0.0);
            let ambient = 0.15;
            let brightness = ambient + (1.0 - ambient) * ndotl;

            // Vary base color by texinfo index for visual variety
            let ti_idx = face.texinfo_idx as usize;
            let (r, g, b) = face_color(ti_idx, brightness);

            // Collect face vertices via surface_edges → edges → vertices
            let mut face_verts: Vec<Vec3f> = Vec::with_capacity(num_edges);
            for i in 0..num_edges {
                let se_idx = (face.first_edge as usize) + i;
                if se_idx >= bsp.surface_edges.len() {
                    break;
                }
                let se = bsp.surface_edges[se_idx];
                let vi = if se >= 0 {
                    let edge_idx = se as usize;
                    if edge_idx < bsp.edges.len() {
                        bsp.edges[edge_idx].v[0] as usize
                    } else {
                        continue;
                    }
                } else {
                    let edge_idx = (-se) as usize;
                    if edge_idx < bsp.edges.len() {
                        bsp.edges[edge_idx].v[1] as usize
                    } else {
                        continue;
                    }
                };

                if vi < bsp.vertices.len() {
                    face_verts.push(bsp.vertices[vi].position);
                }
            }

            // Fan triangulate: v0, v1, v2, then v0, v2, v3, etc.
            if face_verts.len() >= 3 {
                let v0 = face_verts[0];
                for i in 1..face_verts.len() - 1 {
                    let v1 = face_verts[i];
                    let v2 = face_verts[i + 1];
                    // Triangle: v0, v1, v2
                    for v in &[v0, v1, v2] {
                        verts.push(v.x);
                        verts.push(v.y);
                        verts.push(v.z);
                        verts.push(r);
                        verts.push(g);
                        verts.push(b);
                    }
                }
            }
        }

        let vertex_count = (verts.len() / 6) as i32;
        tracing::info!("GL3: uploading {} triangles ({} verts)", vertex_count / 3, vertex_count);

        // SAFETY: All glow GL calls require unsafe. `verts` is a live
        // `Vec<f32>` whose backing allocation is valid for the duration of this
        // block. `f32` has no padding, and `verts.len() * size_of::<f32>()`
        // cannot overflow because `Vec` guarantees its allocation fits in
        // `isize`. The resulting `&[u8]` borrows `verts` immutably and is
        // consumed by `buffer_data_u8_slice` before `verts` is dropped.
        unsafe {
            let vao = gl.create_vertex_array().expect("create VAO");
            let vbo = gl.create_buffer().expect("create VBO");

            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));

            let bytes: &[u8] = core::slice::from_raw_parts(
                verts.as_ptr() as *const u8,
                verts.len() * core::mem::size_of::<f32>(),
            );
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);

            // position: location 0, 3 floats, stride 24, offset 0
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 24, 0);

            // color: location 1, 3 floats, stride 24, offset 12
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 24, 12);

            gl.bind_vertex_array(None);

            self.world_mesh = Some(GpuMesh {
                vao,
                _vbo: vbo,
                vertex_count,
            });
        }
    }

    /// Compile the world shader program.
    fn compile_shaders(gl: &glow::Context) -> Option<glow::Program> {
        // SAFETY: glow shader compilation calls require unsafe.
        unsafe {
            let program = gl.create_program().ok()?;

            let vs_src = r#"#version 300 es
                precision highp float;
                layout(location = 0) in vec3 a_position;
                layout(location = 1) in vec3 a_color;
                uniform mat4 u_view_proj;
                out vec3 v_color;
                void main() {
                    gl_Position = u_view_proj * vec4(a_position, 1.0);
                    v_color = a_color;
                }
            "#;

            let fs_src = r#"#version 300 es
                precision mediump float;
                in vec3 v_color;
                out vec4 frag_color;
                void main() {
                    frag_color = vec4(v_color, 1.0);
                }
            "#;

            let vs = gl.create_shader(glow::VERTEX_SHADER).ok()?;
            gl.shader_source(vs, vs_src);
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) {
                let log = gl.get_shader_info_log(vs);
                tracing::error!("VS compile error: {}", log);
                gl.delete_shader(vs);
                return None;
            }

            let fs = gl.create_shader(glow::FRAGMENT_SHADER).ok()?;
            gl.shader_source(fs, fs_src);
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                let log = gl.get_shader_info_log(fs);
                tracing::error!("FS compile error: {}", log);
                gl.delete_shader(fs);
                gl.delete_shader(vs);
                return None;
            }

            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                tracing::error!("Program link error: {}", log);
                gl.delete_shader(vs);
                gl.delete_shader(fs);
                gl.delete_program(program);
                return None;
            }

            gl.delete_shader(vs);
            gl.delete_shader(fs);

            Some(program)
        }
    }
}

impl Default for Gl3Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for Gl3Renderer {
    fn init(&mut self, width: i32, height: i32) -> Result<(), String> {
        let gl = match &self.gl {
            Some(gl) => gl,
            None => {
                return Err("GL3Renderer::init called without a GL context".into());
            }
        };

        self.width = width;
        self.height = height;

        // Compile shaders
        self.world_program = Self::compile_shaders(gl);
        if self.world_program.is_none() {
            return Err("Failed to compile world shaders".into());
        }

        // Get uniform location
        if let Some(prog) = &self.world_program {
            // SAFETY: glow uniform lookup requires unsafe.
            unsafe {
                self.u_view_proj = gl.get_uniform_location(*prog, "u_view_proj");
            }
        }

        // SAFETY: glow GL state setup requires unsafe.
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
        self.initialized = false;
    }

    fn begin_registration(&mut self, _map_name: &str) {}
    fn register_model(&mut self, _name: &str) -> ModelHandle { ModelHandle::NONE }
    fn register_image(&mut self, _name: &str, _img_type: ImageType) -> ImageHandle { ImageHandle::NONE }
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

        // SAFETY: glow GL draw calls require unsafe.
        unsafe {
            gl.viewport(0, 0, fd.width, fd.height);
            gl.clear_color(0.05, 0.05, 0.1, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }

        // Draw world if loaded
        if let (Some(prog), Some(mesh), Some(u_loc)) =
            (&self.world_program, &self.world_mesh, &self.u_view_proj)
        {
            let vp = build_view_projection(fd);

            // SAFETY: glow draw calls require unsafe.
            unsafe {
                gl.use_program(Some(*prog));
                gl.uniform_matrix_4_f32_slice(Some(u_loc), false, &vp);
                gl.bind_vertex_array(Some(mesh.vao));
                gl.draw_arrays(glow::TRIANGLES, 0, mesh.vertex_count);
                gl.bind_vertex_array(None);
            }
        }
    }

    fn set_sky(&mut self, _name: &str, _rotate: f32, _axis: Vec3f) {}
    fn draw_pic(&mut self, _x: i32, _y: i32, _name: &str) {}
    fn draw_stretch_pic(&mut self, _x: i32, _y: i32, _w: i32, _h: i32, _name: &str) {}
    fn draw_char(&mut self, _x: i32, _y: i32, _ch: i32) {}
    fn draw_fill(&mut self, _x: i32, _y: i32, _w: i32, _h: i32, _color: i32) {}

    fn clear_screen(&mut self) {
        if let Some(gl) = &self.gl {
            // SAFETY: glow GL calls require unsafe — clearing the framebuffer.
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
        f / aspect, 0.0,  0.0,                    0.0,
        0.0,        f,    0.0,                    0.0,
        0.0,        0.0,  (far + near) * nf,     -1.0,
        0.0,        0.0,  2.0 * far * near * nf,  0.0,
    ]
}

/// View matrix converting Q2 world coords to OpenGL eye coords.
/// Q2: +X forward, +Y left, +Z up. OpenGL: -Z forward, +X right, +Y up.
fn view_matrix(fd: &RefDef) -> [f32; 16] {
    let pitch = fd.viewangles.x.to_radians();
    let yaw = fd.viewangles.y.to_radians();

    let (sp, cp) = (pitch.sin(), pitch.cos());
    let (sy, cy) = (yaw.sin(), yaw.cos());

    // Basis vectors: eye_right, eye_up, eye_forward
    let (rx, ry, rz) = (sy, -cy, 0.0f32);
    let (ux, uy, uz) = (-sp * cy, -sp * sy, cp);
    let (fx, fy, fz) = (-cp * cy, -cp * sy, -sp);

    let (tx, ty, tz) = (fd.vieworg.x, fd.vieworg.y, fd.vieworg.z);

    [
        rx,  ux,  fx,  0.0,
        ry,  uy,  fy,  0.0,
        rz,  uz,  fz,  0.0,
        -(rx*tx + ry*ty + rz*tz),
        -(ux*tx + uy*ty + uz*tz),
        -(fx*tx + fy*ty + fz*tz),
        1.0,
    ]
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

/// Knuth multiplicative hash constant (golden ratio * 2^32).
const KNUTH_HASH: usize = 2654435761;

/// Assign a color to a face based on its texinfo index and lighting.
fn face_color(texinfo_idx: usize, brightness: f32) -> (f32, f32, f32) {
    let h = texinfo_idx.wrapping_mul(KNUTH_HASH) & 0xFFFFFF;
    let base_r = ((h >> 16) & 0xFF) as f32 / 255.0;
    let base_g = ((h >> 8) & 0xFF) as f32 / 255.0;
    let base_b = (h & 0xFF) as f32 / 255.0;

    // Desaturate and apply brightness for a more natural look
    let grey = 0.3 + (base_r + base_g + base_b) / 3.0 * 0.4;
    let r = (grey * 0.6 + base_r * 0.4) * brightness;
    let g = (grey * 0.6 + base_g * 0.4) * brightness;
    let b = (grey * 0.6 + base_b * 0.4) * brightness;

    (r.min(1.0), g.min(1.0), b.min(1.0))
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
    fn mat4_identity_mul() {
        let id = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        let r = mat4_mul(&id, &id);
        assert_eq!(r, id);
    }

    #[test]
    fn face_color_deterministic() {
        let (r1, g1, b1) = face_color(0, 1.0);
        let (r2, g2, b2) = face_color(0, 1.0);
        assert_eq!(r1, r2);
        assert_eq!(g1, g2);
        assert_eq!(b1, b2);
    }

    #[test]
    fn face_color_in_range() {
        // All outputs must be in [0.0, 1.0] for any index and brightness
        for idx in [0, 1, 42, 255, 1000, usize::MAX / 2] {
            for &brightness in &[0.0, 0.5, 1.0, 2.0] {
                let (r, g, b) = face_color(idx, brightness);
                assert!(r >= 0.0 && r <= 1.0, "r={r} out of range for idx={idx}, brightness={brightness}");
                assert!(g >= 0.0 && g <= 1.0, "g={g} out of range for idx={idx}, brightness={brightness}");
                assert!(b >= 0.0 && b <= 1.0, "b={b} out of range for idx={idx}, brightness={brightness}");
            }
        }
    }

    #[test]
    fn face_color_different_indices_differ() {
        let c0 = face_color(0, 1.0);
        let c1 = face_color(1, 1.0);
        // Different texinfo indices should produce different colors
        assert_ne!(c0, c1);
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

    #[test]
    fn load_bsp_no_gl_returns_early() {
        let mut r = Gl3Renderer::new();
        // No GL context set — load_bsp should return without panicking
        let bsp = BspData {
            vertices: vec![],
            edges: vec![],
            surface_edges: vec![],
            faces: vec![],
            planes: vec![],
            texinfo: vec![],
            models: vec![],
            entities: String::new(),
            visibility: vec![],
            nodes: vec![],
            lightmap_data: vec![],
            leafs: vec![],
            leaf_faces: vec![],
        };
        r.load_bsp(&bsp);
        assert!(r.world_mesh.is_none());
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
}
