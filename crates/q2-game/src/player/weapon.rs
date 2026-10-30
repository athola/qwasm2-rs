//! Player noises monsters hear. C ref: `player/weapon.c` (`PlayerNoise`).
//! The weapon state machine around it is #16.

use q2_shared::constants::SVF_NOCLIENT;
use q2_shared::types::Vec3f;

use crate::constants::EntityFlags;
use crate::entity::EntityKey;
use crate::world::GameWorld;

/// `spawnflags` of a player's own noise marker (C `PLAYER_NOISE_SELF`).
pub const PLAYER_NOISE_SELF: i32 = 0;
/// `spawnflags` of a player's impact noise marker (C `PLAYER_NOISE_IMPACT`).
pub const PLAYER_NOISE_IMPACT: i32 = 1;

/// What made a player noise (C `PNOISE_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerNoise {
    /// Jumping, splashing, gasping (C `PNOISE_SELF`).
    SelfNoise,
    /// Firing a weapon (C `PNOISE_WEAPON`).
    Weapon,
    /// Where a shot landed (C `PNOISE_IMPACT`).
    Impact,
}

impl GameWorld {
    /// Each player owns two invisible markers that monsters hear: one for
    /// its own and weapon noises (`level.sound_entity`), one for impacts
    /// (`level.sound2_entity`, heard only by monsters without an enemy).
    /// A noise moves the marker to `at`, at most once every 4 frames per
    /// kind. Deathmatch monsters hear nothing; a notarget player makes no
    /// sound, and a silenced shot uses up a silencer charge instead.
    /// C ref: `PlayerNoise` (player/weapon.c). Markers are made on first
    /// use as in id's original; yquake2's `PlayerNoise_Verify` scan for
    /// orphans from a loaded save waits on save/load (#15).
    pub fn player_noise(&mut self, who: EntityKey, at: Vec3f, kind: PlayerNoise) {
        if self.client(who).is_none() {
            return;
        }
        if kind == PlayerNoise::Weapon {
            if let Some(client) = self.client_mut(who).filter(|c| c.silencer_shots > 0) {
                client.silencer_shots -= 1;
                return;
            }
        }
        if self.game.deathmatch
            || self
                .entities
                .get(who)
                .is_some_and(|e| e.game.flags.contains(EntityFlags::NOTARGET))
        {
            return;
        }
        self.player_noise_verify(who);

        let framenum = self.level.framenum;
        let noise = if kind == PlayerNoise::Impact {
            if framenum <= self.level.sound2_entity_framenum + 3 {
                return;
            }
            let Some(noise) = self.entities.get(who).and_then(|e| e.game.mynoise2) else {
                return;
            };
            self.level.sound2_entity = Some(noise);
            self.level.sound2_entity_framenum = framenum;
            noise
        } else {
            if framenum <= self.level.sound_entity_framenum + 3 {
                return;
            }
            let Some(noise) = self.entities.get(who).and_then(|e| e.game.mynoise) else {
                return;
            };
            self.level.sound_entity = Some(noise);
            self.level.sound_entity_framenum = framenum;
            noise
        };
        let time = self.level.time;
        if let Some(ent) = self.entities.get_mut(noise) {
            ent.state.origin = at;
            ent.game.last_sound_time = time;
        }
        self.link_entity(noise);
    }

    /// Make the player's markers that are missing or freed.
    /// C ref: `PlayerNoise_Verify` / `PlayerNoise_Spawn`.
    fn player_noise_verify(&mut self, who: EntityKey) {
        let Some(me) = self.entities.get(who) else {
            return;
        };
        let live = |k: Option<EntityKey>| k.filter(|&k| self.entities.is_live(k));
        let (mut n1, mut n2) = (live(me.game.mynoise), live(me.game.mynoise2));
        if n1.is_none() {
            n1 = self.player_noise_spawn(who, PLAYER_NOISE_SELF);
        }
        if n2.is_none() {
            n2 = self.player_noise_spawn(who, PLAYER_NOISE_IMPACT);
        }
        if let Some(me) = self.entities.get_mut(who) {
            me.game.mynoise = n1;
            me.game.mynoise2 = n2;
        }
    }

