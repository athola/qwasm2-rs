//! Entities to draw between two server frames. C ref: `CL_AddPacketEntities`
//! (client/cl_entities.c) and the no-lerp rules of `CL_ParseEntity`
//! (client/cl_parse.c). Particle trails and lights are `fx`'s; player
//! skins are not done.

use q2_shared::constants::{
    EF_ANIM01, EF_ANIM23, EF_ANIM_ALL, EF_ANIM_ALLFAST, EF_COLOR_SHELL, EF_DOUBLE, EF_HALF_DAMAGE,
    EF_PENT, EF_QUAD, EF_ROTATE, EF_SPINNINGLIGHTS, EV_OTHER_TELEPORT, EV_PLAYER_TELEPORT, RF_BEAM,
    RF_FRAMELERP, RF_SHELL_BLUE, RF_SHELL_DOUBLE, RF_SHELL_GREEN, RF_SHELL_HALF_DAM, RF_SHELL_RED,
    RF_TRANSLUCENT, RF_VIEWERMODEL,
};
use q2_shared::types::{EntityState, Vec3f};

/// One entity as the renderer draws it this frame (C `entity_t`), with the
/// model still as its `CS_MODELS` index.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityView {
    /// The entity's number and `EF_*` effects, for what the effects add.
    pub number: i32,
    pub effects: u32,
    pub modelindex: i32,
    pub frame: i32,
    pub oldframe: i32,
    /// 0 draws `frame`, 1 draws `oldframe` (C `backlerp`).
    pub backlerp: f32,
    pub origin: Vec3f,
    /// The drawn origin, or `old_origin` for frame-lerped entities and
    /// beams (a beam's other end).
    pub oldorigin: Vec3f,
    pub angles: Vec3f,
    pub skinnum: i32,
    /// `RF_*` flags.
    pub flags: i32,
    pub alpha: f32,
}

/// C `LerpAngle`: the short way round between two angles in degrees.
pub(crate) fn lerp_angle(a2: f32, a1: f32, frac: f32) -> f32 {
    let mut a1 = a1;
    if a1 - a2 > 180.0 {
        a1 -= 360.0;
    }
    if a1 - a2 < -180.0 {
        a1 += 360.0;
    }
    a2 + frac * (a1 - a2)
}

/// C `anglemod`, through 16-bit angle units like the original, scaled in
/// double as C promotes it.
fn anglemod(a: f32) -> f32 {
    (360.0 / 65536.0) * (((f64::from(a) * (65536.0 / 360.0)) as i32) & 65535) as f32
}

/// Whether `s` must not be lerped from `prev` (C `CL_ParseEntity` sets
/// `serverframe = -99`): a model change, a teleport, or a jump of more than
/// 512 units on an axis.
pub(crate) fn breaks_lerp(prev: &EntityState, s: &EntityState) -> bool {
    let jump = (s.origin - prev.origin)
        .to_array()
        .iter()
        .any(|d| (*d as i32).abs() > 512);
    s.modelindex != prev.modelindex
        || s.modelindex2 != prev.modelindex2
        || s.modelindex3 != prev.modelindex3
        || s.modelindex4 != prev.modelindex4
        || s.event == EV_PLAYER_TELEPORT
        || s.event == EV_OTHER_TELEPORT
        || jump
}

/// The frame C draws for `s`: an `EF_ANIM*` effect cycles frames by client
/// time, otherwise the server's frame.
fn anim_frame(s: &EntityState, time_ms: i32) -> i32 {
    let autoanim = 2 * time_ms / 1000;
    if s.effects & EF_ANIM01 != 0 {
        autoanim & 1
    } else if s.effects & EF_ANIM23 != 0 {
        2 + (autoanim & 1)
    } else if s.effects & EF_ANIM_ALL != 0 {
        autoanim
    } else if s.effects & EF_ANIM_ALLFAST != 0 {
        time_ms / 100
    } else {
        s.frame
    }
}

