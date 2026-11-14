//! Target entities — produce effects when triggered.
//!
//! Port of `g_target.c`. Each target has a `use` callback that fires when
//! the entity is triggered. Every C target class is ported.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_target.c`

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage};
use crate::weapons::{spark_temp_entity, temp_entity};
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
    temp_entity(
        world,
        TE_EXPLOSION1,
        &[origin],
        None,
        origin,
        Multicast::PHS,
    );

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
    if world.game.deathmatch {
        world.free_entity(key);
        return;
    }
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.message.is_empty() {
        let warning = format!(
            "{} with no message at {}\n",
            ent.game.classname,
            crate::ai::vtos(ent.state.origin)
        );
        world.gi.dprintf(&warning);
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
    // Spawnflag 4 sends it reliably.
    let chan = if ent.game.spawnflags & 4 != 0 {
        CHAN_VOICE | CHAN_RELIABLE
    } else {
        CHAN_VOICE
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
        // Tested after the damage, as C does: a monster it gibbed has lost
        // SVF_MONSTER, and a freed one (C zeroes its svflags) is gone.
        let body = world
            .entities
            .get(hit)
            .is_some_and(|h| h.svflags & SvFlags::MONSTER.bits() != 0 || h.client.is_some());
        if !body {
            let moved = world
                .entities
                .get(self_key)
                .is_some_and(|e| e.game.spawnflags & LASER_MOVED != 0);
            if moved {
                if let Some(ent) = world.entities.get_mut(self_key) {
                    ent.game.spawnflags &= !LASER_MOVED;
                }
                spark_temp_entity(
                    world,
                    q2_shared::constants::TE_LASER_SPARKS,
                    count,
                    tr.endpos,
                    tr.plane.normal,
                    skinnum,
                );
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
                    bad_target = Some(format!(
                        "{} at {}: {name} is a bad target\n",
                        ent.game.classname,
                        crate::ai::vtos(ent.state.origin)
                    ));
                }
                ent.game.enemy = found;
            }
            None => {
                ent.game.movedir = GameWorld::set_movedir(ent.state.angles);
                ent.state.angles = Vec3f::ZERO; // C G_SetMovedir: VectorClear
            }
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
    if let Some(message) = bad_target {
        world.gi.dprintf(&message);
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

/// Send the `style` temp entity at the target. C ref: `Use_Target_Tent`.
fn use_target_tent(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (style, origin) = (ent.game.style, ent.state.origin);
    temp_entity(world, style, &[origin], None, origin, Multicast::PVS);
}

/// A `count`-particle splash of colour `sounds` along `movedir`, and
/// radius damage when `dmg` is set. C ref: `use_target_splash`.
fn use_target_splash(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (count, origin, movedir, sounds, dmg) = (
        ent.game.count,
        ent.state.origin,
        ent.game.movedir,
        ent.game.sounds,
        ent.game.dmg,
    );
    spark_temp_entity(world, TE_SPLASH, count, origin, movedir, sounds);
    if dmg != 0 {
        world.t_radius_damage(
            key,
            activator,
            dmg as f32,
            None,
            (dmg + 40) as f32,
            MeansOfDeath::Splash,
        );
    }
}

/// Spawn an entity of the classname in `target` at the spawner, telefrag
/// what it lands in, and launch it along `movedir` when `speed` is set.
/// C ref: `use_target_spawner`.
fn use_target_spawner(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (classname, origin, angles, speed, movedir) = (
        ent.game.target.clone(),
        ent.state.origin,
        ent.state.angles,
        ent.game.speed,
        ent.game.movedir,
    );
    let Some(spawned) = world.spawn() else {
        return;
    };
    if let Some(e) = world.entities.get_mut(spawned) {
        e.game.classname = classname.clone();
        e.state.origin = origin;
        e.state.angles = angles;
    }
    if classname.is_empty() {
        world.gi.dprintf("ED_CallSpawn: NULL classname\n");
        world.free_entity(spawned);
        return;
    }
    let fields = HashMap::from([("classname".to_string(), classname.clone())]);
    world.call_spawn(spawned, &classname, &fields);
    // C goes on to KillBox the freed edict, at the origin with no size,
    // when the spawn function removed it; that is skipped here.
    if !world.entities.is_live(spawned) {
        return;
    }
    world.unlink_entity(spawned);
    world.kill_box(spawned);
    world.link_entity(spawned);
    if speed != 0.0 {
        if let Some(e) = world.entities.get_mut(spawned) {
            e.velocity = movedir;
        }
    }
}

/// Fire a blaster bolt along `movedir`. C passes `MOD_TARGET_BLASTER` in
/// fire_blaster's `hyper` slot, so the bolt counts as a hyperblaster's.
/// C ref: `use_target_blaster`.
fn use_target_blaster(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (origin, movedir, dmg, speed, noise) = (
        ent.state.origin,
        ent.game.movedir,
        ent.game.dmg,
        ent.game.speed,
        ent.game.noise_index,
    );
    world.fire_blaster(
        key,
        origin,
        movedir,
        dmg,
        speed as i32,
        q2_shared::constants::EF_BLASTER,
        true,
    );
    world
        .gi
        .sound(world.number_of(key), CHAN_VOICE, noise, 1.0, ATTN_NORM, 0.0);
}

/// Set the unit's cross-level flags, fire the targets and go.
/// C ref: `trigger_crosslevel_trigger_use`.
fn trigger_crosslevel_trigger_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let flags = world.entities.get(key).map_or(0, |e| e.game.spawnflags);
    world.game.serverflags |= flags;
    world.use_targets(key, activator);
    world.free_entity(key);
}

/// Once the level has started, fire the targets (as their own activator)
/// if every flag this target names was set earlier in the unit.
/// C ref: `target_crosslevel_target_think`.
fn target_crosslevel_target_think(world: &mut GameWorld, key: EntityKey) {
    let flags = world.entities.get(key).map_or(0, |e| e.game.spawnflags);
    if flags == world.game.serverflags & SFL_CROSS_TRIGGER_MASK & flags {
        world.use_targets(key, key);
        world.free_entity(key);
    }
}

/// target_lightramp spawnflag: reverse the ramp each time it ends.
const LIGHTRAMP_TOGGLE: i32 = 1;

/// Write the ramp's current letter to the light's style, a frame at a time
/// for `speed` seconds. `movedir` holds the start and end letters (from
/// 'a') and the step per frame. C ref: `target_lightramp_think`.
fn target_lightramp_think(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let Some(style) = ent
        .game
        .enemy
        .and_then(|l| world.entities.get(l))
        .map(|l| l.game.style)
    else {
        return;
    };
    let elapsed = time - ent.game.timestamp;
    // C promotes to double through FRAMETIME and truncates to a char.
    let letter = f64::from(b'a')
        + f64::from(ent.game.movedir.x)
        + f64::from(elapsed) / crate::func::FRAMETIME_C * f64::from(ent.game.movedir.z);
    let letter = char::from(letter as i32 as u8);
    world.gi.configstring(
        q2_shared::constants::CS_LIGHTS as i32 + style,
        &letter.to_string(),
    );
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if elapsed < ent.game.speed {
        ent.game.nextthink = crate::func::time_plus(time, crate::func::FRAMETIME_C);
    } else if ent.game.spawnflags & LIGHTRAMP_TOGGLE != 0 {
        let d = &mut ent.game.movedir;
        (d.x, d.y) = (d.y, d.x.trunc());
        d.z *= -1.0;
    }
}

/// Find the light on first use (complaining about targets that are not
/// lights), then start the ramp. C ref: `target_lightramp_use`.
fn target_lightramp_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.enemy.is_none() {
        let (classname, target, origin) = (
            ent.game.classname.clone(),
            ent.game.target.clone(),
            ent.state.origin,
        );
        let mut light = None;
        let mut search = None;
        while let Some(e) = world.find_by_targetname(search, &target) {
            search = Some(e);
            let Some(found) = world.entities.get(e) else {
                continue;
            };
            if found.game.classname != "light" {
                let warning = format!(
                    "{classname} at {} target {target} ({} at {}) is not a light\n",
                    crate::ai::vtos(origin),
                    found.game.classname,
                    crate::ai::vtos(found.state.origin)
                );
                world.gi.dprintf(&warning);
            } else {
                light = Some(e);
            }
        }
        let Some(light) = light else {
            world.gi.dprintf(&format!(
                "{classname} target {target} not found at {}\n",
                crate::ai::vtos(origin)
            ));
            world.free_entity(key);
            return;
        };
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.enemy = Some(light);
        }
    }
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.timestamp = time;
    }
    target_lightramp_think(world, key);
}

