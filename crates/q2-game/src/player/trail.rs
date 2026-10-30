//! Player trail: the spots a player left while out of sight of the last
//! one, which monsters follow after losing sight of their enemy.
//! C ref: `player/trail.c`.
//!
//! C keeps the eight nodes as `player_trail` edicts; only their origin,
//! timestamp and yaw are ever read, so here they are plain data on
//! `GameWorld` and take no entity slots. As in C, there is no trail in
//! deathmatch.

use q2_shared::types::Vec3f;

use crate::entity::EntityKey;
use crate::monster::movement::vectoyaw;
use crate::world::GameWorld;

/// Number of trail nodes (a power of two: C wraps with a mask).
pub const TRAIL_LENGTH: usize = 8;

const fn next(n: usize) -> usize {
    (n + 1) & (TRAIL_LENGTH - 1)
}

const fn prev(n: usize) -> usize {
    (n + TRAIL_LENGTH - 1) & (TRAIL_LENGTH - 1)
}

/// One breadcrumb: where, when, and the yaw from the previous one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrailNode {
    pub origin: Vec3f,
    pub timestamp: f32,
    pub yaw: f32,
}

/// Ring of trail nodes; `head` is the oldest, the next to be overwritten.
#[derive(Debug, Clone, Default)]
pub struct PlayerTrail {
    nodes: [TrailNode; TRAIL_LENGTH],
    head: usize,
    active: bool,
}

impl PlayerTrail {
    /// Fresh nodes at the map origin with timestamp 0, as C's newly spawned
    /// edicts. C ref: `PlayerTrail_Init`.
    pub fn init(&mut self) {
        *self = Self {
            active: true,
            ..Self::default()
        };
    }

    /// C ref: `PlayerTrail_Add`.
    pub fn add(&mut self, spot: Vec3f, time: f32) {
        if !self.active {
            return;
        }
        let from = self.nodes[prev(self.head)].origin;
        self.nodes[self.head] = TrailNode {
            origin: spot,
            timestamp: time,
            yaw: vectoyaw(spot - from),
        };
        self.head = next(self.head);
    }

    /// The newest node. C ref: `PlayerTrail_LastSpot`.
    pub fn last_spot(&self) -> Option<TrailNode> {
        self.active.then(|| self.nodes[prev(self.head)])
    }

    /// Index of the oldest node newer than `trail_time`, or `head` again
    /// when the monster has followed them all.
    fn first_unfollowed(&self, trail_time: f32) -> usize {
        let mut marker = self.head;
        for _ in 0..TRAIL_LENGTH {
            if self.nodes[marker].timestamp > trail_time {
                break;
            }
            marker = next(marker);
        }
        marker
    }

    /// C ref: `PlayerTrail_PickNext`.
    pub fn pick_next(&self, trail_time: f32) -> Option<TrailNode> {
        self.active
            .then(|| self.nodes[self.first_unfollowed(trail_time)])
    }
}

impl GameWorld {
    /// The first node `key` has not followed, or the one before it when
    /// only that one is in sight. C ref: `PlayerTrail_PickFirst`.
    pub fn player_trail_pick_first(&self, key: EntityKey) -> Option<TrailNode> {
        if !self.trail.active {
            return None;
        }
        let trail_time = self
            .entities
            .get(key)
            .and_then(|e| e.monsterinfo.as_ref())
            .map_or(0.0, |mi| mi.trail_time);
        let marker = self.trail.first_unfollowed(trail_time);
        let first = self.trail.nodes[marker];
        if crate::ai::visible_point(self, key, first.origin) {
            return Some(first);
        }
        let before = self.trail.nodes[prev(marker)];
        if crate::ai::visible_point(self, key, before.origin) {
            return Some(before);
        }
        Some(first)
    }

