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

/// target_changelevel — exit to next map. Single player only: the
/// deathmatch noexit damage and exit message are not ported. C goes through
/// BeginIntermission (p_hud.c), which this port does not have yet, so the
/// exit happens on the next frame without the intermission screen.
/// C ref: `use_target_changelevel` (g_target.c).
fn target_changelevel_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    if world.level.intermission_time != 0.0 || world.level.exitintermission {
        return; // Already activated.
    }
    // A dead single player cannot exit (C reads g_edicts[1].health, which
    // is 0 when no client is in the game).
    let player_alive = world
        .entities
        .key_for_number(1)
        .and_then(|k| world.entities.get(k))
        .is_some_and(|p| p.in_use && p.game.health > 0);
    if !player_alive {
        return;
    }
    let Some(map) = world.entities.get(self_key).map(|e| e.game.map.clone()) else {
        return;
    };
    // Going to a new unit clears the cross-level triggers.
    if map.contains('*') {
        world.game.serverflags &= !SFL_CROSS_TRIGGER_MASK;
    }
    world.level.changemap = map;
    world.level.exitintermission = true;
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

/// Play the secret sound, count the secret, fire the targets and go.
/// C ref: `use_target_secret`.
fn target_secret_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    play_found_sound(world, self_key);
    world.level.found_secrets += 1;
    world.use_targets(self_key, activator);
    world.free_entity(self_key);
}

/// As `target_secret_use`, and the last goal found stops the CD track.
/// C ref: `use_target_goal`.
fn target_goal_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    play_found_sound(world, self_key);
    world.level.found_goals += 1;
    if world.level.found_goals == world.level.total_goals {
        world
            .gi
            .configstring(q2_shared::constants::CS_CDTRACK as i32, "0");
    }
    world.use_targets(self_key, activator);
    world.free_entity(self_key);
}

/// C `gi.sound(ent, CHAN_VOICE, ent->noise_index, 1, ATTN_NORM, 0)`.
fn play_found_sound(world: &GameWorld, key: EntityKey) {
    let noise = world.entities.get(key).map_or(0, |e| e.game.noise_index);
    world
        .gi
        .sound(world.number_of(key), CHAN_VOICE, noise, 1.0, ATTN_NORM, 0.0);
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

/// C ref: `SP_target_secret`; `finish_found_target` does the rest.
pub fn sp_target_secret(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(target_secret_use);
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
    }
}

/// C ref: `SP_target_goal`; `finish_found_target` does the rest.
pub fn sp_target_goal(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(target_goal_use);
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
    }
}

/// The `GameWorld` half of C SP_target_secret and SP_target_goal: freed in
/// deathmatch, otherwise the noise (default `misc/secret.wav`) and the
/// level total.
pub fn finish_found_target(
    world: &mut GameWorld,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if world.game.deathmatch {
        world.free_entity(key);
        return;
    }
    let noise = fields
        .get("noise")
        .map_or("misc/secret.wav", String::as_str);
    let noise_index = world.gi.sound_index(noise);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.noise_index = noise_index;
    if ent.game.classname == "target_goal" {
        world.level.total_goals += 1;
        return;
    }
    world.level.total_secrets += 1;
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;
    use q2_shared::types::Vec3f;

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

    fn spawn_map(
        mapname: &str,
        deathmatch: bool,
        entities: &str,
    ) -> (GameWorld, std::rc::Rc<crate::world::MockGameImport>) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.game.deathmatch = deathmatch;
        world.init_game();
        world.spawn_entities(
            mapname,
            &format!("{{ \"classname\" \"worldspawn\" }}\n{entities}"),
            "",
        );
        (world, gi)
    }

    #[test]
    fn secrets_and_goals_count_their_totals_and_hide() {
        let (world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_secret" } { "classname" "target_secret" "noise" "x.wav" }
               { "classname" "target_goal" }"#,
        );
        assert_eq!((world.level.total_secrets, world.level.total_goals), (2, 1));
        let secret = world.find_by_classname(None, "target_secret").unwrap();
        let ent = world.entities.get(secret).unwrap();
        assert_eq!(ent.svflags, q2_shared::constants::SVF_NOCLIENT);
        let sounds = gi.sounds.borrow();
        assert_eq!(sounds[ent.game.noise_index as usize - 1], "misc/secret.wav");
        assert!(sounds.contains(&"x.wav".to_string()));
    }

    #[test]
    fn deathmatch_removes_secrets_and_goals() {
        let (world, _) = spawn_map(
            "base1",
            true,
            r#"{ "classname" "target_secret" } { "classname" "target_goal" }"#,
        );
        assert!(world.find_by_classname(None, "target_secret").is_none());
        assert!(world.find_by_classname(None, "target_goal").is_none());
        assert_eq!((world.level.total_secrets, world.level.total_goals), (0, 0));
    }

    #[test]
    fn a_secret_plays_its_sound_counts_once_and_is_freed() {
        let (mut world, gi) = spawn_map("base1", false, r#"{ "classname" "target_secret" }"#);
        let secret = world.find_by_classname(None, "target_secret").unwrap();
        let use_fn = world.entities.get(secret).unwrap().use_fn.unwrap();
        let number = world.number_of(secret);
        use_fn(&mut world, secret, secret, secret);
        assert_eq!(world.level.found_secrets, 1);
        assert!(!world.entities.is_live(secret));
        let noise = 1 + gi
            .sounds
            .borrow()
            .iter()
            .position(|s| s == "misc/secret.wav")
            .unwrap() as i32;
        assert!(gi
            .sound_calls
            .borrow()
            .iter()
            .any(|c| c.0 == number && c.1 == CHAN_VOICE && c.2 == noise));
    }

    #[test]
    fn finding_the_last_goal_sets_the_cd_track_to_0() {
        let (mut world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_goal" } { "classname" "target_goal" }"#,
        );
        let first = world.find_by_classname(None, "target_goal").unwrap();
        let second = world.find_by_classname(Some(first), "target_goal").unwrap();
        let cdtrack = |gi: &crate::world::MockGameImport| {
            gi.configstrings
                .borrow()
                .iter()
                .filter(|(i, v)| *i == q2_shared::constants::CS_CDTRACK as i32 && v == "0")
                .count()
        };
        let before = cdtrack(&gi);
        target_goal_use(&mut world, first, first, first);
        assert_eq!(cdtrack(&gi), before);
        target_goal_use(&mut world, second, second, second);
        assert_eq!(cdtrack(&gi), before + 1);
        assert!(!world.entities.is_live(second));
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
