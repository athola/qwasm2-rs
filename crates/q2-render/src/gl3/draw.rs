//! GPU objects for the world and sky box, and the draw loops that use them
//! (C `GL3_DrawWorld`, `GL3_DrawTextureChains`, `GL3_DrawAlphaSurfaces`,
//! `GL3_DrawSkyBox`).

use glow::HasContext;

use super::lightmap::{BLOCK_HEIGHT, BLOCK_WIDTH, MAX_LIGHTMAPS_PER_SURFACE};
use super::pics::{batch_runs, Quad, QuadImage, QUAD_VERTICES};
use super::shader::{
    AliasUniforms, UiUniforms, WorldUniforms, MODE_LIT, MODE_PLAIN, MODE_SKY, MODE_WARP,
};
use super::sky::{sky_box_vertices, SKY_TEX_ORDER, VERTEX_FLOATS as SKY_VERTEX_FLOATS};
use super::texture::{upload_rgba, Image, Sampling, WorldImages, CLAMPED_SAMPLING, WORLD_SAMPLING};
use super::world::{Batch, Shading, Translucency, WorldMesh, VERTEX_FLOATS};

/// Uploaded world model: vertex buffer, textures and the draw list.
pub struct WorldGpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    batches: Vec<Batch>,
    /// Batches of inline model `*n` at index `n - 1`.
    submodels: Vec<Vec<Batch>>,
    textures: Vec<glow::Texture>,
    /// Parallel to `textures`; lava is drawn at full brightness, water at half.
    is_lava: Vec<bool>,
    /// Layer `n` of page `p` at `p * MAX_LIGHTMAPS_PER_SURFACE + n`.
    lightmaps: Vec<glow::Texture>,
}

/// Six sky faces in file-suffix order plus the box geometry.
pub struct SkyGpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    faces: Vec<glow::Texture>,
}

/// World vertex attributes: position, texture uv, lightmap uv, styles,
/// normal.
const WORLD_ATTRIBUTES: [Attribute; 5] = [(0, 3, 0), (1, 2, 3), (2, 2, 5), (3, 4, 7), (4, 3, 11)];
/// Sky vertices stop after the lightmap uv the sky never reads.
const SKY_ATTRIBUTES: [Attribute; 3] = [(0, 3, 0), (1, 2, 3), (2, 2, 5)];

/// The bytes of `floats` as GL reads them (native byte order), without a
/// copy: MD2 and 2D vertices are re-uploaded every draw, and converting them
/// float by float dominated the browser frame time.
fn float_bytes(floats: &[f32]) -> &[u8] {
    bytemuck::cast_slice(floats)
}

/// C's `glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA)` for colour, but
/// destination alpha stays 1: desktop GL ignores framebuffer alpha, while a
/// WebGL canvas (alpha, premultiplied) would show the page through it.
pub(super) fn set_alpha_blend(gl: &glow::Context) {
    // SAFETY: state-only call on the live context `gl`.
    unsafe {
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
    }
}

/// Create a VAO/VBO for `stride_floats`-float vertices.
fn create_vertex_buffers(
    gl: &glow::Context,
    floats: &[f32],
    stride_floats: usize,
    attributes: &[Attribute],
) -> Result<(glow::VertexArray, glow::Buffer), String> {
    let bytes = float_bytes(floats);
    let stride = (stride_floats * 4) as i32;
    // SAFETY: VAO and VBO are created here on `gl`; `bytes` borrows `floats`
    // and is copied by `buffer_data_u8_slice`, and the attributes lie within
    // the caller's `stride_floats` layout.
    unsafe {
        let vao = gl.create_vertex_array()?;
        let vbo = match gl.create_buffer() {
            Ok(buffer) => buffer,
            Err(error) => {
                gl.delete_vertex_array(vao);
                return Err(error);
            }
        };
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
        for &(location, components, offset) in attributes {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_f32(
                location,
                components,
                glow::FLOAT,
                false,
                stride,
                (offset * 4) as i32,
            );
        }
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        Ok((vao, vbo))
    }
}

/// Upload textures one by one; on failure free what was created.
fn upload_all<'a>(
    gl: &glow::Context,
    images: impl Iterator<Item = (&'a Image, &'a Sampling)>,
) -> Result<Vec<glow::Texture>, String> {
    let mut textures = Vec::new();
    for (image, sampling) in images {
        match upload_rgba(gl, image.width, image.height, &image.rgba, sampling) {
            Ok(texture) => textures.push(texture),
            Err(error) => {
                delete_textures(gl, &textures);
                return Err(error);
            }
        }
    }
    Ok(textures)
}

