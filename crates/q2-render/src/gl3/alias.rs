//! MD2 entity maths (C `gl3_mesh.c`: `LerpVerts`, `DrawAliasFrameLerp`, the
//! lighting part of `GL3_DrawAliasModel`) and `AngleVectors`. GL-free.
//!
//! Frames are interpolated on the CPU every draw, as C does: positions for
//! all `num_xyz` vertices are lerped once and then expanded to one vertex per
//! triangle corner ([`ALIAS_VERTEX_FLOATS`] floats: position(3), skin uv(2),
//! colour(4)) for a stream upload. C builds the same data from the GL command
//! list; the MD2 triangle lump is equivalent and is what the parser keeps.

use q2_render_api::RenderEntity;
use q2_shared::constants::{RF_FULLBRIGHT, RF_GLOW, RF_TRANSLUCENT};

use crate::files::md2::{Md2Frame, Md2Model, VERTEX_NORMALS};

/// C `RF_MINLIGHT`; not yet in `q2_shared::constants`.
pub const RF_MINLIGHT: i32 = 1;
/// C `MAX_MD2SKINS`.
pub const MAX_SKINS: usize = 32;
/// Floats per vertex: position(3), skin uv(2), colour(4).
pub const ALIAS_VERTEX_FLOATS: usize = 9;
/// C `SHADEDOT_QUANT`: yaw steps for the precomputed light directions.
const SHADEDOT_QUANT: i32 = 16;
/// C `entity->alpha * 0.666f` for `RF_TRANSLUCENT` alias models.
const TRANSLUCENT_SCALE: f32 = 0.666;

/// One triangle corner: which frame vertex it uses and its skin coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AliasCorner {
    pub vertex: u16,
    pub uv: [f32; 2],
}

/// Corners of every triangle in file order. Skin coordinates are
/// `(st + 0.5) / skin size`, the value C's GL command lists store (verified
/// against `soldier/tris.md2`).
pub fn build_corners(model: &Md2Model) -> Vec<AliasCorner> {
    let width = model.skin_width.max(1) as f32;
    let height = model.skin_height.max(1) as f32;
    let mut corners = Vec::with_capacity(model.triangles.len() * 3);
    for triangle in &model.triangles {
        for k in 0..3 {
            let [s, t] = model
                .st
                .get(usize::from(triangle.st[k]))
                .copied()
                .unwrap_or([0, 0]);
            corners.push(AliasCorner {
                vertex: triangle.vertex[k],
                uv: [(f32::from(s) + 0.5) / width, (f32::from(t) + 0.5) / height],
            });
        }
    }
    corners
}

/// C `CullAliasModel` resets each out-of-range frame to 0 on its own, and
/// writes the result back into the entity before the draw-time check below.
pub fn clamp_frames_independently(frame: i32, oldframe: i32, num_frames: usize) -> (usize, usize) {
    let valid = |f: i32| {
        usize::try_from(f)
            .ok()
            .filter(|&f| f < num_frames)
            .unwrap_or(0)
    };
    (valid(frame), valid(oldframe))
}

/// C `GL3_DrawAliasModel` resets both frames to 0 when either is out of range.
pub fn clamp_frames(frame: i32, oldframe: i32, num_frames: usize) -> (usize, usize) {
    let valid = |f: i32| usize::try_from(f).ok().filter(|&f| f < num_frames);
    match (valid(frame), valid(oldframe)) {
        (Some(frame), Some(oldframe)) => (frame, oldframe),
        _ => (0, 0),
    }
}

/// One view frustum plane: inward normal and distance from the origin.
pub type FrustumPlane = ([f32; 3], f32);