/// Rumble every half second and throw every client standing on something,
/// each frame until `timestamp`. C ref: `target_earthquake_think`.
fn target_earthquake_think(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(number) = world.number_of(key) else {
        return;
    };
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let (origin, noise, speed, until) = (
        ent.state.origin,
        ent.game.noise_index,
        ent.game.speed,
        ent.game.timestamp,
    );
    if ent.game.last_move_time < time {
        ent.game.last_move_time = time + 0.5;
        world
            .gi
            .positioned_sound(origin, number, CHAN_AUTO, noise, 1.0, ATTN_NONE, 0.0);
    }
    let shaken: Vec<EntityKey> = world.entities.keys_from(1).collect();
    for e in shaken {
        let Some(mass) = world
            .entities
            .get(e)
            .filter(|e| e.in_use && e.client.is_some() && e.game.ground_entity.is_some())
            .map(|e| e.game.mass)
        else {
            continue;
        };
        let x = world.crandom() * 150.0;
        let y = world.crandom() * 150.0;
        if let Some(e) = world.entities.get_mut(e) {
            e.game.ground_entity = None;
            e.velocity.x += x;
            e.velocity.y += y;
            e.velocity.z = (f64::from(speed) * (100.0 / f64::from(mass))) as f32;
        }
    }
    if time < until {
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.nextthink = crate::func::time_plus(time, crate::func::FRAMETIME_C);
        }
    }
}