fn delete_textures(gl: &glow::Context, textures: &[glow::Texture]) {
    // SAFETY: every handle was created by `upload_rgba` on this same `gl`
    // and the caller drops its copy of the list after this call.
    unsafe {
        for &texture in textures {
            gl.delete_texture(texture);
        }
    }
}

pub fn upload_world(
    gl: &glow::Context,
    images: &WorldImages,
    mesh: &WorldMesh,
) -> Result<WorldGpu, String> {
    // SAFETY: state-only call on the live context.
    unsafe { gl.active_texture(glow::TEXTURE0) };
    let textures = upload_all(
        gl,
        images.images.iter().map(|image| (image, &WORLD_SAMPLING)),
    )?;
    let page_images: Vec<Image> = mesh
        .lightmaps
        .pages
        .iter()
        .flat_map(|page| &page.layers)
        .map(|layer| Image {
            width: BLOCK_WIDTH,
            height: BLOCK_HEIGHT,
            rgba: layer.clone(),
            is_lava: false,
        })
        .collect();
    let lightmaps = match upload_all(gl, page_images.iter().map(|page| (page, &CLAMPED_SAMPLING))) {
        Ok(pages) => pages,
        Err(error) => {
            delete_textures(gl, &textures);
            return Err(error);
        }
    };
    match create_vertex_buffers(gl, &mesh.vertices, VERTEX_FLOATS, &WORLD_ATTRIBUTES) {
        Ok((vao, vbo)) => Ok(WorldGpu {
            vao,
            vbo,
            batches: mesh.batches.clone(),
            submodels: mesh.submodels.clone(),
            is_lava: images.images.iter().map(|image| image.is_lava).collect(),
            textures,
            lightmaps,
        }),
        Err(error) => {
            delete_textures(gl, &textures);
            delete_textures(gl, &lightmaps);
            Err(error)
        }
    }
}

pub fn upload_sky(gl: &glow::Context, faces: &[Image; 6]) -> Result<SkyGpu, String> {
    // SAFETY: state-only call on the live context.
    unsafe { gl.active_texture(glow::TEXTURE0) };
    let textures = upload_all(gl, faces.iter().map(|face| (face, &CLAMPED_SAMPLING)))?;
    match create_vertex_buffers(gl, &sky_box_vertices(), SKY_VERTEX_FLOATS, &SKY_ATTRIBUTES) {
        Ok((vao, vbo)) => Ok(SkyGpu {
            vao,
            vbo,
            faces: textures,
        }),
        Err(error) => {
            delete_textures(gl, &textures);
            Err(error)
        }
    }
}

pub fn free_world(gl: &glow::Context, world: WorldGpu) {
    delete_textures(gl, &world.textures);
    delete_textures(gl, &world.lightmaps);
    // SAFETY: `vao` and `vbo` were created on this `gl` by upload_world and
    // `world` is consumed, so neither handle is used again.
    unsafe {
        gl.delete_vertex_array(world.vao);
        gl.delete_buffer(world.vbo);
    }
}

pub fn free_sky(gl: &glow::Context, sky: SkyGpu) {
    delete_textures(gl, &sky.faces);
    // SAFETY: `vao` and `vbo` were created on this `gl` by upload_sky and
    // `sky` is consumed, so neither handle is used again.
    unsafe {
        gl.delete_vertex_array(sky.vao);
        gl.delete_buffer(sky.vbo);
    }
}

/// Texture scroll for a batch (C `GL3_EmitWaterPolys` / `GL3_DrawGLFlowingPoly`
/// via the `scroll` uniform). `-64` replaces an exact `0` like C does.
pub fn batch_scroll(shading: Shading, flowing: bool, time: f32) -> f32 {
    let period_fraction = match (shading, flowing) {
        (Shading::Warp, true) => time * 0.5,
        (Shading::Warp, false) => return 0.0,
        (_, true) => time / 40.0,
        (_, false) => return 0.0,
    };
    let scroll = -64.0 * (period_fraction - period_fraction.trunc());
    if scroll == 0.0 {
        -64.0
    } else {
        scroll
    }
}

