//! Miscellaneous entities — explobox, teleporters, gibs, path_corners, lights.
//!
//! Part of `g_misc.c`: misc_explobox, misc_teleporter(_dest), path_corner,
//! point_combat, info_null/info_notnull and func_group. The decorative
//! misc_* models, gibs and light_mine spawn as static models. The gib,
//! debris and explosion helpers monsters die through (ThrowGib, ThrowHead,
//! ThrowClientHead, ThrowDebris, BecomeExplosion1/2) live here too.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_misc.c`

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

use q2_shared::constants::{EF_GIB, EF_TELEPORTER, EV_OTHER_TELEPORT, EV_PLAYER_TELEPORT};

/// C `vectoyaw`.
fn vectoyaw(v: Vec3f) -> f32 {
    if v.y == 0.0 && v.x == 0.0 {
        return 0.0;
    }
    let mut yaw = v.y.atan2(v.x).to_degrees().trunc();
    if yaw < 0.0 {
        yaw += 360.0;
    }
    yaw
}

/// Move a touching client to the teleporter's destination, hold it briefly
/// and reset its view, telefragging whatever is at the destination.
/// C ref: `teleporter_touch`.
fn teleporter_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    if world
        .entities
        .get(other_key)
        .is_none_or(|o| o.client.is_none())
    {
        return;
    }
    let Some(target) = world.entities.get(self_key).map(|e| e.game.target.clone()) else {
        return;
    };
    let Some(dest) = world.find_by_targetname(None, &target) else {
        world.gi.dprintf("Couldn't find destination\n");
        return;
    };
    let Some((dest_origin, dest_angles)) = world
        .entities
        .get(dest)
        .map(|d| (d.state.origin, d.state.angles))
    else {
        return;
    };

    // Unlink to make sure it can't possibly interfere with KillBox.
    if let Some(number) = world.number_of(other_key) {
        world.gi.unlink_entity(number);
    }
    let Some(other) = world.entities.get_mut(other_key) else {
        return;
    };
    other.state.origin = dest_origin;
    other.state.old_origin = dest_origin;
    other.state.origin.z += 10.0;
    // Clear the velocity and hold them in place briefly.
    other.velocity = Vec3f::ZERO;
    other.state.event = EV_PLAYER_TELEPORT;
    other.state.angles = Vec3f::ZERO;
    if let Some(client) = other.client.and_then(|slot| world.clients.get_mut(slot)) {
        client.ps.pmove.pm_time = 160 >> 3;
        client.ps.pmove.pm_flags |= q2_common::pmove::PMF_TIME_TELEPORT;
        for i in 0..3 {
            let delta = dest_angles[i] - client.resp.cmd_angles[i];
            client.ps.pmove.delta_angles[i] = ((delta * 65536.0 / 360.0) as i32 & 65535) as i16;
        }
        client.ps.viewangles = Vec3f::ZERO;
    }
    // Draw the teleport splash at the source too.
    if let Some(pad) = world
        .entities
        .get(self_key)
        .and_then(|t| t.owner)
        .and_then(|k| world.entities.get_mut(k))
    {
        pad.state.event = EV_PLAYER_TELEPORT;
    }
    // Kill anything at the destination.
    world.kill_box(other_key);
    world.link_entity(other_key);
}

/// A monster reached the path_corner it was heading for: fire pathtarget,
/// pick the next corner (teleporting to it if flagged), pause for `wait`.
/// C ref: `path_corner_touch`.
fn path_corner_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let Some(other) = world.entities.get(other_key) else {
        return;
    };
    if other.game.movetarget != Some(self_key) || other.game.enemy.is_some() {
        return;
    }
    let Some(corner) = world.entities.get(self_key) else {
        return;
    };
    let (pathtarget, target, wait) = (
        corner.game.pathtarget.clone(),
        corner.game.target.clone(),
        corner.game.wait,
    );

    if !pathtarget.is_empty() {
        if let Some(c) = world.entities.get_mut(self_key) {
            c.game.target = pathtarget;
        }
        world.use_targets(self_key, other_key);
        if let Some(c) = world.entities.get_mut(self_key) {
            c.game.target = target.clone();
        }
    }

    let mut next = world.pick_target(&target);
    if let Some((origin, mins_z, next_target)) = next
        .and_then(|n| world.entities.get(n))
        .filter(|n| n.game.spawnflags & 1 != 0)
        .map(|n| (n.state.origin, n.mins.z, n.game.target.clone()))
    {
        let other_mins_z = world.entities.get(other_key).map_or(0.0, |o| o.mins.z);
        if let Some(other) = world.entities.get_mut(other_key) {
            other.state.origin = origin;
            other.state.origin.z += mins_z - other_mins_z;
            other.state.event = EV_OTHER_TELEPORT;
        }
        next = world.pick_target(&next_target);
    }

    let time = world.level.time;
    let goal_origin = next
        .and_then(|n| world.entities.get(n))
        .map(|n| n.state.origin);
    let Some(other) = world.entities.get_mut(other_key) else {
        return;
    };
    other.game.goalentity = next;
    other.game.movetarget = next;

    let pause = if wait != 0.0 {
        Some(time + wait)
    } else if next.is_none() {
        Some(time + 100_000_000.0)
    } else {
        None
    };
    match (pause, goal_origin) {
        (Some(until), _) => {
            let stand = other.monsterinfo.as_mut().and_then(|m| {
                m.pausetime = until;
                m.stand
            });
            if let Some(stand) = stand {
                stand(world, other_key);
            }
        }
        (None, Some(goal)) => other.game.ideal_yaw = vectoyaw(goal - other.state.origin),
        (None, None) => {}
    }
}

/// The barrel was killed: explode two frames later. C ref: `barrel_delay`.
fn barrel_delay(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.takedamage = TakeDamage::No;
        ent.game.nextthink = time + 2.0 * FRAMETIME;
        ent.think = Some(barrel_explode);
        ent.game.activator = Some(attacker);
    }
}

/// Radius damage, debris, then the barrel becomes an explosion.
/// C ref: `barrel_explode`.
fn barrel_explode(world: &mut GameWorld, self_key: EntityKey) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let (dmg, save, absmin, size) = (ent.game.dmg, ent.state.origin, ent.absmin, ent.size);
    let attacker = ent.game.activator.unwrap_or(self_key);
    world.t_radius_damage(
        self_key,
        attacker,
        dmg as f32,
        None,
        (dmg + 40) as f32,
        MeansOfDeath::Barrel,
    );
    // C throws from the box centre, then puts the origin back.
    let center = absmin + size * 0.5;
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.state.origin = center;
    }
    let random_point = |world: &mut GameWorld| {
        let x = center.x + world.crandom() * size.x;
        let y = center.y + world.crandom() * size.y;
        let z = center.z + world.crandom() * size.z;
        Vec3f::new(x, y, z)
    };

    // A few big chunks.
    let spd = 1.5 * dmg as f32 / 200.0;
    for _ in 0..2 {
        let org = random_point(world);
        throw_debris(world, self_key, "models/objects/debris1/tris.md2", spd, org);
    }

    // Bottom corners.
    let spd = 1.75 * dmg as f32 / 200.0;
    for (dx, dy) in [(0.0, 0.0), (size.x, 0.0), (0.0, size.y), (size.x, size.y)] {
        let org = absmin + Vec3f::new(dx, dy, 0.0);
        throw_debris(world, self_key, "models/objects/debris3/tris.md2", spd, org);
    }

    // A bunch of little chunks; C computes this speed in integers.
    let spd = (2 * dmg / 200) as f32;
    for _ in 0..8 {
        let org = random_point(world);
        throw_debris(world, self_key, "models/objects/debris2/tris.md2", spd, org);
    }

    let grounded = match world.entities.get_mut(self_key) {
        Some(ent) => {
            ent.state.origin = save;
            ent.game.ground_entity.is_some()
        }
        None => return,
    };
    if grounded {
        become_explosion2(world, self_key);
    } else {
        become_explosion1(world, self_key);
    }
}

// ---------------------------------------------------------------------------
// Gibs, debris and explosions
// ---------------------------------------------------------------------------

/// C `GIB_ORGANIC` / `GIB_METALLIC`: organic gibs fall and splat, metal
/// ones bounce twice as hard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GibType {
    Organic,
    Metallic,
}

/// `EF_FLIES` (shared.h): the corpse fly swarm a head no longer has.
const EF_FLIES: u32 = 0x0000_4000;
/// The gib that animates when it lands (C `sm_meat_index`).
const SM_MEAT_MODEL: &str = "models/objects/gibs/sm_meat/tris.md2";

/// C `think = G_FreeEdict`.
fn free_think(world: &mut GameWorld, key: EntityKey) {
    world.free_entity(key);
}