/// C `R_SetFrustum`: the right, left, bottom and top planes of the view.
/// C rotates the view direction by `90 - fov / 2` towards each edge; the
/// result is `forward * sin(fov / 2) -+ side * cos(fov / 2)`.
pub fn frustum(origin: [f32; 3], angles: [f32; 3], fov_x: f32, fov_y: f32) -> [FrustumPlane; 4] {
    let (forward, right, up) = angle_vectors(angles);
    let edge = |side: [f32; 3], half_fov: f32, sign: f32| {
        let (sin, cos) = half_fov.to_radians().sin_cos();
        let normal: [f32; 3] = std::array::from_fn(|i| forward[i] * sin + sign * side[i] * cos);
        (normal, dot(normal, origin))
    };
    [
        edge(right, fov_x / 2.0, -1.0),
        edge(right, fov_x / 2.0, 1.0),
        edge(up, fov_y / 2.0, 1.0),
        edge(up, fov_y / 2.0, -1.0),
    ]
}

/// C `CullAliasModel`: true when the box holding both lerped frames,
/// turned by the entity's angles, lies wholly outside one frustum plane.
pub fn cull_alias(
    frame: &Md2Frame,
    oldframe: &Md2Frame,
    origin: [f32; 3],
    angles: [f32; 3],
    frustum: &[FrustumPlane; 4],
) -> bool {
    let bounds = |f: &Md2Frame, axis: usize| {
        let low = f.translate[axis];
        (low, low + f.scale[axis] * 255.0)
    };
    let (mut mins, mut maxs) = ([0.0f32; 3], [0.0f32; 3]);
    for axis in 0..3 {
        let ((this_min, this_max), (old_min, old_max)) =
            (bounds(frame, axis), bounds(oldframe, axis));
        mins[axis] = this_min.min(old_min);
        maxs[axis] = this_max.max(old_max);
    }
    // C negates yaw, then maps p to (forward.p, -right.p, up.p).
    let (forward, right, up) = angle_vectors([angles[0], -angles[1], angles[2]]);
    let mut outside_all = !0u32;
    for corner in 0..8 {
        let pick = |axis: usize| {
            if corner & (1 << axis) != 0 {
                mins[axis]
            } else {
                maxs[axis]
            }
        };
        let local = [pick(0), pick(1), pick(2)];
        let world = [
            origin[0] + dot(forward, local),
            origin[1] - dot(right, local),
            origin[2] + dot(up, local),
        ];
        let outside = frustum
            .iter()
            .enumerate()
            .filter(|(_, (normal, dist))| dot(*normal, world) - dist < 0.0)
            .fold(0u32, |mask, (plane, _)| mask | 1 << plane);
        outside_all &= outside;
    }
    outside_all != 0
}

/// C `AngleVectors`: forward, right, up for pitch/yaw/roll degrees. Plain
/// f32 on purpose: this only places models on screen, so it skips the
/// double-path rounding that `q2_common::pmove`'s copy reproduces for
/// client/server prediction sync.
pub fn angle_vectors(angles: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sy, cy) = angles[1].to_radians().sin_cos();
    let (sp, cp) = angles[0].to_radians().sin_cos();
    let (sr, cr) = angles[2].to_radians().sin_cos();
    let forward = [cp * cy, cp * sy, -sp];
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    (forward, right, up)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Offset from the current to the previous origin in model axes (forward,
/// left, up): the `move` of C `DrawAliasFrameLerp` before frame translation.
pub fn model_space_delta(origin: [f32; 3], oldorigin: [f32; 3], angles: [f32; 3]) -> [f32; 3] {
    let delta = [
        oldorigin[0] - origin[0],
        oldorigin[1] - origin[1],
        oldorigin[2] - origin[2],
    ];
    let (forward, right, up) = angle_vectors(angles);
    [dot(delta, forward), -dot(delta, right), dot(delta, up)]
}

/// Per-draw constants of the frame interpolation:
/// `pos = translate + old * back_scale + new * front_scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LerpParams {
    pub translate: [f32; 3],
    pub front_scale: [f32; 3],
    pub back_scale: [f32; 3],
}