/// Draw the six sky faces around the eye; `view_proj` must exclude translation.
pub fn draw_sky(gl: &glow::Context, uniforms: &WorldUniforms, sky: &SkyGpu, view_proj: &[f32; 16]) {
    // SAFETY: `sky.vao` and the face textures were created on `gl` and are
    // owned by `sky`; uniform locations belong to the currently bound program.
    unsafe {
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.uniform_matrix_4_f32_slice(uniforms.view_proj.as_ref(), false, view_proj);
        gl.uniform_1_i32(uniforms.mode.as_ref(), MODE_SKY);
        gl.uniform_1_f32(uniforms.scroll.as_ref(), 0.0);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_vertex_array(Some(sky.vao));
        for (face, &image_index) in SKY_TEX_ORDER.iter().enumerate() {
            gl.bind_texture(glow::TEXTURE_2D, sky.faces.get(image_index).copied());
            gl.draw_arrays(glow::TRIANGLES, (face * 6) as i32, 6);
        }
        gl.bind_vertex_array(None);
        gl.enable(glow::DEPTH_TEST);
        gl.enable(glow::CULL_FACE);
    }
}

/// Which batches of a model a draw call covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchFilter {
    Opaque,
    Translucent,
    All,
}

impl BatchFilter {
    fn includes(self, translucency: Translucency) -> bool {
        match self {
            BatchFilter::Opaque => translucency == Translucency::Opaque,
            BatchFilter::Translucent => translucency != Translucency::Opaque,
            BatchFilter::All => true,
        }
    }
}

/// One world or brush-entity draw.
pub struct BatchDraw<'a> {
    pub view_proj: &'a [f32; 16],
    pub time: f32,
    /// Texture animation frame (C `R_TextureAnimation`).
    pub frame: i32,
    pub filter: BatchFilter,
    /// An `RF_TRANSLUCENT` brush entity: every batch blends, none writes depth.
    pub translucent_entity: bool,
    /// The view's area bits (see `Batch::in_visible_area`); `None` for
    /// brush entities, which the server already culls by area.
    pub areabits: Option<&'a [u8]>,
}

impl WorldGpu {
    /// Batches of model `model` (0 is the world, `n` is `*n`).
    pub fn model_batches(&self, model: usize) -> Option<&[Batch]> {
        match model {
            0 => Some(&self.batches),
            n => self.submodels.get(n - 1).map(Vec::as_slice),
        }
    }
}

/// Whether a batch is drawn blended and without depth writes: translucent
/// faces always, and every face of an `RF_TRANSLUCENT` brush entity (C
/// `R_DrawInlineBModel` enables blending inside the translucent entity pass,
/// which runs under `glDepthMask(0)`; opaque faces keep alpha 1).
fn batch_blends(translucency: Translucency, translucent_entity: bool) -> bool {
    translucent_entity || translucency != Translucency::Opaque
}

/// Draw the selected batches of one model. Translucent batches blend and
/// write no depth. `batches` must come from `world`.
pub fn draw_batches(
    gl: &glow::Context,
    uniforms: &WorldUniforms,
    world: &WorldGpu,
    batches: &[Batch],
    draw: &BatchDraw,
) {
    // SAFETY: `world.vao`, textures and lightmap pages were created on `gl`
    // and are owned by `world`; batch ranges were produced together with the
    // vertex buffer by `build_world_mesh`; uniform locations belong to the
    // currently bound program.
    unsafe {
        gl.uniform_matrix_4_f32_slice(uniforms.view_proj.as_ref(), false, draw.view_proj);
        gl.uniform_1_f32(uniforms.time.as_ref(), draw.time);
        gl.bind_vertex_array(Some(world.vao));
        let (mut blending, mut writing_depth) = (false, true);
        for batch in batches.iter().filter(|batch| {
            draw.filter.includes(batch.translucency) && batch.in_visible_area(draw.areabits)
        }) {
            let blend = batch_blends(batch.translucency, draw.translucent_entity);
            if blend != blending {
                if blend {
                    gl.enable(glow::BLEND);
                    set_alpha_blend(gl);
                } else {
                    gl.disable(glow::BLEND);
                }
                blending = blend;
            }
            // Blended draws never write depth (C draws them under glDepthMask(0)).
            if writing_depth == blend {
                gl.depth_mask(!blend);
                writing_depth = !blend;
            }
            let mode = match batch.shading {
                Shading::Lit => MODE_LIT,
                Shading::Plain => MODE_PLAIN,
                Shading::Warp => MODE_WARP,
            };
            gl.uniform_1_i32(uniforms.mode.as_ref(), mode);
            gl.uniform_1_f32(uniforms.alpha.as_ref(), batch.translucency.alpha());
            gl.uniform_1_f32(
                uniforms.scroll.as_ref(),
                batch_scroll(batch.shading, batch.flowing, draw.time),
            );
            let image = batch.image(draw.frame);
            let lava = world.is_lava.get(image).copied().unwrap_or(false);
            gl.uniform_1_f32(uniforms.turb_light.as_ref(), if lava { 1.0 } else { 0.5 });

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, world.textures.get(image).copied());
            if batch.shading == Shading::Lit {
                let first = batch.lightmap_page * MAX_LIGHTMAPS_PER_SURFACE;
                for (unit, layer) in
                    (glow::TEXTURE1..).zip(first..first + MAX_LIGHTMAPS_PER_SURFACE)
                {
                    gl.active_texture(unit);
                    gl.bind_texture(glow::TEXTURE_2D, world.lightmaps.get(layer).copied());
                }
                gl.active_texture(glow::TEXTURE0);
            }
            gl.draw_arrays(glow::TRIANGLES, batch.first_vertex, batch.vertex_count);
        }
        if blending {
            gl.disable(glow::BLEND);
        }
        if !writing_depth {
            gl.depth_mask(true);
        }
        gl.bind_vertex_array(None);
    }
}