/// Start a `count`-second quake next frame. C ref: `target_earthquake_use`.
fn target_earthquake_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.timestamp = time + ent.game.count as f32;
        ent.game.nextthink = crate::func::time_plus(time, crate::func::FRAMETIME_C);
        ent.game.activator = Some(activator);
        ent.game.last_move_time = 0.0;
    }
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
            let origin = self
                .entities
                .get(key)
                .map_or(Vec3f::ZERO, |e| e.state.origin);
            self.gi.dprintf(&format!(
                "target_speaker with no noise set at {}\n",
                crate::ai::vtos(origin)
            ));
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

/// For targets whose whole C SP_* function needs `GameWorld`
/// (target_help, target_crosslevel_target, target_lightramp): the finish
/// function in their SPAWNS row does it.
pub fn sp_target_in_finish(
    _storage: &mut EntityStorage,
    _key: EntityKey,
    _fields: &HashMap<String, String>,
) {
}

/// C ref: `SP_target_temp_entity`.
pub fn sp_target_temp_entity(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(use_target_tent);
    }
}

/// C ref: `SP_target_splash`.
pub fn sp_target_splash(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(use_target_splash);
        ent.game.movedir = GameWorld::set_movedir(ent.state.angles);
        ent.state.angles = Vec3f::ZERO;
        if ent.game.count == 0 {
            ent.game.count = 32;
        }
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
    }
}

/// C ref: `SP_target_spawner`; `finish_target_spawner` has its fact2 fix.
pub fn sp_target_spawner(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(use_target_spawner);
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
        if ent.game.speed != 0.0 {
            ent.game.movedir = GameWorld::set_movedir(ent.state.angles) * ent.game.speed;
            ent.state.angles = Vec3f::ZERO;
        }
    }
}

/// C SP_target_spawner's map hack, from KMQuake2: fact2's spawner sits 8
/// units too high.
pub fn finish_target_spawner(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let fact2 = world.level.mapname.eq_ignore_ascii_case("fact2");
    if let Some(ent) = world.entities.get_mut(key) {
        if fact2 && ent.state.origin == Vec3f::new(-1504.0, 512.0, 72.0) {
            ent.state.origin.z -= 8.0;
        }
    }
}

/// C ref: `SP_target_blaster`; `finish_target_blaster` registers its sound.
pub fn sp_target_blaster(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(use_target_blaster);
        ent.game.movedir = GameWorld::set_movedir(ent.state.angles);
        ent.state.angles = Vec3f::ZERO;
        if ent.game.dmg == 0 {
            ent.game.dmg = 15;
        }
        if ent.game.speed == 0.0 {
            ent.game.speed = 1000.0;
        }
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
    }
}

/// The `GameWorld` half of C SP_target_blaster.
pub fn finish_target_blaster(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let noise = world.gi.sound_index("weapons/laser2.wav");
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.noise_index = noise;
    }
}

/// C ref: `SP_target_crosslevel_trigger`.
pub fn sp_target_crosslevel_trigger(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
        ent.use_fn = Some(trigger_crosslevel_trigger_use);
    }
}

/// C ref: `SP_target_crosslevel_target`, thinking `delay` (default 1)
/// seconds into the level.
pub fn finish_target_crosslevel_target(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        if ent.game.delay == 0.0 {
            ent.game.delay = 1.0;
        }
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
        ent.think = Some(target_crosslevel_target_think);
        ent.game.nextthink = time + ent.game.delay;
    }
}

/// C ref: `SP_target_lightramp`. The ramp is two different letters a-z;
/// removed in deathmatch.
pub fn finish_target_lightramp(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let deathmatch = world.game.deathmatch;
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let ramp = ent.game.message.as_bytes();
    let letter = |c: u8| c.is_ascii_lowercase();
    if ramp.len() != 2 || !letter(ramp[0]) || !letter(ramp[1]) || ramp[0] == ramp[1] {
        let warning = format!(
            "target_lightramp has bad ramp ({}) at {}\n",
            ent.game.message,
            crate::ai::vtos(ent.state.origin)
        );
        world.gi.dprintf(&warning);
        world.free_entity(key);
        return;
    }
    if deathmatch {
        world.free_entity(key);
        return;
    }
    if ent.game.target.is_empty() {
        let warning = format!(
            "{} with no target at {}\n",
            ent.game.classname,
            crate::ai::vtos(ent.state.origin)
        );
        world.gi.dprintf(&warning);
        world.free_entity(key);
        return;
    }
    let (start, end) = (f32::from(ramp[0] - b'a'), f32::from(ramp[1] - b'a'));
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.svflags |= q2_shared::constants::SVF_NOCLIENT;
    ent.use_fn = Some(target_lightramp_use);
    ent.think = Some(target_lightramp_think);
    let step = f64::from(end - start) / (f64::from(ent.game.speed) / crate::func::FRAMETIME_C);
    ent.game.movedir = Vec3f::new(start, end, step as f32);
}

