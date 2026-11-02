//! GPU side of entity drawing: the MD2 and 2D programs, their stream
//! buffers and the entity draw loop (C `GL3_DrawEntitiesOnList`,
//! `GL3_DrawBrushModel`, `GL3_DrawAliasModel`).
//!
//! MD2 models outside the view frustum are skipped (C `CullAliasModel`).
//!
//! Brush entities are lit by the frame's dynamic lights through their model
//! matrix, as C `vertexSrc3Dlm` does with `transModel`.
//!
//! Not modelled relative to C: frustum culling of brush entities,
//! `RF_IR_VISIBLE`, the null-model placeholder and entity shadows. Beams and sprites are drawn in the same passes
//! through `fx`. Entities whose model is none of the above are skipped.

use glow::HasContext;
use q2_render_api::{DLight, RefDef, RenderEntity};
use q2_shared::constants::{
    MAX_LIGHTSTYLES, RF_BEAM, RF_TRANSLUCENT, RF_VIEWERMODEL, RF_WEAPONMODEL,
};

use super::alias::{
    build_corners, clamp_frames, clamp_frames_independently, cull_alias, entity_skin,
    entity_vertices, AliasModel, FrustumPlane, ALIAS_VERTEX_FLOATS, MAX_SKINS,
};
use super::draw::{
    create_stream_buffer, draw_batches, draw_mesh, free_stream_buffer, Attribute, BatchDraw,
    BatchFilter, MeshDraw, StreamBuffer,
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
use crate::files::{
    md2::{load_md2, Md2Model},
    palette::Palette,
};

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

/// MD2 vertices: position(3), uv(2), colour(4).
const ALIAS_ATTRIBUTES: [Attribute; 3] = [(0, 3, 0), (1, 2, 3), (2, 4, 5)];
/// 2D vertices: position(2), uv(2), colour(4).
const UI_ATTRIBUTES: [Attribute; 3] = [(0, 2, 0), (1, 2, 2), (2, 4, 4)];

/// Every step is attempted; if any fails, the objects the others made are
/// freed before the first error is returned, so a retried `init` leaks none.
pub fn create_extra(gl: &glow::Context) -> Result<ExtraGpu, String> {
    let alias = compile_alias_program(gl);
    let ui = compile_ui_program(gl);
    let alias_stream = create_stream_buffer(gl, ALIAS_VERTEX_FLOATS, &ALIAS_ATTRIBUTES);
    let ui_stream = create_stream_buffer(gl, UI_VERTEX_FLOATS, &UI_ATTRIBUTES);
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
    /// C cvar `hand`: 1 mirrors the gun, 2 hides it.
    pub hand: i32,
    /// Beam and sprite programs; without them neither is drawn.
    pub fx: Option<&'a super::fx::FxGpu>,
    /// Beam colours.
    pub palette: Option<&'a Palette>,
    /// The view's right and up, which sprites face.
    pub view_right_up: (q2_shared::types::Vec3f, q2_shared::types::Vec3f),
}

fn vec3(v: q2_shared::types::Vec3f) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// How an entity is drawn (C `GL3_DrawEntitiesOnList`'s dispatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityDraw {
    Beam,
    /// Inline model `n` of the world.
    Brush(usize),
    Alias,
    Sprite,
}

/// The opaque (`translucent == false`) or `RF_TRANSLUCENT` entities in
/// list order, as C's two passes take them: a beam whatever its model,
/// the rest by model type. The viewer's own model and modelless entities
/// (C `GL3_DrawNullModel`) are left out.
pub fn entity_draws<'a, A>(
    entities: &'a [RenderEntity],
    models: &'a ModelRegistry<A>,
    translucent: bool,
) -> impl Iterator<Item = (&'a RenderEntity, EntityDraw)> + 'a {
    entities
        .iter()
        .filter(move |e| {
            e.flags & RF_VIEWERMODEL == 0 && (e.flags & RF_TRANSLUCENT != 0) == translucent
        })
        .filter_map(|e| {
            if e.flags & RF_BEAM != 0 {
                return Some((e, EntityDraw::Beam));
            }
            let kind = match models.get(e.model)? {
                Model::Brush { submodel } => EntityDraw::Brush(*submodel),
                Model::Alias(_) => EntityDraw::Alias,
                Model::Sprite(_) => EntityDraw::Sprite,
            };
            Some((e, kind))
        })
}

/// The program an entity draw needs bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    World,
    Alias,
    Fx,
}