/// The entities of the current frame, interpolated from the previous one
/// by `lerpfrac` (0 = previous frame, 1 = current). Both lists are sorted
/// by entity number. An entity that was not in the previous frame, or that
/// breaks the lerp (see `breaks_lerp`), starts from its `old_origin` (its
/// new origin after `EV_OTHER_TELEPORT`), as C `CL_DeltaEntity` does.
/// `time_ms` is the client time (C `cl.time`), which drives
/// `EF_ROTATE` and `EF_ANIM*`; `playernum` is the local client's slot (its
/// entity is `playernum + 1`).
pub fn entity_views(
    game: &str,
    current: &[EntityState],
    previous: &[EntityState],
    lerpfrac: f32,
    time_ms: i32,
    playernum: i32,
) -> Vec<EntityView> {
    let autorotate = anglemod(time_ms as f32 * 0.1);
    current
        .iter()
        .filter(|s| s.modelindex != 0)
        .flat_map(|s| {
            let reset;
            let prev = match previous
                .binary_search_by_key(&s.number, |p| p.number)
                .ok()
                .map(|i| &previous[i])
                .filter(|prev| !breaks_lerp(prev, s))
            {
                Some(prev) => prev,
                None => {
                    reset = EntityState {
                        origin: if s.event == EV_OTHER_TELEPORT {
                            s.origin
                        } else {
                            s.old_origin
                        },
                        ..s.clone()
                    };
                    &reset
                }
            };
            let (shelled, renderfx) = shell_effects(s.effects, s.renderfx);
            // C: the renderfx go on the colour shell entity.
            let mut flags = if shelled { 0 } else { renderfx };
            let viewer = s.number == playernum + 1;
            if viewer {
                flags |= RF_VIEWERMODEL; // only seen in mirrors / third person
            }
            let angles = if s.effects & EF_ROTATE != 0 {
                Vec3f::new(0.0, autorotate, 0.0)
            } else if s.effects & EF_SPINNINGLIGHTS != 0 {
                Vec3f::new(0.0, anglemod((time_ms / 2) as f32) + s.angles.y, 180.0)
            } else {
                Vec3f::new(
                    lerp_angle(prev.angles.x, s.angles.x, lerpfrac),
                    lerp_angle(prev.angles.y, s.angles.y, lerpfrac),
                    lerp_angle(prev.angles.z, s.angles.z, lerpfrac),
                )
            };
            // Frame-lerped models and beams step their origin: the frames
            // animate the move.
            let (origin, oldorigin) = if s.renderfx & (RF_FRAMELERP | RF_BEAM) != 0 {
                (s.origin, s.old_origin)
            } else {
                let origin = prev.origin + (s.origin - prev.origin) * lerpfrac;
                (origin, origin)
            };
            // C: "only used for black hole model right now".
            let mut alpha = if renderfx & RF_TRANSLUCENT != 0 && renderfx & RF_BEAM == 0 {
                0.70
            } else {
                1.0
            };
            if let Some(effect_alpha) = crate::fx::effect_alpha(s.effects) {
                flags |= RF_TRANSLUCENT;
                alpha = effect_alpha;
            }
            let view = EntityView {
                number: s.number,
                effects: s.effects,
                modelindex: s.modelindex,
                frame: anim_frame(s, time_ms),
                oldframe: prev.frame,
                backlerp: 1.0 - lerpfrac,
                origin,
                oldorigin,
                angles,
                skinnum: s.skinnum,
                flags,
                alpha,
            };
            // C skips the viewer before adding its entities; its own model is
            // kept here, flagged, for the renderer to leave out.
            let shell = (shelled && !viewer).then(|| EntityView {
                flags: shell_combinations(renderfx, game == "rogue") | RF_TRANSLUCENT,
                alpha: 0.30,
                ..view.clone()
            });
            std::iter::once(view).chain(shell)
        })
        .collect()
}

/// C's "quad and pent can do different things on client": `EF_PENT`,
/// `EF_QUAD`, `EF_DOUBLE` and `EF_HALF_DAMAGE` become a red, blue, yellow
/// or grey shell. Whether the entity has a colour shell, and its renderfx
/// with the shell colours.
fn shell_effects(effects: u32, renderfx: i32) -> (bool, i32) {
    let mut renderfx = renderfx;
    for (effect, shell) in [
        (EF_PENT, RF_SHELL_RED),
        (EF_QUAD, RF_SHELL_BLUE),
        (EF_DOUBLE, RF_SHELL_DOUBLE),
        (EF_HALF_DAMAGE, RF_SHELL_HALF_DAM),
    ] {
        if effects & effect != 0 {
            renderfx |= shell;
        }
    }
    let shelled = effects & (EF_COLOR_SHELL | EF_PENT | EF_QUAD | EF_DOUBLE | EF_HALF_DAMAGE) != 0;
    (shelled, renderfx)
}

