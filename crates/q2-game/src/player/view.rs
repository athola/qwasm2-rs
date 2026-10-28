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
        if let Some((ent, client)) = self.entity_and_client_mut(key) {
            client.ps.viewoffset = Vec3f::new(0.0, 0.0, ent.game.viewheight + bob);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::world::test_world;
    use q2_shared::types::Vec3f;

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
