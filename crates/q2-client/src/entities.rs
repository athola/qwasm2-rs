//! Entities to draw between two server frames. C ref: `CL_AddPacketEntities`
//! (client/cl_entities.c), without the effects, beams and player skins.

use q2_shared::constants::{EF_ROTATE, RF_TRANSLUCENT, RF_VIEWERMODEL};
use q2_shared::types::{EntityState, Vec3f};

/// One entity as the renderer draws it this frame (C `entity_t`), with the
/// model still as its `CS_MODELS` index.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityView {
    pub modelindex: i32,
    pub frame: i32,
    pub oldframe: i32,
    /// 0 draws `frame`, 1 draws `oldframe` (C `backlerp`).
    pub backlerp: f32,
    pub origin: Vec3f,
    pub angles: Vec3f,
    pub skinnum: i32,
    /// `RF_*` flags.
    pub flags: i32,
    pub alpha: f32,
}

/// C `LerpAngle`: the short way round between two angles in degrees.
fn lerp_angle(a2: f32, a1: f32, frac: f32) -> f32 {
    let mut a1 = a1;
    if a1 - a2 > 180.0 {
        a1 -= 360.0;
    }
    if a1 - a2 < -180.0 {
        a1 += 360.0;
    }
    a2 + frac * (a1 - a2)
}

/// C `anglemod`, through 16-bit angle units like the original.
fn anglemod(a: f32) -> f32 {
    (360.0 / 65536.0) * (((a * (65536.0 / 360.0)) as i32) & 65535) as f32
}

/// The entities of the current frame, interpolated from the previous one
/// by `lerpfrac` (0 = previous frame, 1 = current). An entity that was not
/// in the previous frame is drawn where it is now. `time_ms` drives
/// `EF_ROTATE`; `playernum` is the local client's slot (its entity is
/// `playernum + 1`).
pub fn entity_views(
    current: &[EntityState],
    previous: &[EntityState],
    lerpfrac: f32,
    time_ms: f32,
    playernum: i32,
) -> Vec<EntityView> {
    let autorotate = anglemod(time_ms * 0.1);
    current
        .iter()
        .filter(|s| s.modelindex != 0)
        .map(|s| {
            let prev = previous.iter().find(|p| p.number == s.number).unwrap_or(s);
            let mut flags = s.renderfx;
            if s.number == playernum + 1 {
                flags |= RF_VIEWERMODEL; // only seen in mirrors / third person
            }
            let angles = if s.effects & EF_ROTATE != 0 {
                Vec3f::new(0.0, autorotate, 0.0)
            } else {
                Vec3f::new(
                    lerp_angle(prev.angles.x, s.angles.x, lerpfrac),
                    lerp_angle(prev.angles.y, s.angles.y, lerpfrac),
                    lerp_angle(prev.angles.z, s.angles.z, lerpfrac),
                )
            };
            EntityView {
                modelindex: s.modelindex,
                frame: s.frame,
                oldframe: prev.frame,
                backlerp: 1.0 - lerpfrac,
                origin: prev.origin + (s.origin - prev.origin) * lerpfrac,
                angles,
                skinnum: s.skinnum,
                flags,
                alpha: if flags & RF_TRANSLUCENT != 0 {
                    0.70
                } else {
                    1.0
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(number: i32, origin: Vec3f, frame: i32) -> EntityState {
        EntityState {
            number,
            origin,
            frame,
            modelindex: 3,
            ..Default::default()
        }
    }

    #[test]
    fn moving_entity_is_drawn_between_frames_with_frame_lerp() {
        let prev = [state(5, Vec3f::ZERO, 1)];
        let cur = [state(5, Vec3f::new(10.0, 0.0, 0.0), 2)];
        let views = entity_views(&cur, &prev, 0.25, 0.0, 0);
        assert_eq!(views[0].origin, Vec3f::new(2.5, 0.0, 0.0));
        assert_eq!((views[0].frame, views[0].oldframe), (2, 1));
        assert_eq!(views[0].backlerp, 0.75);
    }

    #[test]
    fn new_entity_is_drawn_where_it_is_now() {
        let cur = [state(5, Vec3f::new(10.0, 0.0, 0.0), 2)];
        let views = entity_views(&cur, &[], 0.5, 0.0, 0);
        assert_eq!(views[0].origin, Vec3f::new(10.0, 0.0, 0.0));
        assert_eq!(views[0].oldframe, 2);
    }

    #[test]
    fn angles_lerp_the_short_way_round() {
        let mut a = state(5, Vec3f::ZERO, 0);
        a.angles.y = 350.0;
        let mut b = a.clone();
        b.angles.y = 10.0;
        let views = entity_views(&[b], &[a], 0.5, 0.0, 0);
        assert_eq!(views[0].angles.y, 360.0);
    }

    #[test]
    fn rotating_items_spin_with_time_and_the_own_player_is_viewer_only() {
        let mut item = state(9, Vec3f::ZERO, 0);
        item.effects = EF_ROTATE;
        let player = state(1, Vec3f::ZERO, 0);
        let views = entity_views(&[player, item], &[], 1.0, 900.0, 0);
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
        assert!(entity_views(&[sound_only], &[], 1.0, 0.0, 0).is_empty());
    }

    #[test]
    fn translucent_entities_draw_at_c_alpha() {
        let mut s = state(4, Vec3f::ZERO, 0);
        s.renderfx = RF_TRANSLUCENT;
        assert_eq!(entity_views(&[s], &[], 1.0, 0.0, 0)[0].alpha, 0.70);
    }
}
