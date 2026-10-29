//! Particles, beams and sprites (C `GL3_DrawParticles`, `GL3_DrawBeam`,
//! `GL3_DrawSpriteModel` in gl3_main.c). Vertex building is GL-free so it
//! can be unit tested; the draws upload it to small stream buffers.

use glow::HasContext;
use q2_render_api::{Particle, RefDef, RenderEntity};
use q2_shared::constants::{RF_BEAM, RF_TRANSLUCENT, RF_VIEWERMODEL};
use q2_shared::types::Vec3f;

use super::alias::{angle_vectors, AliasModel};
use super::draw::{
    create_stream_buffer, fill_stream, free_stream_buffer, set_alpha_blend, StreamBuffer,
};
use super::images::{ImageKind, ImageStore};
use super::model::{Model, ModelRegistry};
use super::shader::{bind_samplers, link_program};
use super::texture::FileLoader;
use crate::files::{palette::Palette, sp2::load_sp2};

/// Floats per particle vertex: position(3), point size, distance to the
/// eye, colour(4) (C `part_vtx`).
pub const PARTICLE_FLOATS: usize = 9;
/// Floats per beam or sprite vertex: position(3), uv(2).
pub const FX_FLOATS: usize = 5;
/// C `gl3_particle_size` at a 480 pixel tall view.
const PARTICLE_SIZE: f32 = 40.0;
/// C `NUM_BEAM_SEGS`: sides of a beam's tube.
const NUM_BEAM_SEGS: usize = 6;

/// Particle vertices for one draw: each at its origin, sized for a view
/// `height` pixels tall, with its distance from the eye and its palette
/// colour. C ref: `GL3_DrawParticles`.
pub fn particle_vertices(
    particles: &[Particle],
    vieworg: Vec3f,
    height: i32,
    palette: &Palette,
) -> Vec<f32> {
    let size = PARTICLE_SIZE * height as f32 / 480.0;
    let mut out = Vec::with_capacity(particles.len() * PARTICLE_FLOATS);
    for p in particles {
        let [r, g, b] = palette.0[(p.color & 0xff) as usize];
        out.extend_from_slice(&p.origin.to_array());
        out.push(size);
        out.push((vieworg - p.origin).length());
        out.extend_from_slice(&[
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            p.alpha,
        ]);
    }
    out
}

/// A unit vector at right angles to `src` (C `PerpendicularVector`).
fn perpendicular(src: Vec3f) -> Vec3f {
    let arr = src.to_array();
    let mut pos = 0;
    let mut min = 1.0;
    for (i, &c) in arr.iter().enumerate() {
        if c.abs() < min {
            pos = i;
            min = c.abs();
        }
    }
    let mut axis = [0.0; 3];
    axis[pos] = 1.0;
    let axis = Vec3f::from_array(axis);
    // Project the axis onto the plane through the origin normal to `src`.
    let inv = 1.0 / src.dot(src);
    (axis - src * (src.dot(axis) * inv)).normalize_or_zero()
}

/// `point` turned `degrees` about the unit `dir` (C
/// `RotatePointAroundVector`).
fn rotate_around(dir: Vec3f, point: Vec3f, degrees: f32) -> Vec3f {
    let (sin, cos) = degrees.to_radians().sin_cos();
    point * cos + dir.cross(point) * sin + dir * dir.dot(point) * (1.0 - cos)
}

/// A beam's tube: `NUM_BEAM_SEGS` sides of width `frame` from `origin` to
/// `oldorigin`, as one triangle strip; none for a beam of no length. C
/// ref: `GL3_DrawBeam`.
pub fn beam_vertices(e: &RenderEntity) -> Option<Vec<f32>> {
    let direction = e.oldorigin - e.origin;
    if direction.length() == 0.0 {
        return None;
    }
    let normalized = direction.normalize();
    let perp = perpendicular(normalized) * (e.frame / 2) as f32;
    let starts: Vec<Vec3f> = (0..NUM_BEAM_SEGS)
        .map(|i| {
            rotate_around(normalized, perp, (360.0 / NUM_BEAM_SEGS as f32) * i as f32) + e.origin
        })
        .collect();
    let mut out = Vec::with_capacity(NUM_BEAM_SEGS * 4 * FX_FLOATS);
    for i in 0..NUM_BEAM_SEGS {
        let next = (i + 1) % NUM_BEAM_SEGS;
        for p in [
            starts[i],
            starts[i] + direction,
            starts[next],
            starts[next] + direction,
        ] {
            out.extend_from_slice(&p.to_array());
            out.extend_from_slice(&[0.0, 0.0]);
        }
    }
    Some(out)
}