/// `delta` is [`model_space_delta`]. `backlerp` 0 is the current frame
/// alone, 1 the old frame alone.
pub fn lerp_params(
    frame: &Md2Frame,
    oldframe: &Md2Frame,
    backlerp: f32,
    delta: [f32; 3],
) -> LerpParams {
    let frontlerp = 1.0 - backlerp;
    let mut params = LerpParams {
        translate: [0.0; 3],
        front_scale: [0.0; 3],
        back_scale: [0.0; 3],
    };
    for (axis, &shift) in delta.iter().enumerate() {
        let from_old = shift + oldframe.translate[axis];
        params.translate[axis] = backlerp * from_old + frontlerp * frame.translate[axis];
        params.front_scale[axis] = frontlerp * frame.scale[axis];
        params.back_scale[axis] = backlerp * oldframe.scale[axis];
    }
    params
}

/// C `LerpVerts` for one vertex (`new` and `old` are quantised positions).
pub fn lerp_position(params: &LerpParams, new: [u8; 3], old: [u8; 3]) -> [f32; 3] {
    let mut position = [0.0; 3];
    for axis in 0..3 {
        position[axis] = params.translate[axis]
            + f32::from(old[axis]) * params.back_scale[axis]
            + f32::from(new[axis]) * params.front_scale[axis];
    }
    position
}

/// Row of C `r_avertexnormal_dots` for a yaw: `((int)(yaw * 16/360)) & 15`.
pub fn shade_row(yaw: f32) -> usize {
    let step = (f64::from(yaw) * (f64::from(SHADEDOT_QUANT) / 360.0)) as i32;
    (step & (SHADEDOT_QUANT - 1)) as usize
}

/// Entry of C's `r_avertexnormal_dots[row][normal]` table. The table holds
/// `1 + d` for `d >= 0` and `1 + 0.3 d` otherwise, where `d` is the normal
/// dotted with the unit light direction `(cos(-a), sin(-a), 1)` at
/// `a = row * 360/16` degrees; the shipped table rounds that to 2 decimals.
/// Computing it instead of embedding the 16x162 table is a deliberate
/// deviation: shade differs from C by at most 0.005, render-only.
/// Unknown normal indices shade as 1.
pub fn normal_dot(row: usize, normal_index: u8) -> f32 {
    let Some(normal) = VERTEX_NORMALS.get(usize::from(normal_index)) else {
        return 1.0;
    };
    let angle = (row as f32) * (std::f32::consts::TAU / SHADEDOT_QUANT as f32);
    let (sin_a, cos_a) = (-angle).sin_cos();
    let norm = (cos_a * cos_a + sin_a * sin_a + 1.0).sqrt();
    let d = (normal[0] * cos_a + normal[1] * sin_a + normal[2]) / norm;
    if d >= 0.0 {
        1.0 + d
    } else {
        1.0 + 0.3 * d
    }
}

/// Light colour of an entity: `RF_FULLBRIGHT` is white, otherwise `lit` (the
/// lightmap sample under it); then `RF_MINLIGHT` and the `RF_GLOW` pulse,
/// in C order. Shell effects (`RF_SHELL_*`) are not modelled.
pub fn shade_light(flags: i32, lit: impl FnOnce() -> [f32; 3], time: f32) -> [f32; 3] {
    let mut light = if flags & RF_FULLBRIGHT != 0 {
        [1.0; 3]
    } else {
        lit()
    };
    if flags & RF_MINLIGHT != 0 && light.iter().all(|&c| c <= 0.1) {
        light = [0.1; 3];
    }
    if flags & RF_GLOW != 0 {
        let scale = 0.1 * (time * 7.0).sin();
        for channel in &mut light {
            let floor = *channel * 0.8;
            *channel = (*channel + scale).max(floor);
        }
    }
    light
}

/// Vertex alpha: `RF_TRANSLUCENT` entities use `alpha * 0.666`, others 1.
pub fn entity_alpha(flags: i32, alpha: f32) -> f32 {
    if flags & RF_TRANSLUCENT != 0 {
        alpha * TRANSLUCENT_SCALE
    } else {
        1.0
    }
}

/// Image-store index of the skin an entity draws with: its custom skin
/// (a `register_image` handle) when set, else the model's own (C
/// `GL3_DrawAliasModel`).
pub fn entity_skin(
    custom: q2_render_api::ImageHandle,
    skinnum: i32,
    skins: &[Option<usize>],
) -> Option<usize> {
    super::images::handle_index(custom).or_else(|| pick_skin(skinnum, skins))
}