/// A VAO/VBO pair refilled every draw (C `GL_STREAM_DRAW` buffers).
pub struct StreamBuffer {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
}

/// One float attribute of a stream vertex: location, components, float offset.
pub type Attribute = (u32, i32, usize);

pub fn create_stream_buffer(
    gl: &glow::Context,
    stride_floats: usize,
    attributes: &[Attribute],
) -> Result<StreamBuffer, String> {
    let stride = i32::try_from(stride_floats * 4).map_err(|e| e.to_string())?;
    // SAFETY: VAO and VBO are created here on `gl`; attribute offsets and
    // stride come from the caller's float layout, whose last attribute ends
    // at `stride_floats`; no data is read until a draw fills the buffer.
    unsafe {
        let vao = gl.create_vertex_array()?;
        let vbo = match gl.create_buffer() {
            Ok(buffer) => buffer,
            Err(error) => {
                gl.delete_vertex_array(vao);
                return Err(error);
            }
        };
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        for &(location, components, offset) in attributes {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_f32(
                location,
                components,
                glow::FLOAT,
                false,
                stride,
                i32::try_from(offset * 4).unwrap_or(0),
            );
        }
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        Ok(StreamBuffer { vao, vbo })
    }
}

pub fn free_stream_buffer(gl: &glow::Context, stream: StreamBuffer) {
    // SAFETY: both objects were created on this `gl` by
    // `create_stream_buffer` and `stream` is consumed here.
    unsafe {
        gl.delete_vertex_array(stream.vao);
        gl.delete_buffer(stream.vbo);
    }
}

/// Replace the stream's contents and leave its VAO bound.
pub(super) fn fill_stream(gl: &glow::Context, stream: &StreamBuffer, floats: &[f32]) {
    let bytes = float_bytes(floats);
    // SAFETY: `stream` objects belong to `gl`; `bytes` borrows `floats` and
    // `buffer_data_u8_slice` copies it before returning.
    unsafe {
        gl.bind_vertex_array(Some(stream.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(stream.vbo));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
    }
}

/// One interpolated MD2 draw (the MD2 program is already bound).
pub struct MeshDraw<'a> {
    /// Vertices in the MD2 stream layout, three per triangle.
    pub vertices: &'a [f32],
    pub vertex_count: i32,
    pub texture: Option<glow::Texture>,
    pub mvp: &'a [f32; 16],
    pub blend: bool,
}

/// Draw one MD2 mesh under the world's cull state: MD2 triangles wind like
/// world faces, and C keeps `GL_FRONT` culling for them too.
pub fn draw_mesh(
    gl: &glow::Context,
    uniforms: &AliasUniforms,
    stream: &StreamBuffer,
    draw: &MeshDraw,
) {
    fill_stream(gl, stream, draw.vertices);
    // SAFETY: the texture and the stream belong to `gl`; the stream was just
    // filled with `vertex_count` vertices in the MD2 layout; the uniform
    // location belongs to the currently bound MD2 program.
    unsafe {
        gl.uniform_matrix_4_f32_slice(uniforms.mvp.as_ref(), false, draw.mvp);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, draw.texture);
        if draw.blend {
            gl.enable(glow::BLEND);
            set_alpha_blend(gl);
            gl.depth_mask(false);
        }
        gl.draw_arrays(glow::TRIANGLES, 0, draw.vertex_count);
        if draw.blend {
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
        }
        gl.bind_vertex_array(None);
    }
}