/// A beam's colour: its `skinnum` palette entry at its alpha.
pub fn beam_color(e: &RenderEntity, palette: &Palette) -> [f32; 4] {
    let [r, g, b] = palette.0[(e.skinnum & 0xff) as usize];
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        e.alpha,
    ]
}

/// One sprite frame's size and the point it hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteFrameSize {
    pub width: i32,
    pub height: i32,
    pub origin_x: i32,
    pub origin_y: i32,
}

/// One sprite frame and its image-store index; `None` when its image did
/// not load, and the frame is then not drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFrame {
    pub size: SpriteFrameSize,
    pub image: Option<usize>,
}

/// A registered `.sp2` (C `dsprite_t` with its `GL3_FindImage` frames).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteModel {
    pub frames: Vec<SpriteFrame>,
}

/// Parse an `.sp2` and register its frame images (C `Mod_LoadSP2`); a bad
/// file returns `None`.
pub fn load_sprite_model(
    gl: Option<&glow::Context>,
    loader: &FileLoader,
    palette: Option<&Palette>,
    images: &mut ImageStore,
    name: &str,
) -> Option<SpriteModel> {
    let bytes = loader(name)?;
    let sprite = match load_sp2(&bytes) {
        Ok(sprite) => sprite,
        Err(error) => {
            tracing::warn!("GL3: bad sprite {name}: {error:?}");
            return None;
        }
    };
    let frames = sprite
        .frames
        .iter()
        .map(|frame| SpriteFrame {
            size: SpriteFrameSize {
                width: frame.width,
                height: frame.height,
                origin_x: frame.origin_x,
                origin_y: frame.origin_y,
            },
            image: images.find(gl, loader, palette, &frame.name, ImageKind::Sprite),
        })
        .collect();
    Some(SpriteModel { frames })
}

/// The sprite's quad facing the view (`right`, `up` are the view's), as a
/// fan: bottom-left, top-left, top-right, bottom-right. C ref:
/// `GL3_DrawSpriteModel`.
pub fn sprite_vertices(
    origin: Vec3f,
    frame: &SpriteFrameSize,
    right: Vec3f,
    up: Vec3f,
) -> [f32; 4 * FX_FLOATS] {
    let (w, h, ox, oy) = (
        frame.width as f32,
        frame.height as f32,
        frame.origin_x as f32,
        frame.origin_y as f32,
    );
    let corners = [
        (origin - up * oy - right * ox, [0.0, 1.0]),
        (origin + up * (h - oy) - right * ox, [0.0, 0.0]),
        (origin + up * (h - oy) + right * (w - ox), [1.0, 0.0]),
        (origin - up * oy + right * (w - ox), [1.0, 1.0]),
    ];
    let mut out = [0.0; 4 * FX_FLOATS];
    for (slot, (p, uv)) in out.chunks_exact_mut(FX_FLOATS).zip(corners) {
        slot[..3].copy_from_slice(&p.to_array());
        slot[3..].copy_from_slice(&uv);
    }
    out
}

/// C `e->frame %= psprite->numframes`, kept in range for negatives.
pub fn sprite_frame(frame: i32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    frame.rem_euclid(count as i32) as usize
}

// C `vertexSrcParticles` / `fragmentSrcParticles` (round, edge-faded
// points; `gl3_particle_fade_factor` 1.2).
pub const PARTICLE_VERTEX_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in float a_size;
layout(location = 2) in float a_dist;
layout(location = 3) in vec4 a_color;
uniform mat4 u_view_proj;
out vec4 v_color;
void main() {
    v_color = a_color;
    gl_Position = u_view_proj * vec4(a_position, 1.0);
    gl_PointSize = a_size / (a_dist * 0.1);
}
"#;

pub const PARTICLE_FRAGMENT_SRC: &str = r#"#version 300 es
precision highp float;
in vec4 v_color;
out vec4 o_color;
uniform float u_gamma;
void main() {
    vec2 offset = 2.0 * (gl_PointCoord - vec2(0.5, 0.5));
    float dist_squared = dot(offset, offset);
    if (dist_squared > 1.0) {
        discard;
    }
    o_color = vec4(pow(v_color.rgb, vec3(u_gamma)), v_color.a * min(1.0, 1.2 * (1.0 - dist_squared)));
}
"#;

// Sprites (C `fragmentSrc3Dsprite`/`SpriteAlpha`, alpha-tested when
// opaque) and beams (C `fragmentSrc3DcolorOnly`, over a white texel).
pub const FX_VERTEX_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec2 a_uv;
uniform mat4 u_view_proj;
out vec2 v_uv;
void main() {
    v_uv = a_uv;
    gl_Position = u_view_proj * vec4(a_position, 1.0);
}
"#;