/// A random upward kick, harder for heavy hits. C ref: `VelocityForDamage`.
pub fn velocity_for_damage(world: &mut GameWorld, damage: i32) -> Vec3f {
    let x = 100.0 * world.crandom();
    let y = 100.0 * world.crandom();
    let z = 200.0 + 100.0 * world.random();
    let v = Vec3f::new(x, y, z);
    if damage < 50 {
        v * 0.7
    } else {
        v * 1.2
    }
}

/// Keep a gib's speed within ±300 sideways and 200..500 up.
/// C ref: `ClipGibVelocity` (on `ent->velocity`).
pub fn clip_gib_velocity(velocity: &mut Vec3f) {
    velocity.x = velocity.x.clamp(-300.0, 300.0);
    velocity.y = velocity.y.clamp(-300.0, 300.0);
    // Always some upwards.
    velocity.z = velocity.z.clamp(200.0, 500.0);
}

/// Landed meat runs frames 1..10, then lingers 8-18 s. C ref: `gib_think`.
pub fn gib_think(world: &mut GameWorld, self_key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.state.frame += 1;
    ent.game.nextthink = time + FRAMETIME;
    if ent.state.frame == 10 {
        ent.think = Some(free_think);
        let linger = world.random() * 10.0;
        if let Some(ent) = world.entities.get_mut(self_key) {
            ent.game.nextthink = time + 8.0 + linger;
        }
    }
}

/// On landing, lie flat along the surface; meat splats and animates.
/// C ref: `gib_touch`.
pub fn gib_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    if ent.game.ground_entity.is_none() {
        return;
    }
    ent.touch = None;
    let Some(plane) = plane else {
        return;
    };

    let splat = world.gi.sound_index("misc/fhit3.wav");
    world.gi.sound(
        world.number_of(self_key),
        CHAN_VOICE,
        splat,
        1.0,
        ATTN_NORM,
        0.0,
    );

    let normal_angles = crate::weapons::vectoangles(plane.normal);
    let (_, right, _) = crate::physics::angle_vectors(normal_angles);
    let meat = world.gi.model_index(SM_MEAT_MODEL);
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.state.angles = crate::weapons::vectoangles(right);
    if ent.state.modelindex == meat {
        ent.state.frame += 1;
        ent.think = Some(gib_think);
        ent.game.nextthink = time + FRAMETIME;
    }
}

/// Shot gibs vanish. C ref: `gib_die`.
pub fn gib_die(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    world.free_entity(self_key);
}

/// The C fields a gib or head takes from its type: organic ones toss and
/// splat at half the kick, metal ones bounce at the full kick.
fn gib_motion(world: &mut GameWorld, key: EntityKey, damage: i32, gib_type: GibType) {
    let vd = velocity_for_damage(world, damage);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let vscale = match gib_type {
        GibType::Organic => {
            ent.game.movetype = MoveType::Toss;
            ent.touch = Some(gib_touch);
            0.5
        }
        GibType::Metallic => {
            ent.game.movetype = MoveType::Bounce;
            1.0
        }
    };
    ent.velocity += vd * vscale;
    clip_gib_velocity(&mut ent.velocity);
}

/// Throw a gib from somewhere in `self_key`'s box, carrying its velocity.
/// C ref: `ThrowGib`.
pub fn throw_gib(
    world: &mut GameWorld,
    self_key: EntityKey,
    gibname: &str,
    damage: i32,
    gib_type: GibType,
) {
    let Some(me) = world.entities.get(self_key) else {
        return;
    };
    let (absmin, size, velocity) = (me.absmin, me.size * 0.5, me.velocity);
    if world.level.gibs_this_frame >= MAX_GIBS {
        return;
    }
    let Some(gib) = world.spawn() else {
        return;
    };
    world.level.gibs_this_frame += 1;
    let origin = absmin + size;
    let x = origin.x + world.crandom() * size.x;
    let y = origin.y + world.crandom() * size.y;
    let z = origin.z + world.crandom() * size.z;
    if let Some(ent) = world.entities.get_mut(gib) {
        ent.state.origin = Vec3f::new(x, y, z);
        ent.velocity = velocity;
    }
    world.set_model(gib, gibname);
    if let Some(ent) = world.entities.get_mut(gib) {
        ent.solid = Solid::Bbox;
        ent.svflags = SvFlags::DEADMONSTER.bits();
        ent.state.effects |= EF_GIB;
        ent.game.flags |= EntityFlags::NO_KNOCKBACK;
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(gib_die);
        ent.game.health = 250;
    }
    gib_motion(world, gib, damage, gib_type);
    let spin = Vec3f::new(world.random(), world.random(), world.random()) * 600.0;
    let linger = world.random() * 10.0;
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(gib) {
        ent.avelocity = spin;
        ent.think = Some(free_think);
        ent.game.nextthink = time + 10.0 + linger;
    }
    world.link_entity(gib);
}

/// Turn `self_key` itself into its severed head. C ref: `ThrowHead`.
pub fn throw_head(
    world: &mut GameWorld,
    self_key: EntityKey,
    gibname: &str,
    damage: i32,
    gib_type: GibType,
) {
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.state.skinnum = 0;
    ent.state.frame = 0;
    ent.mins = Vec3f::ZERO;
    ent.maxs = Vec3f::ZERO;
    ent.state.modelindex2 = 0;
    world.set_model(self_key, gibname);
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.solid = Solid::Bbox;
    ent.state.effects |= EF_GIB;
    ent.state.effects &= !EF_FLIES;
    ent.state.sound = 0;
    ent.game.flags |= EntityFlags::NO_KNOCKBACK;
    ent.svflags &= !SvFlags::MONSTER.bits();
    ent.game.takedamage = TakeDamage::Yes;
    ent.game.targetname.clear();
    ent.die = Some(gib_die);
    // C: the entity still has the monster's clipmask.
    ent.clipmask = MASK_SHOT;
    gib_motion(world, self_key, damage, gib_type);
    let yaw = world.crandom() * 600.0;
    let linger = world.random() * 10.0;
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.avelocity.y = yaw;
        ent.think = Some(free_think);
        ent.game.nextthink = time + 10.0 + linger;
    }
    world.link_entity(self_key);
}

/// Turn a gibbed player (or queued body) into a bouncing head or skull.
/// C ref: `ThrowClientHead`.
pub fn throw_client_head(world: &mut GameWorld, self_key: EntityKey, damage: i32) {
    let (gibname, skin) = if world.random_u32() & 1 != 0 {
        // The second skin is the player's.
        ("models/objects/gibs/head2/tris.md2", 1)
    } else {
        ("models/objects/gibs/skull/tris.md2", 0)
    };
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.state.skinnum = skin;
    ent.state.origin.z += 32.0;
    ent.state.frame = 0;
    world.set_model(self_key, gibname);
    let vd = velocity_for_damage(world, damage);
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.mins = Vec3f::new(-16.0, -16.0, 0.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 16.0);
    ent.game.takedamage = TakeDamage::No;
    ent.solid = Solid::Bbox;
    ent.state.effects = EF_GIB;
    ent.state.sound = 0;
    ent.game.flags |= EntityFlags::NO_KNOCKBACK;
    ent.clipmask = MASK_SHOT;
    ent.game.movetype = MoveType::Bounce;
    ent.velocity += vd;
    // Bodies in the queue have no client any more.
    match ent.client {
        Some(slot) => {
            let frame = ent.state.frame;
            if let Some(client) = world.clients.get_mut(slot) {
                client.anim_priority = ANIM_DEATH;
                client.anim_end = frame;
            }
        }
        None => {
            ent.think = None;
            ent.game.nextthink = 0.0;
        }
    }
    world.link_entity(self_key);
}

/// Shot debris vanishes. C ref: `debris_die`.
pub fn debris_die(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    world.free_entity(self_key);
}

/// Fling a chunk of `modelname` from `origin`, `speed` times a random kick
/// on top of `self_key`'s velocity. C ref: `ThrowDebris`.
pub fn throw_debris(
    world: &mut GameWorld,
    self_key: EntityKey,
    modelname: &str,
    speed: f32,
    origin: Vec3f,
) {
    let Some(velocity) = world.entities.get(self_key).map(|e| e.velocity) else {
        return;
    };
    if world.level.debris_this_frame >= MAX_DEBRIS {
        return;
    }
    let Some(chunk) = world.spawn() else {
        return;
    };
    world.level.debris_this_frame += 1;
    if let Some(ent) = world.entities.get_mut(chunk) {
        ent.state.origin = origin;
    }
    world.set_model(chunk, modelname);
    let x = 100.0 * world.crandom();
    let y = 100.0 * world.crandom();
    let z = 100.0 + 100.0 * world.crandom();
    let spin = Vec3f::new(world.random(), world.random(), world.random()) * 600.0;
    let linger = world.random() * 5.0;
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(chunk) {
        ent.velocity = velocity + Vec3f::new(x, y, z) * speed;
        ent.game.movetype = MoveType::Bounce;
        ent.solid = Solid::Not;
        ent.avelocity = spin;
        ent.think = Some(free_think);
        ent.game.nextthink = time + 5.0 + linger;
        ent.state.frame = 0;
        ent.game.flags = EntityFlags::empty();
        ent.game.classname = "debris".into();
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(debris_die);
        ent.game.health = 250;
    }
    world.link_entity(chunk);
}