    /// Drop a node where the player was if the newest one is out of its
    /// sight. C ref: the trail part of `ClientBeginServerFrame`.
    pub(crate) fn player_trail_follow(&mut self, player: EntityKey) {
        if self.game.deathmatch {
            return;
        }
        let Some(last) = self.trail.last_spot() else {
            return;
        };
        if crate::ai::visible_point(self, player, last.origin) {
            return;
        }
        let Some(old_origin) = self.entities.get(player).map(|e| e.state.old_origin) else {
            return;
        };
        self.trail.add(old_origin, self.level.time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster::test_support::{floor_world, walker};

    fn trail_with(times: &[f32]) -> PlayerTrail {
        let mut trail = PlayerTrail::default();
        trail.init();
        for (i, &t) in times.iter().enumerate() {
            trail.add(Vec3f::new(100.0 * (i + 1) as f32, 0.0, 0.0), t);
        }
        trail
    }

    #[test]
    fn trail_is_inactive_until_init() {
        let mut trail = PlayerTrail::default();
        trail.add(Vec3f::X, 1.0);
        assert!(trail.last_spot().is_none());
        assert!(trail.pick_next(0.0).is_none());
    }

    #[test]
    fn trail_add_records_time_and_yaw_from_the_previous_node() {
        let trail = trail_with(&[1.0]);
        let last = trail.last_spot().unwrap();
        assert_eq!(last.origin, Vec3f::new(100.0, 0.0, 0.0));
        assert_eq!(last.timestamp, 1.0);
        assert_eq!(last.yaw, 0.0); // from the origin, along +X
        let mut trail = trail;
        trail.add(Vec3f::new(100.0, 50.0, 0.0), 2.0);
        assert_eq!(trail.last_spot().unwrap().yaw, 90.0);
    }

    #[test]
    fn trail_wraps_and_overwrites_the_oldest_node() {
        let times: Vec<f32> = (1..=10).map(|t| t as f32).collect();
        let trail = trail_with(&times);
        assert_eq!(trail.last_spot().unwrap().timestamp, 10.0);
        // Nodes 1 and 2 were overwritten: the oldest left is 3.
        assert_eq!(trail.pick_next(0.0).unwrap().timestamp, 3.0);
    }

    #[test]
    fn trail_pick_next_skips_nodes_the_monster_already_followed() {
        let trail = trail_with(&[1.0, 2.0, 3.0]);
        assert_eq!(trail.pick_next(1.5).unwrap().timestamp, 2.0);
        assert_eq!(trail.pick_next(2.0).unwrap().timestamp, 3.0);
    }

    #[test]
    fn trail_pick_next_returns_an_unused_origin_node_when_all_are_followed() {
        // C quirk kept: the marker wraps back to head, a timestamp-0 node.
        let trail = trail_with(&[1.0, 2.0]);
        assert_eq!(trail.pick_next(5.0).unwrap(), TrailNode::default());
    }

    #[test]
    fn trail_pick_first_skips_nodes_the_monster_already_followed() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.trail = trail_with(&[1.0, 2.0, 3.0]);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .trail_time = 1.0;
        let node = world.player_trail_pick_first(key).unwrap();
        assert_eq!(node.timestamp, 2.0);
    }

    #[test]
    fn trail_pick_first_falls_back_to_the_previous_node_in_sight() {
        // Node 2 (x = 200) is behind the wall at x = 150, node 1 is not.
        let mut world = floor_world(Some(150.0));
        let key = walker(&mut world, 0.0, 0.0);
        world.trail = trail_with(&[1.0, 2.0, 3.0]);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .trail_time = 1.0;
        let node = world.player_trail_pick_first(key).unwrap();
        assert_eq!(node.timestamp, 1.0);
    }

    #[test]
    fn client_frame_adds_no_trail_node_during_intermission_or_when_dead() {
        let mut world = floor_world(Some(200.0));
        let player = walker(&mut world, 100.0, 0.0);
        world.trail.init();
        world.trail.add(Vec3f::new(300.0, 0.0, 24.0), 3.0); // out of sight
        let before = world.trail.last_spot();

        world.level.intermission_time = 1.0;
        world.client_begin_server_frame(player);
        assert_eq!(world.trail.last_spot(), before);

        world.level.intermission_time = 0.0;
        world.entities.get_mut(player).unwrap().game.deadflag = crate::constants::DeadFlag::Dead;
        world.client_begin_server_frame(player);
        assert_eq!(world.trail.last_spot(), before);

        world.entities.get_mut(player).unwrap().game.deadflag = crate::constants::DeadFlag::No;
        world.client_begin_server_frame(player);
        assert_ne!(world.trail.last_spot(), before, "alive and in play: added");
    }

    #[test]
    fn deathmatch_has_no_player_trail() {
        // C PlayerTrail_Init returns in deathmatch, and ClientBeginServerFrame
        // adds trail spots only when !deathmatch.
        for (deathmatch, active) in [(false, true), (true, false)] {
            let mut world = crate::world::test_world();
            world.game.deathmatch = deathmatch;
            world.init_game();
            world.spawn_entities("base1", "{ \"classname\" \"worldspawn\" }", "");
            assert_eq!(world.trail.last_spot().is_some(), active);
        }

        let mut world = floor_world(Some(200.0));
        let player = walker(&mut world, 100.0, 0.0);
        world.trail.init();
        world.trail.add(Vec3f::new(300.0, 0.0, 24.0), 3.0); // out of sight
        let before = world.trail.last_spot();
        world.game.deathmatch = true;
        world.client_begin_server_frame(player);
        assert_eq!(world.trail.last_spot(), before);
    }

    #[test]
    fn client_frame_adds_a_trail_node_only_when_the_last_spot_is_out_of_sight() {
        let mut world = floor_world(Some(200.0));
        let player = walker(&mut world, 100.0, 0.0);
        world.entities.get_mut(player).unwrap().state.old_origin = Vec3f::new(90.0, 0.0, 24.0);
        world.level.time = 4.0;
        world.trail.init();

        // The newest node (the origin) is in sight: nothing is added.
        world.player_trail_follow(player);
        assert_eq!(world.trail.last_spot().unwrap(), TrailNode::default());

        // Behind the wall: the player's old origin becomes the newest node.
        world.trail.add(Vec3f::new(300.0, 0.0, 24.0), 3.0);
        world.player_trail_follow(player);
        let last = world.trail.last_spot().unwrap();
        assert_eq!(last.origin, Vec3f::new(90.0, 0.0, 24.0));
        assert_eq!(last.timestamp, 4.0);
    }
}
