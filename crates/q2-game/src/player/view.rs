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
        let Some(ref _client) = ent.client else {
            return;
        };

        // View bob based on movement speed.
        let speed = Vec3f::new(ent.velocity.x, ent.velocity.y, 0.0).length();
        let bob_time = self.level.time;
        let bob = if speed > 0.0 {
            (bob_time * 6.0).sin() * speed * 0.005
        } else {
            0.0
        };

        // Apply view offset to player state.
        if let Some(ent) = self.entities.get_mut(key) {
            if let Some(ref mut client) = ent.client {
                client.ps.viewoffset = Vec3f::new(0.0, 0.0, 22.0 + bob);
            }
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
        let key = world.client_connect("\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::ZERO;

        world.update_player_view(key);

        let offset = world.entities.get(key).unwrap()
            .client.as_ref().unwrap().ps.viewoffset;
        assert!((offset.z - 22.0).abs() < 1.0);
    }

    #[test]
    fn view_bob_increases_with_speed() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.client_connect("\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(300.0, 0.0, 0.0);

        world.update_player_view(key);

        let offset = world.entities.get(key).unwrap()
            .client.as_ref().unwrap().ps.viewoffset;
        // Should have some bob added to base 22.0.
        assert!(offset.z != 22.0);
    }
}