/// Replace the entity with an explosion temp entity at its origin.
fn become_explosion(world: &mut GameWorld, self_key: EntityKey, effect: i32) {
    let Some(origin) = world.entities.get(self_key).map(|e| e.state.origin) else {
        return;
    };
    world
        .gi
        .write_byte(q2_shared::protocol::SvcOp::TempEntity as i32);
    world.gi.write_byte(effect);
    world.gi.write_position(origin);
    world.gi.multicast(origin, Multicast::PVS);
    world.free_entity(self_key);
}

/// C ref: `BecomeExplosion1` (`TE_EXPLOSION1`, the airborne blast).
pub fn become_explosion1(world: &mut GameWorld, self_key: EntityKey) {
    become_explosion(world, self_key, TE_EXPLOSION1);
}

/// C ref: `BecomeExplosion2` (`TE_EXPLOSION2`, the grounded blast).
pub fn become_explosion2(world: &mut GameWorld, self_key: EntityKey) {
    become_explosion(world, self_key, TE_EXPLOSION2);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

/// Roll away from whoever walks into it, by their mass against its own,
/// unless they are in the air or standing on it. C ref: `barrel_touch`.
pub fn barrel_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    let (Some(barrel), Some(pusher)) = (world.entities.get(self_key), world.entities.get(other))
    else {
        return;
    };
    if pusher.game.ground_entity.is_none() || pusher.game.ground_entity == Some(self_key) {
        return;
    }
    let ratio = pusher.game.mass as f32 / barrel.game.mass as f32;
    let away = barrel.state.origin - pusher.state.origin;
    crate::monster::movement::m_walkmove(world, self_key, vectoyaw(away), 20.0 * ratio * FRAMETIME);
}

/// C ref: `SP_misc_explobox` (the model comes from `spawn::SPAWNS`);
/// `finish_misc_explobox` does the parts that need the level.
pub fn sp_misc_explobox(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.solid = Solid::Bbox;
        ent.game.movetype = MoveType::Step;
        ent.mins = Vec3f::new(-16.0, -16.0, 0.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 40.0);
        if ent.game.mass == 0 {
            ent.game.mass = 400;
        }
        if ent.game.health == 0 {
            ent.game.health = 10;
        }
        if ent.game.dmg == 0 {
            ent.game.dmg = 150;
        }
        ent.die = Some(barrel_delay);
        ent.game.takedamage = TakeDamage::Yes;
        ent.monsterinfo.get_or_insert_with(Box::default).aiflags = AiFlags::NOSTEP;
        ent.touch = Some(barrel_touch);
    }
}

/// The rest of C `SP_misc_explobox`: gone in deathmatch, the debris
/// models precached, and dropped to the floor two frames in.
pub fn finish_misc_explobox(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if world.game.deathmatch {
        world.free_entity(key);
        return;
    }
    for model in [
        "models/objects/debris1/tris.md2",
        "models/objects/debris2/tris.md2",
        "models/objects/debris3/tris.md2",
    ] {
        world.gi.model_index(model);
    }
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.think = Some(crate::ai::m_droptofloor);
        ent.game.nextthink = time + 2.0 * FRAMETIME;
    }
    world.link_entity(key);
}

/// C ref: `SP_misc_teleporter`: a solid pad plus a separate trigger above
/// it that does the teleporting.
pub fn sp_misc_teleporter(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    if ent.game.target.is_empty() {
        tracing::debug!("teleporter without a target");
        storage.soft_free(key);
        return;
    }
    ent.state.skinnum = 1;
    ent.state.effects = EF_TELEPORTER;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-32.0, -32.0, -24.0);
    ent.maxs = Vec3f::new(32.0, 32.0, -16.0);
    let (origin, target) = (ent.state.origin, ent.game.target.clone());

    let Some(trig) = storage.spawn() else {
        return;
    };
    if let Some(t) = storage.get_mut(trig) {
        t.game.classname = "teleporter_touch".to_string();
        t.touch = Some(teleporter_touch);
        t.solid = Solid::Trigger;
        t.game.target = target;
        t.owner = Some(key);
        t.state.origin = origin;
        t.mins = Vec3f::new(-8.0, -8.0, 8.0);
        t.maxs = Vec3f::new(8.0, 8.0, 24.0);
    }
}

/// C ref: `SP_misc_teleporter_dest`: a solid spawn pad (its model comes
/// from `spawn::SPAWNS`).
pub fn sp_misc_teleporter_dest(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.state.skinnum = 0;
        ent.solid = Solid::Bbox;
        ent.mins = Vec3f::new(-32.0, -32.0, -24.0);
        ent.maxs = Vec3f::new(32.0, 32.0, -16.0);
    }
}

/// C ref: `SP_path_corner`.
pub fn sp_path_corner(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    if ent.game.targetname.is_empty() {
        tracing::debug!("path_corner with no targetname at {:?}", ent.state.origin);
        storage.soft_free(key);
        return;
    }
    ent.solid = Solid::Trigger;
    ent.touch = Some(path_corner_touch);
    ent.mins = Vec3f::splat(-8.0);
    ent.maxs = Vec3f::splat(8.0);
    ent.svflags |= q2_shared::constants::SVF_NOCLIENT;
}

pub fn sp_point_combat(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "point_combat".to_string();
        if let Some(tn) = fields.get("targetname") {
            ent.game.targetname = tn.clone();
        }
    }
}

pub fn sp_info_null(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    // info_null and func_group are freed immediately — they're map editor hints.
    storage.soft_free(key);
}

pub fn sp_info_notnull(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "info_notnull".to_string();
        if let Some(tn) = fields.get("targetname") {
            ent.game.targetname = tn.clone();
        }
    }
}

pub fn sp_misc_generic(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "misc".to_string());
    }
}

pub fn sp_misc_gib(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "misc_gib".to_string());
        ent.game.movetype = MoveType::Toss;
        ent.solid = Solid::Not;
    }
}

