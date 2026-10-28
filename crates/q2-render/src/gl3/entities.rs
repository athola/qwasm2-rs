//! GPU side of entity drawing: the MD2 and 2D programs, their stream
//! buffers and the entity draw loop (C `GL3_DrawEntitiesOnList`,
//! `GL3_DrawBrushModel`, `GL3_DrawAliasModel`).
//!
//! MD2 models outside the view frustum are skipped (C `CullAliasModel`).
//!
//! Not modelled relative to C: frustum culling of brush entities, the
//! `RF_WEAPONMODEL` gun FOV and left-hand mirroring, `RF_DEPTHHACK`,
//! `RF_SHELL_*` colour-only shells, `RF_IR_VISIBLE`, `RF_BEAM` beams, the
//! null-model placeholder, entity shadows, sprite (`.sp2`) models and
//! dynamic lights on brush models. Entities whose model
//! is none of the above are skipped.

use glow::HasContext;
use q2_render_api::{DLight, RefDef, RenderEntity};
use q2_shared::constants::{MAX_LIGHTSTYLES, RF_TRANSLUCENT, RF_VIEWERMODEL, RF_WEAPONMODEL};

use super::alias::{
    build_corners, clamp_frames, clamp_frames_independently, cull_alias, entity_skin,
    entity_vertices, AliasModel, FrustumPlane, ALIAS_VERTEX_FLOATS, MAX_SKINS,
};
use super::draw::{
    create_stream_buffer, draw_batches, draw_mesh, free_stream_buffer, BatchDraw, BatchFilter,
    MeshDraw, StreamBuffer,
};
use super::entity::{alias_matrix, brush_matrix};
use super::images::{ImageKind, ImageStore};
use super::light::LightTree;
use super::mat4_mul;
use super::model::{Model, ModelRegistry};
use super::pics::UI_VERTEX_FLOATS;
use super::shader::{
    compile_alias_program, compile_ui_program, AliasUniforms, UiUniforms, WorldUniforms,
};
use super::texture::{notexture, upload_rgba, FileLoader, CLAMPED_SAMPLING};
use crate::files::{md2::load_md2, palette::Palette};

/// Programs, stream buffers and fallback textures for entities and 2D.
pub struct ExtraGpu {
    pub alias_program: glow::Program,
    pub alias_uniforms: AliasUniforms,
    pub alias_stream: StreamBuffer,
    pub ui_program: glow::Program,
    pub ui_uniforms: UiUniforms,
    pub ui_stream: StreamBuffer,
    /// 1x1 white, sampled by 2D fills.
    pub white: glow::Texture,
    /// Checker used for models whose skin is missing.
    pub notexture: glow::Texture,
}

/// Every step is attempted; if any fails, the objects the others made are
/// freed before the first error is returned, so a retried `init` leaks none.
pub fn create_extra(gl: &glow::Context) -> Result<ExtraGpu, String> {
    let alias = compile_alias_program(gl);
    let ui = compile_ui_program(gl);
    // Layouts: alias is position(3) uv(2) colour(4); 2D is position(2) uv(2) colour(4).
    let alias_stream =
        create_stream_buffer(gl, ALIAS_VERTEX_FLOATS, &[(0, 3, 0), (1, 2, 3), (2, 4, 5)]);
    let ui_stream = create_stream_buffer(gl, UI_VERTEX_FLOATS, &[(0, 2, 0), (1, 2, 2), (2, 4, 4)]);
    let white = upload_rgba(gl, 1, 1, &[255; 4], &CLAMPED_SAMPLING);
    let checker = notexture();
    let notexture = upload_rgba(
        gl,
        checker.width,
        checker.height,
        &checker.rgba,
        &CLAMPED_SAMPLING,
    );
    match (alias, ui, alias_stream, ui_stream, white, notexture) {
        (
            Ok((alias_program, alias_uniforms)),
            Ok((ui_program, ui_uniforms)),
            Ok(alias_stream),
            Ok(ui_stream),
            Ok(white),
            Ok(notexture),
        ) => Ok(ExtraGpu {
            alias_program,
            alias_uniforms,
            alias_stream,
            ui_program,
            ui_uniforms,
            ui_stream,
            white,
            notexture,
        }),
        (alias, ui, alias_stream, ui_stream, white, notexture) => {
            let mut first_error = None;
            let mut note = |error: String| {
                first_error.get_or_insert(error);
            };
            for program in [alias.map(|(p, _)| p), ui.map(|(p, _)| p)] {
                match program {
                    // SAFETY: the program was linked on this `gl` above and
                    // is dropped here, never stored.
                    Ok(program) => unsafe { gl.delete_program(program) },
                    Err(error) => note(error),
                }
            }
            for stream in [alias_stream, ui_stream] {
                match stream {
                    Ok(stream) => free_stream_buffer(gl, stream),
                    Err(error) => note(error),
                }
            }
            for texture in [white, notexture] {
                match texture {
                    // SAFETY: the texture was created on this `gl` above and
                    // is dropped here, never stored.
                    Ok(texture) => unsafe { gl.delete_texture(texture) },
                    Err(error) => note(error),
                }
            }
            Err(first_error.unwrap_or_default())
        }
    }
}

