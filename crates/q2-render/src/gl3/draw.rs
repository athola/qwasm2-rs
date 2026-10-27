//! GPU objects for the world and sky box, and the draw loops that use them
//! (C `GL3_DrawWorld`, `GL3_DrawTextureChains`, `GL3_DrawAlphaSurfaces`,
//! `GL3_DrawSkyBox`).

use glow::HasContext;

use super::lightmap::{BLOCK_HEIGHT, BLOCK_WIDTH};
use super::shader::{WorldUniforms, MODE_LIT, MODE_PLAIN, MODE_SKY, MODE_WARP};
use super::sky::{sky_box_vertices, SKY_TEX_ORDER, VERTEX_FLOATS};
use super::texture::{upload_rgba, Image, Sampling, WorldImages, CLAMPED_SAMPLING, WORLD_SAMPLING};
use super::world::{Batch, Shading, Translucency, WorldMesh};

/// Uploaded world model: vertex buffer, textures and the draw list.
pub struct WorldGpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    batches: Vec<Batch>,
    textures: Vec<glow::Texture>,
    /// Parallel to `textures`; lava is drawn at full brightness, water at half.
    is_lava: Vec<bool>,
    lightmaps: Vec<glow::Texture>,
}

/// Six sky faces in file-suffix order plus the box geometry.
pub struct SkyGpu {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    faces: Vec<glow::Texture>,
}

/// Create a VAO/VBO for [`VERTEX_FLOATS`]-float vertices.
fn create_vertex_buffers(
    gl: &glow::Context,
    floats: &[f32],
) -> Result<(glow::VertexArray, glow::Buffer), String> {
    let bytes: Vec<u8> = floats
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let stride = (VERTEX_FLOATS * 4) as i32;
    // SAFETY: VAO and VBO are created here on `gl`; `bytes` is a live slice
    // consumed by `buffer_data_u8_slice` before it is dropped, and the
    // attribute offsets (0, 12, 20) and stride (28) match the 7-float layout.
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
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 12);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, 20);
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
        .map(|page| Image {
            width: BLOCK_WIDTH,
            height: BLOCK_HEIGHT,
            rgba: page.clone(),
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
    match create_vertex_buffers(gl, &mesh.vertices) {
        Ok((vao, vbo)) => Ok(WorldGpu {
            vao,
            vbo,
            batches: mesh.batches.clone(),
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
    match create_vertex_buffers(gl, &sky_box_vertices()) {
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

/// Draw opaque batches, then translucent ones with blending and no depth writes.
pub fn draw_world(
    gl: &glow::Context,
    uniforms: &WorldUniforms,
    world: &WorldGpu,
    view_proj: &[f32; 16],
    time: f32,
) {
    // SAFETY: `world.vao`, textures and lightmap pages were created on `gl`
    // and are owned by `world`; batch ranges were produced together with the
    // vertex buffer by `build_world_mesh`; uniform locations belong to the
    // currently bound program.
    unsafe {
        gl.uniform_matrix_4_f32_slice(uniforms.view_proj.as_ref(), false, view_proj);
        gl.uniform_1_f32(uniforms.time.as_ref(), time);
        gl.bind_vertex_array(Some(world.vao));
        let mut blending = false;
        for batch in &world.batches {
            if batch.translucency != Translucency::Opaque && !blending {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.depth_mask(false);
                blending = true;
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
                batch_scroll(batch.shading, batch.flowing, time),
            );
            let lava = world.is_lava.get(batch.image).copied().unwrap_or(false);
            gl.uniform_1_f32(uniforms.turb_light.as_ref(), if lava { 1.0 } else { 0.5 });

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, world.textures.get(batch.image).copied());
            if batch.shading == Shading::Lit {
                gl.active_texture(glow::TEXTURE1);
                gl.bind_texture(
                    glow::TEXTURE_2D,
                    world.lightmaps.get(batch.lightmap_page).copied(),
                );
                gl.active_texture(glow::TEXTURE0);
            }
            gl.draw_arrays(glow::TRIANGLES, batch.first_vertex, batch.vertex_count);
        }
        if blending {
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
        }
        gl.bind_vertex_array(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
