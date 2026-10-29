//! Target entities — produce effects when triggered.
//!
//! Port of `g_target.c`. Each target has a `use` callback that fires when
//! the entity is triggered. Ported: target_explosion, target_speaker,
//! target_changelevel (without intermission), target_secret, target_goal,
//! target_help, target_laser.
//! The other C target classes (temp_entity, splash, spawner, blaster,
//! crosslevel_*, lightramp, earthquake) are registered as
//! relays that only fire their own targets until they are ported.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_target.c`

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage};
use crate::world::GameWorld;
use q2_shared::types::{Multicast, Solid, Vec3f};
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

/// target_help: set the help computer's first (spawnflags 1) or second
/// objective and flag new help. Not in deathmatch, and none without a
/// message. C ref: `SP_target_help`, `Use_Target_Help` (g_target.c).
pub fn finish_target_help(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let has_message = world
        .entities
        .get(key)
        .is_some_and(|e| !e.game.message.is_empty());
    if world.game.deathmatch || !has_message {
        if !has_message {
            world.gi.dprintf("target_help with no message\n");
        }
        world.free_entity(key);
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.use_fn = Some(target_help_use);
    }
}

fn target_help_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let message = ent.game.message.clone();
    if ent.game.spawnflags & 1 != 0 {
        world.game.helpmessage1 = message;
    } else {
        world.game.helpmessage2 = message;
    }
    world.game.helpchanged += 1;
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

/// C's `0x80000000` spawnflag: the beam moved, so its end sparks.
const LASER_MOVED: i32 = i32::MIN;
const LASER_ON: i32 = 1;

/// Burn along the beam: hurt everything it crosses until something that
/// is neither a monster nor a player, spark there when the beam moved,
/// and end the beam (`old_origin`) where it stopped. A laser with a
/// target aims at its centre. C ref: `target_laser_think`.
fn target_laser_think(world: &mut GameWorld, self_key: EntityKey) {
    let enemy_point = world
        .entities
        .get(self_key)
        .and_then(|e| e.game.enemy)
        .and_then(|enemy| world.entities.get(enemy))
        .map(|enemy| enemy.absmin + enemy.size * 0.5);
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    let count = if ent.game.spawnflags & LASER_MOVED != 0 {
        8
    } else {
        4
    };
    if let Some(point) = enemy_point {
        let last = ent.game.movedir;
        ent.game.movedir = (point - ent.state.origin).normalize_or_zero();
        if ent.game.movedir != last {
            ent.game.spawnflags |= LASER_MOVED;
        }
    }
    let (origin, dir, dmg, skinnum) = (
        ent.state.origin,
        ent.game.movedir,
        ent.game.dmg,
        ent.state.skinnum,
    );
    let activator = ent.game.activator.unwrap_or(self_key);
    let end = origin + dir * 2048.0;
    let mask = q2_shared::constants::CONTENTS_SOLID
        | q2_shared::constants::CONTENTS_MONSTER
        | q2_shared::constants::CONTENTS_DEADMONSTER;
    let mut start = origin;
    let mut ignore = world.number_of(self_key);
    let tr = loop {
        let tr = world
            .gi
            .trace(start, Vec3f::ZERO, Vec3f::ZERO, end, ignore, mask);
        let Some(hit) = tr.ent_index.and_then(|n| world.entities.key_for_number(n)) else {
            break tr;
        };
        let Some(h) = world.entities.get(hit) else {
            break tr;
        };
        let hurtable = h.game.takedamage != TakeDamage::No
            && !h.game.flags.contains(EntityFlags::IMMUNE_LASER);
        let body = h.svflags & SvFlags::MONSTER.bits() != 0 || h.client.is_some();
        if hurtable {
            world.t_damage(
                hit,
                self_key,
                activator,
                dir,
                tr.endpos,
                Vec3f::ZERO,
                dmg,
                1,
                DamageFlags::ENERGY,
                MeansOfDeath::TargetLaser,
            );
        }
        if !body {
            let moved = world
                .entities
                .get(self_key)
                .is_some_and(|e| e.game.spawnflags & LASER_MOVED != 0);
            if moved {
                if let Some(ent) = world.entities.get_mut(self_key) {
                    ent.game.spawnflags &= !LASER_MOVED;
                }
                world
                    .gi
                    .write_byte(q2_shared::protocol::SvcOp::TempEntity as i32);
                world.gi.write_byte(q2_shared::constants::TE_LASER_SPARKS);
                world.gi.write_byte(count);
                world.gi.write_position(tr.endpos);
                world.gi.write_dir(tr.plane.normal);
                world.gi.write_byte(skinnum);
                world.gi.multicast(tr.endpos, Multicast::PVS);
            }
            break tr;
        }
        ignore = tr.ent_index;
        start = tr.endpos;
    };
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.state.old_origin = tr.endpos;
        ent.game.nextthink = time + FRAMETIME;
    }
}

