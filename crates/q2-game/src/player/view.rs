//! Player view — camera offset, view bob, damage kicks, fall effects.
//! C ref: `player/view.c` (1,426 lines)

use q2_shared::types::*;

use crate::entity::EntityKey;
use crate::world::GameWorld;

impl GameWorld {
    /// Calculate view offset for a player entity.
    /// Applies view bob, damage kick, and fall effects.
    /// C ref: `SV_CalcViewOffset` (player/view.c).
    pub fn update_player_view(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get(key) else {
            return;
        };
        if ent.client.is_none() {
            return;
        }

        // View bob based on movement speed.
        let speed = Vec3f::new(ent.velocity.x, ent.velocity.y, 0.0).length();
        let bob_time = self.level.time;
        let bob = if speed > 0.0 {
            (bob_time * 6.0).sin() * speed * 0.005
        } else {
            0.0
        };

        // Apply view offset to player state: eye height comes from the
        // entity's viewheight (C SV_CalcViewOffset), which pmove lowers when
        // ducking.
        // The last shot's kick (C: base angles are `kick_angles`, cleared
        // for the dead; the origin kick adds to the eye, clamped).
        if let Some((ent, client)) = self.entity_and_client_mut(key) {
            client.ps.kick_angles = if ent.game.deadflag != crate::constants::DeadFlag::No {
                Vec3f::ZERO
            } else {
                client.kick_angles
            };
            let v = Vec3f::new(0.0, 0.0, ent.game.viewheight + bob) + client.kick_origin;
            client.ps.viewoffset = Vec3f::new(
                v.x.clamp(-14.0, 14.0),
                v.y.clamp(-14.0, 14.0),
                v.z.clamp(-22.0, 30.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::world::test_world;
    use q2_shared::types::Vec3f;

    #[test]
    fn a_shot_kicks_the_view_for_one_frame_like_c() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        let client = world.client_mut(key).unwrap();
        client.kick_angles = Vec3f::new(-1.0, 0.0, 0.0);
        client.kick_origin = Vec3f::new(-2.0, 0.0, 0.0);

        world.client_end_server_frames();
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.kick_angles, Vec3f::new(-1.0, 0.0, 0.0));
        assert_eq!(client.ps.viewoffset.x, -2.0);
        assert_eq!(
            client.kick_angles,
            Vec3f::ZERO,
            "ClientEndServerFrame clears it"
        );
        assert_eq!(client.kick_origin, Vec3f::ZERO);

        world.client_end_server_frames();
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.kick_angles, Vec3f::ZERO);
        assert_eq!(client.ps.viewoffset.x, 0.0);
    }

    #[test]
    fn the_kick_offset_is_clamped_like_sv_calcviewoffset() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        world.client_mut(key).unwrap().kick_origin = Vec3f::new(-30.0, 30.0, 40.0);
        world.update_player_view(key);
        let offset = world.client(key).unwrap().ps.viewoffset;
        assert_eq!((offset.x, offset.y, offset.z), (-14.0, 14.0, 30.0));
    }

    #[test]
    fn a_dead_player_gets_no_kick_angles() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        world.entities.get_mut(key).unwrap().game.deadflag = crate::constants::DeadFlag::Dead;
        world.client_mut(key).unwrap().kick_angles = Vec3f::new(-1.0, 0.0, 0.0);
        world.update_player_view(key);
        assert_eq!(world.client(key).unwrap().ps.kick_angles, Vec3f::ZERO);
    }

    #[test]
    fn view_offset_default_height() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key); // viewheight 22
        world.entities.get_mut(key).unwrap().velocity = Vec3f::ZERO;

        world.update_player_view(key);

        let offset = world.client(key).unwrap().ps.viewoffset;
        assert!((offset.z - 22.0).abs() < 1.0);
    }

    #[test]
    fn view_offset_follows_the_entity_viewheight_like_sv_calcviewoffset() {
        // pmove sets viewheight to -2 when ducked; the eye must drop with it.
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().game.viewheight = -2.0;

        world.update_player_view(key);

        assert_eq!(world.client(key).unwrap().ps.viewoffset.z, -2.0);
    }

    #[test]
    fn view_bob_increases_with_speed() {
        let bob_at = |speed: f32| {
            let mut world = test_world();
            world.level.time = 1.0;
            let key = world.connect_client(1, "\\name\\P").unwrap();
            world.begin_client(key);
            let ent = world.entities.get_mut(key).unwrap();
            ent.velocity = Vec3f::new(speed, 0.0, 0.0);
            ent.game.ground_entity = Some(key);
            world.update_player_view(key);
            let ent = world.entities.get(key).unwrap();
            world.client(key).unwrap().ps.viewoffset.z - ent.game.viewheight
        };
        let (still, running) = (bob_at(0.0), bob_at(300.0));
        assert_eq!(still, 0.0);
        assert_ne!(running, still);
    }
}