pub fn sp_light_mine(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "light_mine".to_string());
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    #[test]
    fn register_misc_spawns() {
        let world = test_world();
        assert!(world.spawn_table.get("misc_explobox").is_some());
        assert!(world.spawn_table.get("misc_teleporter").is_some());
        assert!(world.spawn_table.get("path_corner").is_some());
        assert!(world.spawn_table.get("info_null").is_some());
    }

    #[test]
    fn teleporter_moves_entity() {
        let mut world = test_world();

        // Create teleporter.
        let tele = world.spawn().unwrap();
        world.entities.get_mut(tele).unwrap().game.target = "dest1".to_string();

        // Create destination.
        let dest = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(dest).unwrap();
            ent.game.targetname = "dest1".to_string();
            ent.state.origin = Vec3f::new(1000.0, 2000.0, 3000.0);
        }

        // Create player at origin.
        let player = world.spawn_test_client(Default::default());

        teleporter_touch(&mut world, tele, player, None, None);

        // C: 10 units above the destination, held briefly.
        let ent = world.entities.get(player).unwrap();
        assert_eq!(ent.state.origin, Vec3f::new(1000.0, 2000.0, 3010.0));
        let pm = world.client(player).unwrap().ps.pmove;
        assert_ne!(pm.pm_flags & q2_common::pmove::PMF_TIME_TELEPORT, 0);
        assert_eq!(pm.pm_time, 160 >> 3, "C: hold time in 8 ms units");
    }

    #[test]
    fn teleporter_ignores_non_clients() {
        let mut world = test_world();
        let tele = world.spawn().unwrap();
        world.entities.get_mut(tele).unwrap().game.target = "dest1".to_string();
        let dest = world.spawn().unwrap();
        world.entities.get_mut(dest).unwrap().game.targetname = "dest1".to_string();
        world.entities.get_mut(dest).unwrap().state.origin = Vec3f::splat(500.0);
        let rocket = world.spawn().unwrap();
        teleporter_touch(&mut world, tele, rocket, None, None);
        assert_eq!(
            world.entities.get(rocket).unwrap().state.origin,
            Vec3f::ZERO
        );
    }

    #[test]
    fn teleporter_pad_spawns_a_touch_trigger_above_it() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "misc_teleporter" "target" "d" "origin" "0 0 64" }"#,
            "",
        );
        let pad = world.find_by_classname(None, "misc_teleporter").unwrap();
        let trig = world.find_by_classname(None, "teleporter_touch").unwrap();
        let pad_ent = world.entities.get(pad).unwrap();
        assert_eq!(pad_ent.solid, Solid::Bbox);
        assert_eq!(pad_ent.maxs, Vec3f::new(32.0, 32.0, -16.0));
        let t = world.entities.get(trig).unwrap();
        assert_eq!(t.solid, Solid::Trigger);
        assert_eq!(t.owner, Some(pad));
        assert_eq!(t.state.origin, Vec3f::new(0.0, 0.0, 64.0));
        assert_eq!(t.maxs, Vec3f::new(8.0, 8.0, 24.0));
    }

    #[test]
    fn path_corner_is_a_small_trigger_and_needs_a_targetname() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "path_corner" "targetname" "p1" }
               { "classname" "path_corner" }"#,
            "",
        );
        let corner = world.find_by_classname(None, "path_corner").unwrap();
        let ent = world.entities.get(corner).unwrap();
        assert_eq!(ent.game.targetname, "p1");
        assert_eq!(ent.solid, Solid::Trigger);
        assert_eq!(ent.maxs, Vec3f::splat(8.0));
        assert!(world
            .find_by_classname(Some(corner), "path_corner")
            .is_none());
    }

    #[test]
    fn monster_reaching_its_path_corner_heads_for_the_next_one() {
        let mut world = test_world();
        let c1 = world.spawn().unwrap();
        let c2 = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(c1).unwrap();
            e.game.targetname = "p1".into();
            e.game.target = "p2".into();
        }
        {
            let e = world.entities.get_mut(c2).unwrap();
            e.game.targetname = "p2".into();
            e.state.origin = Vec3f::new(0.0, 100.0, 0.0);
        }
        let monster = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().game.movetarget = Some(c1);
        let player = world.spawn().unwrap();

        path_corner_touch(&mut world, c1, player, None, None);
        assert_eq!(world.entities.get(player).unwrap().game.target, "");

        path_corner_touch(&mut world, c1, monster, None, None);
        let m = world.entities.get(monster).unwrap();
        assert_eq!(m.game.movetarget, Some(c2));
        assert_eq!(m.game.goalentity, Some(c2));
        assert_eq!(m.game.ideal_yaw, 90.0);
    }

    #[test]
    fn explobox_dies_with_radius_damage() {
        let mut world = test_world();

        let box_ent = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(box_ent).unwrap();
            ent.game.dmg = 150;
            ent.state.origin = Vec3f::ZERO;
        }

        let victim = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(victim).unwrap();
            ent.state.origin = Vec3f::new(20.0, 0.0, 0.0);
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
            ent.solid = Solid::Bbox; // findradius skips SOLID_NOT
        }

        let attacker = world.spawn().unwrap();

        barrel_delay(&mut world, box_ent, attacker, attacker, 100, Vec3f::ZERO);
        // C barrel_delay: the blast comes two frames later.
        world.run_frame();
        assert_eq!(world.entities.get(victim).unwrap().game.health, 200);
        world.run_frame();

        // Victim should have taken splash damage.
        assert!(world.entities.get(victim).unwrap().game.health < 200);
        // Box should be freed.
        assert!(!world.entities.get(box_ent).is_some_and(|e| e.in_use));
    }

    #[test]
    fn explobox_spawns_with_the_c_box_and_health() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "misc_explobox" }"#,
            "",
        );
        let barrel = world.find_by_classname(None, "misc_explobox").unwrap();
        let ent = world.entities.get(barrel).unwrap();
        assert_eq!(ent.game.health, 10);
        assert_eq!(ent.game.mass, 400);
        assert_eq!(ent.game.dmg, 150);
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-16.0, -16.0, 0.0), Vec3f::new(16.0, 16.0, 40.0))
        );
    }

    #[test]
    fn explobox_is_pushable_and_drops_to_the_floor_two_frames_in() {
        let mut world = test_world();
        world.init_game();
        world.level.time = 1.0;
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "misc_explobox" }"#,
            "",
        );
        let barrel = world.find_by_classname(None, "misc_explobox").unwrap();
        let ent = world.entities.get(barrel).unwrap();
        assert!(ent.touch == Some(barrel_touch as crate::entity::TouchFn));
        assert!(ent
            .monsterinfo
            .as_ref()
            .is_some_and(|m| m.aiflags.contains(AiFlags::NOSTEP)));
        assert!(ent.think == Some(crate::ai::m_droptofloor as crate::entity::ThinkFn));
        assert!((ent.game.nextthink - (world.level.time + 0.2)).abs() < 1e-5);
        assert_eq!(world.level.total_monsters, 0, "not a monster");
    }

    #[test]
    fn deathmatch_removes_exploboxes() {
        let mut world = test_world();
        world.init_game();
        world.game.deathmatch = true;
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "misc_explobox" }"#,
            "",
        );
        assert!(world.find_by_classname(None, "misc_explobox").is_none());
    }

    /// A barrel on the floor at the origin and a 200-mass player at `x`.
    fn barrel_and_pusher(x: f32) -> (GameWorld, EntityKey, EntityKey) {
        let mut world = crate::monster::test_support::floor_world(None);
        let ground = world.entities.key_for_number(0);
        let barrel = world.spawn().unwrap();
        sp_misc_explobox(&mut world.entities, barrel, &HashMap::new());
        world.entities.get_mut(barrel).unwrap().game.ground_entity = ground;
        world.link_entity(barrel);
        let player = world.spawn().unwrap();
        let p = world.entities.get_mut(player).unwrap();
        p.state.origin = Vec3f::new(x, 0.0, 24.0);
        p.game.mass = 200;
        p.game.ground_entity = ground;
        (world, barrel, player)
    }

    #[test]
    fn walking_into_a_barrel_pushes_it_away_by_mass_ratio() {
        // C barrel_touch: 20 * (200 / 400) * FRAMETIME = 1 unit per touch,
        // away from the toucher.
        let (mut world, barrel, player) = barrel_and_pusher(-40.0);
        barrel_touch(&mut world, barrel, player, None, None);
        let origin = world.entities.get(barrel).unwrap().state.origin;
        assert!((origin - Vec3f::new(1.0, 0.0, 0.0)).length() < 0.01, "{origin}");
    }

    #[test]
    fn a_barrel_ignores_touches_from_the_air_or_from_on_top() {
        let (mut world, barrel, player) = barrel_and_pusher(-40.0);
        world.entities.get_mut(player).unwrap().game.ground_entity = None;
        barrel_touch(&mut world, barrel, player, None, None);
        assert_eq!(world.entities.get(barrel).unwrap().state.origin, Vec3f::ZERO);
        world.entities.get_mut(player).unwrap().game.ground_entity = Some(barrel);
        barrel_touch(&mut world, barrel, player, None, None);
        assert_eq!(world.entities.get(barrel).unwrap().state.origin, Vec3f::ZERO);
    }

    #[test]
    fn info_null_frees_immediately() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        assert_eq!(world.entities.count(), 1);

        sp_info_null(&mut world.entities, key, &HashMap::new());

        // C G_FreeEdict: a soft free, the number stays reserved.
        assert!(!world.entities.is_live(key));
    }

    #[test]
    fn path_corner_sets_next_target() {
        let mut world = test_world();

        let corner = world.spawn().unwrap();
        world.entities.get_mut(corner).unwrap().game.target = "corner2".to_string();

        let train = world.spawn().unwrap();
        world.entities.get_mut(train).unwrap().game.target = "corner1".to_string();

        path_corner_touch(&mut world, corner, train, None, None);

        // C path_corner_touch only steers an entity whose movetarget is
        // this corner, through movetarget/goalentity, never its `target`.
        assert_eq!(world.entities.get(train).unwrap().game.target, "corner1");
    }

    #[test]
    fn teleporter_telefrags_whatever_stands_on_the_destination() {
        let (mut world, gi) = crate::world::test_world_recording();
        let tele = world.spawn().unwrap();
        world.entities.get_mut(tele).unwrap().game.target = "dest1".to_string();
        let dest = world.spawn().unwrap();
        world.entities.get_mut(dest).unwrap().game.targetname = "dest1".to_string();
        let player = world.spawn_test_client(Default::default());
        let camper = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(camper).unwrap();
            e.game.health = 100;
            e.game.takedamage = TakeDamage::Yes;
            e.solid = Solid::Bbox;
        }
        // KillBox's trace at the destination hits the camper.
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.0,
            endpos: Vec3f::ZERO,
            allsolid: true,
            startsolid: true,
            plane: Plane::default(),
            surface: None,
            contents: 0,
            ent_index: world.number_of(camper),
        });
        teleporter_touch(&mut world, tele, player, None, None);
        assert!(world.entities.get(camper).unwrap().game.health <= 0);
    }

    #[test]
    fn exploding_barrel_reaches_dmg_plus_40() {
        // C barrel_explode: T_RadiusDamage(self, activator, dmg, NULL, dmg+40).
        let mut world = test_world();
        let barrel = world.spawn().unwrap();
        world.entities.get_mut(barrel).unwrap().game.dmg = 150;
        let victim = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(victim).unwrap();
            e.state.origin = Vec3f::new(170.0, 0.0, 0.0); // beyond dmg, within dmg + 40
            e.game.health = 200;
            e.game.takedamage = TakeDamage::Yes;
            e.solid = Solid::Bbox;
        }
        barrel_explode(&mut world, barrel);
        assert!(world.entities.get(victim).unwrap().game.health < 200);
    }

    fn note_stand(world: &mut GameWorld, key: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 1;
    }

    /// Corner `p1` (with `corner` keys) leading to `p2` at (0 100 0), and a
    /// standing-capable monster walking to `p1`.
    fn patrol(
        world: &mut GameWorld,
        corner: impl Fn(&mut crate::entity::Entity),
    ) -> (EntityKey, EntityKey, EntityKey) {
        let c1 = world.spawn().unwrap();
        let c2 = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(c1).unwrap();
            e.game.targetname = "p1".into();
            e.game.target = "p2".into();
            corner(e);
        }
        {
            let e = world.entities.get_mut(c2).unwrap();
            e.game.targetname = "p2".into();
            e.state.origin = Vec3f::new(0.0, 100.0, 0.0);
        }
        let monster = world.spawn().unwrap();
        {
            let m = world.entities.get_mut(monster).unwrap();
            m.game.movetarget = Some(c1);
            m.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                stand: Some(note_stand),
                ..Default::default()
            }));
        }
        (c1, c2, monster)
    }

    fn pausetime(world: &GameWorld, monster: EntityKey) -> f32 {
        world
            .entities
            .get(monster)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap()
            .pausetime
    }

    #[test]
    fn monster_pauses_and_stands_at_a_corner_with_a_wait() {
        let mut world = test_world();
        world.level.time = 3.0;
        let (c1, _, monster) = patrol(&mut world, |e| e.game.wait = 2.0);
        path_corner_touch(&mut world, c1, monster, None, None);
        assert_eq!(pausetime(&world, monster), 5.0);
        assert_eq!(world.entities.get(monster).unwrap().game.count, 1, "stand");
    }

    #[test]
    fn monster_stands_for_good_at_the_last_corner() {
        let mut world = test_world();
        let (_, c2, monster) = patrol(&mut world, |_| {});
        world.entities.get_mut(monster).unwrap().game.movetarget = Some(c2);
        path_corner_touch(&mut world, c2, monster, None, None);
        assert_eq!(pausetime(&world, monster), 100_000_000.0);
        assert_eq!(world.entities.get(monster).unwrap().game.count, 1, "stand");
        assert_eq!(world.entities.get(monster).unwrap().game.movetarget, None);
    }

    #[test]
    fn monster_at_a_corner_fires_its_pathtarget_and_keeps_the_corners_target() {
        let mut world = test_world();
        let (c1, _, monster) = patrol(&mut world, |e| e.game.pathtarget = "alarm".into());
        let alarm = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(alarm).unwrap();
            e.game.targetname = "alarm".into();
            e.use_fn = Some(|w, k, _, _| w.entities.get_mut(k).unwrap().game.count += 1);
        }
        path_corner_touch(&mut world, c1, monster, None, None);
        assert_eq!(world.entities.get(alarm).unwrap().game.count, 1);
        assert_eq!(world.entities.get(c1).unwrap().game.target, "p2");
    }

    #[test]
    fn monster_reaching_a_corner_before_a_teleport_corner_jumps_there() {
        // C path_corner_touch: a TELEPORT (1) next corner moves the monster
        // onto it, mins on its mins, and heads for the corner after it.
        let mut world = test_world();
        let (c1, c2, monster) = patrol(&mut world, |_| {});
        {
            let e = world.entities.get_mut(c2).unwrap();
            e.game.spawnflags = 1;
            e.game.target = "p3".into();
            e.mins = Vec3f::splat(-8.0);
        }
        let c3 = world.spawn().unwrap();
        world.entities.get_mut(c3).unwrap().game.targetname = "p3".into();
        world.entities.get_mut(monster).unwrap().mins = Vec3f::new(-16.0, -16.0, -24.0);
        path_corner_touch(&mut world, c1, monster, None, None);
        let m = world.entities.get(monster).unwrap();
        assert_eq!(m.state.origin, Vec3f::new(0.0, 100.0, 16.0));
        assert_eq!(m.state.event, EV_OTHER_TELEPORT);
        assert_eq!(m.game.movetarget, Some(c3));
    }
}

