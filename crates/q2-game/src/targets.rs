//! Target entities — produce effects when triggered.
//!
//! Port of `g_target.c`. Each target has a `use` callback that fires when
//! the entity is triggered. Ported: target_explosion, target_speaker,
//! target_changelevel (without intermission), target_secret, target_goal.
//! The other C target classes (temp_entity, splash, spawner, blaster,
//! crosslevel_*, laser, help, lightramp, earthquake) are registered as
//! relays that only fire their own targets until they are ported.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_target.c`

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage};
use crate::world::GameWorld;
use q2_shared::types::Vec3f;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Target use callbacks
// ---------------------------------------------------------------------------

/// Blast at the target: explosion effect, radius damage, then its own
/// targets. C ref: `target_explosion_explode`.
fn target_explosion_explode(world: &mut GameWorld, self_key: EntityKey) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let (dmg, origin, activator) = (
        ent.game.dmg as f32,
        ent.state.origin,
        ent.game.activator.unwrap_or(self_key),
    );
    world
        .gi
        .write_byte(q2_shared::protocol::SvcOp::TempEntity as i32);
    world.gi.write_byte(TE_EXPLOSION1);
    world.gi.write_position(origin);
    world.gi.multicast(origin, q2_shared::types::Multicast::PHS);

    world.t_radius_damage(
        self_key,
        activator,
        dmg,
        None,
        dmg + 40.0,
        MeansOfDeath::Explosive,
    );

    // Fire the targets now, not after another `delay`.
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    let save = std::mem::take(&mut ent.game.delay);
    world.use_targets(self_key, activator);
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.delay = save;
    }
}

/// C ref: `use_target_explosion`.
fn target_explosion_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.game.activator = Some(activator);
    if ent.game.delay == 0.0 {
        target_explosion_explode(world, self_key);
        return;
    }
    ent.think = Some(target_explosion_explode);
    ent.game.nextthink = time + ent.game.delay;
}

/// target_changelevel — exit to the next map through the intermission.
/// A deathmatch exit kills whoever touches it unless `DF_ALLOW_EXIT`.
/// C ref: `use_target_changelevel` (g_target.c).
fn target_changelevel_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    other: EntityKey,
    activator: EntityKey,
) {
    if world.level.intermission_time != 0.0 || world.level.exitintermission {
        return; // Already activated.
    }
    let (deathmatch, coop) = (world.game.deathmatch, world.game.coop);
    if !deathmatch && !coop {
        // A dead single player cannot exit (C reads g_edicts[1].health,
        // which is 0 when no client is in the game).
        let player_alive = world
            .entities
            .key_for_number(1)
            .and_then(|k| world.entities.get(k))
            .is_some_and(|p| p.in_use && p.game.health > 0);
        if !player_alive {
            return;
        }
    }
    // If noexit, do a ton of damage to other.
    let other_is_world = world.number_of(other) == Some(0);
    if deathmatch && world.game.dmflags & DF_ALLOW_EXIT == 0 && !other_is_world {
        let (origin, max_health) = world
            .entities
            .get(other)
            .map_or((Vec3f::ZERO, 0), |e| (e.state.origin, e.game.max_health));
        world.t_damage(
            other,
            self_key,
            self_key,
            Vec3f::ZERO,
            origin,
            Vec3f::ZERO,
            10 * max_health,
            1000,
            DamageFlags::empty(),
            MeansOfDeath::Exit,
        );
        return;
    }
    // If multiplayer, let everyone know who hit the exit.
    if deathmatch {
        if let Some(name) = world.client(activator).map(|c| c.pers.netname.clone()) {
            world.gi.bprintf(
                q2_shared::constants::PRINT_HIGH,
                &format!("{name} exited the level.\n"),
            );
        }
    }
    let map = world
        .entities
        .get(self_key)
        .map(|e| e.game.map.clone())
        .unwrap_or_default();
    // Going to a new unit clears the cross-level triggers.
    if map.contains('*') {
        world.game.serverflags &= !SFL_CROSS_TRIGGER_MASK;
    }
    world.begin_intermission(self_key);
}

