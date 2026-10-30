//! Player view — camera offset and view bob.
//! C ref: `player/view.c` (1,426 lines)

use q2_shared::types::*;

use crate::entity::EntityKey;
use crate::world::GameWorld;

/// C `bob_up` cvar default.
const BOB_UP: f32 = 0.005;

impl GameWorld {
    /// The view bob cycle and the eye offset it gives: eye height plus bob
    /// (at most 6), bounded to the player box. Fall height, kicks and the
    /// bob's view angles are not ported.
    /// C ref: `ClientEndServerFrame`'s bob setup and `SV_CalcViewOffset`
    /// (player/view.c).
    pub fn update_player_view(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get(key) else {
            return;
        };
        if ent.client.is_none() {
            return;
        }
        let (vel, on_ground) = (ent.velocity, ent.game.ground_entity.is_some());

        // Speed and cycle for all cyclic walking effects; bobbing only
        // cycles on the ground.
        let xyspeed = f64::from(vel.x * vel.x + vel.y * vel.y).sqrt() as f32;
        let restart = xyspeed < 5.0;
        if restart {
            self.game.bobmove = 0.0;
        } else if on_ground {
            self.game.bobmove = if xyspeed > 210.0 {
                0.25
            } else if xyspeed > 100.0 {
                0.125
            } else {
                0.0625
            };
        }
        let bobmove = self.game.bobmove;
        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        if restart {
            client.bobtime = 0.0; // start at beginning of cycle again
        }
        client.bobtime += bobmove;
        let mut bobtime = client.bobtime;
        if client.ps.pmove.pm_flags & q2_common::pmove::PMF_DUCKED != 0 {
            bobtime *= 4.0;
        }
        let bobfracsin = (f64::from(bobtime) * std::f64::consts::PI).sin().abs() as f32;

        // Eye height comes from the entity's viewheight, which pmove
        // lowers when ducking.
        let bob = (bobfracsin * xyspeed * BOB_UP).min(6.0);
        let z = (ent.game.viewheight + bob).clamp(-22.0, 30.0);
        client.ps.viewoffset = Vec3f::new(0.0, 0.0, z);
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityKey;
    use crate::world::{test_world, GameWorld};
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

    /// A player standing on the floor (or not) moving at `speed`; returns
    /// its eye height (22 plus the bob) after one view update.
    fn bob(world: &mut GameWorld, key: EntityKey, speed: f32, on_ground: bool) -> f32 {
        let ent = world.entities.get_mut(key).unwrap();
        ent.velocity = Vec3f::new(speed, 0.0, 0.0);
        ent.game.ground_entity = on_ground.then_some(key);
        world.update_player_view(key);
        world.client(key).unwrap().ps.viewoffset.z
    }

    fn player(world: &mut GameWorld) -> EntityKey {
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.begin_client(key);
        key
    }

    /// C: 22 + fabs(sin(bobtime * M_PI)) * xyspeed * bob_up (0.005).
    fn c_bob(bobtime: f32, xyspeed: f32) -> f32 {
        let fracsin = (f64::from(bobtime) * std::f64::consts::PI).sin().abs() as f32;
        22.0 + fracsin * xyspeed * 0.005
    }

    #[test]
    fn view_bob_follows_the_c_bob_cycle_on_the_ground() {
        let mut world = test_world();
        let key = player(&mut world);
        // Over 210 units/s the cycle advances 0.25 a frame.
        assert_eq!(bob(&mut world, key, 300.0, true), c_bob(0.25, 300.0));
        assert_eq!(bob(&mut world, key, 300.0, true), c_bob(0.5, 300.0));
        // Off the ground the last bobmove keeps going (C's static).
        assert_eq!(bob(&mut world, key, 150.0, false), c_bob(0.75, 150.0));
        // Below 5 units/s the cycle restarts.
        assert_eq!(bob(&mut world, key, 4.0, true), 22.0);
        assert_eq!(world.client(key).unwrap().bobtime, 0.0);
        // 100..=210 advances 0.125, slower 0.0625.
        assert_eq!(bob(&mut world, key, 150.0, true), c_bob(0.125, 150.0));
        assert_eq!(bob(&mut world, key, 50.0, true), c_bob(0.1875, 50.0));
    }

    #[test]
    fn view_bob_is_at_most_6_and_ducking_quickens_it() {
        let mut world = test_world();
        let key = player(&mut world);
        world.client_mut(key).unwrap().bobtime = 0.25;
        assert_eq!(bob(&mut world, key, 3000.0, true), 28.0);
        world.client_mut(key).unwrap().bobtime = 0.0;
        world.client_mut(key).unwrap().ps.pmove.pm_flags |= q2_common::pmove::PMF_DUCKED;
        // bobtime 0.0625 runs as 0.25 when ducked.
        assert_eq!(bob(&mut world, key, 50.0, true), c_bob(0.25, 50.0));
    }

    #[test]
    fn view_offset_stays_inside_the_player_box() {
        let mut world = test_world();
        let key = player(&mut world);
        world.entities.get_mut(key).unwrap().game.viewheight = 40.0;
        world.update_player_view(key);
        assert_eq!(world.client(key).unwrap().ps.viewoffset.z, 30.0);
    }
}