/// C's fix-ups of clashing shells, which it makes only when the game
/// directory (C cvar `game`) is `rogue`: half damage gives
/// way to any other shell, and double damage turns red purple and blue
/// cyan, or green if green is on too.
pub fn shell_combinations(renderfx: i32, rogue: bool) -> i32 {
    let mut renderfx = renderfx;
    if !rogue {
        return renderfx;
    }
    if renderfx & RF_SHELL_HALF_DAM != 0
        && renderfx & (RF_SHELL_RED | RF_SHELL_BLUE | RF_SHELL_DOUBLE) != 0
    {
        renderfx &= !RF_SHELL_HALF_DAM;
    }
    if renderfx & RF_SHELL_DOUBLE != 0 {
        if renderfx & (RF_SHELL_RED | RF_SHELL_BLUE | RF_SHELL_GREEN) != 0 {
            renderfx &= !RF_SHELL_DOUBLE;
        }
        if renderfx & RF_SHELL_RED != 0 {
            renderfx |= RF_SHELL_BLUE;
        } else if renderfx & RF_SHELL_BLUE != 0 {
            if renderfx & RF_SHELL_GREEN != 0 {
                renderfx &= !RF_SHELL_BLUE;
            } else {
                renderfx |= RF_SHELL_GREEN;
            }
        }
    }
    renderfx
}