/// A `GameImport` that records the network writes, multicasts and sounds
/// `MockGameImport` drops, and whose `model_index` agrees with `set_model`
/// as the real server's does.
#[cfg(test)]
pub(crate) mod wire {
    use std::cell::RefCell;
    use std::rc::Rc;

    use q2_shared::types::*;

    use crate::traits::{EntityLink, GameImport, ModelInfo};
    use crate::world::{GameWorld, MockGameImport};

    /// One recorded `write_*` or `multicast` call.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Wire {
        Byte(i32),
        Short(i32),
        Position(Vec3f),
        Multicast(Vec3f, Multicast),
    }

    /// (entity, channel, sound index, attenuation) of a `sound` call.
    pub type SoundCall = (Option<usize>, i32, i32, f32);

    #[derive(Default)]
    pub struct WireImport {
        pub inner: MockGameImport,
        pub wire: RefCell<Vec<Wire>>,
        pub sounds: RefCell<Vec<SoundCall>>,
    }

    /// A test world whose import records the wire.
    pub fn wire_world() -> (GameWorld, Rc<WireImport>) {
        let gi = Rc::new(WireImport::default());
        (GameWorld::new(gi.clone(), 1024), gi)
    }

    impl GameImport for WireImport {
        fn bprintf(&self, a: i32, b: &str) {
            self.inner.bprintf(a, b)
        }
        fn dprintf(&self, a: &str) {
            self.inner.dprintf(a)
        }
        fn cprintf(&self, a: Option<usize>, b: i32, c: &str) {
            self.inner.cprintf(a, b, c)
        }
        fn centerprintf(&self, a: Option<usize>, b: &str) {
            self.inner.centerprintf(a, b)
        }
        #[allow(clippy::too_many_arguments)]
        fn positioned_sound(
            &self,
            origin: Vec3f,
            ent: usize,
            channel: i32,
            index: i32,
            volume: f32,
            attenuation: f32,
            time_ofs: f32,
        ) {
            self.inner
                .positioned_sound(origin, ent, channel, index, volume, attenuation, time_ofs)
        }
        fn sound(&self, ent: Option<usize>, chan: i32, index: i32, _: f32, attn: f32, _: f32) {
            self.sounds.borrow_mut().push((ent, chan, index, attn));
        }
        fn model_index(&self, name: &str) -> i32 {
            self.inner.set_model(0, name).index
        }
        fn sound_index(&self, a: &str) -> i32 {
            self.inner.sound_index(a)
        }
        fn image_index(&self, a: &str) -> i32 {
            self.inner.image_index(a)
        }
        fn set_model(&self, a: usize, b: &str) -> ModelInfo {
            self.inner.set_model(a, b)
        }
        fn trace(&self, a: Vec3f, b: Vec3f, c: Vec3f, d: Vec3f, e: Option<usize>, f: i32) -> Trace {
            self.inner.trace(a, b, c, d, e, f)
        }
        fn point_contents(&self, a: Vec3f) -> i32 {
            self.inner.point_contents(a)
        }
        fn in_pvs(&self, a: Vec3f, b: Vec3f) -> bool {
            self.inner.in_pvs(a, b)
        }
        fn in_phs(&self, a: Vec3f, b: Vec3f) -> bool {
            self.inner.in_phs(a, b)
        }
        fn link_entity(&self, a: &EntityLink) {
            self.inner.link_entity(a)
        }
        fn unlink_entity(&self, a: usize) {
            self.inner.unlink_entity(a)
        }
        fn box_edicts(&self, a: Vec3f, b: Vec3f, c: usize, d: i32) -> Vec<usize> {
            self.inner.box_edicts(a, b, c, d)
        }
        fn configstring(&self, a: i32, b: &str) {
            self.inner.configstring(a, b)
        }
        fn write_byte(&self, c: i32) {
            self.wire.borrow_mut().push(Wire::Byte(c));
        }
        fn write_short(&self, c: i32) {
            self.wire.borrow_mut().push(Wire::Short(c));
        }
        fn write_long(&self, _: i32) {}
        fn write_float(&self, _: f32) {}
        fn write_string(&self, _: &str) {}
        fn write_position(&self, pos: Vec3f) {
            self.wire.borrow_mut().push(Wire::Position(pos));
        }
        fn write_dir(&self, _: Vec3f) {}
        fn write_angle(&self, _: f32) {}
        fn multicast(&self, origin: Vec3f, to: Multicast) {
            self.wire.borrow_mut().push(Wire::Multicast(origin, to));
        }
        fn unicast(&self, a: usize, b: bool) {
            self.inner.unicast(a, b)
        }
        fn argc(&self) -> i32 {
            self.inner.argc()
        }
        fn argv(&self, a: i32) -> String {
            self.inner.argv(a)
        }
        fn args(&self) -> String {
            self.inner.args()
        }
        fn add_command_string(&self, a: &str) {
            self.inner.add_command_string(a)
        }
    }
}