/// Draw one entity pass in list order (C `GL3_DrawEntitiesOnList`). The
/// world program must be bound; it is bound again on return.
pub fn draw_entities(scene: &EntityScene, entities: &[RenderEntity], translucent: bool) {
    let gl = scene.gl;
    let mut bound = Bound::World;
    for (entity, kind) in entity_draws(entities, scene.models, translucent) {
        let want = match kind {
            EntityDraw::Brush(_) => Bound::World,
            EntityDraw::Alias => Bound::Alias,
            EntityDraw::Beam | EntityDraw::Sprite => Bound::Fx,
        };
        if want == Bound::Fx && scene.fx.is_none() {
            continue;
        }
        if want != bound {
            match (want, scene.fx) {
                // SAFETY: the world program was linked on `gl` by init.
                (Bound::World, _) => unsafe { gl.use_program(Some(scene.world_program)) },
                // SAFETY: the alias program was linked on `gl` by `create_extra`.
                (Bound::Alias, _) => unsafe { gl.use_program(Some(scene.extra.alias_program)) },
                (Bound::Fx, Some(fx)) => super::fx::bind(gl, fx, &scene.view_proj),
                (Bound::Fx, None) => {}
            }
            bound = want;
        }
        match kind {
            // The SURF_TRANS33/66 faces wait for `draw_brush_alpha_faces`.
            EntityDraw::Brush(submodel) => {
                draw_brush(scene, entity, submodel, BatchFilter::Opaque, translucent)
            }
            EntityDraw::Alias => draw_alias(scene, entity, translucent),
            EntityDraw::Beam => {
                if let (Some(fx), Some(palette)) = (scene.fx, scene.palette) {
                    super::fx::draw_beam(gl, fx, scene.extra.white, palette, entity);
                }
            }
            EntityDraw::Sprite => {
                if let (Some(fx), Some(Model::Sprite(sprite))) =
                    (scene.fx, scene.models.get(entity.model))
                {
                    super::fx::draw_sprite(
                        gl,
                        fx,
                        (scene.images, scene.extra.notexture),
                        sprite,
                        entity,
                        scene.view_right_up,
                        translucent,
                    );
                }
            }
        }
    }
    if bound != Bound::World {
        // SAFETY: the world program was linked on `gl` by `init`.
        unsafe { gl.use_program(Some(scene.world_program)) };
    }
}

/// The `filter` faces of a brush entity. C ref: `GL3_DrawBrushModel`.
fn draw_brush(
    scene: &EntityScene,
    entity: &RenderEntity,
    submodel: usize,
    filter: BatchFilter,
    translucent: bool,
) {
    let Some(world) = scene.world else {
        return;
    };
    let Some(batches) = world.model_batches(submodel) else {
        return;
    };
    let matrix = brush_matrix(vec3(entity.origin), vec3(entity.angles));
    let draw = BatchDraw {
        view_proj: &scene.view_proj,
        model: &matrix,
        time: scene.refdef.time,
        frame: entity.frame,
        filter,
        translucent_entity: translucent,
        areabits: None,
    };
    draw_batches(scene.gl, scene.world_uniforms, world, batches, &draw);
}

/// The SURF_TRANS33/66 faces of every brush entity, in the final alpha pass
/// after all entities (C `DrawInlineBModel` chains them on
/// `gl3_alpha_surfaces`, whatever the entity's own flags). Deliberate
/// deviation: C draws that chain with the identity matrix, so a moved or
/// rotated brush entity's glass stays where the entity spawned; here it
/// keeps the entity's matrix. The world program must be bound.
pub fn draw_brush_alpha_faces(scene: &EntityScene, entities: &[RenderEntity]) {
    for (entity, submodel) in brush_entities(entities, scene.models) {
        draw_brush(scene, entity, submodel, BatchFilter::Translucent, false);
    }
}