/// C ref: `target_laser_on`.
fn target_laser_on(world: &mut GameWorld, self_key: EntityKey) {
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.game.activator.get_or_insert(self_key);
    ent.game.spawnflags |= LASER_MOVED | LASER_ON;
    ent.svflags &= !q2_shared::constants::SVF_NOCLIENT;
    target_laser_think(world, self_key);
}

/// C ref: `target_laser_off`.
fn target_laser_off(world: &mut GameWorld, self_key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.spawnflags &= !LASER_ON;
        ent.svflags |= q2_shared::constants::SVF_NOCLIENT;
        ent.game.nextthink = 0.0;
    }
}

/// Toggle the laser for `activator`. C ref: `target_laser_use`.
fn target_laser_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.game.activator = Some(activator);
    if ent.game.spawnflags & LASER_ON != 0 {
        target_laser_off(world, self_key);
    } else {
        target_laser_on(world, self_key);
    }
}

/// A beam (`RF_BEAM`, 4 or with spawnflag 64 16 wide) in the colour its
/// spawnflags pick (2 red, 4 green, 8 blue, 16 yellow, 32 orange; four
/// palette indices packed in `skinnum`), aimed at its target or along its
/// angles, on when spawnflag 1 is set. C ref: `target_laser_start`.
fn target_laser_start(world: &mut GameWorld, self_key: EntityKey) {
    let target_name = world
        .entities
        .get(self_key)
        .filter(|e| e.game.enemy.is_none() && !e.game.target.is_empty())
        .map(|e| e.game.target.clone());
    let target = target_name
        .as_deref()
        .map(|name| (name, world.find_by_targetname(None, name)));
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.game.movetype = MoveType::None;
    ent.solid = Solid::Not;
    ent.state.renderfx |= q2_shared::constants::RF_BEAM | q2_shared::constants::RF_TRANSLUCENT;
    ent.state.modelindex = 1; // must be non-zero
    ent.state.frame = if ent.game.spawnflags & 64 != 0 { 16 } else { 4 };
    let flags = ent.game.spawnflags;
    let colour: Option<u32> = if flags & 2 != 0 {
        Some(0xf2f2_f0f0)
    } else if flags & 4 != 0 {
        Some(0xd0d1_d2d3)
    } else if flags & 8 != 0 {
        Some(0xf3f3_f1f1)
    } else if flags & 16 != 0 {
        Some(0xdcdd_dedf)
    } else if flags & 32 != 0 {
        Some(0xe0e1_e2e3)
    } else {
        None
    };
    if let Some(colour) = colour {
        ent.state.skinnum = colour as i32;
    }
    let mut bad_target = None;
    if ent.game.enemy.is_none() {
        match target {
            Some((name, found)) => {
                if found.is_none() {
                    bad_target = Some((ent.state.origin, name.to_string()));
                }
                ent.game.enemy = found;
            }
            None => ent.game.movedir = GameWorld::set_movedir(ent.state.angles),
        }
    }
    ent.use_fn = Some(target_laser_use);
    ent.think = Some(target_laser_think);
    if ent.game.dmg == 0 {
        ent.game.dmg = 1;
    }
    ent.mins = Vec3f::splat(-8.0);
    ent.maxs = Vec3f::splat(8.0);
    let on = ent.game.spawnflags & LASER_ON != 0;
    if let Some((origin, name)) = bad_target {
        world.gi.dprintf(&format!(
            "target_laser at {origin:?}: {name} is a bad target\n"
        ));
    }
    world.link_entity(self_key);
    if on {
        target_laser_on(world, self_key);
    } else {
        target_laser_off(world, self_key);
    }
}