/// C `CL_AddPacketEntities` for `RF_BEAM`: the four colours are packed in
/// `skinnum`, and each frame draws one, picked by `randk`, at alpha 0.30
/// with no model.
pub fn pick_beam_colours(views: &mut [EntityView], randk: &mut impl FnMut() -> u32) {
    for view in views.iter_mut().filter(|v| v.flags & RF_BEAM != 0) {
        let shift = (randk() % 4) * 8;
        view.skinnum = ((view.skinnum as u32 >> shift) & 0xff) as i32;
        view.alpha = 0.30;
        view.modelindex = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2_shared::constants::{
        EF_BFG, EF_PLASMA, EF_POWERSCREEN, EF_SPHERETRANS, EF_TRACKERTRAIL,
    };

    /// A state as the server sends it: `old_origin` is where the entity
    /// was when the frame began, here where it is.
    fn state(number: i32, origin: Vec3f, frame: i32) -> EntityState {
        EntityState {
            number,
            origin,
            old_origin: origin,
            frame,
            modelindex: 3,
            ..Default::default()
        }
    }

    #[test]
    fn moving_entity_is_drawn_between_frames_with_frame_lerp() {
        let prev = [state(5, Vec3f::ZERO, 1)];
        let cur = [state(5, Vec3f::new(10.0, 0.0, 0.0), 2)];
        let views = entity_views("baseq2", &cur, &prev, 0.25, 0, 0);
        assert_eq!(views[0].origin, Vec3f::new(2.5, 0.0, 0.0));
        assert_eq!((views[0].frame, views[0].oldframe), (2, 1));
        assert_eq!(views[0].backlerp, 0.75);
    }

    #[test]
    fn new_entity_is_drawn_where_it_is_now() {
        let cur = [state(5, Vec3f::new(10.0, 0.0, 0.0), 2)];
        let views = entity_views("baseq2", &cur, &[], 0.5, 0, 0);
        assert_eq!(views[0].origin, Vec3f::new(10.0, 0.0, 0.0));
        assert_eq!(views[0].oldframe, 2);
    }

    #[test]
    fn new_entity_lerps_in_from_its_old_origin() {
        // C CL_DeltaEntity: prev.origin = state->old_origin.
        let mut cur = state(5, Vec3f::new(10.0, 0.0, 0.0), 2);
        cur.old_origin = Vec3f::new(2.0, 0.0, 0.0);
        let views = entity_views("baseq2", &[cur], &[], 0.5, 0, 0);
        assert_eq!(views[0].origin, Vec3f::new(6.0, 0.0, 0.0));
    }

    #[test]
    fn other_teleport_starts_at_the_new_origin_not_the_old_one() {
        let prev = state(5, Vec3f::ZERO, 1);
        let mut cur = state(5, Vec3f::new(10.0, 0.0, 0.0), 2);
        cur.old_origin = Vec3f::new(2.0, 0.0, 0.0);
        cur.event = EV_OTHER_TELEPORT;
        assert_eq!(half_way(prev.clone(), cur.clone()).origin, cur.origin);
        cur.event = EV_PLAYER_TELEPORT;
        assert_eq!(half_way(prev, cur).origin, Vec3f::new(6.0, 0.0, 0.0));
    }

    #[test]
    fn angles_lerp_the_short_way_round() {
        let mut a = state(5, Vec3f::ZERO, 0);
        a.angles.y = 350.0;
        let mut b = a.clone();
        b.angles.y = 10.0;
        let views = entity_views("baseq2", &[b], &[a], 0.5, 0, 0);
        assert_eq!(views[0].angles.y, 360.0);
    }

    #[test]
    fn rotating_items_spin_with_time_and_the_own_player_is_viewer_only() {
        let mut item = state(9, Vec3f::ZERO, 0);
        item.effects = EF_ROTATE;
        let player = state(1, Vec3f::ZERO, 0);
        let views = entity_views("baseq2", &[player, item], &[], 1.0, 900, 0);
        assert_ne!(views[0].flags & RF_VIEWERMODEL, 0);
        assert!(
            (views[1].angles.y - 90.0).abs() < 0.01,
            "{:?}",
            views[1].angles
        );
        assert_eq!(views[1].flags & RF_VIEWERMODEL, 0);
    }

    #[test]
    fn entities_without_a_model_are_not_drawn() {
        let mut sound_only = state(4, Vec3f::ZERO, 0);
        sound_only.modelindex = 0;
        assert!(entity_views("baseq2", &[sound_only], &[], 1.0, 0, 0).is_empty());
    }

    #[test]
    fn a_beam_draws_one_of_its_four_colours_at_0_30_without_a_model() {
        let mut laser = state(4, Vec3f::ZERO, 0);
        laser.renderfx = RF_BEAM | RF_TRANSLUCENT;
        laser.modelindex = 1;
        laser.skinnum = 0xf2f2_f0f0_u32 as i32;
        let plain = state(5, Vec3f::ZERO, 0);
        let mut views = entity_views("baseq2", &[laser, plain], &[], 1.0, 0, 0);
        // randk() % 4 picks the byte: 2 is bits 16..24.
        pick_beam_colours(&mut views, &mut || 6);
        assert_eq!(
            (views[0].skinnum, views[0].alpha, views[0].modelindex),
            (0xf2, 0.30, 0)
        );
        assert_eq!(views[1].skinnum, 0, "only beams");
        assert_eq!(views[1].modelindex, plain_modelindex());
    }

    fn plain_modelindex() -> i32 {
        state(5, Vec3f::ZERO, 0).modelindex
    }

    #[test]
    fn spinning_lights_spin_on_client_time_upside_down() {
        let mut s = state(4, Vec3f::ZERO, 0);
        s.effects = EF_SPINNINGLIGHTS;
        s.angles = Vec3f::new(10.0, 30.0, 0.0);
        let view = &entity_views("baseq2", &[s], &[], 1.0, 200, 0)[0];
        // C: (0, anglemod(cl.time / 2) + angles[1], 180).
        assert!(
            (view.angles - Vec3f::new(0.0, 130.0, 180.0)).length() < 0.01,
            "{:?}",
            view.angles
        );
    }

    #[test]
    fn bfg_plasma_and_sphere_effects_draw_translucent_at_their_alpha() {
        for (effects, alpha) in [
            (EF_BFG, 0.30),
            (EF_PLASMA, 0.6),
            (EF_SPHERETRANS, 0.3),
            (EF_SPHERETRANS | EF_TRACKERTRAIL, 0.6),
        ] {
            let mut s = state(4, Vec3f::ZERO, 0);
            s.effects = effects;
            let view = &entity_views("baseq2", &[s], &[], 1.0, 0, 0)[0];
            assert_ne!(view.flags & RF_TRANSLUCENT, 0);
            assert_eq!(view.alpha, alpha, "{effects:#x}");
        }
    }

    #[test]
    fn a_view_keeps_its_entity_number_and_effects() {
        let mut s = state(4, Vec3f::ZERO, 0);
        s.effects = EF_POWERSCREEN;
        let view = &entity_views("baseq2", &[s], &[], 1.0, 0, 0)[0];
        assert_eq!((view.number, view.effects), (4, EF_POWERSCREEN));
    }

    #[test]
    fn a_quad_entity_draws_plain_then_again_as_a_blue_shell() {
        let mut s = state(4, Vec3f::ZERO, 3);
        s.effects = EF_QUAD;
        s.renderfx = RF_TRANSLUCENT;
        let views = entity_views("baseq2", &[s], &[], 1.0, 0, 0);
        assert_eq!(views.len(), 2);
        // C: the renderfx go on the shell; the model keeps C's 0.70 alpha.
        assert_eq!(views[0].flags, 0);
        assert_eq!(
            views[1].flags,
            RF_SHELL_BLUE | RF_TRANSLUCENT,
            "{:#x}",
            views[1].flags
        );
        assert_eq!(views[1].alpha, 0.30);
        assert_eq!(
            (views[1].modelindex, views[1].frame),
            (views[0].modelindex, views[0].frame)
        );
    }

    #[test]
    fn pent_double_half_damage_and_server_shells_each_pick_their_colour() {
        for (effects, renderfx, shell) in [
            (EF_PENT, 0, RF_SHELL_RED),
            (EF_DOUBLE, 0, RF_SHELL_DOUBLE),
            (EF_HALF_DAMAGE, 0, RF_SHELL_HALF_DAM),
            (EF_COLOR_SHELL, RF_SHELL_GREEN, RF_SHELL_GREEN),
        ] {
            let mut s = state(4, Vec3f::ZERO, 0);
            s.effects = effects;
            s.renderfx = renderfx;
            let views = entity_views("baseq2", &[s], &[], 1.0, 0, 0);
            assert_eq!(views[1].flags, shell | RF_TRANSLUCENT, "{effects:#x}");
        }
    }

    #[test]
    fn the_viewer_gets_no_shell() {
        let mut s = state(1, Vec3f::ZERO, 0);
        s.effects = EF_QUAD;
        let views = entity_views("baseq2", &[s], &[], 1.0, 0, 0);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].flags, RF_VIEWERMODEL);
    }

    #[test]
    fn under_the_rogue_game_a_quad_double_shell_turns_cyan() {
        let mut s = state(4, Vec3f::ZERO, 0);
        s.effects = EF_QUAD | EF_DOUBLE;
        let shell = |game| entity_views(game, std::slice::from_ref(&s), &[], 1.0, 0, 0)[1].flags;
        assert_eq!(
            shell("rogue"),
            RF_SHELL_BLUE | RF_SHELL_GREEN | RF_TRANSLUCENT
        );
        assert_eq!(
            shell("baseq2"),
            RF_SHELL_BLUE | RF_SHELL_DOUBLE | RF_TRANSLUCENT
        );
    }

    #[test]
    fn rogue_turns_clashing_shells_into_c_colours() {
        let rogue = |fx| shell_combinations(fx, true);
        assert_eq!(
            rogue(RF_SHELL_RED | RF_SHELL_DOUBLE),
            RF_SHELL_RED | RF_SHELL_BLUE
        );
        assert_eq!(
            rogue(RF_SHELL_BLUE | RF_SHELL_DOUBLE),
            RF_SHELL_BLUE | RF_SHELL_GREEN
        );
        assert_eq!(
            rogue(RF_SHELL_BLUE | RF_SHELL_GREEN | RF_SHELL_DOUBLE),
            RF_SHELL_GREEN
        );
        assert_eq!(rogue(RF_SHELL_HALF_DAM | RF_SHELL_RED), RF_SHELL_RED);
        assert_eq!(rogue(RF_SHELL_DOUBLE), RF_SHELL_DOUBLE);
        // Only the rogue game dir recombines them.
        let mixed = RF_SHELL_RED | RF_SHELL_DOUBLE | RF_SHELL_HALF_DAM;
        assert_eq!(shell_combinations(mixed, false), mixed);
    }

    #[test]
    fn translucent_entities_draw_at_c_alpha() {
        let mut s = state(4, Vec3f::ZERO, 0);
        s.renderfx = RF_TRANSLUCENT;
        assert_eq!(entity_views("baseq2", &[s], &[], 1.0, 0, 0)[0].alpha, 0.70);
    }

    #[test]
    fn frame_lerped_and_beam_entities_step_their_origin() {
        // C: the frames animate the move, so the origin is not lerped.
        for renderfx in [RF_FRAMELERP, RF_BEAM] {
            let prev = [state(5, Vec3f::ZERO, 1)];
            let mut cur = state(5, Vec3f::new(10.0, 0.0, 0.0), 2);
            cur.renderfx = renderfx;
            cur.old_origin = Vec3f::new(4.0, 0.0, 0.0);
            let views = entity_views("baseq2", &[cur], &prev, 0.25, 0, 0);
            assert_eq!(views[0].origin, Vec3f::new(10.0, 0.0, 0.0));
            assert_eq!(views[0].oldorigin, Vec3f::new(4.0, 0.0, 0.0));
            assert_eq!((views[0].oldframe, views[0].backlerp), (1, 0.75));
        }
    }

    #[test]
    fn lerped_entity_oldorigin_is_its_drawn_origin() {
        let prev = [state(5, Vec3f::ZERO, 1)];
        let cur = [state(5, Vec3f::new(10.0, 0.0, 0.0), 2)];
        let views = entity_views("baseq2", &cur, &prev, 0.5, 0, 0);
        assert_eq!(views[0].oldorigin, views[0].origin);
    }

    /// The view of `cur` after `prev`, half way between them.
    fn half_way(prev: EntityState, cur: EntityState) -> EntityView {
        entity_views("baseq2", &[cur], &[prev], 0.5, 0, 0).remove(0)
    }

    #[test]
    fn model_change_does_not_lerp() {
        let prev = state(5, Vec3f::ZERO, 7);
        let mut cur = state(5, Vec3f::new(10.0, 0.0, 0.0), 2);
        cur.modelindex = 4;
        let view = half_way(prev.clone(), cur.clone());
        assert_eq!((view.origin, view.oldframe), (cur.origin, 2));

        let mut cur = state(5, Vec3f::new(10.0, 0.0, 0.0), 2);
        cur.modelindex3 = 9;
        assert_eq!(half_way(prev, cur.clone()).origin, cur.origin);
    }

    #[test]
    fn teleport_events_do_not_lerp() {
        for event in [EV_PLAYER_TELEPORT, EV_OTHER_TELEPORT] {
            let prev = state(5, Vec3f::ZERO, 1);
            let mut cur = state(5, Vec3f::new(10.0, 0.0, 0.0), 2);
            cur.event = event;
            assert_eq!(half_way(prev, cur.clone()).origin, cur.origin);
        }
    }

    #[test]
    fn jumps_over_512_units_on_an_axis_do_not_lerp() {
        let prev = state(5, Vec3f::ZERO, 1);
        let far = state(5, Vec3f::new(0.0, 0.0, -513.0), 1);
        assert_eq!(half_way(prev.clone(), far.clone()).origin, far.origin);
        let edge = state(5, Vec3f::new(512.9, 0.0, 0.0), 1);
        assert_eq!(
            half_way(prev, edge).origin,
            Vec3f::new(256.45, 0.0, 0.0),
            "C truncates the delta to int before comparing"
        );
    }

    #[test]
    fn animated_effects_pick_the_frame_from_client_time() {
        // C autoanim = 2 * cl.time / 1000; EF_ANIM_ALLFAST uses cl.time / 100.
        let frame = |effects: u32, time_ms: i32| {
            let mut s = state(5, Vec3f::ZERO, 9);
            s.effects = effects;
            entity_views("baseq2", &[s], &[], 1.0, time_ms, 0)[0].frame
        };
        assert_eq!((frame(EF_ANIM01, 400), frame(EF_ANIM01, 500)), (0, 1));
        assert_eq!((frame(EF_ANIM23, 400), frame(EF_ANIM23, 500)), (2, 3));
        assert_eq!(frame(EF_ANIM_ALL, 2600), 5);
        assert_eq!(frame(EF_ANIM_ALLFAST, 2600), 26);
        assert_eq!(frame(0, 2600), 9);
    }

    #[test]
    fn anglemod_scales_in_double_like_c() {
        // C promotes to double before truncating to 16-bit units: at
        // cl.time 2069, a = 206.9 gives 37664 units, not f32's 37665.
        assert_eq!(anglemod(2069.0 * 0.1), 37664.0 * (360.0 / 65536.0));
    }
}