/// C ref: `SP_target_earthquake`: `speed` (default 200) is how hard it
/// throws, `count` (default 5) how many seconds it lasts.
pub fn sp_target_earthquake(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        if ent.game.count == 0 {
            ent.game.count = 5;
        }
        if ent.game.speed == 0.0 {
            ent.game.speed = 200.0;
        }
        ent.svflags |= q2_shared::constants::SVF_NOCLIENT;
        ent.think = Some(target_earthquake_think);
        ent.use_fn = Some(target_earthquake_use);
    }
}

/// The `GameWorld` half of C SP_target_earthquake: the untargeted warning
/// and the rumble sound.
pub fn finish_target_earthquake(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.targetname.is_empty() {
        let warning = format!(
            "untargeted {} at {}\n",
            ent.game.classname,
            crate::ai::vtos(ent.state.origin)
        );
        world.gi.dprintf(&warning);
    }
    let noise = world.gi.sound_index("world/quake.wav");
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.noise_index = noise;
    }
}

/// No `use` until `target_laser_start` sets it a second later, so an early
/// use does nothing. C ref: `SP_target_laser`; `finish_target_laser`
/// schedules the start.
pub fn sp_target_laser(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_laser".to_string();
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
        let warning = format!(
            "target_changelevel with no map at {}\n",
            crate::ai::vtos(ent.state.origin)
        );
        storage.dprintf(warning);
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
/// level total, plus C's mine3 quirk giving one secret its message.
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
    let mine3 = world.level.mapname.eq_ignore_ascii_case("mine3");
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.noise_index = noise_index;
    if ent.game.classname == "target_goal" {
        world.level.total_goals += 1;
        return;
    }
    world.level.total_secrets += 1;
    if mine3 && ent.state.origin == q2_shared::types::Vec3f::new(280.0, -2048.0, -624.0) {
        ent.game.message = "You have found a secret area.".to_string();
    }
}