/// Skin slot for `skinnum`: out of range or empty falls back to slot 0.
pub fn pick_skin(skinnum: i32, skins: &[Option<usize>]) -> Option<usize> {
    let slot = usize::try_from(skinnum).ok().filter(|&n| n < MAX_SKINS);
    slot.and_then(|n| skins.get(n).copied().flatten())
        .or_else(|| skins.first().copied().flatten())
}

/// Everything about one entity draw that shapes the vertex stream.
pub struct AliasDraw {
    pub frame: usize,
    pub oldframe: usize,
    pub backlerp: f32,
    /// [`model_space_delta`] result.
    pub delta: [f32; 3],
    pub yaw: f32,
    /// Light colour from [`shade_light`].
    pub light: [f32; 3],
    pub alpha: f32,
}

/// Interpolated, lit vertex stream (three vertices per triangle) for a draw.
/// `None` when the model has no such frames.
pub fn build_vertices(
    model: &Md2Model,
    corners: &[AliasCorner],
    draw: &AliasDraw,
) -> Option<Vec<f32>> {
    let frame = model.frames.get(draw.frame)?;
    let oldframe = model.frames.get(draw.oldframe)?;
    let params = lerp_params(frame, oldframe, draw.backlerp, draw.delta);
    let row = shade_row(draw.yaw);

    // (position, shade) per frame vertex, lerped once.
    let lerped: Vec<([f32; 3], f32)> = frame
        .verts
        .iter()
        .zip(&oldframe.verts)
        .map(|(new, old)| {
            (
                lerp_position(&params, new.v, old.v),
                normal_dot(row, new.normal_index),
            )
        })
        .collect();

    let mut vertices = Vec::with_capacity(corners.len() * ALIAS_VERTEX_FLOATS);
    for corner in corners {
        let (position, shade) = *lerped.get(usize::from(corner.vertex))?;
        vertices.extend_from_slice(&position);
        vertices.extend_from_slice(&corner.uv);
        vertices.extend_from_slice(&[
            shade * draw.light[0],
            shade * draw.light[1],
            shade * draw.light[2],
            draw.alpha,
        ]);
    }
    Some(vertices)
}

/// Vertex stream for one entity: clamps frames like C (logging nothing but a
/// trace), lights it with `lit` (the lightmap sample under the entity) and
/// interpolates. `None` when the model cannot be drawn.
pub fn entity_vertices(
    model: &AliasModel,
    entity: &RenderEntity,
    lit: impl FnOnce() -> [f32; 3],
    time: f32,
) -> Option<Vec<f32>> {
    let (frame, oldframe) = clamp_frames(entity.frame, entity.oldframe, model.md2.frames.len());
    if (frame, oldframe) != (entity.frame as usize, entity.oldframe as usize) {
        tracing::trace!(
            "GL3: no such frame {} / {} in model, using frame 0",
            entity.frame,
            entity.oldframe
        );
    }
    let origin = [entity.origin.x, entity.origin.y, entity.origin.z];
    let oldorigin = [entity.oldorigin.x, entity.oldorigin.y, entity.oldorigin.z];
    let angles = [entity.angles.x, entity.angles.y, entity.angles.z];
    let draw = AliasDraw {
        frame,
        oldframe,
        backlerp: entity.backlerp,
        delta: model_space_delta(origin, oldorigin, angles),
        yaw: angles[1],
        light: shade_light(entity.flags, lit, time),
        alpha: entity_alpha(entity.flags, entity.alpha),
    };
    build_vertices(&model.md2, &model.corners, &draw)
}

/// A registered MD2: parsed frames, prebuilt corners and its skins.
pub struct AliasModel {
    pub md2: Md2Model,
    pub corners: Vec<AliasCorner>,
    /// Image-store index per skin slot; `None` for a skin that failed to load.
    pub skins: Vec<Option<usize>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::md2::{Md2Triangle, Md2Vertex};

    /// A 32-unit cube frame centred on the entity origin.
    fn cube() -> Md2Frame {
        frame([32.0 / 255.0; 3], [-16.0; 3], &[])
    }