pub fn free_extra(gl: &glow::Context, extra: ExtraGpu) {
    // SAFETY: every handle was created on this `gl` by `create_extra` and
    // `extra` is consumed, so none is used again.
    unsafe {
        gl.delete_program(extra.alias_program);
        gl.delete_program(extra.ui_program);
        gl.delete_texture(extra.white);
        gl.delete_texture(extra.notexture);
    }
    free_stream_buffer(gl, extra.alias_stream);
    free_stream_buffer(gl, extra.ui_stream);
}

/// Parse an `.md2` and register its skins (C `Mod_LoadAliasModel` plus the
/// `GL3_FindImage` calls of `RegisterModel`). A skin that fails to load stays
/// `None` and draws as the notexture checker; a bad model returns `None`.
pub fn load_alias_model(
    gl: Option<&glow::Context>,
    loader: &FileLoader,
    palette: Option<&Palette>,
    images: &mut ImageStore,
    name: &str,
) -> Option<AliasModel> {
    let bytes = loader(name)?;
    let md2 = match load_md2(&bytes) {
        Ok(md2) => md2,
        Err(error) => {
            tracing::warn!("GL3: bad model {name}: {error:?}");
            return None;
        }
    };
    let corners = build_corners(&md2);
    let skins = md2
        .skins
        .iter()
        .take(MAX_SKINS)
        .map(|skin| images.find(gl, loader, palette, skin, ImageKind::Skin))
        .collect();
    Some(AliasModel {
        md2,
        corners,
        skins,
    })
}

/// Everything an entity draw reads.
pub struct EntityScene<'a> {
    pub gl: &'a glow::Context,
    pub world: Option<&'a super::draw::WorldGpu>,
    pub world_program: glow::Program,
    pub world_uniforms: &'a WorldUniforms,
    pub extra: &'a ExtraGpu,
    pub models: &'a ModelRegistry<AliasModel>,
    pub images: &'a ImageStore,
    pub light: Option<&'a LightTree>,
    pub refdef: &'a RefDef,
    pub dlights: &'a [DLight],
    /// Light style brightness from `refdef`, padded to every style.
    pub styles: &'a [f32; MAX_LIGHTSTYLES],
    /// View frustum for culling MD2 models (C `frustum`).
    pub frustum: [FrustumPlane; 4],
    pub view_proj: [f32; 16],
    /// `view_proj` with the gun's fov, for `RF_WEAPONMODEL`.
    pub gun_view_proj: [f32; 16],
}