#[cfg(test)]
mod gib_tests {
    use super::wire::{wire_world, Wire};
    use super::*;
    use crate::entity::{DieFn, ThinkFn, TouchFn};
    use crate::traits::GameImport;
    use crate::world::test_world;
    use q2_shared::protocol::SvcOp;

    const MEAT: &str = "models/objects/gibs/sm_meat/tris.md2";
    const BONE: &str = "models/objects/gibs/bone/tris.md2";

    /// The `n` next `random()`/`crandom()` draws without consuming them.
    fn peek_draws(world: &mut GameWorld, draws: &[bool]) -> Vec<f32> {
        let saved = world.level.rand_state;
        let out = draws
            .iter()
            .map(|&centered| {
                if centered {
                    world.crandom()
                } else {
                    world.random()
                }
            })
            .collect();
        world.level.rand_state = saved;
        out
    }

    /// A monster-sized body at (100 0 0) moving at (10 0 0).
    fn body(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        let e = world.entities.get_mut(key).unwrap();
        e.state.origin = Vec3f::new(100.0, 0.0, 0.0);
        e.absmin = Vec3f::new(84.0, -16.0, -24.0);
        e.size = Vec3f::new(32.0, 32.0, 56.0);
        e.velocity = Vec3f::new(10.0, 0.0, 0.0);
        key
    }

    fn freed(world: &GameWorld, key: EntityKey) -> bool {
        !world.entities.get(key).is_some_and(|e| e.in_use)
    }

    fn run_think(world: &mut GameWorld, key: EntityKey) {
        let think = world.entities.get(key).unwrap().think.unwrap();
        think(world, key);
    }

    #[test]
    fn velocity_for_damage_under_50_scales_the_kick_by_0_7() {
        let mut world = test_world();
        world.level.rand_state = 7;
        let r = peek_draws(&mut world, &[true, true, false]);
        let v = velocity_for_damage(&mut world, 49);
        let raw = Vec3f::new(100.0 * r[0], 100.0 * r[1], 200.0 + 100.0 * r[2]);
        assert_eq!(v, raw * 0.7);
    }

    #[test]
    fn velocity_for_damage_of_50_or_more_scales_the_kick_by_1_2() {
        let mut world = test_world();
        world.level.rand_state = 7;
        let r = peek_draws(&mut world, &[true, true, false]);
        let v = velocity_for_damage(&mut world, 50);
        let raw = Vec3f::new(100.0 * r[0], 100.0 * r[1], 200.0 + 100.0 * r[2]);
        assert_eq!(v, raw * 1.2);
    }

    #[test]
    fn clip_gib_velocity_clamps_low_values_and_keeps_some_upwards() {
        let mut v = Vec3f::new(-400.0, -301.0, 100.0);
        clip_gib_velocity(&mut v);
        assert_eq!(v, Vec3f::new(-300.0, -300.0, 200.0));
    }

    #[test]
    fn clip_gib_velocity_clamps_high_values() {
        let mut v = Vec3f::new(400.0, 301.0, 600.0);
        clip_gib_velocity(&mut v);
        assert_eq!(v, Vec3f::new(300.0, 300.0, 500.0));
    }

    #[test]
    fn clip_gib_velocity_keeps_in_range_values() {
        let mut v = Vec3f::new(-300.0, 299.0, 350.0);
        clip_gib_velocity(&mut v);
        assert_eq!(v, Vec3f::new(-300.0, 299.0, 350.0));
    }