    /// Eye at the origin looking down +x with C's default 90 x 73.74 fov.
    fn ahead() -> [([f32; 3], f32); 4] {
        frustum([0.0; 3], [0.0; 3], 90.0, 73.74)
    }

    #[test]
    fn models_in_front_are_kept_and_behind_or_beside_are_culled() {
        let (cube, view) = (cube(), ahead());
        let culled = |origin| cull_alias(&cube, &cube, origin, [0.0; 3], &view);
        assert!(!culled([200.0, 0.0, 0.0]));
        assert!(culled([-200.0, 0.0, 0.0]), "behind the eye");
        assert!(culled([100.0, 300.0, 0.0]), "outside the left edge");
        assert!(culled([100.0, 0.0, -300.0]), "below the bottom edge");
    }

    #[test]
    fn a_model_straddling_an_edge_or_around_the_eye_is_kept() {
        let (cube, view) = (cube(), ahead());
        // Centre just outside the left plane (y = x at 90 fov), corner inside.
        assert!(!cull_alias(
            &cube,
            &cube,
            [100.0, 110.0, 0.0],
            [0.0; 3],
            &view
        ));
        assert!(!cull_alias(&cube, &cube, [0.0; 3], [0.0; 3], &view));
    }

    #[test]
    fn cull_box_spans_both_lerped_frames() {
        let view = ahead();
        // Only the old frame's box, shifted 400 units forward, is in view.
        let far = frame([32.0 / 255.0; 3], [384.0, -16.0, -16.0], &[]);
        assert!(!cull_alias(
            &cube(),
            &far,
            [-200.0, 0.0, 0.0],
            [0.0; 3],
            &view
        ));
        assert!(cull_alias(
            &cube(),
            &cube(),
            [-200.0, 0.0, 0.0],
            [0.0; 3],
            &view
        ));
    }

    #[test]
    fn entity_yaw_rotates_the_cull_box() {
        let view = ahead();
        // A box stretched 400 units along its own +x reaches into view only
        // when the entity is turned to face the eye's view direction.
        let long = frame(
            [400.0 / 255.0, 32.0 / 255.0, 32.0 / 255.0],
            [0.0, -16.0, -16.0],
            &[],
        );
        assert!(cull_alias(
            &long,
            &long,
            [-200.0, 0.0, 0.0],
            [0.0, 180.0, 0.0],
            &view
        ));
        assert!(!cull_alias(
            &long,
            &long,
            [-200.0, 0.0, 0.0],
            [0.0; 3],
            &view
        ));
    }

    fn frame(scale: [f32; 3], translate: [f32; 3], verts: &[[u8; 3]]) -> Md2Frame {
        Md2Frame {
            name: "f".into(),
            scale,
            translate,
            verts: verts
                .iter()
                .map(|&v| Md2Vertex { v, normal_index: 0 })
                .collect(),
        }
    }