/// Start once everything else has spawned. C ref: `SP_target_laser`.
pub fn finish_target_laser(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.think = Some(target_laser_start);
        ent.game.nextthink = time + 1.0;
    }
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

    fn laser_world(
        spawnflags: i32,
    ) -> (
        GameWorld,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.entities.spawn_numbered(0).unwrap();
        let laser = world.spawn().unwrap();
        let ent = world.entities.get_mut(laser).unwrap();
        ent.game.classname = "target_laser".into();
        ent.game.spawnflags = spawnflags;
        ent.state.angles = Vec3f::ZERO;
        world.level.time = 2.0;
        finish_target_laser(&mut world, laser, &HashMap::new());
        (world, laser, gi)
    }

    fn wall(endpos: Vec3f) -> q2_shared::types::Trace {
        q2_shared::types::Trace {
            fraction: 0.5,
            endpos,
            plane: q2_shared::types::Plane {
                normal: Vec3f::new(-1.0, 0.0, 0.0),
                ..Default::default()
            },
            ent_index: Some(0),
            ..Default::default()
        }
    }

    fn laser_sparks(gi: &crate::world::MockGameImport) -> usize {
        let writes = gi.writes.borrow();
        writes
            .windows(2)
            .filter(|w| {
                w[0] == q2_shared::protocol::SvcOp::TempEntity as i32
                    && w[1] == q2_shared::constants::TE_LASER_SPARKS
            })
            .count()
    }

    #[test]
    fn a_target_laser_starts_a_second_after_spawning_as_a_coloured_beam() {
        let (mut world, laser, gi) = laser_world(1 | 2 | 64);
        let ent = world.entities.get(laser).unwrap();
        assert_eq!(ent.game.nextthink, 3.0, "C: level.time + 1");
        let start = ent.think.unwrap();
        let hit = Vec3f::new(300.0, 0.0, 0.0);
        gi.queued_traces.borrow_mut().push_back(wall(hit));
        start(&mut world, laser);
        let ent = world.entities.get(laser).unwrap();
        assert_eq!(
            ent.state.renderfx,
            q2_shared::constants::RF_BEAM | q2_shared::constants::RF_TRANSLUCENT
        );
        assert_eq!(ent.state.modelindex, 1);
        assert_eq!(ent.state.frame, 16, "spawnflag 64 is the fat beam");
        assert_eq!(ent.state.skinnum, 0xf2f2_f0f0_u32 as i32, "red");
        assert_eq!(ent.game.dmg, 1);
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::splat(-8.0), Vec3f::splat(8.0))
        );
        // Started on: visible, fired once, ends at the wall with sparks.
        assert_eq!(ent.svflags & q2_shared::constants::SVF_NOCLIENT, 0);
        assert_eq!(ent.state.old_origin, hit);
        assert_eq!(laser_sparks(&gi), 1);
        // Facing angles 0: along +x, 2048 units.
        assert_eq!(ent.game.movedir, Vec3f::X);
    }

    #[test]
    fn a_target_laser_burns_what_it_crosses_and_sparks_only_when_it_moves() {
        let (mut world, laser, gi) = laser_world(1);
        let start = world.entities.get(laser).unwrap().think.unwrap();
        let monster = world.spawn().unwrap();
        let m = world.entities.get_mut(monster).unwrap();
        m.svflags |= SvFlags::MONSTER.bits();
        m.game.takedamage = TakeDamage::Yes;
        m.game.health = 100;
        let through = q2_shared::types::Trace {
            fraction: 0.2,
            endpos: Vec3f::new(100.0, 0.0, 0.0),
            ent_index: world.number_of(monster),
            ..Default::default()
        };
        let hit = Vec3f::new(300.0, 0.0, 0.0);
        gi.queued_traces
            .borrow_mut()
            .extend([through.clone(), wall(hit)]);
        start(&mut world, laser);
        assert_eq!(world.entities.get(monster).unwrap().game.health, 99);
        assert_eq!(world.means_of_death, MeansOfDeath::TargetLaser);
        // The second trace starts at the monster and ignores it.
        let passes = gi.trace_pass_ents.borrow().clone();
        assert_eq!(passes.last(), Some(&world.number_of(monster)));
        assert_eq!(laser_sparks(&gi), 1);
        let think = world.entities.get(laser).unwrap().think.unwrap();
        gi.queued_traces.borrow_mut().push_back(wall(hit));
        think(&mut world, laser);
        assert_eq!(laser_sparks(&gi), 1, "same aim: no new sparks");
        assert_eq!(world.entities.get(laser).unwrap().game.nextthink, 2.1);
    }

    #[test]
    fn a_target_laser_follows_its_target_and_sparks_when_the_aim_changes() {
        let (mut world, laser, gi) = laser_world(1);
        let target = world.spawn().unwrap();
        let t = world.entities.get_mut(target).unwrap();
        t.game.targetname = "spot".into();
        t.absmin = Vec3f::new(0.0, 100.0, -10.0);
        t.size = Vec3f::new(0.0, 0.0, 20.0);
        world.entities.get_mut(laser).unwrap().game.target = "spot".into();
        let start = world.entities.get(laser).unwrap().think.unwrap();
        start(&mut world, laser);
        assert_eq!(world.entities.get(laser).unwrap().game.enemy, Some(target));
        assert_eq!(world.entities.get(laser).unwrap().game.movedir, Vec3f::Y);
        let before = laser_sparks(&gi);
        world.entities.get_mut(target).unwrap().absmin = Vec3f::new(100.0, 0.0, -10.0);
        gi.queued_traces.borrow_mut().push_back(wall(Vec3f::X));
        let think = world.entities.get(laser).unwrap().think.unwrap();
        think(&mut world, laser);
        assert_eq!(world.entities.get(laser).unwrap().game.movedir, Vec3f::X);
        assert_eq!(laser_sparks(&gi), before + 1);
    }

    #[test]
    fn using_a_target_laser_toggles_it_and_off_hides_it() {
        let (mut world, laser, _) = laser_world(0);
        let start = world.entities.get(laser).unwrap().think.unwrap();
        start(&mut world, laser);
        let ent = world.entities.get(laser).unwrap();
        assert_ne!(
            ent.svflags & q2_shared::constants::SVF_NOCLIENT,
            0,
            "starts off"
        );
        assert_eq!(ent.game.nextthink, 0.0);
        let use_fn = ent.use_fn.unwrap();
        let player = world.spawn().unwrap();
        use_fn(&mut world, laser, player, player);
        let ent = world.entities.get(laser).unwrap();
        assert_eq!(ent.svflags & q2_shared::constants::SVF_NOCLIENT, 0);
        assert_eq!(ent.game.activator, Some(player));
        assert_eq!(ent.game.spawnflags & 1, 1);
        use_fn(&mut world, laser, player, player);
        let ent = world.entities.get(laser).unwrap();
        assert_ne!(ent.svflags & q2_shared::constants::SVF_NOCLIENT, 0);
        assert_eq!(ent.game.nextthink, 0.0);
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