/// The `GameWorld` half of C SP_target_changelevel: its quirk sending
/// fact1's exit to fact3 to the secret spawn point.
pub fn finish_target_changelevel(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let fact1 = world.level.mapname.eq_ignore_ascii_case("fact1");
    if let Some(ent) = world.entities.get_mut(key) {
        if fact1 && ent.game.map.eq_ignore_ascii_case("fact3") {
            ent.game.map = "fact3$secret1".to_string();
        }
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
        world.ensure_player_start();
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

    fn help_world(
        deathmatch: bool,
        message: &str,
        spawnflags: i32,
    ) -> (
        GameWorld,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.game.deathmatch = deathmatch;
        let help = world.spawn().unwrap();
        let ent = world.entities.get_mut(help).unwrap();
        ent.game.classname = "target_help".into();
        ent.game.message = message.into();
        ent.game.spawnflags = spawnflags;
        ent.state.origin = Vec3f::new(1.0, 2.0, 3.0);
        finish_target_help(&mut world, help, &HashMap::new());
        (world, help, gi)
    }

    #[test]
    fn target_help_with_spawnflag_1_sets_the_first_help_message() {
        let (mut world, help, _) = help_world(false, "find the key", 1);
        let use_fn = world.entities.get(help).unwrap().use_fn.unwrap();
        use_fn(&mut world, help, help, help);
        assert_eq!(world.game.helpmessage1, "find the key");
        assert_eq!(world.game.helpmessage2, "");
        assert_eq!(world.game.helpchanged, 1);
    }

    #[test]
    fn target_help_without_spawnflag_1_sets_the_second_help_message() {
        let (mut world, help, _) = help_world(false, "open the door", 0);
        let use_fn = world.entities.get(help).unwrap().use_fn.unwrap();
        use_fn(&mut world, help, help, help);
        use_fn(&mut world, help, help, help);
        assert_eq!(world.game.helpmessage1, "");
        assert_eq!(world.game.helpmessage2, "open the door");
        assert_eq!(world.game.helpchanged, 2);
    }

    #[test]
    fn target_help_without_a_message_is_freed_with_a_warning() {
        let (world, help, gi) = help_world(false, "", 0);
        assert!(!world.entities.is_live(help));
        assert_eq!(
            *gi.dprints.borrow(),
            ["target_help with no message at (1 2 3)\n"]
        );
    }

    #[test]
    fn target_help_is_freed_silently_in_deathmatch() {
        // C checks deathmatch before the message, so no warning either way.
        for message in ["objective", ""] {
            let (world, help, gi) = help_world(true, message, 0);
            assert!(!world.entities.is_live(help), "{message:?}");
            assert!(gi.dprints.borrow().is_empty(), "{message:?}");
        }
    }

    #[test]
    fn a_target_laser_ignores_uses_until_it_starts() {
        // C SP_target_laser leaves `use` NULL until target_laser_start.
        let mut world = test_world();
        let laser = world.spawn().unwrap();
        world.entities.get_mut(laser).unwrap().game.target = "aim".into();
        let fields = HashMap::from([("classname".to_string(), "target_laser".to_string())]);
        world.call_spawn(laser, "target_laser", &fields);
        assert!(world.entities.get(laser).unwrap().use_fn.is_none());
    }

    const TE: i32 = q2_shared::protocol::SvcOp::TempEntity as i32;

    #[test]
    fn a_laser_turning_on_against_a_wall_writes_c_s_sparks_layout() {
        // C target_laser_think: TE_LASER_SPARKS, count, endpos, normal,
        // skinnum, then the PVS of endpos.
        use crate::world::Write;
        let (mut world, laser, gi) = laser_world(1);
        let start = world.entities.get(laser).unwrap().think.unwrap();
        let hit = Vec3f::new(300.0, 0.0, 0.0);
        gi.queued_traces.borrow_mut().push_back(wall(hit));
        start(&mut world, laser);
        let skinnum = world.entities.get(laser).unwrap().state.skinnum;
        assert_eq!(
            *gi.message.borrow(),
            [
                Write::Byte(TE),
                Write::Byte(q2_shared::constants::TE_LASER_SPARKS),
                Write::Byte(8),
                Write::Position(hit),
                Write::Dir(Vec3f::new(-1.0, 0.0, 0.0)),
                Write::Byte(skinnum),
            ]
        );
        assert_eq!(*gi.multicasts.borrow(), [(hit, Multicast::PVS)]);
    }

    #[test]
    fn target_explosion_writes_explosion1_to_the_phs() {
        // C target_explosion_explode.
        use crate::world::Write;
        let (mut world, gi) = crate::world::test_world_recording();
        let key = world.spawn().unwrap();
        let at = Vec3f::new(1.0, 2.0, 3.0);
        world.entities.get_mut(key).unwrap().state.origin = at;
        target_explosion_explode(&mut world, key);
        assert_eq!(
            *gi.message.borrow(),
            [
                Write::Byte(TE),
                Write::Byte(TE_EXPLOSION1),
                Write::Position(at)
            ]
        );
        assert_eq!(*gi.multicasts.borrow(), [(at, Multicast::PHS)]);
    }

    #[test]
    fn a_target_laser_that_hits_nothing_ends_on_the_world_with_sparks() {
        // SV_Trace names the world when nothing is hit, so C's not-a-body
        // branch runs: sparks (the laser just turned on) at the far end.
        let (mut world, laser, gi) = laser_world(1);
        let start = world.entities.get(laser).unwrap().think.unwrap();
        start(&mut world, laser);
        assert_eq!(laser_sparks(&gi), 1);
        assert_eq!(
            world.entities.get(laser).unwrap().state.old_origin,
            Vec3f::new(2048.0, 0.0, 0.0)
        );
    }

    #[test]
    fn a_target_laser_with_a_missing_target_warns_in_c_form() {
        let (mut world, laser, gi) = laser_world(0);
        let ent = world.entities.get_mut(laser).unwrap();
        ent.game.target = "nowhere".into();
        ent.state.origin = Vec3f::new(1.0, 2.0, 3.0);
        let start = ent.think.unwrap();
        start(&mut world, laser);
        assert_eq!(
            *gi.dprints.borrow(),
            ["target_laser at (1 2 3): nowhere is a bad target\n"]
        );
    }

    #[test]
    fn an_untargeted_target_laser_clears_its_angles_like_g_setmovedir() {
        let (mut world, laser, gi) = laser_world(1);
        world.entities.get_mut(laser).unwrap().state.angles = Vec3f::new(0.0, 90.0, 0.0);
        let start = world.entities.get(laser).unwrap().think.unwrap();
        gi.queued_traces
            .borrow_mut()
            .push_back(wall(Vec3f::new(0.0, 300.0, 0.0)));
        start(&mut world, laser);
        let ent = world.entities.get(laser).unwrap();
        assert_eq!(
            ent.game.movedir,
            GameWorld::set_movedir(Vec3f::new(0.0, 90.0, 0.0))
        );
        assert_eq!(ent.state.angles, Vec3f::ZERO, "C G_SetMovedir: VectorClear");
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

    /// C ThrowHead: the gibbed monster's edict stays, minus SVF_MONSTER.
    fn die_into_a_head(
        world: &mut GameWorld,
        key: EntityKey,
        _inflictor: EntityKey,
        _attacker: EntityKey,
        _damage: i32,
        _point: Vec3f,
    ) {
        world.entities.get_mut(key).unwrap().svflags &= !SvFlags::MONSTER.bits();
    }

    /// C BecomeExplosion1: the monster's edict is freed.
    fn die_and_vanish(
        world: &mut GameWorld,
        key: EntityKey,
        _inflictor: EntityKey,
        _attacker: EntityKey,
        _damage: i32,
        _point: Vec3f,
    ) {
        world.free_entity(key);
    }

    #[test]
    fn a_target_laser_stops_at_a_monster_its_damage_gibs_or_frees() {
        // C target_laser_think tests SVF_MONSTER after T_Damage, so a
        // monster the beam kills into a head or an explosion ends it.
        for die in [die_into_a_head as crate::entity::DieFn, die_and_vanish] {
            let (mut world, laser, gi) = laser_world(1);
            let start = world.entities.get(laser).unwrap().think.unwrap();
            let monster = world.spawn().unwrap();
            let m = world.entities.get_mut(monster).unwrap();
            m.svflags |= SvFlags::MONSTER.bits();
            m.game.takedamage = TakeDamage::Yes;
            m.game.health = 1;
            m.die = Some(die);
            let at = Vec3f::new(100.0, 0.0, 0.0);
            let through = q2_shared::types::Trace {
                fraction: 0.05,
                endpos: at,
                ent_index: world.number_of(monster),
                ..Default::default()
            };
            gi.queued_traces
                .borrow_mut()
                .extend([through, wall(Vec3f::new(300.0, 0.0, 0.0))]);
            start(&mut world, laser);
            assert_eq!(gi.trace_pass_ents.borrow().len(), 1, "one trace");
            assert_eq!(world.entities.get(laser).unwrap().state.old_origin, at);
            assert_eq!(laser_sparks(&gi), 1, "sparks where it stopped");
        }
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
        assert_eq!(laser_sparks(&gi), 1, "turned on: sparks at the world");
        world.entities.get_mut(target).unwrap().absmin = Vec3f::new(100.0, 0.0, -10.0);
        gi.queued_traces.borrow_mut().push_back(wall(Vec3f::X));
        let think = world.entities.get(laser).unwrap().think.unwrap();
        think(&mut world, laser);
        assert_eq!(world.entities.get(laser).unwrap().game.movedir, Vec3f::X);
        assert_eq!(laser_sparks(&gi), 2);
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
    fn fact1_exit_to_fact3_goes_to_the_secret_spawn() {
        let entities = r#"{ "classname" "target_changelevel" "map" "FACT3" }"#;
        let (world, _) = spawn_map("fact1", false, entities);
        let exit = world.find_by_classname(None, "target_changelevel").unwrap();
        assert_eq!(world.entities.get(exit).unwrap().game.map, "fact3$secret1");
        let (world, _) = spawn_map("fact2", false, entities);
        let exit = world.find_by_classname(None, "target_changelevel").unwrap();
        assert_eq!(world.entities.get(exit).unwrap().game.map, "FACT3");
    }

    #[test]
    fn mine3_secret_gets_its_missing_message() {
        let entities = r#"{ "classname" "target_secret" "origin" "280 -2048 -624" }"#;
        for (map, message) in [("MINE3", "You have found a secret area."), ("mine2", "")] {
            let (world, _) = spawn_map(map, false, entities);
            let secret = world.find_by_classname(None, "target_secret").unwrap();
            assert_eq!(world.entities.get(secret).unwrap().game.message, message);
        }
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
        assert_eq!(
            played,
            vec![(
                Vec3f::new(1.0, 2.0, 3.0),
                number,
                CHAN_VOICE | CHAN_RELIABLE,
                9
            )]
        );
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

    /// Use the entity `targetname` names, as a trigger would.
    fn use_named(world: &mut GameWorld, targetname: &str) -> EntityKey {
        let key = world.find_by_targetname(None, targetname).unwrap();
        let use_fn = world.entities.get(key).unwrap().use_fn.unwrap();
        use_fn(world, key, key, key);
        key
    }

    #[test]
    fn target_temp_entity_sends_its_style_at_its_origin_to_the_pvs() {
        let (mut world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_temp_entity" "style" "5" "origin" "1 2 3" "targetname" "t" }"#,
        );
        let (writes, vectors) = (gi.writes.borrow().len(), gi.vectors.borrow().len());
        use_named(&mut world, "t");
        assert_eq!(gi.writes.borrow()[writes..], [3, 5]);
        assert_eq!(gi.vectors.borrow()[vectors..], [Vec3f::new(1.0, 2.0, 3.0)]);
        assert_eq!(
            gi.multicasts.borrow().last(),
            Some(&(Vec3f::new(1.0, 2.0, 3.0), Multicast::PVS))
        );
    }

    #[test]
    fn target_splash_sends_count_origin_movedir_and_sounds_and_hurts_with_dmg() {
        let (mut world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_splash" "sounds" "2" "angle" "90" "dmg" "50" "targetname" "s" }"#,
        );
        let victim = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(victim).unwrap();
            e.state.origin = Vec3f::new(70.0, 0.0, 0.0); // beyond dmg, within dmg + 40
            e.game.health = 100;
            e.game.takedamage = TakeDamage::Yes;
            e.solid = Solid::Bbox;
        }
        let (writes, vectors) = (gi.writes.borrow().len(), gi.vectors.borrow().len());
        let splash = use_named(&mut world, "s");
        let ent = world.entities.get(splash).unwrap();
        assert_eq!(ent.svflags, q2_shared::constants::SVF_NOCLIENT);
        assert_eq!(ent.state.angles, Vec3f::ZERO); // G_SetMovedir clears them
                                                   // svc_temp_entity, TE_SPLASH, the default count 32, then sounds
                                                   // (the damage's blood follows).
        assert_eq!(
            gi.writes.borrow()[writes..writes + 4],
            [3, TE_SPLASH, 32, 2]
        );
        let sent = gi.vectors.borrow()[vectors..].to_vec();
        assert_eq!(sent[0], Vec3f::ZERO);
        assert!((sent[1] - Vec3f::new(0.0, 1.0, 0.0)).length() < 1e-6);
        assert!(world.entities.get(victim).unwrap().game.health < 100);
    }

    #[test]
    fn target_spawner_spawns_its_target_classname_with_its_movedir_velocity() {
        let (mut world, _) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_spawner" "target" "info_notnull" "origin" "8 0 0" "angle" "90" "speed" "100" "targetname" "sp" }"#,
        );
        assert!(world.find_by_classname(None, "info_notnull").is_none());
        use_named(&mut world, "sp");
        let spawned = world.find_by_classname(None, "info_notnull").unwrap();
        let e = world.entities.get(spawned).unwrap();
        assert_eq!(e.state.origin, Vec3f::new(8.0, 0.0, 0.0));
        // G_SetMovedir cleared the spawner's angles when it set movedir.
        assert_eq!(e.state.angles, Vec3f::ZERO);
        assert!((e.velocity - Vec3f::new(0.0, 100.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn target_spawner_on_fact2_lowers_the_insane_spawner_by_8() {
        let (world, _) = spawn_map(
            "fact2",
            false,
            r#"{ "classname" "target_spawner" "target" "misc_insane" "origin" "-1504 512 72" }"#,
        );
        let spawner = world.find_by_classname(None, "target_spawner").unwrap();
        let e = world.entities.get(spawner).unwrap();
        assert_eq!(e.state.origin, Vec3f::new(-1504.0, 512.0, 64.0));
        assert_eq!(e.svflags, q2_shared::constants::SVF_NOCLIENT);
    }

    #[test]
    fn target_blaster_fires_a_hyperblaster_bolt_along_its_movedir() {
        let (mut world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_blaster" "targetname" "b" }"#,
        );
        assert!(gi
            .sounds
            .borrow()
            .contains(&"weapons/laser2.wav".to_string()));
        let blaster = use_named(&mut world, "b");
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        let b = world.entities.get(bolt).unwrap();
        assert_eq!(b.game.dmg, 15);
        assert_eq!(b.velocity, Vec3f::new(1000.0, 0.0, 0.0));
        // C passes MOD_TARGET_BLASTER in fire_blaster's `hyper` slot.
        assert_eq!(b.game.spawnflags, 1);
        assert_eq!(b.owner, Some(blaster));
        let noise = world.entities.get(blaster).unwrap().game.noise_index;
        assert!(gi
            .sound_calls
            .borrow()
            .iter()
            .any(|s| s.1 == CHAN_VOICE && s.2 == noise));
    }

    #[test]
    fn target_crosslevel_trigger_sets_its_serverflags_fires_and_goes() {
        let (mut world, _) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_crosslevel_trigger" "spawnflags" "3" "targetname" "c" "target" "next" }"#,
        );
        let next = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(next).unwrap();
            e.game.targetname = "next".into();
            e.use_fn = Some(bump);
        }
        world.game.serverflags = 0x100;
        let trigger = use_named(&mut world, "c");
        assert_eq!(world.game.serverflags, 0x103);
        assert_eq!(world.entities.get(next).unwrap().game.count, 1);
        assert!(!world.entities.is_live(trigger));
    }

    #[test]
    fn target_crosslevel_target_fires_after_two_delays_only_when_its_flags_are_set() {
        let (mut world, _) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_crosslevel_target" "spawnflags" "3" "target" "yes" }
               { "classname" "target_crosslevel_target" "spawnflags" "4" "target" "no" }"#,
        );
        world.game.serverflags = 0x103;
        let mut counter = |name: &str| {
            let key = world.spawn().unwrap();
            let e = world.entities.get_mut(key).unwrap();
            e.game.targetname = name.into();
            e.use_fn = Some(bump);
            key
        };
        let (yes, no) = (counter("yes"), counter("no"));
        // It thinks at its delay (default 1 s), and G_UseTargets waits the
        // same delay again through a DelayedUse.
        for _ in 0..19 {
            world.run_frame();
        }
        assert_eq!(world.entities.get(yes).unwrap().game.count, 0);
        world.run_frame();
        assert_eq!(world.entities.get(yes).unwrap().game.count, 1);
        assert_eq!(world.entities.get(no).unwrap().game.count, 0);
    }

    fn lightramp_world(ramp: &str) -> (GameWorld, std::rc::Rc<crate::world::MockGameImport>) {
        spawn_map(
            "base1",
            false,
            &format!(
                r#"{{ "classname" "light" "targetname" "l" "style" "32" }}
                   {{ "classname" "target_lightramp" "target" "l" "message" "{ramp}" "speed" "1" "targetname" "r" }}"#
            ),
        )
    }

    #[test]
    fn target_lightramp_ramps_the_light_style_each_frame_without_using_the_light() {
        let (mut world, gi) = lightramp_world("kz");
        let index = q2_shared::constants::CS_LIGHTS as i32 + 32;
        let light = world.find_by_targetname(None, "l").unwrap();
        let spawnflags = world.entities.get(light).unwrap().game.spawnflags;
        let before = gi.configstrings.borrow().len();
        use_named(&mut world, "r");
        for _ in 0..12 {
            world.run_frame();
        }
        let ramp: String = gi.configstrings.borrow()[before..]
            .iter()
            .filter(|(i, _)| *i == index)
            .map(|(_, s)| s.as_str())
            .collect();
        // C's float time over its double FRAMETIME, truncated to a letter.
        assert_eq!(ramp, "klnoqrtuwxz");
        assert_eq!(
            world.entities.get(light).unwrap().game.spawnflags,
            spawnflags
        );
    }

    #[test]
    fn target_lightramp_with_a_bad_ramp_or_no_target_is_freed_with_a_warning() {
        let (world, gi) = lightramp_world("zz");
        assert!(world.find_by_classname(None, "target_lightramp").is_none());
        assert!(gi
            .dprints
            .borrow()
            .contains(&"target_lightramp has bad ramp (zz) at (0 0 0)\n".to_string()));
        let (world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_lightramp" "message" "az" "origin" "1 2 3" }"#,
        );
        assert!(world.find_by_classname(None, "target_lightramp").is_none());
        assert!(gi
            .dprints
            .borrow()
            .contains(&"target_lightramp with no target at (1 2 3)\n".to_string()));
    }

    #[test]
    fn target_earthquake_throws_grounded_clients_and_rumbles() {
        let (mut world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_earthquake" "targetname" "q" "origin" "4 5 6" }"#,
        );
        let ground = world.entities.key_for_number(0);
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        {
            let p = world.entities.get_mut(player).unwrap();
            p.game.ground_entity = ground;
            p.game.mass = 200;
        }
        let monster = world.spawn().unwrap();
        {
            let m = world.entities.get_mut(monster).unwrap();
            m.game.ground_entity = ground;
            m.game.mass = 200;
        }
        let quake = use_named(&mut world, "q");
        world.run_frame();
        let p = world.entities.get(player).unwrap();
        assert_eq!(p.game.ground_entity, None);
        // C `speed * (100.0 / mass)`, speed defaulting to 200.
        assert_eq!(p.velocity.z, 100.0);
        assert_eq!(
            world.entities.get(monster).unwrap().game.ground_entity,
            ground
        );
        let noise = world.entities.get(quake).unwrap().game.noise_index;
        assert_eq!(
            gi.sounds.borrow()[noise as usize - 1],
            "world/quake.wav".to_string()
        );
        let number = world.number_of(quake).unwrap();
        assert_eq!(
            gi.positioned_sounds.borrow().as_slice(),
            [(Vec3f::new(4.0, 5.0, 6.0), number, CHAN_AUTO, noise)]
        );
    }

    #[test]
    fn untargeted_target_earthquake_warns_in_c_form() {
        let (_, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_earthquake" "origin" "1 2 3" }"#,
        );
        assert!(gi
            .dprints
            .borrow()
            .contains(&"untargeted target_earthquake at (1 2 3)\n".to_string()));
    }

    #[test]
    fn target_splash_writes_c_field_types_in_c_order() {
        use crate::world::Write::{Byte, Dir, Position};
        let (mut world, gi) = spawn_map(
            "base1",
            false,
            r#"{ "classname" "target_splash" "sounds" "2" "origin" "1 2 3" "targetname" "s" }"#,
        );
        let before = gi.message.borrow().len();
        use_named(&mut world, "s");
        assert_eq!(
            gi.message.borrow()[before..],
            [
                Byte(3),
                Byte(TE_SPLASH),
                Byte(32),
                Position(Vec3f::new(1.0, 2.0, 3.0)),
                Dir(Vec3f::new(1.0, 0.0, 0.0)),
                Byte(2),
            ]
        );
        assert_eq!(
            gi.multicasts.borrow().last(),
            Some(&(Vec3f::new(1.0, 2.0, 3.0), Multicast::PVS))
        );
    }
}