pub const FX_FRAGMENT_SRC: &str = r#"#version 300 es
precision highp float;
precision highp int;
uniform sampler2D u_texture;
uniform vec4 u_color;
uniform int u_alpha_test;
uniform float u_gamma;
in vec2 v_uv;
out vec4 o_color;
void main() {
    vec4 texel = texture(u_texture, v_uv) * u_color;
    if (u_alpha_test != 0 && texel.a <= 0.666) {
        discard;
    }
    o_color = vec4(pow(texel.rgb, vec3(u_gamma)), texel.a);
}
"#;

/// Programs and stream buffers for particles, beams and sprites.
pub struct FxGpu {
    particle_program: glow::Program,
    particle_view_proj: Option<glow::UniformLocation>,
    particle_stream: StreamBuffer,
    fx_program: glow::Program,
    fx_view_proj: Option<glow::UniformLocation>,
    fx_color: Option<glow::UniformLocation>,
    fx_alpha_test: Option<glow::UniformLocation>,
    fx_stream: StreamBuffer,
}

impl FxGpu {
    /// Every program, for `u_gamma`.
    pub fn programs(&self) -> [glow::Program; 2] {
        [self.particle_program, self.fx_program]
    }
}

/// Every step is attempted; on failure the objects already made are freed
/// and the first error is returned.
pub fn create_fx(gl: &glow::Context) -> Result<FxGpu, String> {
    let particle_program = link_program(gl, PARTICLE_VERTEX_SRC, PARTICLE_FRAGMENT_SRC);
    let fx_program = link_program(gl, FX_VERTEX_SRC, FX_FRAGMENT_SRC);
    let particle_stream = create_stream_buffer(
        gl,
        PARTICLE_FLOATS,
        &[(0, 3, 0), (1, 1, 3), (2, 1, 4), (3, 4, 5)],
    );
    let fx_stream = create_stream_buffer(gl, FX_FLOATS, &[(0, 3, 0), (1, 2, 3)]);
    match (particle_program, fx_program, particle_stream, fx_stream) {
        (Ok(particle_program), Ok(fx_program), Ok(particle_stream), Ok(fx_stream)) => {
            bind_samplers(gl, fx_program, &["u_texture"]);
            // SAFETY: both programs were linked on `gl` above; the lookups
            // only read them.
            let (particle_view_proj, fx_view_proj, fx_color, fx_alpha_test) = unsafe {
                (
                    gl.get_uniform_location(particle_program, "u_view_proj"),
                    gl.get_uniform_location(fx_program, "u_view_proj"),
                    gl.get_uniform_location(fx_program, "u_color"),
                    gl.get_uniform_location(fx_program, "u_alpha_test"),
                )
            };
            Ok(FxGpu {
                particle_program,
                particle_view_proj,
                particle_stream,
                fx_program,
                fx_view_proj,
                fx_color,
                fx_alpha_test,
                fx_stream,
            })
        }
        (particle_program, fx_program, particle_stream, fx_stream) => {
            let mut first_error = None;
            for program in [particle_program, fx_program] {
                match program {
                    // SAFETY: the program was linked on this `gl` above and
                    // is dropped here, never stored.
                    Ok(program) => unsafe { gl.delete_program(program) },
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            for stream in [particle_stream, fx_stream] {
                match stream {
                    Ok(stream) => free_stream_buffer(gl, stream),
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            Err(first_error.unwrap_or_default())
        }
    }
}

pub fn free_fx(gl: &glow::Context, fx: FxGpu) {
    // SAFETY: both programs were created on this `gl` by `create_fx` and
    // `fx` is consumed, so neither is used again.
    unsafe {
        gl.delete_program(fx.particle_program);
        gl.delete_program(fx.fx_program);
    }
    free_stream_buffer(gl, fx.particle_stream);
    free_stream_buffer(gl, fx.fx_stream);
}

/// Everything the effect draws read.
pub struct FxScene<'a> {
    pub gl: &'a glow::Context,
    pub fx: &'a FxGpu,
    /// 1x1 white, sampled by beams.
    pub white: glow::Texture,
    pub models: &'a ModelRegistry<AliasModel>,
    pub images: &'a ImageStore,
    pub palette: Option<&'a Palette>,
    pub refdef: &'a RefDef,
    pub view_proj: &'a [f32; 16],
}

/// Draw `RF_BEAM` entities and sprites, then the particles, after every
/// other entity. C draws beams and sprites inside the entity passes and
/// the particles before the translucent world; here they go over both.
/// No program is bound on return.
pub fn draw_fx(scene: &FxScene, entities: &[RenderEntity], particles: &[Particle]) {
    let gl = scene.gl;
    let fx = scene.fx;
    // SAFETY: the program and its uniforms were made on `gl` by `create_fx`.
    unsafe {
        gl.use_program(Some(fx.fx_program));
        gl.uniform_matrix_4_f32_slice(fx.fx_view_proj.as_ref(), false, scene.view_proj);
        gl.active_texture(glow::TEXTURE0);
    }
    let (_, right, up) = angle_vectors(scene.refdef.viewangles.to_array());
    let (right, up) = (Vec3f::from_array(right), Vec3f::from_array(up));
    for entity in entities.iter().filter(|e| e.flags & RF_VIEWERMODEL == 0) {
        if entity.flags & RF_BEAM != 0 {
            let (Some(palette), Some(vertices)) = (scene.palette, beam_vertices(entity)) else {
                continue;
            };
            fx_draw(
                scene,
                &FxDraw {
                    vertices: &vertices,
                    mode: glow::TRIANGLE_STRIP,
                    texture: scene.white,
                    color: beam_color(entity, palette),
                    alpha_test: false,
                },
            );
            continue;
        }
        let Some(Model::Sprite(sprite)) = scene.models.get(entity.model) else {
            continue;
        };
        let Some(frame) = sprite
            .frames
            .get(sprite_frame(entity.frame, sprite.frames.len()))
        else {
            continue;
        };
        let Some(texture) = frame
            .image
            .and_then(|i| scene.images.get(i))
            .and_then(|image| image.texture)
        else {
            continue;
        };
        // C: translucent sprites blend at the entity's alpha, the rest are
        // alpha tested.
        let alpha = if entity.flags & RF_TRANSLUCENT != 0 {
            entity.alpha
        } else {
            1.0
        };
        fx_draw(
            scene,
            &FxDraw {
                vertices: &sprite_vertices(entity.origin, &frame.size, right, up),
                mode: glow::TRIANGLE_FAN,
                texture,
                color: [1.0, 1.0, 1.0, alpha],
                alpha_test: alpha == 1.0,
            },
        );
    }

    if let (Some(palette), false) = (scene.palette, particles.is_empty()) {
        let vertices = particle_vertices(
            particles,
            scene.refdef.vieworg,
            scene.refdef.height,
            palette,
        );
        fill_stream(gl, &fx.particle_stream, &vertices);
        // SAFETY: the program, uniform and stream belong to `gl`; the stream
        // was just filled with one vertex per particle.
        unsafe {
            gl.use_program(Some(fx.particle_program));
            gl.uniform_matrix_4_f32_slice(fx.particle_view_proj.as_ref(), false, scene.view_proj);
            gl.enable(glow::BLEND);
            set_alpha_blend(gl);
            gl.depth_mask(false);
            gl.draw_arrays(
                glow::POINTS,
                0,
                i32::try_from(particles.len()).unwrap_or(i32::MAX),
            );
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
            gl.bind_vertex_array(None);
        }
    }
    // SAFETY: unbinding the program is a state-only call.
    unsafe { gl.use_program(None) };
}

struct FxDraw<'a> {
    vertices: &'a [f32],
    mode: u32,
    texture: glow::Texture,
    color: [f32; 4],
    alpha_test: bool,
}

/// One beam or sprite with the fx program bound; anything not opaque
/// blends without writing depth, as C does.
fn fx_draw(scene: &FxScene, draw: &FxDraw) {
    let gl = scene.gl;
    fill_stream(gl, &scene.fx.fx_stream, draw.vertices);
    let blend = draw.color[3] < 1.0;
    let count = i32::try_from(draw.vertices.len() / FX_FLOATS).unwrap_or(i32::MAX);
    // SAFETY: the texture, stream and uniforms belong to `gl` and the bound
    // fx program; the stream was just filled with `count` vertices.
    unsafe {
        gl.uniform_4_f32_slice(scene.fx.fx_color.as_ref(), &draw.color);
        gl.uniform_1_i32(scene.fx.fx_alpha_test.as_ref(), i32::from(draw.alpha_test));
        gl.bind_texture(glow::TEXTURE_2D, Some(draw.texture));
        if blend {
            gl.enable(glow::BLEND);
            set_alpha_blend(gl);
            gl.depth_mask(false);
        }
        gl.draw_arrays(draw.mode, 0, count);
        if blend {
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
        }
        gl.bind_vertex_array(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey_palette() -> Palette {
        Palette(std::array::from_fn(|i| [i as u8, i as u8, 255 - i as u8]))
    }

    #[test]
    fn particles_grow_with_the_view_height_and_carry_their_distance_and_colour() {
        let particles = [Particle {
            origin: Vec3f::new(100.0, 0.0, 0.0),
            color: 0xe8,
            alpha: 0.5,
        }];
        let v = particle_vertices(&particles, Vec3f::ZERO, 960, &grey_palette());
        assert_eq!(v.len(), PARTICLE_FLOATS);
        assert_eq!(&v[..3], &[100.0, 0.0, 0.0]);
        assert_eq!(v[3], 80.0, "40 at 480 tall, scaled");
        assert_eq!(v[4], 100.0);
        assert_eq!(&v[5..], &[232.0 / 255.0, 232.0 / 255.0, 23.0 / 255.0, 0.5]);
    }

    fn beam(origin: Vec3f, oldorigin: Vec3f, width: i32) -> RenderEntity {
        RenderEntity {
            origin,
            oldorigin,
            frame: width,
            skinnum: 0xd0,
            alpha: 0.3,
            ..Default::default()
        }
    }

    #[test]
    fn a_beam_is_a_six_sided_tube_of_its_width_between_its_two_origins() {
        let v = beam_vertices(&beam(Vec3f::ZERO, Vec3f::new(0.0, 0.0, 100.0), 8)).unwrap();
        assert_eq!(v.len(), NUM_BEAM_SEGS * 4 * FX_FLOATS);
        for corner in v.chunks(FX_FLOATS) {
            let radius = (corner[0] * corner[0] + corner[1] * corner[1]).sqrt();
            assert!((radius - 4.0).abs() < 1e-4, "half the width: {radius}");
            assert!(corner[2] == 0.0 || corner[2] == 100.0);
        }
        // Each side: start, end, next start, next end.
        assert_eq!((v[2], v[FX_FLOATS + 2]), (0.0, 100.0));
    }

    #[test]
    fn a_beam_with_no_length_draws_nothing() {
        assert!(beam_vertices(&beam(Vec3f::ONE, Vec3f::ONE, 8)).is_none());
    }

    #[test]
    fn a_beam_takes_its_colour_from_the_palette_and_its_alpha() {
        assert_eq!(
            beam_color(&beam(Vec3f::ZERO, Vec3f::Z, 4), &grey_palette()),
            [208.0 / 255.0, 208.0 / 255.0, 47.0 / 255.0, 0.3]
        );
    }

    #[test]
    fn a_sprite_faces_the_view_around_its_frame_origin() {
        let right = Vec3f::new(0.0, -1.0, 0.0);
        let up = Vec3f::Z;
        let frame = SpriteFrameSize {
            width: 32,
            height: 16,
            origin_x: 16,
            origin_y: 8,
        };
        let v = sprite_vertices(Vec3f::new(10.0, 0.0, 0.0), &frame, right, up);
        // C: bottom-left, top-left, top-right, bottom-right, as a fan.
        assert_eq!(&v[0..5], &[10.0, 16.0, -8.0, 0.0, 1.0]);
        assert_eq!(&v[5..10], &[10.0, 16.0, 8.0, 0.0, 0.0]);
        assert_eq!(&v[10..15], &[10.0, -16.0, 8.0, 1.0, 0.0]);
        assert_eq!(&v[15..20], &[10.0, -16.0, -8.0, 1.0, 1.0]);
    }

    #[test]
    fn a_sprite_frame_wraps_round_its_frame_count() {
        assert_eq!(sprite_frame(7, 3), 1);
        assert_eq!(sprite_frame(-1, 3), 2);
        assert_eq!(sprite_frame(0, 0), 0);
    }

    #[test]
    fn the_fx_shaders_follow_gl3s_particle_and_sprite_shaders() {
        assert!(PARTICLE_VERTEX_SRC.contains("gl_PointSize = a_size / (a_dist * 0.1);"));
        assert!(PARTICLE_FRAGMENT_SRC.contains("if (dist_squared > 1.0)"));
        assert!(PARTICLE_FRAGMENT_SRC.contains("min(1.0, 1.2 * (1.0 - dist_squared))"));
        assert!(FX_FRAGMENT_SRC.contains("if (u_alpha_test != 0 && texel.a <= 0.666)"));
        for src in [PARTICLE_FRAGMENT_SRC, FX_FRAGMENT_SRC] {
            assert!(src.contains("uniform float u_gamma;"));
        }
    }
}