    fn assert_near(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len());
        for (a, e) in actual.iter().zip(expected) {
            assert!(
                (a - e).abs() < 1e-4,
                "got {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn lerp_matches_hand_computed_blend_of_two_frames() {
        let new = frame([1.0, 2.0, 3.0], [10.0, 20.0, 30.0], &[[4, 5, 6]]);
        let old = frame([2.0; 3], [0.0; 3], &[[1, 1, 1]]);
        // backlerp 0.25: translate = 0.25*0 + 0.75*(10,20,30) = (7.5,15,22.5),
        // front scale = 0.75*(1,2,3), back scale = 0.25*2 = 0.5.
        let params = lerp_params(&new, &old, 0.25, [0.0; 3]);
        assert_near(&params.translate, &[7.5, 15.0, 22.5]);
        assert_near(&params.front_scale, &[0.75, 1.5, 2.25]);
        assert_near(&params.back_scale, &[0.5; 3]);
        // 7.5 + 1*0.5 + 4*0.75 = 11 ; 15 + 0.5 + 5*1.5 = 23 ; 22.5 + 0.5 + 6*2.25 = 36.5
        assert_near(
            &lerp_position(&params, [4, 5, 6], [1, 1, 1]),
            &[11.0, 23.0, 36.5],
        );
    }

    #[test]
    fn backlerp_zero_is_the_current_frame_and_one_is_the_old_frame() {
        let new = frame([1.0, 2.0, 3.0], [10.0, 20.0, 30.0], &[[4, 5, 6]]);
        let old = frame([2.0; 3], [1.0, 1.0, 1.0], &[[1, 1, 1]]);
        let at_new = lerp_params(&new, &old, 0.0, [0.0; 3]);
        let p = lerp_position(&at_new, [4, 5, 6], [1, 1, 1]);
        let expected = new.vertex_position(0);
        assert_near(&p, &[expected.x, expected.y, expected.z]);
        let at_old = lerp_params(&new, &old, 1.0, [0.0; 3]);
        let p = lerp_position(&at_old, [4, 5, 6], [1, 1, 1]);
        // old: 1 + 1*2
        assert_near(&p, &[3.0, 3.0, 3.0]);
    }

    #[test]
    fn origin_delta_is_expressed_in_forward_left_up_and_scaled_by_backlerp() {
        // Previous origin 10 units to +X; facing +Y (yaw 90) that is to the
        // entity's right, so "left" is -10.
        let delta = model_space_delta([0.0; 3], [10.0, 0.0, 0.0], [0.0, 90.0, 0.0]);
        assert_near(&delta, &[0.0, -10.0, 0.0]);
        let still = frame([1.0; 3], [0.0; 3], &[[0, 0, 0]]);
        let params = lerp_params(&still, &still, 0.5, delta);
        assert_near(&params.translate, &[0.0, -5.0, 0.0]);
    }

    #[test]
    fn angle_vectors_match_the_quake_convention() {
        let (forward, right, up) = angle_vectors([0.0, 90.0, 0.0]);
        assert_near(&forward, &[0.0, 1.0, 0.0]);
        assert_near(&right, &[1.0, 0.0, 0.0]);
        assert_near(&up, &[0.0, 0.0, 1.0]);
        // Positive pitch looks down.
        let (forward, _, up) = angle_vectors([90.0, 0.0, 0.0]);
        assert_near(&forward, &[0.0, 0.0, -1.0]);
        assert_near(&up, &[1.0, 0.0, 0.0]);
        // Positive roll tips the right vector downward.
        let (_, right, _) = angle_vectors([0.0, 0.0, 90.0]);
        assert_near(&right, &[0.0, 0.0, -1.0]);
    }

    #[test]
    fn out_of_range_frames_reset_both_to_zero() {
        assert_eq!(clamp_frames(3, 2, 4), (3, 2));
        assert_eq!(clamp_frames(4, 2, 4), (0, 0));
        assert_eq!(clamp_frames(1, -1, 4), (0, 0));
        assert_eq!(clamp_frames(-5, 9, 4), (0, 0));
    }

    #[test]
    fn cull_resets_each_out_of_range_frame_on_its_own() {
        assert_eq!(clamp_frames_independently(3, 2, 4), (3, 2));
        assert_eq!(clamp_frames_independently(4, 2, 4), (0, 2));
        assert_eq!(clamp_frames_independently(1, -1, 4), (1, 0));
        assert_eq!(clamp_frames_independently(-5, 9, 4), (0, 0));
    }

    #[test]
    fn shade_row_quantises_yaw_into_16_steps_like_c() {
        assert_eq!(shade_row(0.0), 0);
        assert_eq!(shade_row(30.0), 1);
        assert_eq!(shade_row(100.0), 4);
        assert_eq!(shade_row(359.0), 15);
        assert_eq!(shade_row(360.0), 0);
        // (int)(-30 * 16/360) = -1, masked to 15.
        assert_eq!(shade_row(-30.0), 15);
    }

    #[test]
    fn normal_dots_reproduce_the_c_table_entries() {
        // Literals from constants/anormtab.h, which rounds to 2 decimals.
        let cases = [(0, 153, 0.70), (0, 72, 0.89), (5, 0, 1.74), (11, 100, 0.78)];
        for (row, normal, table) in cases {
            let formula = normal_dot(row, normal);
            assert!(
                (formula - table).abs() < 0.006,
                "row {row} normal {normal}: formula {formula}, table {table}"
            );
        }
        assert_eq!(normal_dot(0, 200), 1.0);
    }

    #[test]
    fn fullbright_ignores_the_lightmap_and_lit_entities_use_it() {
        let dark = || [0.2, 0.3, 0.4];
        assert_eq!(shade_light(RF_FULLBRIGHT, dark, 0.0), [1.0; 3]);
        assert_eq!(shade_light(0, dark, 0.0), [0.2, 0.3, 0.4]);
        // The lightmap is not even sampled for fullbright entities.
        assert_eq!(
            shade_light(RF_FULLBRIGHT, || panic!("sampled"), 0.0),
            [1.0; 3]
        );
    }

    #[test]
    fn minlight_lifts_only_entities_with_every_channel_at_or_below_a_tenth() {
        assert_eq!(shade_light(RF_MINLIGHT, || [0.0; 3], 0.0), [0.1; 3]);
        assert_eq!(shade_light(RF_MINLIGHT, || [0.05, 0.1, 0.0], 0.0), [0.1; 3]);
        assert_eq!(
            shade_light(RF_MINLIGHT, || [0.05, 0.5, 0.0], 0.0),
            [0.05, 0.5, 0.0]
        );
    }

    #[test]
    fn glow_pulses_with_sine_of_seven_times_the_clock_but_not_below_80_percent() {
        // sin(7t) = 1 at t = pi/14: +0.1.
        let peak = std::f32::consts::PI / 14.0;
        assert_near(&shade_light(RF_GLOW, || [0.5; 3], peak), &[0.6; 3]);
        // sin = -1: 0.5 - 0.1 = 0.4, exactly the 80% floor.
        assert_near(&shade_light(RF_GLOW, || [0.5; 3], 3.0 * peak), &[0.4; 3]);
        // Dark entity: 0.05 - 0.1 would go negative; the floor is 0.04.
        assert_near(&shade_light(RF_GLOW, || [0.05; 3], 3.0 * peak), &[0.04; 3]);
    }

    #[test]
    fn translucent_entities_use_two_thirds_alpha() {
        assert!((entity_alpha(RF_TRANSLUCENT, 0.5) - 0.333).abs() < 1e-6);
        assert_eq!(entity_alpha(0, 0.5), 1.0);
    }

    #[test]
    fn custom_entity_skin_wins_over_the_models_own() {
        use q2_render_api::ImageHandle;
        let skins = [Some(7), Some(9)];
        // C GL3_DrawAliasModel: `if (entity->skin) skin = entity->skin;`
        assert_eq!(entity_skin(ImageHandle::new(4), 1, &skins), Some(3));
        assert_eq!(entity_skin(ImageHandle::NONE, 1, &skins), Some(9));
    }

    #[test]
    fn skin_selection_falls_back_to_slot_zero() {
        let skins = [Some(7), None, Some(9)];
        assert_eq!(pick_skin(0, &skins), Some(7));
        assert_eq!(pick_skin(2, &skins), Some(9));
        assert_eq!(pick_skin(1, &skins), Some(7)); // empty slot
        assert_eq!(pick_skin(5, &skins), Some(7)); // past the list
        assert_eq!(pick_skin(MAX_SKINS as i32, &skins), Some(7));
        assert_eq!(pick_skin(-1, &skins), Some(7));
        assert_eq!(pick_skin(0, &[]), None);
    }

    fn triangle_model() -> Md2Model {
        Md2Model {
            skin_width: 8,
            skin_height: 4,
            skins: vec![],
            st: vec![[1, 1], [3, 2], [5, 3]],
            triangles: vec![Md2Triangle {
                vertex: [2, 0, 1],
                st: [0, 1, 2],
            }],
            frames: vec![
                frame([1.0; 3], [0.0; 3], &[[0, 0, 0], [10, 0, 0], [0, 10, 0]]),
                frame(
                    [1.0; 3],
                    [0.0, 0.0, 100.0],
                    &[[0, 0, 0], [10, 0, 0], [0, 10, 0]],
                ),
            ],
        }
    }

    #[test]
    fn corners_use_half_texel_centred_skin_coordinates() {
        let corners = build_corners(&triangle_model());
        assert_eq!(corners.len(), 3);
        assert_eq!(corners[0].vertex, 2);
        assert_eq!(corners[0].uv, [1.5 / 8.0, 1.5 / 4.0]);
        assert_eq!(corners[2].uv, [5.5 / 8.0, 3.5 / 4.0]);
    }

    #[test]
    fn vertex_stream_has_position_uv_and_lit_colour_per_corner() {
        let model = triangle_model();
        let corners = build_corners(&model);
        let draw = AliasDraw {
            frame: 0,
            oldframe: 1,
            backlerp: 0.5,
            delta: [0.0; 3],
            yaw: 0.0,
            light: [0.5, 0.25, 1.0],
            alpha: 0.7,
        };
        let stream = build_vertices(&model, &corners, &draw).unwrap();
        assert_eq!(stream.len(), 3 * ALIAS_VERTEX_FLOATS);
        // First corner is vertex 2 = (0,10,0): z blends 0 and 100 halfway.
        assert_near(&stream[0..3], &[0.0, 10.0, 50.0]);
        assert_near(&stream[3..5], &[1.5 / 8.0, 1.5 / 4.0]);
        let shade = normal_dot(0, 0);
        assert_near(&stream[5..9], &[shade * 0.5, shade * 0.25, shade, 0.7]);
        // Second corner is vertex 0 = origin.
        assert_near(&stream[9..12], &[0.0, 0.0, 50.0]);
    }

    fn registered_triangle() -> AliasModel {
        let md2 = triangle_model();
        let corners = build_corners(&md2);
        AliasModel {
            md2,
            corners,
            skins: vec![],
        }
    }

    #[test]
    fn entity_with_bad_frames_draws_frame_zero_lit_by_its_flags() {
        let model = registered_triangle();
        let entity = RenderEntity {
            frame: 99,
            oldframe: 1,
            backlerp: 0.5,
            flags: RF_FULLBRIGHT | RF_TRANSLUCENT,
            alpha: 0.5,
            ..Default::default()
        };
        let stream = entity_vertices(&model, &entity, || panic!("sampled"), 0.0).unwrap();
        // Both frames reset to 0, so z stays 0 instead of blending toward 100.
        assert_near(&stream[0..3], &[0.0, 10.0, 0.0]);
        let shade = normal_dot(0, 0);
        assert_near(&stream[5..9], &[shade, shade, shade, 0.333]);
    }

    #[test]
    fn entity_origin_delta_shifts_the_blend_toward_the_old_position() {
        let model = registered_triangle();
        let entity = RenderEntity {
            frame: 0,
            oldframe: 0,
            backlerp: 1.0,
            origin: q2_shared::types::Vec3f::new(0.0, 0.0, 0.0),
            oldorigin: q2_shared::types::Vec3f::new(4.0, 0.0, 0.0),
            ..Default::default()
        };
        // Facing +X, the old origin is 4 units ahead: backlerp 1 moves the
        // model forward by the whole delta.
        let stream = entity_vertices(&model, &entity, || [1.0; 3], 0.0).unwrap();
        assert_near(&stream[0..3], &[4.0, 10.0, 0.0]);
    }

    #[test]
    fn missing_frame_or_vertex_yields_no_stream() {
        let model = triangle_model();
        let corners = build_corners(&model);
        let draw = |frame| AliasDraw {
            frame,
            oldframe: 0,
            backlerp: 0.0,
            delta: [0.0; 3],
            yaw: 0.0,
            light: [1.0; 3],
            alpha: 1.0,
        };
        assert!(build_vertices(&model, &corners, &draw(5)).is_none());
        let bad_corner = [AliasCorner {
            vertex: 9,
            uv: [0.0; 2],
        }];
        assert!(build_vertices(&model, &bad_corner, &draw(0)).is_none());
    }
}