fn vec3(v: q2_shared::types::Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// Draw the opaque (`translucent == false`) or `RF_TRANSLUCENT` entities.
/// The world program must be bound; it is bound again on return. The viewer's
/// own model is never drawn here.
pub fn draw_entities(scene: &EntityScene, entities: &[RenderEntity], translucent: bool) {
    let wanted = |entity: &&RenderEntity| {
        entity.flags & RF_VIEWERMODEL == 0 && (entity.flags & RF_TRANSLUCENT != 0) == translucent
    };

    for entity in entities.iter().filter(wanted) {
        let Some(Model::Brush { submodel }) = scene.models.get(entity.model) else {
            continue;
        };
        let Some(world) = scene.world else {
            continue;
        };
        let Some(batches) = world.model_batches(*submodel) else {
            continue;
        };
        let matrix = brush_matrix(vec3(entity.origin), vec3(entity.angles));
        let view_proj = mat4_mul(&scene.view_proj, &matrix);
        let draw = BatchDraw {
            view_proj: &view_proj,
            time: scene.refdef.time,
            frame: entity.frame,
            filter: BatchFilter::All,
            translucent_entity: translucent,
            areabits: None,
        };
        draw_batches(scene.gl, scene.world_uniforms, world, batches, &draw);
    }

    let mut alias_program_bound = false;
    for entity in entities.iter().filter(wanted) {
        let Some(Model::Alias(model)) = scene.models.get(entity.model) else {
            continue;
        };
        let origin = vec3(entity.origin);
        let frame_count = model.md2.frames.len();
        let weapon = entity.flags & RF_WEAPONMODEL != 0;
        // C culls every MD2 but the gun, and the cull writes each frame
        // reset on its own back into the entity before the draw sees it.
        let (frame, oldframe) = if weapon {
            clamp_frames(entity.frame, entity.oldframe, frame_count)
        } else {
            clamp_frames_independently(entity.frame, entity.oldframe, frame_count)
        };
        let culled = match (model.md2.frames.get(frame), model.md2.frames.get(oldframe)) {
            (Some(frame), Some(oldframe)) => {
                !weapon && cull_alias(frame, oldframe, origin, vec3(entity.angles), &scene.frustum)
            }
            _ => true,
        };
        if culled {
            continue;
        }
        let entity = &RenderEntity {
            frame: i32::try_from(frame).unwrap_or(0),
            oldframe: i32::try_from(oldframe).unwrap_or(0),
            ..entity.clone()
        };
        let lit = || {
            scene.light.map_or([1.0; 3], |tree| {
                tree.light_point(origin, scene.styles, scene.dlights)
            })
        };
        let Some(vertices) = entity_vertices(model, entity, lit, scene.refdef.time) else {
            continue;
        };
        if !alias_program_bound {
            // SAFETY: the program was linked on `scene.gl` by `create_extra`.
            unsafe { scene.gl.use_program(Some(scene.extra.alias_program)) };
            alias_program_bound = true;
        }
        let texture = entity_skin(entity.skin, entity.skinnum, &model.skins)
            .and_then(|index| scene.images.get(index))
            .and_then(|image| image.texture)
            .unwrap_or(scene.extra.notexture);
        let matrix = alias_matrix(origin, vec3(entity.angles));
        let view_proj = if weapon {
            &scene.gun_view_proj
        } else {
            &scene.view_proj
        };
        let mvp = mat4_mul(view_proj, &matrix);
        let (near, far) = super::alias_depth_range(entity.flags);
        if (near, far) != (0.0, 1.0) {
            // SAFETY: state-only call on the live context.
            unsafe { scene.gl.depth_range_f32(near, far) };
        }
        draw_mesh(
            scene.gl,
            &scene.extra.alias_uniforms,
            &scene.extra.alias_stream,
            &MeshDraw {
                vertices: &vertices,
                vertex_count: i32::try_from(vertices.len() / ALIAS_VERTEX_FLOATS)
                    .unwrap_or(i32::MAX),
                texture: Some(texture),
                mvp: &mvp,
                blend: translucent,
            },
        );
        if (near, far) != (0.0, 1.0) {
            // SAFETY: state-only call on the live context.
            unsafe { scene.gl.depth_range_f32(0.0, 1.0) };
        }
    }
    if alias_program_bound {
        // SAFETY: the world program was linked on `scene.gl` by `init`.
        unsafe { scene.gl.use_program(Some(scene.world_program)) };
    }
}