/// Looping speakers toggle; others play their sound once.
/// C ref: `Use_Target_Speaker`. The one-shot sound is positioned at the
/// speaker, which is invisible and so never sent to clients.
fn target_speaker_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(number) = world.number_of(self_key) else {
        return;
    };
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    if ent.game.spawnflags & 3 != 0 {
        // Looping sound toggles.
        ent.state.sound = if ent.state.sound != 0 {
            0
        } else {
            ent.game.noise_index
        };
        return;
    }
    // CHAN_VOICE, plus CHAN_RELIABLE with spawnflag 4.
    let chan = if ent.game.spawnflags & 4 != 0 {
        2 | 16
    } else {
        2
    };
    let (origin, noise, volume, attenuation) = (
        ent.state.origin,
        ent.game.noise_index,
        ent.game.volume,
        ent.game.attenuation,
    );
    world
        .gi
        .positioned_sound(origin, number, chan, noise, volume, attenuation, 0.0);
}

/// target_secret — increment secret counter.
fn target_secret_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.level.found_secrets += 1;
    world.use_targets(self_key, activator);
}

/// target_goal — increment goal counter.
fn target_goal_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.level.found_goals += 1;
    world.use_targets(self_key, activator);
}

/// Generic target use — just fires its own targets (relay behavior).
fn target_generic_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.use_targets(self_key, activator);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// C SP_target_speaker's sound registration (spawn functions only see
    /// `EntityStorage`).
    pub(crate) fn finish_target_speaker(
        &mut self,
        key: EntityKey,
        fields: &HashMap<String, String>,
    ) {
        let Some(noise) = fields.get("noise") else {
            let origin = self.entities.get(key).map(|e| e.state.origin);
            self.gi
                .dprintf(&format!("target_speaker with no noise set at {origin:?}\n"));
            return;
        };
        let name = if noise.contains(".wav") {
            noise.clone()
        } else {
            format!("{noise}.wav")
        };
        let index = self.gi.sound_index(&name);
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        ent.game.noise_index = index;
        if ent.game.volume == 0.0 {
            ent.game.volume = 1.0;
        }
        if ent.game.attenuation == 0.0 {
            ent.game.attenuation = 1.0;
        } else if ent.game.attenuation == -1.0 {
            ent.game.attenuation = 0.0; // -1 so that 0 can default to 1
        }
        // Prestarted looping sound.
        if ent.game.spawnflags & 1 != 0 {
            ent.state.sound = index;
        }
        ent.use_fn = Some(target_speaker_use);
    }
}

pub fn sp_target_generic(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "target".to_string());
        ent.use_fn = Some(target_generic_use);
    }
}

/// C ref: `SP_target_speaker`; `finish_target_speaker` does the setup.
pub fn sp_target_speaker(
    _storage: &mut EntityStorage,
    _key: EntityKey,
    _fields: &HashMap<String, String>,
) {
}

pub fn sp_target_explosion(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(target_explosion_use);
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
    }
}

pub fn sp_target_changelevel(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    if ent.game.map.is_empty() {
        tracing::debug!("target_changelevel with no map at {:?}", ent.state.origin);
        storage.soft_free(key);
        return;
    }
    ent.game.classname = "target_changelevel".to_string();
    ent.use_fn = Some(target_changelevel_use);
    ent.svflags = q2_shared::constants::SVF_NOCLIENT;
}

pub fn sp_target_secret(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_secret".to_string();
        ent.use_fn = Some(target_secret_use);
        let _ = fields;
    }
}