    #[test]
    fn throw_gib_organic_is_a_tossed_touchable_gib_at_half_kick() {
        let mut world = test_world();
        world.level.time = 5.0;
        let monster = body(&mut world);
        world.level.rand_state = 99;
        // origin x3, VelocityForDamage (c, c, r), avelocity x3, nextthink.
        let r = peek_draws(
            &mut world,
            &[
                true, true, true, true, true, false, false, false, false, false,
            ],
        );
        let before = world.entities.iter().count();
        throw_gib(&mut world, monster, MEAT, 100, GibType::Organic);
        assert_eq!(
            world.entities.iter().count(),
            before + 1,
            "one gib per call"
        );
        let gib = world
            .entities
            .iter()
            .map(|(k, _)| k)
            .find(|&k| k != monster && world.entities.get(k).unwrap().game.health == 250)
            .unwrap();
        let g = world.entities.get(gib).unwrap();
        let center = Vec3f::new(100.0, 0.0, 4.0);
        let half = Vec3f::new(16.0, 16.0, 28.0);
        assert_eq!(
            g.state.origin,
            center + Vec3f::new(r[0] * half.x, r[1] * half.y, r[2] * half.z)
        );
        assert_ne!(g.state.modelindex, 0);
        assert_eq!(g.solid, Solid::Bbox);
        assert_eq!(g.svflags, SvFlags::DEADMONSTER.bits());
        assert_ne!(g.state.effects & EF_GIB, 0);
        assert!(g.game.flags.contains(EntityFlags::NO_KNOCKBACK));
        assert_eq!(g.game.takedamage, TakeDamage::Yes);
        assert!(g
            .die
            .is_some_and(|f| std::ptr::fn_addr_eq(f, gib_die as DieFn)));
        assert_eq!(g.game.movetype, MoveType::Toss);
        assert!(g
            .touch
            .is_some_and(|f| std::ptr::fn_addr_eq(f, gib_touch as TouchFn)));
        let kick = Vec3f::new(100.0 * r[3], 100.0 * r[4], 200.0 + 100.0 * r[5]) * 1.2;
        let mut expected = Vec3f::new(10.0, 0.0, 0.0) + kick * 0.5;
        clip_gib_velocity(&mut expected);
        assert_eq!(g.velocity, expected);
        assert_eq!(g.avelocity, Vec3f::new(r[6], r[7], r[8]) * 600.0);
        assert_eq!(g.game.nextthink, 5.0 + 10.0 + r[9] * 10.0);
        assert!(g
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, free_think as ThinkFn)));
        assert!(g.linkcount != 0, "C gi.linkentity");
    }

    #[test]
    fn throw_gib_metallic_bounces_at_full_kick_without_touch() {
        let mut world = test_world();
        let monster = body(&mut world);
        world.level.rand_state = 3;
        let r = peek_draws(&mut world, &[true, true, true, true, true, false]);
        throw_gib(&mut world, monster, BONE, 10, GibType::Metallic);
        let g = world
            .entities
            .iter()
            .find(|(k, e)| *k != monster && e.game.health == 250)
            .unwrap()
            .1;
        assert_eq!(g.game.movetype, MoveType::Bounce);
        assert!(g.touch.is_none());
        let kick = Vec3f::new(100.0 * r[3], 100.0 * r[4], 200.0 + 100.0 * r[5]) * 0.7;
        let mut expected = Vec3f::new(10.0, 0.0, 0.0) + kick;
        clip_gib_velocity(&mut expected);
        assert_eq!(g.velocity, expected);
    }

    #[test]
    fn gib_frees_itself_10_to_20_seconds_after_landing_untouched() {
        let mut world = test_world();
        let monster = body(&mut world);
        throw_gib(&mut world, monster, BONE, 10, GibType::Metallic);
        let gib = world
            .entities
            .iter()
            .find(|(k, e)| *k != monster && e.game.health == 250)
            .unwrap()
            .0;
        let at = world.entities.get(gib).unwrap().game.nextthink;
        assert!((10.0..=20.0).contains(&at));
        world.level.time = at;
        run_think(&mut world, gib);
        assert!(freed(&world, gib));
    }

    #[test]
    fn gib_think_advances_a_frame_a_tick_and_frees_8_to_18_seconds_after_frame_10() {
        let mut world = test_world();
        let gib = world.spawn().unwrap();
        world.entities.get_mut(gib).unwrap().state.frame = 8;
        world.level.time = 3.0;
        gib_think(&mut world, gib);
        let g = world.entities.get(gib).unwrap();
        assert_eq!(g.state.frame, 9);
        assert_eq!(g.game.nextthink, 3.0 + FRAMETIME);
        assert!(g.think.is_none(), "think untouched until frame 10");

        world.level.time = 3.1;
        gib_think(&mut world, gib);
        let g = world.entities.get(gib).unwrap();
        assert_eq!(g.state.frame, 10);
        assert!(g
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, free_think as ThinkFn)));
        assert!((3.1 + 8.0..=3.1 + 18.0).contains(&g.game.nextthink));
    }

    #[test]
    fn gib_touch_in_the_air_keeps_touching() {
        let mut world = test_world();
        let gib = world.spawn().unwrap();
        world.entities.get_mut(gib).unwrap().touch = Some(gib_touch);
        let other = world.spawn().unwrap();
        gib_touch(&mut world, gib, other, Some(&Plane::default()), None);
        assert!(world.entities.get(gib).unwrap().touch.is_some());
    }

    #[test]
    fn gib_touch_on_ground_without_a_plane_only_stops_touching() {
        let (mut world, gi) = wire_world();
        let gib = world.spawn().unwrap();
        let ground = world.spawn().unwrap();
        {
            let g = world.entities.get_mut(gib).unwrap();
            g.touch = Some(gib_touch);
            g.game.ground_entity = Some(ground);
            g.state.angles = Vec3f::new(1.0, 2.0, 3.0);
        }
        gib_touch(&mut world, gib, ground, None, None);
        let g = world.entities.get(gib).unwrap();
        assert!(g.touch.is_none());
        assert_eq!(g.state.angles, Vec3f::new(1.0, 2.0, 3.0));
        assert!(gi.sounds.borrow().is_empty());
    }

    #[test]
    fn gib_touch_on_a_floor_lies_flat_and_plays_the_splat() {
        let (mut world, gi) = wire_world();
        let gib = world.spawn().unwrap();
        let ground = world.spawn().unwrap();
        world.set_model(gib, BONE);
        {
            let g = world.entities.get_mut(gib).unwrap();
            g.touch = Some(gib_touch);
            g.game.ground_entity = Some(ground);
        }
        let floor = Plane {
            normal: Vec3f::new(0.0, 0.0, 1.0),
            ..Plane::default()
        };
        gib_touch(&mut world, gib, ground, Some(&floor), None);
        let g = world.entities.get(gib).unwrap();
        // C: vectoangles(right of vectoangles(up)) = facing -y, level.
        assert_eq!(g.state.angles, Vec3f::new(0.0, 270.0, 0.0));
        let splat = gi.inner.sound_index("misc/fhit3.wav");
        assert_eq!(
            *gi.sounds.borrow(),
            vec![(world.number_of(gib), CHAN_VOICE, splat, ATTN_NORM)]
        );
        assert!(g.think.is_none(), "only meat animates");
        assert_eq!(g.state.frame, 0);
    }

    #[test]
    fn gib_touch_of_meat_starts_the_frame_animation() {
        let (mut world, _gi) = wire_world();
        world.level.time = 2.0;
        let gib = world.spawn().unwrap();
        let ground = world.spawn().unwrap();
        world.set_model(gib, MEAT);
        {
            let g = world.entities.get_mut(gib).unwrap();
            g.game.ground_entity = Some(ground);
        }
        gib_touch(&mut world, gib, ground, Some(&Plane::default()), None);
        let g = world.entities.get(gib).unwrap();
        assert_eq!(g.state.frame, 1);
        assert!(g
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, gib_think as ThinkFn)));
        assert_eq!(g.game.nextthink, 2.0 + FRAMETIME);
    }

    #[test]
    fn gib_die_frees_the_gib() {
        let mut world = test_world();
        let gib = world.spawn().unwrap();
        gib_die(&mut world, gib, gib, gib, 10, Vec3f::ZERO);
        assert!(freed(&world, gib));
    }

    #[test]
    fn throw_head_turns_the_body_into_a_tossed_head() {
        let mut world = test_world();
        world.level.time = 1.0;
        let monster = body(&mut world);
        {
            let e = world.entities.get_mut(monster).unwrap();
            e.state.skinnum = 1;
            e.state.frame = 40;
            e.mins = Vec3f::splat(-16.0);
            e.maxs = Vec3f::splat(16.0);
            e.state.modelindex2 = 3;
            e.state.effects = EF_FLIES;
            e.state.sound = 5;
            e.svflags = SvFlags::MONSTER.bits();
            e.game.targetname = "boss".into();
            e.clipmask = MASK_MONSTERSOLID;
        }
        world.level.rand_state = 11;
        let r = peek_draws(&mut world, &[true, true, false, true, false]);
        let before = world.entities.iter().count();
        throw_head(
            &mut world,
            monster,
            "models/objects/gibs/head2/tris.md2",
            100,
            GibType::Organic,
        );
        assert_eq!(
            world.entities.iter().count(),
            before,
            "the body becomes the head"
        );
        let h = world.entities.get(monster).unwrap();
        assert_eq!((h.state.skinnum, h.state.frame), (0, 0));
        assert_eq!((h.mins, h.maxs), (Vec3f::ZERO, Vec3f::ZERO));
        assert_eq!(h.state.modelindex2, 0);
        assert_ne!(h.state.modelindex, 0);
        assert_eq!(h.solid, Solid::Bbox);
        assert_eq!(h.state.effects, EF_GIB);
        assert_eq!(h.state.sound, 0);
        assert!(h.game.flags.contains(EntityFlags::NO_KNOCKBACK));
        assert_eq!(h.svflags & SvFlags::MONSTER.bits(), 0);
        assert_eq!(h.game.takedamage, TakeDamage::Yes);
        assert!(h.game.targetname.is_empty());
        assert!(h
            .die
            .is_some_and(|f| std::ptr::fn_addr_eq(f, gib_die as DieFn)));
        assert_eq!(h.clipmask, MASK_SHOT);
        assert_eq!(h.game.movetype, MoveType::Toss);
        assert!(h
            .touch
            .is_some_and(|f| std::ptr::fn_addr_eq(f, gib_touch as TouchFn)));
        let kick = Vec3f::new(100.0 * r[0], 100.0 * r[1], 200.0 + 100.0 * r[2]) * 1.2;
        let mut expected = Vec3f::new(10.0, 0.0, 0.0) + kick * 0.5;
        clip_gib_velocity(&mut expected);
        assert_eq!(h.velocity, expected);
        assert_eq!(h.avelocity, Vec3f::new(0.0, r[3] * 600.0, 0.0));
        assert!(h
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, free_think as ThinkFn)));
        assert_eq!(h.game.nextthink, 1.0 + 10.0 + r[4] * 10.0);
    }

    #[test]
    fn throw_head_metallic_bounces_without_touch() {
        let mut world = test_world();
        let monster = body(&mut world);
        throw_head(
            &mut world,
            monster,
            "models/objects/gibs/gear/tris.md2",
            10,
            GibType::Metallic,
        );
        let h = world.entities.get(monster).unwrap();
        assert_eq!(h.game.movetype, MoveType::Bounce);
        assert!(h.touch.is_none());
    }

    /// A rand state whose first draw has the low bit `bit`.
    fn seed_with_low_bit(world: &mut GameWorld, bit: u32) {
        for seed in 0.. {
            world.level.rand_state = seed;
            if world.random_u32() & 1 == bit {
                world.level.rand_state = seed;
                return;
            }
        }
    }

    #[test]
    fn throw_client_head_odd_draw_throws_head2_with_the_player_skin() {
        let (mut world, gi) = wire_world();
        let corpse = body(&mut world);
        seed_with_low_bit(&mut world, 1);
        throw_client_head(&mut world, corpse, 50);
        let h = world.entities.get(corpse).unwrap();
        assert_eq!(h.state.skinnum, 1);
        assert_eq!(
            h.state.modelindex,
            gi.model_index("models/objects/gibs/head2/tris.md2")
        );
    }

    #[test]
    fn throw_client_head_even_draw_throws_the_skull() {
        let (mut world, gi) = wire_world();
        let corpse = body(&mut world);
        world.entities.get_mut(corpse).unwrap().state.skinnum = 3;
        seed_with_low_bit(&mut world, 0);
        throw_client_head(&mut world, corpse, 50);
        let h = world.entities.get(corpse).unwrap();
        assert_eq!(h.state.skinnum, 0);
        assert_eq!(
            h.state.modelindex,
            gi.model_index("models/objects/gibs/skull/tris.md2")
        );
    }

    #[test]
    fn throw_client_head_raises_and_bounces_an_untouchable_head() {
        let mut world = test_world();
        let corpse = body(&mut world);
        {
            let e = world.entities.get_mut(corpse).unwrap();
            e.state.frame = 7;
            e.state.effects = EF_FLIES | EF_GIB | 0x10;
            e.state.sound = 4;
            e.game.takedamage = TakeDamage::Yes;
            e.think = Some(gib_think);
            e.game.nextthink = 9.0;
        }
        world.level.rand_state = 5;
        // randk, then VelocityForDamage.
        let saved = world.level.rand_state;
        world.random_u32();
        let r = peek_draws(&mut world, &[true, true, false]);
        world.level.rand_state = saved;
        throw_client_head(&mut world, corpse, 10);
        let h = world.entities.get(corpse).unwrap();
        assert_eq!(h.state.origin, Vec3f::new(100.0, 0.0, 32.0));
        assert_eq!(h.state.frame, 0);
        assert_eq!(h.mins, Vec3f::new(-16.0, -16.0, 0.0));
        assert_eq!(h.maxs, Vec3f::new(16.0, 16.0, 16.0));
        assert_eq!(h.game.takedamage, TakeDamage::No);
        assert_eq!(h.solid, Solid::Bbox);
        assert_eq!(h.state.effects, EF_GIB, "C assigns, dropping other effects");
        assert_eq!(h.state.sound, 0);
        assert!(h.game.flags.contains(EntityFlags::NO_KNOCKBACK));
        assert_eq!(h.clipmask, MASK_SHOT);
        assert_eq!(h.game.movetype, MoveType::Bounce);
        let kick = Vec3f::new(100.0 * r[0], 100.0 * r[1], 200.0 + 100.0 * r[2]) * 0.7;
        // C adds the kick unclipped.
        assert_eq!(h.velocity, Vec3f::new(10.0, 0.0, 0.0) + kick);
        assert!(h.think.is_none(), "a queued body stops thinking");
        assert_eq!(h.game.nextthink, 0.0);
    }

    #[test]
    fn throw_client_head_of_a_live_client_keeps_its_think() {
        let mut world = test_world();
        let player = world.spawn_test_client(Default::default());
        {
            let e = world.entities.get_mut(player).unwrap();
            e.think = Some(gib_think);
            e.game.nextthink = 9.0;
        }
        throw_client_head(&mut world, player, 10);
        let h = world.entities.get(player).unwrap();
        assert!(h.think.is_some());
        assert_eq!(h.game.nextthink, 9.0);
    }

    #[test]
    fn throw_client_head_of_a_live_client_holds_the_death_animation() {
        // C ThrowClientHead: anim_priority = ANIM_DEATH, anim_end = s.frame
        // (just reset to 0).
        let mut world = test_world();
        let player = world.spawn_test_client(Default::default());
        let slot = world.entities.get(player).unwrap().client.unwrap();
        world.clients[slot].anim_end = 7;
        throw_client_head(&mut world, player, 10);
        assert_eq!(world.clients[slot].anim_priority, ANIM_DEATH);
        assert_eq!(world.clients[slot].anim_end, 0);
    }

    fn gib_count(world: &GameWorld) -> usize {
        world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.state.effects & EF_GIB != 0)
            .count()
    }

    fn debris_count(world: &GameWorld) -> usize {
        world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.game.classname == "debris")
            .count()
    }

    #[test]
    fn throw_gib_stops_at_twenty_gibs_a_frame_like_c_max_gibs() {
        let mut world = test_world();
        let monster = body(&mut world);
        for _ in 0..25 {
            throw_gib(&mut world, monster, MEAT, 10, GibType::Organic);
        }
        assert_eq!(gib_count(&world), 20);
        assert_eq!(world.level.gibs_this_frame, 20);
    }

    #[test]
    fn throw_debris_stops_at_twenty_chunks_a_frame_like_c_max_debris() {
        let mut world = test_world();
        let barrel = body(&mut world);
        for _ in 0..25 {
            throw_debris(&mut world, barrel, BONE, 1.0, Vec3f::ZERO);
        }
        assert_eq!(debris_count(&world), 20);
        assert_eq!(world.level.debris_this_frame, 20);
    }

    #[test]
    fn gib_and_debris_caps_reset_each_frame_and_after_client_frames() {
        // C G_RunFrame and ClientEndServerFrames both zero the counters.
        let mut world = test_world();
        world.init_game();
        world.level.gibs_this_frame = 20;
        world.level.debris_this_frame = 20;
        world.client_end_server_frames();
        assert_eq!(
            (world.level.gibs_this_frame, world.level.debris_this_frame),
            (0, 0)
        );
        world.level.gibs_this_frame = 20;
        world.level.debris_this_frame = 20;
        let monster = body(&mut world);
        world.run_frame();
        throw_gib(&mut world, monster, MEAT, 10, GibType::Organic);
        throw_debris(&mut world, monster, BONE, 1.0, Vec3f::ZERO);
        assert_eq!(gib_count(&world), 1);
        assert_eq!(debris_count(&world), 1);
    }

    #[test]
    fn throw_debris_flings_a_chunk_from_the_given_origin() {
        let mut world = test_world();
        world.level.time = 4.0;
        let barrel = body(&mut world);
        world.level.rand_state = 21;
        let r = peek_draws(&mut world, &[true, true, true, false, false, false, false]);
        let origin = Vec3f::new(1.0, 2.0, 3.0);
        let before = world.entities.iter().count();
        throw_debris(
            &mut world,
            barrel,
            "models/objects/debris1/tris.md2",
            2.0,
            origin,
        );
        assert_eq!(world.entities.iter().count(), before + 1);
        let chunk = world.find_by_classname(None, "debris").unwrap();
        let c = world.entities.get(chunk).unwrap();
        assert_eq!(c.state.origin, origin);
        assert_ne!(c.state.modelindex, 0);
        let v = Vec3f::new(100.0 * r[0], 100.0 * r[1], 100.0 + 100.0 * r[2]);
        assert_eq!(c.velocity, Vec3f::new(10.0, 0.0, 0.0) + v * 2.0);
        assert_eq!(c.game.movetype, MoveType::Bounce);
        assert_eq!(c.solid, Solid::Not);
        assert_eq!(c.avelocity, Vec3f::new(r[3], r[4], r[5]) * 600.0);
        assert!(c
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, free_think as ThinkFn)));
        assert_eq!(c.game.nextthink, 4.0 + 5.0 + r[6] * 5.0);
        assert_eq!(c.state.frame, 0);
        assert!(c.game.flags.is_empty());
        assert_eq!(c.game.takedamage, TakeDamage::Yes);
        assert!(c
            .die
            .is_some_and(|f| std::ptr::fn_addr_eq(f, debris_die as DieFn)));
        assert_eq!(c.game.health, 250);
        assert!(c.linkcount != 0);
    }

    #[test]
    fn debris_die_frees_the_chunk() {
        let mut world = test_world();
        let chunk = world.spawn().unwrap();
        debris_die(&mut world, chunk, chunk, chunk, 1, Vec3f::ZERO);
        assert!(freed(&world, chunk));
    }

    #[test]
    fn become_explosion1_sends_te_explosion1_to_the_pvs_and_frees() {
        let (mut world, gi) = wire_world();
        let ent = world.spawn().unwrap();
        let at = Vec3f::new(5.0, 6.0, 7.0);
        world.entities.get_mut(ent).unwrap().state.origin = at;
        become_explosion1(&mut world, ent);
        assert_eq!(
            *gi.wire.borrow(),
            vec![
                Wire::Byte(SvcOp::TempEntity as i32),
                Wire::Byte(5),
                Wire::Position(at),
                Wire::Multicast(at, Multicast::PVS),
            ]
        );
        assert!(freed(&world, ent));
    }

    #[test]
    fn become_explosion2_sends_te_explosion2_to_the_pvs_and_frees() {
        let (mut world, gi) = wire_world();
        let ent = world.spawn().unwrap();
        become_explosion2(&mut world, ent);
        assert_eq!(gi.wire.borrow()[1], Wire::Byte(6));
        assert_eq!(
            gi.wire.borrow()[3],
            Wire::Multicast(Vec3f::ZERO, Multicast::PVS)
        );
        assert!(freed(&world, ent));
    }

    fn barrel(world: &mut GameWorld, grounded: bool) -> EntityKey {
        let b = world.spawn().unwrap();
        let ground = world.spawn().unwrap();
        let e = world.entities.get_mut(b).unwrap();
        e.game.dmg = 150;
        e.state.origin = Vec3f::new(0.0, 0.0, 0.0);
        e.absmin = Vec3f::new(-16.0, -16.0, 0.0);
        e.size = Vec3f::new(32.0, 32.0, 40.0);
        e.game.ground_entity = grounded.then_some(ground);
        b
    }

    #[test]
    fn barrel_explode_throws_fourteen_chunks_and_explodes_from_its_origin() {
        let (mut world, gi) = wire_world();
        let b = barrel(&mut world, false);
        barrel_explode(&mut world, b);
        let debris = world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.game.classname == "debris")
            .count();
        assert_eq!(debris, 14, "C: 2 big, 4 corners, 8 small");
        let wire = gi.wire.borrow();
        assert_eq!(wire[1], Wire::Byte(5), "airborne: TE_EXPLOSION1");
        assert_eq!(wire[2], Wire::Position(Vec3f::ZERO), "origin restored");
        assert!(freed(&world, b));
    }

    #[test]
    fn barrel_explode_on_the_ground_uses_explosion2() {
        let (mut world, gi) = wire_world();
        let b = barrel(&mut world, true);
        barrel_explode(&mut world, b);
        assert_eq!(gi.wire.borrow()[1], Wire::Byte(6));
    }

    #[test]
    fn barrel_corner_debris_starts_at_the_box_corners() {
        let mut world = test_world();
        let b = barrel(&mut world, true);
        barrel_explode(&mut world, b);
        let mut origins: Vec<Vec3f> = world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.game.classname == "debris")
            .map(|(_, e)| e.state.origin)
            .collect();
        origins.retain(|o| o.z == 0.0 && o.x.abs() == 16.0 && o.y.abs() == 16.0);
        assert_eq!(origins.len(), 4);
    }
}