    /// C ref: `PlayerNoise_Spawn` (player/weapon.c).
    fn player_noise_spawn(&mut self, who: EntityKey, spawnflags: i32) -> Option<EntityKey> {
        let noise = self.spawn()?;
        let ent = self.entities.get_mut(noise)?;
        ent.game.classname = "player_noise".to_string();
        ent.game.spawnflags = spawnflags;
        ent.mins = Vec3f::splat(-8.0);
        ent.maxs = Vec3f::splat(8.0);
        ent.owner = Some(who);
        ent.svflags = SVF_NOCLIENT;
        Some(noise)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::AiFlags;
    use crate::entity::{ClientData, MonsterInfo};
    use crate::world::test_world;

    fn player(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn_test_client(ClientData::default());
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        key
    }

    /// A frame late enough that C's "+ 3 frames" throttle has passed.
    fn noisy_world() -> GameWorld {
        let mut world = test_world();
        world.level.framenum = 10;
        world.level.sound_entity_framenum = -10;
        world.level.sound2_entity_framenum = -10;
        world
    }

    #[test]
    fn player_noise_self_moves_the_players_marker_there_and_makes_it_the_sound_entity() {
        let mut world = noisy_world();
        let who = player(&mut world);
        let at = Vec3f::new(10.0, 20.0, 30.0);

        world.player_noise(who, at, PlayerNoise::SelfNoise);

        let noise = world.level.sound_entity.expect("a noise");
        assert_eq!(world.level.sound_entity_framenum, 10);
        assert_eq!(world.entities.get(who).unwrap().game.mynoise, Some(noise));
        let ent = world.entities.get(noise).unwrap();
        assert_eq!(ent.game.classname, "player_noise");
        assert_eq!(ent.game.spawnflags, PLAYER_NOISE_SELF);
        assert_eq!(ent.owner, Some(who));
        assert_eq!(ent.svflags, SVF_NOCLIENT);
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::splat(-8.0), Vec3f::splat(8.0))
        );
        assert_eq!(ent.state.origin, at);
        assert!(ent.linkcount > 0, "linked");
        assert_eq!(world.level.sound2_entity, None);
    }

    #[test]
    fn player_noise_stamps_its_marker_with_the_time() {
        // C PlayerNoise: noise->last_sound_time = level.time.
        let mut world = noisy_world();
        let who = player(&mut world);
        world.level.time = 1.0;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);
        let noise = world.level.sound2_entity.expect("a noise");
        assert_eq!(world.entities.get(noise).unwrap().game.last_sound_time, 1.0);
    }

    #[test]
    fn player_noise_weapon_uses_the_same_marker_as_self() {
        let mut world = noisy_world();
        let who = player(&mut world);

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Weapon);

        let mynoise = world.entities.get(who).unwrap().game.mynoise;
        assert_eq!(world.level.sound_entity, mynoise);
        assert!(mynoise.is_some());
    }

    #[test]
    fn player_noise_impact_moves_the_second_marker_and_makes_it_sound2() {
        let mut world = noisy_world();
        let who = player(&mut world);
        let at = Vec3f::new(-5.0, 0.0, 0.0);

        world.player_noise(who, at, PlayerNoise::Impact);

        let noise = world.level.sound2_entity.expect("an impact noise");
        assert_eq!(world.level.sound2_entity_framenum, 10);
        assert_eq!(world.level.sound_entity, None);
        let me = world.entities.get(who).unwrap();
        assert_eq!(me.game.mynoise2, Some(noise));
        assert_ne!(me.game.mynoise, Some(noise));
        let ent = world.entities.get(noise).unwrap();
        assert_eq!(ent.game.spawnflags, PLAYER_NOISE_IMPACT);
        assert_eq!(ent.state.origin, at);
    }

    #[test]
    fn player_noise_waits_three_frames_after_the_last_noise() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.level.sound_entity_framenum = 7;
        world.level.sound2_entity_framenum = 7;

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);
        assert_eq!(world.level.sound_entity, None);
        assert_eq!(world.level.sound2_entity, None);

        world.level.framenum = 11;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);
        assert_eq!(world.level.sound_entity_framenum, 11);
        assert_eq!(world.level.sound2_entity_framenum, 11);
    }

    #[test]
    fn player_noise_keeps_the_players_markers_from_noise_to_noise() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        let first = world.level.sound_entity;
        let count = world.entities.count();

        world.level.framenum = 20;
        world.player_noise(who, Vec3f::X, PlayerNoise::SelfNoise);

        assert_eq!(world.level.sound_entity, first);
        assert_eq!(world.entities.count(), count);
    }

    #[test]
    fn player_noise_replaces_a_freed_marker() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        let first = world.level.sound_entity.unwrap();
        world.free_entity(first);

        world.level.framenum = 20;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);

        let second = world.level.sound_entity.unwrap();
        assert_ne!(second, first);
        assert!(world.entities.is_live(second));
    }

    #[test]
    fn player_noise_is_silent_in_deathmatch() {
        let mut world = noisy_world();
        world.game.deathmatch = true;
        let who = player(&mut world);

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);

        assert_eq!(world.level.sound_entity, None);
        assert_eq!(world.entities.get(who).unwrap().game.mynoise, None);
    }

    #[test]
    fn player_noise_is_silent_for_a_notarget_player() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.entities.get_mut(who).unwrap().game.flags |= EntityFlags::NOTARGET;

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Impact);

        assert_eq!(world.level.sound2_entity, None);
    }

    #[test]
    fn player_noise_ignores_entities_without_a_client() {
        let mut world = noisy_world();
        let monster = world.spawn().unwrap();

        world.player_noise(monster, Vec3f::ZERO, PlayerNoise::SelfNoise);

        assert_eq!(world.level.sound_entity, None);
        assert_eq!(world.entities.count(), 1);
    }

    #[test]
    fn player_noise_weapon_uses_up_a_silenced_shot_instead() {
        let mut world = noisy_world();
        let who = player(&mut world);
        world.client_mut(who).unwrap().silencer_shots = 1;

        world.player_noise(who, Vec3f::ZERO, PlayerNoise::Weapon);
        assert_eq!(world.client(who).unwrap().silencer_shots, 0);
        assert_eq!(world.level.sound_entity, None);

        // The silencer muffles only weapon fire.
        world.client_mut(who).unwrap().silencer_shots = 1;
        world.player_noise(who, Vec3f::ZERO, PlayerNoise::SelfNoise);
        assert_eq!(world.client(who).unwrap().silencer_shots, 1);
        assert!(world.level.sound_entity.is_some());
    }

    /// A monster with no enemy standing 300 units from the player.
    fn listening_monster(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::new(300.0, 0.0, 24.0);
        ent.svflags = q2_shared::constants::SVF_MONSTER;
        ent.game.health = 20;
        ent.monsterinfo = Some(Box::new(MonsterInfo::default()));
        key
    }

    #[test]
    fn a_player_firing_makes_a_noise_a_monster_in_hearing_range_hunts() {
        let mut world = noisy_world();
        let who = player(&mut world);
        // Behind a wall the monster cannot see through: it can only hear.
        world.entities.get_mut(who).unwrap().game.light_level = 0;
        let monster = listening_monster(&mut world);
        assert!(!crate::ai::find_target(&mut world, monster), "unseen");

        world.fire_blaster(
            who,
            Vec3f::new(0.0, 0.0, 30.0),
            Vec3f::Y,
            15,
            1000,
            0,
            false,
        );
        let noise = world.level.sound_entity.expect("the shot is heard");
        assert_eq!(world.entities.get(noise).unwrap().owner, Some(who));

        assert!(crate::ai::find_target(&mut world, monster));
        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(noise));
        assert!(ent
            .monsterinfo
            .as_ref()
            .unwrap()
            .aiflags
            .contains(AiFlags::SOUND_TARGET));
    }
}