pub fn sp_target_goal(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_goal".to_string();
        ent.use_fn = Some(target_goal_use);
        let _ = fields;
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;
    use q2_shared::types::Vec3f;

    fn dm_exit() -> (
        GameWorld,
        EntityKey,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_items();
        world.game.deathmatch = true;
        world.entities.spawn_numbered(0).unwrap();
        let start = world.spawn().unwrap();
        world.entities.get_mut(start).unwrap().game.classname = "info_player_deathmatch".into();
        let player = world.connect_client(1, "\\name\\Ann").unwrap();
        world.begin_client(player);
        let exit = world.spawn().unwrap();
        world.entities.get_mut(exit).unwrap().game.map = "q2dm2".into();
        (world, exit, player, gi)
    }

    #[test]
    fn a_deathmatch_exit_kills_whoever_touches_it_unless_allowed() {
        let (mut world, exit, player, _) = dm_exit();
        target_changelevel_use(&mut world, exit, player, player);
        assert!(world.entities.get(player).unwrap().game.health <= 0);
        assert_eq!(world.means_of_death, MeansOfDeath::Exit);
        assert_eq!(world.level.intermission_time, 0.0);
    }

    #[test]
    fn an_allowed_deathmatch_exit_says_who_left_and_starts_the_intermission() {
        let (mut world, exit, player, gi) = dm_exit();
        world.game.dmflags = crate::constants::DF_ALLOW_EXIT;
        world.level.time = 3.0;
        target_changelevel_use(&mut world, exit, player, player);
        assert_eq!(
            gi.bprints.borrow().last().unwrap().1,
            "Ann exited the level.\n"
        );
        assert_eq!(world.level.intermission_time, 3.0);
        assert_eq!(world.level.changemap, "q2dm2");
        assert!(!world.level.exitintermission, "shows the intermission");
    }

    #[test]
    fn register_target_spawns() {
        let world = test_world();
        assert!(world.spawn_table.get("target_explosion").is_some());
        assert!(world.spawn_table.get("target_changelevel").is_some());
        assert!(world.spawn_table.get("target_speaker").is_some());
        assert!(world.spawn_table.get("target_laser").is_some());
    }

    #[test]
    fn target_changelevel_sets_exit() {
        let mut world = test_world();
        let player = world.entities.spawn_numbered(1).unwrap();
        world.entities.get_mut(player).unwrap().game.health = 100;

        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.map = "base2".to_string();

        assert!(!world.level.exitintermission);

        target_changelevel_use(&mut world, target, target, target);

        assert!(world.level.exitintermission);
        assert_eq!(world.level.changemap, "base2");
    }

    #[test]
    fn exit_requests_the_map_change_and_the_next_frame_runs_entities() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "target_changelevel" "map" "base2$start" "targetname" "x" }"#,
            "",
        );
        let target = world.find_by_classname(None, "target_changelevel").unwrap();
        let player = world.entities.spawn_numbered(1).unwrap();
        world.entities.get_mut(player).unwrap().game.health = 100;
        target_changelevel_use(&mut world, target, target, target);

        world.run_frame();
        assert_eq!(*gi.commands.borrow(), ["gamemap \"base2$start\"\n"]);
        assert!(!world.level.exitintermission);

        fn count_think(world: &mut GameWorld, key: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }
        let ticker = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(ticker).unwrap();
            ent.think = Some(count_think);
            ent.game.nextthink = world.level.time + 0.1;
        }
        world.run_frame();
        assert_eq!(world.entities.get(ticker).unwrap().game.count, 1);
        assert_eq!(gi.commands.borrow().len(), 1);
    }

    #[test]
    fn target_changelevel_without_a_map_is_removed() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "target_changelevel" }"#,
            "",
        );
        assert!(world
            .find_by_classname(None, "target_changelevel")
            .is_none());
    }

    #[test]
    fn dead_player_cannot_exit_the_level() {
        let mut world = test_world();
        let player = world.entities.spawn_numbered(1).unwrap();
        world.entities.get_mut(player).unwrap().game.health = 0;
        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.map = "base2".to_string();
        target_changelevel_use(&mut world, target, player, player);
        assert!(!world.level.exitintermission);
    }

    #[test]
    fn target_secret_increments_counter() {
        let mut world = test_world();
        let target = world.spawn().unwrap();

        assert_eq!(world.level.found_secrets, 0);
        target_secret_use(&mut world, target, target, target);
        assert_eq!(world.level.found_secrets, 1);
    }

    #[test]
    fn target_goal_increments_counter() {
        let mut world = test_world();
        let target = world.spawn().unwrap();

        assert_eq!(world.level.found_goals, 0);
        target_goal_use(&mut world, target, target, target);
        assert_eq!(world.level.found_goals, 1);
    }

    #[test]
    fn target_explosion_does_radius_damage() {
        let mut world = test_world();
        let target_ent = world.spawn().unwrap();
        let victim = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target_ent).unwrap();
            ent.game.dmg = 50;
            ent.state.origin = Vec3f::ZERO;
        }
        {
            let ent = world.entities.get_mut(victim).unwrap();
            ent.state.origin = Vec3f::new(10.0, 0.0, 0.0);
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
            ent.solid = q2_shared::types::Solid::Bbox; // findradius skips SOLID_NOT
        }

        target_explosion_use(&mut world, target_ent, target_ent, target_ent);

        assert!(world.entities.get(victim).unwrap().game.health < 200);
    }
    fn bump(world: &mut GameWorld, key: EntityKey, _o: EntityKey, _a: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 1;
    }

    #[test]
    fn target_explosion_reaches_dmg_plus_40_and_fires_its_targets() {
        let mut world = test_world();
        let boom = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(boom).unwrap();
            e.game.dmg = 50;
            e.game.target = "next".into();
        }
        let next = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(next).unwrap();
            e.game.targetname = "next".into();
            e.use_fn = Some(bump);
        }
        let victim = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(victim).unwrap();
            e.state.origin = Vec3f::new(70.0, 0.0, 0.0); // beyond dmg, within dmg + 40
            e.game.health = 100;
            e.game.takedamage = TakeDamage::Yes;
            e.solid = q2_shared::types::Solid::Bbox; // findradius skips SOLID_NOT
        }
        target_explosion_use(&mut world, boom, boom, boom);
        assert!(world.entities.get(victim).unwrap().game.health < 100);
        assert_eq!(world.entities.get(next).unwrap().game.count, 1);
    }

    #[test]
    fn delayed_target_explosion_waits() {
        let mut world = test_world();
        let boom = world.spawn().unwrap();
        world.entities.get_mut(boom).unwrap().game.delay = 0.2;
        let activator = world.spawn().unwrap();
        target_explosion_use(&mut world, boom, activator, activator);
        let e = world.entities.get(boom).unwrap();
        assert!(e.think.is_some());
        assert!((e.game.nextthink - 0.2).abs() < 1e-6);
        assert_eq!(e.game.activator, Some(activator));
    }

    #[test]
    fn looping_target_speaker_toggles_its_sound() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "target_speaker" "noise" "world/amb1" "spawnflags" "1" }"#,
            "",
        );
        let speaker = world.find_by_classname(None, "target_speaker").unwrap();
        let on = world.entities.get(speaker).unwrap().state.sound;
        assert_eq!(on, world.entities.get(speaker).unwrap().game.noise_index);
        target_speaker_use(&mut world, speaker, speaker, speaker);
        assert_eq!(world.entities.get(speaker).unwrap().state.sound, 0);
        target_speaker_use(&mut world, speaker, speaker, speaker);
        assert_eq!(world.entities.get(speaker).unwrap().state.sound, on);
    }

    #[test]
    fn one_shot_target_speaker_plays_a_positioned_sound_at_itself_on_chan_voice() {
        let (mut world, gi) = crate::world::test_world_recording();
        let speaker = world.spawn().unwrap();
        let number = world.number_of(speaker).unwrap();
        {
            let e = world.entities.get_mut(speaker).unwrap();
            e.game.spawnflags = 4; // reliable
            e.game.noise_index = 9;
            e.state.origin = Vec3f::new(1.0, 2.0, 3.0);
        }
        target_speaker_use(&mut world, speaker, speaker, speaker);
        let played = gi.positioned_sounds.borrow().clone();
        assert_eq!(played, vec![(Vec3f::new(1.0, 2.0, 3.0), number, 2 | 16, 9)]);
    }

    #[test]
    fn looping_target_speaker_use_turns_its_sound_off_then_back_on() {
        let mut world = test_world();
        let speaker = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(speaker).unwrap();
            e.game.spawnflags = 1;
            e.game.noise_index = 7;
            e.state.sound = 7;
        }
        target_speaker_use(&mut world, speaker, speaker, speaker);
        assert_eq!(world.entities.get(speaker).unwrap().state.sound, 0);
        target_speaker_use(&mut world, speaker, speaker, speaker);
        assert_eq!(world.entities.get(speaker).unwrap().state.sound, 7);
    }

    #[test]
    fn target_speaker_attenuation_minus_one_means_no_attenuation() {
        // C SP_target_speaker: -1 so that 0 can default to 1.
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            concat!(
                "{ \"classname\" \"worldspawn\" }\n",
                "{ \"classname\" \"target_speaker\" \"noise\" \"a\" \"attenuation\" \"-1\" \"targetname\" \"far\" }\n",
                "{ \"classname\" \"target_speaker\" \"noise\" \"a\" \"targetname\" \"near\" }\n",
            ),
            "",
        );
        let attenuation = |name: &str| {
            let key = world.find_by_targetname(None, name).unwrap();
            world.entities.get(key).unwrap().game.attenuation
        };
        assert_eq!(attenuation("far"), 0.0);
        assert_eq!(attenuation("near"), 1.0);
    }

    #[test]
    fn changelevel_to_a_new_unit_clears_the_cross_level_triggers() {
        // C use_target_changelevel: a '*' in the map name starts a new unit.
        let changelevel = |map: &str| {
            let mut world = test_world();
            let player = world.entities.spawn_numbered(1).unwrap();
            world.entities.get_mut(player).unwrap().game.health = 100;
            world.game.serverflags = 0x0000_0103;
            let target = world.spawn().unwrap();
            world.entities.get_mut(target).unwrap().game.map = map.to_string();
            target_changelevel_use(&mut world, target, player, player);
            world.game.serverflags
        };
        assert_eq!(changelevel("base2"), 0x0000_0103);
        assert_eq!(changelevel("*bunk1$start"), 0x0000_0100);
    }
}