/// Every brush entity with its inline model, whatever its translucency;
/// beams and the viewer's own model are left out as in the entity passes.
fn brush_entities<'a, A>(
    entities: &'a [RenderEntity],
    models: &'a ModelRegistry<A>,
) -> impl Iterator<Item = (&'a RenderEntity, usize)> + 'a {
    entities
        .iter()
        .filter(|e| e.flags & (RF_VIEWERMODEL | RF_BEAM) == 0)
        .filter_map(|e| match models.get(e.model)? {
            Model::Brush { submodel } => Some((e, *submodel)),
            _ => None,
        })
}

/// The face culled while drawing an MD2, or `None` when it is not drawn:
/// only the gun follows C cvar `hand`. C ref: `GL3_DrawAliasModel`.
fn alias_cull_face(flags: i32, hand: i32) -> Option<u32> {
    if flags & RF_WEAPONMODEL != 0 {
        super::gun_cull_face(hand)
    } else {
        Some(super::CULL_FACE)
    }
}

/// The frames an MD2 draws, or `None` when it is culled. C culls every
/// MD2 but the gun, and the cull writes each frame reset on its own back
/// into the entity before the draw sees it; the gun only gets the draw's
/// check, which resets both. C ref: `CullAliasModel`, `GL3_DrawAliasModel`.
fn alias_frames(
    md2: &Md2Model,
    entity: &RenderEntity,
    frustum: &[FrustumPlane; 4],
) -> Option<(usize, usize)> {
    let weapon = entity.flags & RF_WEAPONMODEL != 0;
    let frame_count = md2.frames.len();
    let (frame, oldframe) = if weapon {
        clamp_frames(entity.frame, entity.oldframe, frame_count)
    } else {
        clamp_frames_independently(entity.frame, entity.oldframe, frame_count)
    };
    let culled = match (md2.frames.get(frame), md2.frames.get(oldframe)) {
        (Some(this), Some(old)) => {
            let (origin, angles) = (vec3(entity.origin), vec3(entity.angles));
            !weapon && cull_alias(this, old, origin, angles, frustum)
        }
        _ => true,
    };
    (!culled).then_some((frame, oldframe))
}

/// A colour shell samples white; a skin whose image did not load, or
/// none, draws as the notexture checker (C `gl3_notexture` fallback).
fn alias_texture(
    entity: &RenderEntity,
    skins: &[Option<usize>],
    images: &ImageStore,
    (white, notexture): (glow::Texture, glow::Texture),
) -> glow::Texture {
    if super::alias::shell_light(entity.flags).is_some() {
        return white;
    }
    entity_skin(entity.skin, entity.skinnum, skins)
        .and_then(|index| images.get(index))
        .and_then(|image| image.texture)
        .unwrap_or(notexture)
}

/// One MD2 with the alias program bound. C ref: `GL3_DrawAliasModel`.
fn draw_alias(scene: &EntityScene, entity: &RenderEntity, translucent: bool) {
    let Some(Model::Alias(model)) = scene.models.get(entity.model) else {
        return;
    };
    let origin = vec3(entity.origin);
    let weapon = entity.flags & RF_WEAPONMODEL != 0;
    let Some(cull_face) = alias_cull_face(entity.flags, scene.hand) else {
        return;
    };
    let Some((frame, oldframe)) = alias_frames(&model.md2, entity, &scene.frustum) else {
        return;
    };
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
        return;
    };
    let shell = super::alias::shell_light(entity.flags).is_some();
    let texture = alias_texture(
        entity,
        &model.skins,
        scene.images,
        (scene.extra.white, scene.extra.notexture),
    );
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
    let mirrored = cull_face != super::CULL_FACE;
    if mirrored {
        // SAFETY: state-only call on the live context.
        unsafe { scene.gl.cull_face(cull_face) };
    }
    draw_mesh(
        scene.gl,
        &scene.extra.alias_uniforms,
        &scene.extra.alias_stream,
        &MeshDraw {
            vertices: &vertices,
            vertex_count: i32::try_from(vertices.len() / ALIAS_VERTEX_FLOATS).unwrap_or(i32::MAX),
            texture: Some(texture),
            mvp: &mvp,
            blend: translucent,
            color_only: shell,
        },
    );
    if (near, far) != (0.0, 1.0) {
        // SAFETY: state-only call on the live context.
        unsafe { scene.gl.depth_range_f32(0.0, 1.0) };
    }
    if mirrored {
        // SAFETY: state-only call on the live context.
        unsafe { scene.gl.cull_face(super::CULL_FACE) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2_render_api::ModelHandle;
    use q2_shared::constants::RF_BEAM;

    #[test]
    fn md2_and_2d_vertices_feed_every_input_of_their_shaders() {
        use super::super::draw::assert_layout;
        use super::super::shader::{ALIAS_VERTEX_SRC, UI_VERTEX_SRC};
        assert_layout(
            &ALIAS_ATTRIBUTES,
            ALIAS_VERTEX_FLOATS,
            ALIAS_VERTEX_SRC,
            true,
        );
        assert_layout(&UI_ATTRIBUTES, UI_VERTEX_FLOATS, UI_VERTEX_SRC, true);
    }

    /// A model of `frames` 16-unit boxes at the model origin.
    fn boxes(frames: usize) -> Md2Model {
        let frame = crate::files::md2::Md2Frame {
            name: "f".into(),
            scale: [16.0 / 255.0; 3],
            translate: [-8.0; 3],
            verts: Vec::new(),
        };
        Md2Model {
            skin_width: 8,
            skin_height: 8,
            skins: Vec::new(),
            st: Vec::new(),
            triangles: Vec::new(),
            frames: vec![frame; frames],
        }
    }

    fn at(x: f32, flags: i32, frame: i32, oldframe: i32) -> RenderEntity {
        RenderEntity {
            origin: q2_shared::types::Vec3f::new(x, 0.0, 0.0),
            flags,
            frame,
            oldframe,
            ..Default::default()
        }
    }

    #[test]
    fn only_the_gun_follows_hand_like_c_drawaliasmodel() {
        for hand in [0, 1, 2] {
            assert_eq!(alias_cull_face(0, hand), Some(super::super::CULL_FACE));
        }
        assert_eq!(
            alias_cull_face(RF_WEAPONMODEL, 0),
            Some(super::super::CULL_FACE)
        );
        assert_eq!(alias_cull_face(RF_WEAPONMODEL, 1), Some(glow::BACK));
        assert_eq!(alias_cull_face(RF_WEAPONMODEL, 2), None);
    }

    #[test]
    fn md2s_outside_the_view_are_culled_but_the_gun_never_is() {
        // Eye at the origin looking down +x.
        let frustum = super::super::alias::frustum([0.0; 3], [0.0; 3], 90.0, 90.0);
        let md2 = boxes(1);
        assert_eq!(
            alias_frames(&md2, &at(500.0, 0, 0, 0), &frustum),
            Some((0, 0))
        );
        assert_eq!(alias_frames(&md2, &at(-500.0, 0, 0, 0), &frustum), None);
        let gun = at(-500.0, RF_WEAPONMODEL, 0, 0);
        assert_eq!(alias_frames(&md2, &gun, &frustum), Some((0, 0)));
    }

    #[test]
    fn bad_frames_reset_one_by_one_except_on_the_gun_like_c() {
        let frustum = super::super::alias::frustum([0.0; 3], [0.0; 3], 90.0, 90.0);
        let md2 = boxes(2);
        // C CullAliasModel resets only the bad frame; the gun skips the
        // cull and GL3_DrawAliasModel resets both.
        assert_eq!(
            alias_frames(&md2, &at(500.0, 0, 5, 1), &frustum),
            Some((0, 1))
        );
        let gun = at(500.0, RF_WEAPONMODEL, 5, 1);
        assert_eq!(alias_frames(&md2, &gun, &frustum), Some((0, 0)));
        // A model without frames has nothing to draw.
        assert_eq!(alias_frames(&boxes(0), &at(500.0, 0, 0, 0), &frustum), None);
    }

    #[test]
    fn the_alpha_pass_takes_every_brush_entity_but_beams_and_the_viewer() {
        let mut models = ModelRegistry::<&'static str>::default();
        let alias = models.register("a.md2", || Some(Model::Alias("a")));
        let brush = models.register_inline("*1", Some(2));
        let ent = |model, flags| RenderEntity {
            model,
            flags,
            ..Default::default()
        };
        let list = [
            // C DrawInlineBModel chains SURF_TRANS faces whatever the
            // entity's own flags.
            ent(brush, RF_TRANSLUCENT),
            ent(brush, RF_VIEWERMODEL),
            ent(brush, RF_BEAM),
            ent(alias, 0),
            ent(brush, 0),
        ];
        let picked: Vec<(usize, usize)> = brush_entities(&list, &models)
            .map(|(e, submodel)| {
                (
                    list.iter().position(|l| std::ptr::eq(l, e)).unwrap(),
                    submodel,
                )
            })
            .collect();
        assert_eq!(picked, [(0, 1), (4, 1)]);
    }

    #[test]
    fn a_shell_samples_white_and_a_missing_skin_the_checker() {
        let texture = |n| glow::NativeTexture(std::num::NonZeroU32::new(n).unwrap());
        let (white, notexture) = (texture(1), texture(2));
        let images = ImageStore::default();
        let shell = RenderEntity {
            flags: q2_shared::constants::RF_SHELL_RED,
            ..Default::default()
        };
        let skins = [Some(0)];
        assert_eq!(
            alias_texture(&shell, &skins, &images, (white, notexture)),
            white
        );
        let plain = RenderEntity::default();
        // C falls back to gl3_notexture with no skin, or one not loaded.
        let no_skins: [Option<usize>; 0] = [];
        for skins in [&no_skins[..], &[None], &skins] {
            assert_eq!(
                alias_texture(&plain, skins, &images, (white, notexture)),
                notexture
            );
        }
    }

    #[test]
    fn a_missing_or_corrupt_md2_registers_no_model() {
        let loader: FileLoader = Box::new(|name| match name {
            "bad.md2" => Some(b"IDP2 not a model".to_vec()),
            _ => None,
        });
        let mut images = ImageStore::default();
        for name in ["bad.md2", "gone.md2"] {
            assert!(load_alias_model(None, &loader, None, &mut images, name).is_none());
        }
    }

    fn sprite() -> Model<&'static str> {
        Model::Sprite(super::super::fx::SpriteModel { frames: Vec::new() })
    }

    #[test]
    fn each_pass_draws_its_entities_in_list_order_like_c_drawentitiesonlist() {
        let mut models = ModelRegistry::<&'static str>::default();
        let alias = models.register("a.md2", || Some(Model::Alias("a")));
        let spr = models.register("s.sp2", || Some(sprite()));
        let brush = models.register_inline("*2", Some(3));
        let ent = |model, flags| RenderEntity {
            model,
            flags,
            ..Default::default()
        };
        let list = [
            ent(spr, 0),
            ent(ModelHandle::NONE, RF_BEAM),
            ent(alias, RF_TRANSLUCENT),
            ent(brush, 0),
            ent(alias, 0),
            // C draws a beam whatever its model; a modelless entity is
            // C's null model, which is not drawn here.
            ent(ModelHandle::NONE, 0),
            ent(alias, RF_VIEWERMODEL),
            ent(ModelHandle::NONE, RF_BEAM | RF_TRANSLUCENT),
        ];
        let kinds = |translucent| -> Vec<(usize, EntityDraw)> {
            entity_draws(&list, &models, translucent)
                .map(|(e, kind)| (list.iter().position(|l| std::ptr::eq(l, e)).unwrap(), kind))
                .collect()
        };
        assert_eq!(
            kinds(false),
            [
                (0, EntityDraw::Sprite),
                (1, EntityDraw::Beam),
                (3, EntityDraw::Brush(2)),
                (4, EntityDraw::Alias),
            ]
        );
        assert_eq!(kinds(true), [(2, EntityDraw::Alias), (7, EntityDraw::Beam)]);
    }
}