/// Where and how the queued 2D quads are drawn.
pub struct UiTarget<'a> {
    pub program: glow::Program,
    pub uniforms: &'a UiUniforms,
    pub stream: &'a StreamBuffer,
    pub ortho: &'a [f32; 16],
    pub canvas: (i32, i32),
}

/// Draw the queued 2D quads in order, on top of everything, then restore the
/// 3D state (depth test and culling on, blending off, no program).
pub fn draw_ui(
    gl: &glow::Context,
    target: &UiTarget,
    quads: &[Quad],
    texture_of: impl Fn(QuadImage) -> Option<glow::Texture>,
) {
    // SAFETY: `target.program` was linked on `gl`; these are state-only calls.
    unsafe {
        gl.viewport(0, 0, target.canvas.0, target.canvas.1);
        gl.use_program(Some(target.program));
        gl.uniform_matrix_4_f32_slice(target.uniforms.ortho.as_ref(), false, target.ortho);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::CULL_FACE);
        gl.enable(glow::BLEND);
        set_alpha_blend(gl);
        gl.active_texture(glow::TEXTURE0);
    }
    let mut floats = Vec::new();
    for (image, first_vertex, vertex_count) in batch_runs(quads) {
        let first_quad = first_vertex / QUAD_VERTICES;
        let run = &quads[first_quad..first_quad + vertex_count / QUAD_VERTICES];
        floats.clear();
        floats.extend(run.iter().flat_map(|quad| quad.vertices));
        fill_stream(gl, target.stream, &floats);
        // SAFETY: the texture belongs to `gl` (or is unbound); the stream
        // was just filled with `vertex_count` vertices in the 2D layout.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, texture_of(image));
            gl.draw_arrays(
                glow::TRIANGLES,
                0,
                i32::try_from(vertex_count).unwrap_or(i32::MAX),
            );
        }
    }
    // SAFETY: state-only calls restoring what init set up.
    unsafe {
        gl.bind_vertex_array(None);
        gl.disable(glow::BLEND);
        gl.enable(glow::CULL_FACE);
        gl.enable(glow::DEPTH_TEST);
        gl.use_program(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_bytes_are_the_native_bytes_of_each_float_in_order() {
        let floats = [1.0f32, -2.5, f32::MAX];
        let expected: Vec<u8> = floats.iter().flat_map(|f| f.to_ne_bytes()).collect();
        assert_eq!(float_bytes(&floats), expected.as_slice());
        assert!(float_bytes(&[]).is_empty());
    }

    #[test]
    fn translucent_brush_entities_blend_every_batch_like_c() {
        assert!(!batch_blends(Translucency::Opaque, false));
        assert!(batch_blends(Translucency::Opaque, true));
        for translucency in [
            Translucency::Opaque,
            Translucency::Third,
            Translucency::TwoThirds,
        ] {
            assert!(batch_blends(translucency, true));
        }
        assert!(batch_blends(Translucency::Third, false));
    }

    #[test]
    fn non_flowing_surfaces_do_not_scroll() {
        assert_eq!(batch_scroll(Shading::Lit, false, 12.3), 0.0);
        assert_eq!(batch_scroll(Shading::Warp, false, 12.3), 0.0);
    }

    #[test]
    fn flowing_warp_scrolls_64_units_per_two_seconds() {
        // time 0.5 -> fraction 0.25 -> -16
        assert_eq!(batch_scroll(Shading::Warp, true, 0.5), -16.0);
        // time 3.0 -> 1.5 -> fraction 0.5 -> -32
        assert_eq!(batch_scroll(Shading::Warp, true, 3.0), -32.0);
    }

    #[test]
    fn flowing_lit_uses_the_40_second_period() {
        // time 10 -> 0.25 -> -16
        assert_eq!(batch_scroll(Shading::Lit, true, 10.0), -16.0);
    }

    #[test]
    fn exact_period_boundary_scrolls_a_full_64_like_c() {
        assert_eq!(batch_scroll(Shading::Warp, true, 0.0), -64.0);
        assert_eq!(batch_scroll(Shading::Warp, true, 2.0), -64.0);
        assert_eq!(batch_scroll(Shading::Plain, true, 40.0), -64.0);
    }
}
