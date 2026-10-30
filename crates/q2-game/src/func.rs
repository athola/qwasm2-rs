//! Functional entities — doors, platforms, buttons, trains, rotating objects.
//!
//! Port of `g_func.c`: func_plat, func_door, func_door_rotating,
//! func_door_secret, func_water, func_button, func_train, func_rotating,
//! func_conveyor, func_timer, func_explosive and func_killbox. These
//! entities use `MoveInfo` for smooth acceleration/deceleration.
//!
//! Not ported yet: func_wall/func_object spawnflags. Sound registration
//! and door message touches are set by the `finish_*` functions, which
//! `SPAWNS` runs after the spawn functions.
//!
//! # Movement pattern
//! All func_* entities follow: `Move_Begin → Move_Final → Move_Done`
//! with `MoveInfo.endfunc` calling back when movement completes.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_func.c`

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{EndMoveFn, EntityKey, EntityStorage, MoveInfo};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Movement states for doors/platforms
// ---------------------------------------------------------------------------

/// Door/platform movement states. C ref: g_func.c STATE_* defines.
pub const STATE_TOP: i32 = 0;
pub const STATE_BOTTOM: i32 = 1;
pub const STATE_UP: i32 = 2;
pub const STATE_DOWN: i32 = 3;

// ---------------------------------------------------------------------------
// Movement callbacks
// ---------------------------------------------------------------------------

/// func_door spawnflags (C g_func.c).
pub const DOOR_START_OPEN: i32 = 1;
pub const DOOR_REVERSE: i32 = 2;
pub const DOOR_CRUSHER: i32 = 4;
pub const DOOR_NOMONSTER: i32 = 8;
/// ANIMATED: the texture cycles (func_door and func_door_rotating).
pub const DOOR_ANIMATED: i32 = 16;
/// func_door's ANIMATED_FAST; the same bit is func_door_rotating's X_AXIS.
pub const DOOR_ANIMATED_FAST: i32 = 64;
pub const DOOR_TOGGLE: i32 = 32;
pub const DOOR_X_AXIS: i32 = 64;
pub const DOOR_Y_AXIS: i32 = 128;
/// func_plat spawnflag.
pub const PLAT_LOW_TRIGGER: i32 = 1;

fn moveinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MoveInfo> {
    world
        .entities
        .get_mut(key)
        .and_then(|e| e.moveinfo.as_deref_mut())
}

/// A team master's start sound and middle loop as it sets off, or its end
/// sound and silence as it stops; team slaves move silently. The
/// `if (!(self->flags & FL_TEAMSLAVE))` block of C plat_go_up,
/// door_hit_top, train_next and the rest.
fn mover_sound(world: &mut GameWorld, key: EntityKey, starting: bool) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.flags.contains(EntityFlags::TEAMSLAVE) {
        return;
    }
    let Some(mi) = ent.moveinfo.as_deref() else {
        return;
    };
    let (sound, looped) = if starting {
        (mi.sound_start, mi.sound_middle)
    } else {
        (mi.sound_end, 0)
    };
    if sound != 0 {
        let number = usize::try_from(ent.state.number).ok();
        world.gi.sound(
            number,
            CHAN_NO_PHS_ADD + CHAN_VOICE,
            sound,
            1.0,
            ATTN_STATIC,
            0.0,
        );
    }
    ent.state.sound = looped;
}

/// Set a mover's sounds from their names ("" for none), indexed in C's
/// order: start, middle, end.
fn set_mover_sounds(world: &mut GameWorld, key: EntityKey, names: [&str; 3]) {
    let [start, middle, end] = names.map(|n| match n {
        "" => 0,
        n => world.gi.sound_index(n),
    });
    if let Some(mi) = moveinfo_mut(world, key) {
        (mi.sound_start, mi.sound_middle, mi.sound_end) = (start, middle, end);
    }
}

const DOOR_SOUNDS: [&str; 3] = [
    "doors/dr1_strt.wav",
    "doors/dr1_mid.wav",
    "doors/dr1_end.wav",
];

/// A mover's `sounds` key (C `ent->sounds`), 0 when absent.
fn sounds_key(fields: &HashMap<String, String>) -> i32 {
    fields.get("sounds").map_or(0, |v| crate::spawn::strtol(v))
}

/// Open or close every func_areaportal the door targets.
/// C ref: `door_use_areaportals`.
fn door_use_areaportals(world: &mut GameWorld, key: EntityKey, open: bool) {
    let Some(target) = world.entities.get(key).map(|e| e.game.target.clone()) else {
        return;
    };
    if target.is_empty() {
        return;
    }
    let mut search = None;
    while let Some(t) = world.find_by_targetname(search, &target) {
        search = Some(t);
        let portal = world
            .entities
            .get(t)
            .filter(|e| e.game.classname.eq_ignore_ascii_case("func_areaportal"));
        if let Some(style) = portal.map(|e| e.game.style) {
            world.gi.set_area_portal_state(style, open);
        }
    }
}

/// Start moving toward `dest`, calling `endfunc` on arrival.
/// C ref: `Move_Calc` (g_func.c).
pub fn move_calc(world: &mut GameWorld, key: EntityKey, dest: Vec3f, endfunc: EndMoveFn) {
    let time = world.level.time;
    let current = world.level.current_entity;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.velocity = Vec3f::ZERO;
    let leader = if ent.game.flags.contains(EntityFlags::TEAMSLAVE) {
        ent.game.teammaster
    } else {
        Some(key)
    };
    let mi = ent.moveinfo.get_or_insert_with(Box::default);
    // C VectorNormalize: scale by the reciprocal length.
    let delta = dest - ent.state.origin;
    let length = delta.length();
    mi.dir = if length != 0.0 {
        delta * (1.0 / length)
    } else {
        delta
    };
    mi.remaining_distance = length;
    mi.endfunc = Some(endfunc);

    if mi.speed == mi.accel && mi.speed == mi.decel {
        if leader.is_some() && current == leader {
            move_begin(world, key);
        } else {
            ent.game.nextthink = time_plus(time, FRAMETIME_C);
            ent.think = Some(move_begin);
        }
    } else {
        mi.current_speed = 0.0;
        ent.think = Some(think_accel_move);
        ent.game.nextthink = time_plus(time, FRAMETIME_C);
    }
}

/// C `FRAMETIME`, the double 0.1: the float expressions it appears in
/// are evaluated in double and rounded to float once.
const FRAMETIME_C: f64 = 0.1;

/// C `level.time + seconds`, where `seconds` is a double expression.
fn time_plus(time: f32, seconds: f64) -> f32 {
    (f64::from(time) + seconds) as f32
}

/// Constant-speed move: whole frames at `speed`, then `move_final` covers
/// the remainder. C ref: `Move_Begin`.
pub fn move_begin(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.moveinfo.as_deref_mut() else {
        return;
    };
    if f64::from(mi.speed) * FRAMETIME_C >= f64::from(mi.remaining_distance) {
        move_final(world, key);
        return;
    }
    ent.velocity = mi.dir * mi.speed;
    let frames = (f64::from(mi.remaining_distance / mi.speed) / FRAMETIME_C).floor() as f32;
    mi.remaining_distance =
        (f64::from(mi.remaining_distance) - f64::from(frames * mi.speed) * FRAMETIME_C) as f32;
    ent.game.nextthink = time_plus(time, f64::from(frames) * FRAMETIME_C);
    ent.think = Some(move_final);
}

/// Cover the remaining distance in exactly one frame. C ref: `Move_Final`.
pub fn move_final(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.moveinfo.as_deref() else {
        return;
    };
    if mi.remaining_distance == 0.0 {
        move_done(world, key);
        return;
    }
    ent.velocity = mi.dir * ((f64::from(mi.remaining_distance) / FRAMETIME_C) as f32);
    ent.think = Some(move_done);
    ent.game.nextthink = time_plus(time, FRAMETIME_C);
}

/// Movement complete — clear velocity and call endfunc. C ref: `Move_Done`.
pub fn move_done(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.velocity = Vec3f::ZERO;
    }
    let endfunc = world
        .entities
        .get(key)
        .and_then(|e| e.moveinfo.as_ref())
        .and_then(|mi| mi.endfunc);
    if let Some(func) = endfunc {
        func(world, key);
    }
}

/// Start turning toward `end_angles` (state UP) or `start_angles`,
/// calling `endfunc` on arrival. Angular moves never accelerate.
/// C ref: `AngleMove_Calc` (g_func.c).
pub fn angle_move_calc(world: &mut GameWorld, key: EntityKey, endfunc: EndMoveFn) {
    let time = world.level.time;
    let current = world.level.current_entity;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.avelocity = Vec3f::ZERO;
    let leader = if ent.game.flags.contains(EntityFlags::TEAMSLAVE) {
        ent.game.teammaster
    } else {
        Some(key)
    };
    ent.moveinfo.get_or_insert_with(Box::default).endfunc = Some(endfunc);
    if leader.is_some() && current == leader {
        angle_move_begin(world, key);
    } else {
        ent.game.nextthink = time_plus(time, FRAMETIME_C);
        ent.think = Some(angle_move_begin);
    }
}

/// The angles still to turn through: to `end_angles` while opening (UP),
/// otherwise back to `start_angles`.
fn angle_move_delta(ent: &crate::entity::Entity, mi: &MoveInfo) -> Vec3f {
    if mi.state == STATE_UP {
        mi.end_angles - ent.state.angles
    } else {
        mi.start_angles - ent.state.angles
    }
}

/// Turn at `speed` degrees a second for whole frames, then let
/// `angle_move_final` cover the rest. C ref: `AngleMove_Begin`.
pub fn angle_move_begin(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.moveinfo.as_deref() else {
        return;
    };
    let destdelta = angle_move_delta(ent, mi);
    let traveltime = destdelta.length() / mi.speed;
    if f64::from(traveltime) < FRAMETIME_C {
        angle_move_final(world, key);
        return;
    }
    let frames = (f64::from(traveltime) / FRAMETIME_C).floor() as f32;
    ent.avelocity = destdelta * ((1.0 / f64::from(traveltime)) as f32);
    ent.game.nextthink = time_plus(time, f64::from(frames) * FRAMETIME_C);
    ent.think = Some(angle_move_final);
}

/// Turn the remaining angles in exactly one frame. C ref: `AngleMove_Final`.
pub fn angle_move_final(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.moveinfo.as_deref() else {
        return;
    };
    let delta = angle_move_delta(ent, mi);
    if delta == Vec3f::ZERO {
        angle_move_done(world, key);
        return;
    }
    ent.avelocity = delta * ((1.0 / FRAMETIME_C) as f32);
    ent.think = Some(angle_move_done);
    ent.game.nextthink = time_plus(time, FRAMETIME_C);
}

/// Turn complete: stop spinning and call endfunc. C ref: `AngleMove_Done`.
pub fn angle_move_done(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.avelocity = Vec3f::ZERO;
    if let Some(func) = ent.moveinfo.as_ref().and_then(|mi| mi.endfunc) {
        func(world, key);
    }
}

/// C `AccelerationDistance`.
fn acceleration_distance(target: f32, rate: f32) -> f32 {
    target * ((target / rate) + 1.0) / 2.0
}

/// C ref: `plat_CalcAcceleratedMove`. Speeds here are per frame.
pub fn plat_calc_accelerated_move(mi: &mut MoveInfo) {
    mi.move_speed = mi.speed;
    if mi.remaining_distance < mi.accel {
        mi.current_speed = mi.remaining_distance;
        return;
    }
    let accel_dist = acceleration_distance(mi.speed, mi.accel);
    let mut decel_dist = acceleration_distance(mi.speed, mi.decel);
    if mi.remaining_distance - accel_dist - decel_dist < 0.0 {
        let f = (mi.accel + mi.decel) / (mi.accel * mi.decel);
        // C passes the float expression to double sqrt.
        let root = f64::from(4.0 - 4.0 * f * (-2.0 * mi.remaining_distance)).sqrt();
        mi.move_speed = ((-2.0 + root) / f64::from(2.0 * f)) as f32;
        decel_dist = acceleration_distance(mi.move_speed, mi.decel);
    }
    mi.decel_distance = decel_dist;
}

/// C `speed * (1.0 - ratio)`: the double 1.0 makes it a double
/// expression, rounded to float once.
fn one_minus(speed: f32, ratio: f32) -> f32 {
    (f64::from(speed) * (1.0 - f64::from(ratio))) as f32
}

/// C ref: `plat_Accelerate`.
pub fn plat_accelerate(mi: &mut MoveInfo) {
    // Decelerating?
    if mi.remaining_distance <= mi.decel_distance {
        if mi.remaining_distance < mi.decel_distance {
            if mi.next_speed != 0.0 {
                mi.current_speed = mi.next_speed;
                mi.next_speed = 0.0;
                return;
            }
            if mi.current_speed > mi.decel {
                mi.current_speed -= mi.decel;
            }
        }
        return;
    }

    // At full speed and starting to decelerate during this move?
    if mi.current_speed == mi.move_speed
        && mi.remaining_distance - mi.current_speed < mi.decel_distance
    {
        let p1_distance = mi.remaining_distance - mi.decel_distance;
        let p2_distance = one_minus(mi.move_speed, p1_distance / mi.move_speed);
        let distance = p1_distance + p2_distance;
        mi.current_speed = mi.move_speed;
        mi.next_speed = mi.move_speed - mi.decel * (p2_distance / distance);
        return;
    }

    // Accelerating?
    if mi.current_speed < mi.speed {
        let old_speed = mi.current_speed;
        mi.current_speed = (mi.current_speed + mi.accel).min(mi.speed);
        if mi.remaining_distance - mi.current_speed >= mi.decel_distance {
            return;
        }
        // Accelerate to move_speed and cross the decel distance in one move:
        // use the average speed over the whole move.
        let p1_distance = mi.remaining_distance - mi.decel_distance;
        let p1_speed = (old_speed + mi.move_speed) / 2.0;
        let p2_distance = one_minus(mi.move_speed, p1_distance / p1_speed);
        let distance = p1_distance + p2_distance;
        mi.current_speed =
            (p1_speed * (p1_distance / distance)) + (mi.move_speed * (p2_distance / distance));
        mi.next_speed = mi.move_speed - mi.decel * (p2_distance / distance);
    }
}

/// C ref: `Think_AccelMove`.
pub fn think_accel_move(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.moveinfo.as_deref_mut() else {
        return;
    };
    mi.remaining_distance -= mi.current_speed;
    if mi.current_speed == 0.0 {
        plat_calc_accelerated_move(mi); // starting or blocked
    }
    plat_accelerate(mi);

    // Will the whole move complete next frame?
    if mi.remaining_distance <= mi.current_speed {
        move_final(world, key);
        return;
    }
    ent.velocity = mi.dir * (mi.current_speed * 10.0);
    ent.game.nextthink = time_plus(time, FRAMETIME_C);
    ent.think = Some(think_accel_move);
}

// ---------------------------------------------------------------------------
// Platform (func_plat) callbacks
// ---------------------------------------------------------------------------

/// C ref: `plat_hit_top`.
pub fn plat_hit_top(world: &mut GameWorld, key: EntityKey) {
    mover_sound(world, key, false);
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(mi) = ent.moveinfo.as_deref_mut() {
            mi.state = STATE_TOP;
        }
        ent.think = Some(plat_go_down);
        ent.game.nextthink = time + 3.0;
    }
}

/// C ref: `plat_hit_bottom`.
pub fn plat_hit_bottom(world: &mut GameWorld, key: EntityKey) {
    mover_sound(world, key, false);
    if let Some(mi) = moveinfo_mut(world, key) {
        mi.state = STATE_BOTTOM;
    }
}

/// C ref: `plat_go_down`.
pub fn plat_go_down(world: &mut GameWorld, key: EntityKey) {
    mover_sound(world, key, true);
    let Some(mi) = moveinfo_mut(world, key) else {
        return;
    };
    mi.state = STATE_DOWN;
    let end = mi.end_origin;
    move_calc(world, key, end, plat_hit_bottom);
}

/// C ref: `plat_go_up`.
pub fn plat_go_up(world: &mut GameWorld, key: EntityKey) {
    mover_sound(world, key, true);
    let Some(mi) = moveinfo_mut(world, key) else {
        return;
    };
    mi.state = STATE_UP;
    let start = mi.start_origin;
    move_calc(world, key, start, plat_hit_top);
}

/// C ref: `Use_Plat`: a targeted plat waits at the top until used once.
pub fn use_plat(world: &mut GameWorld, key: EntityKey, _other: EntityKey, _activator: EntityKey) {
    if world.entities.get(key).is_some_and(|e| e.think.is_some()) {
        return; // already down
    }
    plat_go_down(world, key);
}

/// C ref: `Touch_Plat_Center` (single player: no coop elevator delay).
pub fn touch_plat_center(
    world: &mut GameWorld,
    trigger: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    let Some(o) = world.entities.get(other) else {
        return;
    };
    if o.client.is_none() || o.game.health <= 0 {
        return;
    }
    let Some(plat) = world.entities.get(trigger).and_then(|t| t.game.enemy) else {
        return;
    };
    let state = world
        .entities
        .get(plat)
        .and_then(|e| e.moveinfo.as_ref())
        .map(|mi| mi.state);
    if state == Some(STATE_BOTTOM) {
        plat_go_up(world, plat);
    } else if state == Some(STATE_TOP) {
        // The player is still on the plat: delay going down.
        let time = world.level.time;
        if let Some(e) = world.entities.get_mut(plat) {
            e.game.nextthink = time + 1.0;
        }
    }
}

/// Crush something that is neither a monster nor a client (C nukes it with
/// an explosion if 100000 damage did not remove it). Returns `true` if
/// `other` was such an entity. Shared head of C plat_blocked/door_blocked.
fn crush_non_living(world: &mut GameWorld, self_key: EntityKey, other: EntityKey) -> bool {
    let Some(o) = world.entities.get(other) else {
        return true;
    };
    if o.svflags & q2_shared::constants::SVF_MONSTER != 0 || o.client.is_some() {
        return false;
    }
    let origin = o.state.origin;
    // Give it a chance to go away on its own terms (like gibs).
    world.t_damage(
        other,
        self_key,
        self_key,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        100_000,
        1,
        DamageFlags::empty(),
        MeansOfDeath::Crush,
    );
    // If it's still there, nuke it (C BecomeExplosion1; the explosion
    // effect is not ported).
    if world.entities.is_live(other) {
        world.free_entity(other);
    }
    true
}

/// Crush what blocks the plat, then reverse. C ref: `plat_blocked`.
pub fn plat_blocked(world: &mut GameWorld, self_key: EntityKey, other: EntityKey) {
    if crush_non_living(world, self_key, other) {
        return;
    }
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let dmg = ent.game.dmg;
    let origin = world
        .entities
        .get(other)
        .map_or(Vec3f::ZERO, |o| o.state.origin);
    world.t_damage(
        other,
        self_key,
        self_key,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        dmg,
        1,
        DamageFlags::empty(),
        MeansOfDeath::Crush,
    );
    // After the damage, as C: a kill can fire a deathtarget that moves us.
    let state = world
        .entities
        .get(self_key)
        .and_then(|e| e.moveinfo.as_ref())
        .map(|mi| mi.state);
    match state {
        Some(STATE_UP) => plat_go_down(world, self_key),
        Some(STATE_DOWN) => plat_go_up(world, self_key),
        _ => {}
    }
}

/// Spawn the plat's "start moving" trigger. C ref:
/// `plat_spawn_inside_trigger`; `lip` is the spawn-time value.
fn plat_spawn_inside_trigger(storage: &mut EntityStorage, plat: EntityKey, lip: f32) {
    let Some(p) = storage.get(plat) else {
        return;
    };
    let (mins, maxs, spawnflags) = (p.mins, p.maxs, p.game.spawnflags);
    let (pos1, pos2) = p
        .moveinfo
        .as_ref()
        .map(|mi| (mi.start_origin, mi.end_origin))
        .unwrap_or_default();
    let mut tmin = Vec3f::new(mins.x + 25.0, mins.y + 25.0, 0.0);
    let mut tmax = Vec3f::new(maxs.x - 25.0, maxs.y - 25.0, maxs.z + 8.0);
    tmin.z = tmax.z - (pos1.z - pos2.z + lip);
    if spawnflags & PLAT_LOW_TRIGGER != 0 {
        tmax.z = tmin.z + 8.0;
    }
    if tmax.x - tmin.x <= 0.0 {
        tmin.x = (mins.x + maxs.x) * 0.5;
        tmax.x = tmin.x + 1.0;
    }
    if tmax.y - tmin.y <= 0.0 {
        tmin.y = (mins.y + maxs.y) * 0.5;
        tmax.y = tmin.y + 1.0;
    }
    let Some(trigger) = storage.spawn() else {
        return;
    };
    if let Some(t) = storage.get_mut(trigger) {
        t.touch = Some(touch_plat_center);
        t.game.movetype = MoveType::None;
        t.solid = Solid::Trigger;
        t.game.enemy = Some(plat);
        t.mins = tmin;
        t.maxs = tmax;
    }
}

// ---------------------------------------------------------------------------
// Door callbacks
// ---------------------------------------------------------------------------

/// C ref: `door_use`.
pub fn door_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    if ent.game.flags.contains(EntityFlags::TEAMSLAVE) {
        return;
    }
    let toggle = ent.game.spawnflags & DOOR_TOGGLE != 0;
    let state = ent.moveinfo.as_ref().map(|mi| mi.state);
    let members = world.team_chain(self_key);
    let closing = toggle && matches!(state, Some(STATE_UP) | Some(STATE_TOP));
    for member in members {
        if let Some(e) = world.entities.get_mut(member) {
            e.game.message.clear();
            e.touch = None;
        }
        if closing {
            door_go_down(world, member);
        } else {
            door_go_up(world, member, activator);
        }
    }
}

/// Crush what blocks the door, then (unless a crusher or `wait -1`) send
/// the whole team back the way it came. C ref: `door_blocked`.
pub fn door_blocked(world: &mut GameWorld, self_key: EntityKey, other: EntityKey) {
    if crush_non_living(world, self_key, other) {
        return;
    }
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let (dmg, crusher, master) = (
        ent.game.dmg,
        ent.game.spawnflags & DOOR_CRUSHER != 0,
        ent.game.teammaster.unwrap_or(self_key),
    );
    let origin = world
        .entities
        .get(other)
        .map_or(Vec3f::ZERO, |o| o.state.origin);
    world.t_damage(
        other,
        self_key,
        self_key,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        dmg,
        1,
        DamageFlags::empty(),
        MeansOfDeath::Crush,
    );
    // A door with a negative wait would never come back if blocked, so
    // let it squash the object to death real fast. Read after the damage,
    // as C does: a kill can fire a deathtarget that moves this door.
    let (wait, state) = world
        .entities
        .get(self_key)
        .and_then(|e| e.moveinfo.as_ref())
        .map_or((0.0, STATE_BOTTOM), |mi| (mi.wait, mi.state));
    if crusher || wait < 0.0 {
        return;
    }
    for member in world.team_chain(master) {
        if state == STATE_DOWN {
            let activator = world
                .entities
                .get(member)
                .and_then(|e| e.game.activator)
                .unwrap_or(member);
            door_go_up(world, member, activator);
        } else {
            door_go_down(world, member);
        }
    }
}

/// Start a door's move or turn to its open position, by classname as C
/// does: func_door (also func_water) slides, func_door_rotating turns.
/// C ref: `door_go_up`.
pub fn door_go_up(world: &mut GameWorld, key: EntityKey, activator: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.moveinfo.as_deref_mut() else {
        return;
    };
    match mi.state {
        STATE_UP => return, // already going up
        STATE_TOP => {
            // Reset the top wait time.
            if mi.wait >= 0.0 {
                ent.game.nextthink = time + mi.wait;
            }
            return;
        }
        _ => {}
    }
    mi.state = STATE_UP;
    let end = mi.end_origin;
    mover_sound(world, key, true);
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    match ent.game.classname.as_str() {
        "func_door" => move_calc(world, key, end, door_hit_top),
        "func_door_rotating" => angle_move_calc(world, key, door_hit_top),
        _ => {}
    }
    world.use_targets(key, activator);
    door_use_areaportals(world, key, true);
}

/// C ref: `door_go_down`.
pub fn door_go_down(world: &mut GameWorld, key: EntityKey) {
    mover_sound(world, key, true);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.max_health != 0 {
        ent.game.takedamage = TakeDamage::Yes;
        ent.game.health = ent.game.max_health;
    }
    let Some(mi) = ent.moveinfo.as_deref_mut() else {
        return;
    };
    mi.state = STATE_DOWN;
    let start = mi.start_origin;
    match ent.game.classname.as_str() {
        "func_door" => move_calc(world, key, start, door_hit_bottom),
        "func_door_rotating" => angle_move_calc(world, key, door_hit_bottom),
        _ => {}
    }
}

/// C ref: `door_hit_top`.
pub fn door_hit_top(world: &mut GameWorld, key: EntityKey) {
    mover_sound(world, key, false);
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let toggle = ent.game.spawnflags & DOOR_TOGGLE != 0;
    let Some(mi) = ent.moveinfo.as_deref_mut() else {
        return;
    };
    mi.state = STATE_TOP;
    if toggle {
        return;
    }
    if mi.wait >= 0.0 {
        ent.think = Some(door_go_down);
        ent.game.nextthink = time + mi.wait;
    }
}

/// C ref: `door_hit_bottom`.
pub fn door_hit_bottom(world: &mut GameWorld, key: EntityKey) {
    mover_sound(world, key, false);
    if let Some(mi) = moveinfo_mut(world, key) {
        mi.state = STATE_BOTTOM;
    }
    door_use_areaportals(world, key, false);
}

/// C ref: `door_killed`: a shot door opens with its whole team. Doors
/// always have a team master; a func_door_secret with health has none,
/// so in C (and here) shooting one does nothing.
pub fn door_killed(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let Some(master) = world.entities.get(self_key).and_then(|e| e.game.teammaster) else {
        return;
    };
    for member in world.team_chain(master) {
        if let Some(e) = world.entities.get_mut(member) {
            e.game.health = e.game.max_health;
            e.game.takedamage = TakeDamage::No;
        }
    }
    door_use(world, master, attacker, attacker);
}

/// C ref: `Touch_DoorTrigger`.
pub fn touch_door_trigger(
    world: &mut GameWorld,
    trigger: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    let Some(o) = world.entities.get(other) else {
        return;
    };
    if o.game.health <= 0 {
        return;
    }
    let monster = o.svflags & q2_shared::constants::SVF_MONSTER != 0;
    if !monster && o.client.is_none() {
        return;
    }
    let Some(t) = world.entities.get(trigger) else {
        return;
    };
    let Some(door) = t.owner else {
        return;
    };
    let door_flags = world.entities.get(door).map_or(0, |d| d.game.spawnflags);
    if door_flags & DOOR_NOMONSTER != 0 && monster {
        return;
    }
    let time = world.level.time;
    if time < t.game.touch_debounce_time {
        return;
    }
    if let Some(t) = world.entities.get_mut(trigger) {
        t.game.touch_debounce_time = time + 1.0;
    }
    door_use(world, door, other, other);
}

/// A targeted door's touch: centerprint its `message` to the client at
/// most every 5 seconds, until `door_use` clears it. C ref: `door_touch`.
pub fn door_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    if world.entities.get(other).is_none_or(|o| o.client.is_none()) {
        return;
    }
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    if time < ent.game.touch_debounce_time {
        return;
    }
    ent.game.touch_debounce_time = time + 5.0;
    let message = ent.game.message.clone();
    let number = world.number_of(other);
    world.gi.centerprintf(number, &message);
    let talk = world.gi.sound_index("misc/talk1.wav");
    world.gi.sound(number, CHAN_AUTO, talk, 1.0, ATTN_NORM, 0.0);
}

/// Make a team finish its moves together. C ref: `Think_CalcMoveSpeed`.
pub fn think_calc_move_speed(world: &mut GameWorld, key: EntityKey) {
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.game.flags.contains(EntityFlags::TEAMSLAVE))
    {
        return; // only the team master does this
    }
    let members = world.team_chain(key);
    let distance = |w: &GameWorld, k: EntityKey| {
        w.entities
            .get(k)
            .and_then(|e| e.moveinfo.as_ref())
            .map_or(0.0, |mi| mi.distance.abs())
    };
    let min = members
        .iter()
        .map(|&k| distance(world, k))
        .fold(f32::INFINITY, f32::min);
    let Some(speed) = world
        .entities
        .get(key)
        .and_then(|e| e.moveinfo.as_ref())
        .map(|mi| mi.speed)
    else {
        return;
    };
    let time = min / speed;
    for member in members {
        let Some(mi) = moveinfo_mut(world, member) else {
            continue;
        };
        let newspeed = mi.distance.abs() / time;
        let ratio = newspeed / mi.speed;
        if mi.accel == mi.speed {
            mi.accel = newspeed;
        } else {
            mi.accel *= ratio;
        }
        if mi.decel == mi.speed {
            mi.decel = newspeed;
        } else {
            mi.decel *= ratio;
        }
        mi.speed = newspeed;
    }
}

/// Spawn the touch field around the whole team, 60 units wider in x/y.
/// C ref: `Think_SpawnDoorTrigger`.
pub fn think_spawn_door_trigger(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.flags.contains(EntityFlags::TEAMSLAVE) {
        return; // only the team leader spawns a trigger
    }
    let (mut mins, mut maxs) = (ent.absmin, ent.absmax);
    for member in world.team_chain(key).into_iter().skip(1) {
        if let Some(m) = world.entities.get(member) {
            mins = mins.min(m.absmin).min(m.absmax);
            maxs = maxs.max(m.absmin).max(m.absmax);
        }
    }
    mins.x -= 60.0;
    mins.y -= 60.0;
    maxs.x += 60.0;
    maxs.y += 60.0;

    let Some(trigger) = world.spawn() else {
        return;
    };
    if let Some(t) = world.entities.get_mut(trigger) {
        t.mins = mins;
        t.maxs = maxs;
        t.owner = Some(key);
        t.solid = Solid::Trigger;
        t.game.movetype = MoveType::None;
        t.touch = Some(touch_door_trigger);
    }
    world.link_entity(trigger);
    if world
        .entities
        .get(key)
        .is_some_and(|e| e.game.spawnflags & DOOR_START_OPEN != 0)
    {
        door_use_areaportals(world, key, true);
    }
    think_calc_move_speed(world, key);
}

// ---------------------------------------------------------------------------
// Button callbacks
// ---------------------------------------------------------------------------

use q2_shared::constants::{EF_ANIM01, EF_ANIM23};

/// C ref: `button_done`.
pub fn button_done(world: &mut GameWorld, key: EntityKey) {
    if let Some(mi) = moveinfo_mut(world, key) {
        mi.state = STATE_BOTTOM;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.effects &= !EF_ANIM23;
        ent.state.effects |= EF_ANIM01;
    }
}

/// C ref: `button_return`.
pub fn button_return(world: &mut GameWorld, key: EntityKey) {
    let Some(mi) = moveinfo_mut(world, key) else {
        return;
    };
    mi.state = STATE_DOWN;
    let start = mi.start_origin;
    move_calc(world, key, start, button_done);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame = 0;
        if ent.game.health != 0 {
            ent.game.takedamage = TakeDamage::Yes;
        }
    }
}

/// Pressed in: fire targets, then return after `wait`. C ref: `button_wait`.
pub fn button_wait(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let activator = world
        .entities
        .get(key)
        .and_then(|e| e.game.activator)
        .unwrap_or(key);
    if let Some(mi) = moveinfo_mut(world, key) {
        mi.state = STATE_TOP;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.effects &= !EF_ANIM01;
        ent.state.effects |= EF_ANIM23;
    }
    world.use_targets(key, activator);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame = 1;
        let wait = ent.moveinfo.as_ref().map_or(-1.0, |mi| mi.wait);
        if wait >= 0.0 {
            ent.game.nextthink = time + wait;
            ent.think = Some(button_return);
        }
    }
}

/// C ref: `button_fire`.
pub fn button_fire(world: &mut GameWorld, key: EntityKey) {
    let Some(mi) = moveinfo_mut(world, key) else {
        return;
    };
    if matches!(mi.state, STATE_UP | STATE_TOP) {
        return;
    }
    mi.state = STATE_UP;
    let end = mi.end_origin;
    let ent = world.entities.get(key);
    let sound = ent
        .filter(|e| !e.game.flags.contains(EntityFlags::TEAMSLAVE))
        .and_then(|e| e.moveinfo.as_ref())
        .map_or(0, |mi| mi.sound_start);
    if sound != 0 {
        let number = world.number_of(key);
        world.gi.sound(
            number,
            CHAN_NO_PHS_ADD + CHAN_VOICE,
            sound,
            1.0,
            ATTN_STATIC,
            0.0,
        );
    }
    move_calc(world, key, end, button_wait);
}

/// C ref: `button_use`.
pub fn button_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.activator = Some(activator);
    }
    button_fire(world, self_key);
}

/// C ref: `button_touch`.
pub fn button_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    let Some(o) = world.entities.get(other) else {
        return;
    };
    if o.client.is_none() || o.game.health <= 0 {
        return;
    }
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.activator = Some(other);
    }
    button_fire(world, self_key);
}

/// C ref: `button_killed`.
pub fn button_killed(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.activator = Some(attacker);
        ent.game.health = ent.game.max_health;
        ent.game.takedamage = TakeDamage::No;
    }
    button_fire(world, self_key);
}

// ---------------------------------------------------------------------------
// func_rotating callbacks
// ---------------------------------------------------------------------------

/// func_rotating spawnflags (C g_func.c).
const ROTATING_START_ON: i32 = 1;
const ROTATING_REVERSE: i32 = 2;
const ROTATING_X_AXIS: i32 = 4;
const ROTATING_Y_AXIS: i32 = 8;
const ROTATING_TOUCH_PAIN: i32 = 16;
const ROTATING_STOP: i32 = 32;
const ANIMATED: i32 = 64;
const ANIMATED_FAST: i32 = 128;

use q2_shared::constants::{EF_ANIM_ALL, EF_ANIM_ALLFAST};

/// Toggle spinning. C ref: `rotating_use` (entity part).
fn rotating_toggle(ent: &mut crate::entity::Entity) {
    if ent.avelocity != Vec3f::ZERO {
        ent.state.sound = 0;
        ent.avelocity = Vec3f::ZERO;
        ent.touch = None;
    } else {
        ent.state.sound = ent.moveinfo.as_ref().map_or(0, |mi| mi.sound_middle);
        ent.avelocity = ent.game.movedir * ent.game.speed;
        if ent.game.spawnflags & ROTATING_TOUCH_PAIN != 0 {
            ent.touch = Some(rotating_touch);
        }
    }
}

/// C ref: `rotating_use`.
pub fn rotating_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    if let Some(ent) = world.entities.get_mut(key) {
        rotating_toggle(ent);
    }
}

/// Damage whatever blocks the spin. C ref: `rotating_blocked`.
pub fn rotating_blocked(world: &mut GameWorld, self_key: EntityKey, other: EntityKey) {
    let dmg = world.entities.get(self_key).map_or(0, |e| e.game.dmg);
    let origin = world
        .entities
        .get(other)
        .map_or(Vec3f::ZERO, |o| o.state.origin);
    world.t_damage(
        other,
        self_key,
        self_key,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        dmg,
        1,
        DamageFlags::empty(),
        MeansOfDeath::Crush,
    );
}

/// Hurt what touches a spinning TOUCH_PAIN rotator. C ref: `rotating_touch`.
pub fn rotating_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    if world
        .entities
        .get(self_key)
        .is_some_and(|e| e.avelocity != Vec3f::ZERO)
    {
        rotating_blocked(world, self_key, other);
    }
}

// ---------------------------------------------------------------------------
// Timer callbacks
// ---------------------------------------------------------------------------

/// func_timer spawnflag.
const TIMER_START_ON: i32 = 1;

/// Fire, then schedule the next firing `wait ± random` later.
/// C ref: `func_timer_think`.
pub fn func_timer_think(world: &mut GameWorld, key: EntityKey) {
    let activator = world
        .entities
        .get(key)
        .and_then(|e| e.game.activator)
        .unwrap_or(key);
    world.use_targets(key, activator);

    let jitter = world.crandom();
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + ent.game.wait + jitter * ent.game.random;
    }
}

/// Toggle the timer: off if running, otherwise start after `delay` (or
/// fire right away). C ref: `func_timer_use`.
pub fn func_timer_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.activator = Some(activator);
    // If on, turn it off.
    if ent.game.nextthink != 0.0 {
        ent.game.nextthink = 0.0;
        return;
    }
    // Turn it on.
    if ent.game.delay != 0.0 {
        ent.game.nextthink = time + ent.game.delay;
    } else {
        func_timer_think(world, key);
    }
}

/// The START_ON part of C SP_func_timer, which needs the game's random
/// numbers (spawn functions only see `EntityStorage`).
pub(crate) fn start_func_timer(
    world: &mut GameWorld,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let pausetime = fields
        .get("pausetime")
        .map_or(0.0, |v| q2_common::cvar::parse_value(v));
    let jitter = world.crandom();
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.spawnflags & TIMER_START_ON != 0 {
        ent.game.nextthink =
            time + 1.0 + pausetime + ent.game.delay + ent.game.wait + jitter * ent.game.random;
        ent.game.activator = Some(key);
    }
}

// ---------------------------------------------------------------------------
// Explosive callbacks
// ---------------------------------------------------------------------------

/// func_explosive spawnflag: invisible and non-solid until used.
const EXPLOSIVE_TRIGGER_SPAWN: i32 = 1;

/// Blow the brush up from its centre (bmodel origins are 0,0,0), fire its
/// targets and remove it. C ref: `func_explosive_explode`; the debris and
/// explosion effect are not ported.
pub fn func_explosive_die(
    world: &mut GameWorld,
    self_key: EntityKey,
    inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let inflictor_origin = world
        .entities
        .get(inflictor)
        .map_or(Vec3f::ZERO, |e| e.state.origin);
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    ent.state.origin = ent.absmin + ent.size * 0.5;
    ent.game.takedamage = TakeDamage::No;
    ent.velocity = (ent.state.origin - inflictor_origin).normalize_or_zero() * 150.0;
    let dmg = ent.game.dmg;

    if dmg != 0 {
        world.t_radius_damage(
            self_key,
            attacker,
            dmg as f32,
            None,
            (dmg + 40) as f32,
            MeansOfDeath::Explosive,
        );
    }

    world.use_targets(self_key, attacker);
    world.free_entity(self_key);
}

/// C ref: `func_explosive_use`.
pub fn func_explosive_use(
    world: &mut GameWorld,
    key: EntityKey,
    other: EntityKey,
    _activator: EntityKey,
) {
    let health = world.entities.get(key).map_or(0, |e| e.game.health);
    func_explosive_die(world, key, key, other, health, Vec3f::ZERO);
}

/// A TRIGGER_SPAWN func_explosive appears when used. C ref:
/// `func_explosive_spawn`.
pub fn func_explosive_spawn(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.solid = Solid::Bsp;
        ent.svflags &= !q2_shared::constants::SVF_NOCLIENT;
        ent.use_fn = None;
    }
    world.kill_box(key);
    world.link_entity(key);
}

// ---------------------------------------------------------------------------
// Killbox callback
// ---------------------------------------------------------------------------

/// Telefrag everything inside the brush; remove it a second after the
/// last of its `count` uses. C ref: `use_killbox`.
pub fn func_killbox_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    world.kill_box(self_key);
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(self_key) {
        // Hack to make sure that really everything is killed.
        ent.game.count -= 1;
        if ent.game.count == 0 {
            ent.think = Some(free_think);
            ent.game.nextthink = time + 1.0;
        }
    }
}

/// C `think = G_FreeEdict`.
fn free_think(world: &mut GameWorld, key: EntityKey) {
    world.free_entity(key);
}

// ---------------------------------------------------------------------------
// func_train callbacks
// ---------------------------------------------------------------------------

/// func_train spawnflags (C g_func.c).
const TRAIN_START_ON: i32 = 1;
const TRAIN_TOGGLE: i32 = 2;
const TRAIN_BLOCK_STOPS: i32 = 4;
/// path_corner spawnflag: jump the train here instead of driving.
const PATH_CORNER_TELEPORT: i32 = 1;

/// Remove debris in the way; hurt monsters and players at most twice a
/// second. Also func_door_secret's `blocked`: C door_secret_blocked differs
/// only in lacking the `dmg == 0` return, and a secret door's dmg is never
/// 0. C ref: `train_blocked`.
pub fn train_blocked(world: &mut GameWorld, self_key: EntityKey, other: EntityKey) {
    if crush_non_living(world, self_key, other) {
        return;
    }
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    if time < ent.game.touch_debounce_time || ent.game.dmg == 0 {
        return;
    }
    ent.game.touch_debounce_time = time + 0.5;
    let dmg = ent.game.dmg;
    let origin = world
        .entities
        .get(other)
        .map_or(Vec3f::ZERO, |o| o.state.origin);
    world.t_damage(
        other,
        self_key,
        self_key,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        dmg,
        1,
        DamageFlags::empty(),
        MeansOfDeath::Crush,
    );
}

/// Arrived at a corner: fire its pathtarget, then wait there, park, or
/// drive on. C ref: `train_wait`.
pub fn train_wait(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (corner, activator) = (ent.game.target_ent, ent.game.activator.unwrap_or(key));
    let pathtarget = corner
        .and_then(|c| world.entities.get(c))
        .map(|c| c.game.pathtarget.clone())
        .filter(|p| !p.is_empty());
    if let (Some(corner), Some(pathtarget)) = (corner, pathtarget) {
        let Some(c) = world.entities.get_mut(corner) else {
            return;
        };
        let savetarget = std::mem::replace(&mut c.game.target, pathtarget);
        world.use_targets(corner, activator);
        if let Some(c) = world.entities.get_mut(corner) {
            c.game.target = savetarget;
        }
        // Make sure we didn't get killed by a killtarget.
        if !world.entities.is_live(key) {
            return;
        }
    }

    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let wait = ent.moveinfo.as_ref().map_or(0.0, |mi| mi.wait);
    if wait == 0.0 {
        train_next(world, key);
        return;
    }
    if wait > 0.0 {
        ent.game.nextthink = time + wait;
        ent.think = Some(train_next);
    } else if ent.game.spawnflags & TRAIN_TOGGLE != 0 {
        train_next(world, key);
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.spawnflags &= !TRAIN_START_ON;
            ent.velocity = Vec3f::ZERO;
            ent.game.nextthink = 0.0;
        }
    }
    mover_sound(world, key, false);
}

/// Drive to the next corner, first jumping through a teleport corner.
/// C ref: `train_next`.
pub fn train_next(world: &mut GameWorld, key: EntityKey) {
    let mut first = true;
    loop {
        let Some(target) = world.entities.get(key).map(|e| e.game.target.clone()) else {
            return;
        };
        if target.is_empty() {
            return;
        }
        let Some(corner) = world.pick_target(&target) else {
            world
                .gi
                .dprintf(&format!("train_next: bad target {target}\n"));
            return;
        };
        let Some(c) = world.entities.get(corner) else {
            return;
        };
        let (c_origin, c_target, c_flags, c_wait) = (
            c.state.origin,
            c.game.target.clone(),
            c.game.spawnflags,
            c.game.wait,
        );
        let Some(ent) = world.entities.get_mut(key) else {
            return;
        };
        ent.game.target = c_target;

        // Check for a teleport path_corner.
        if c_flags & PATH_CORNER_TELEPORT != 0 {
            if !first {
                world.gi.dprintf(&format!(
                    "connected teleport path_corners, see path_corner at {c_origin:?}\n"
                ));
                return;
            }
            first = false;
            ent.state.origin = c_origin - ent.mins;
            ent.state.old_origin = ent.state.origin;
            ent.state.event = q2_shared::constants::EV_OTHER_TELEPORT;
            world.link_entity(key);
            continue;
        }

        ent.game.target_ent = Some(corner);
        let dest = c_origin - ent.mins;
        let origin = ent.state.origin;
        let mi = ent.moveinfo.get_or_insert_with(Box::default);
        mi.wait = c_wait;
        mi.state = STATE_TOP;
        mi.start_origin = origin;
        mi.end_origin = dest;
        mover_sound(world, key, true);
        move_calc(world, key, dest, train_wait);
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.spawnflags |= TRAIN_START_ON;
        }
        return;
    }
}

/// Drive on to the corner a toggled-off train stopped short of.
/// C ref: `train_resume`.
pub fn train_resume(world: &mut GameWorld, key: EntityKey) {
    let Some(corner) = world.entities.get(key).and_then(|e| e.game.target_ent) else {
        return;
    };
    let Some(c_origin) = world.entities.get(corner).map(|c| c.state.origin) else {
        return;
    };
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let dest = c_origin - ent.mins;
    let origin = ent.state.origin;
    let mi = ent.moveinfo.get_or_insert_with(Box::default);
    mi.state = STATE_TOP;
    mi.start_origin = origin;
    mi.end_origin = dest;
    move_calc(world, key, dest, train_wait);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.spawnflags |= TRAIN_START_ON;
    }
}

/// Second-frame setup, once the corners exist: put the train's mins on
/// its first corner and start it unless something targets it.
/// C ref: `func_train_find`.
pub fn func_train_find(world: &mut GameWorld, key: EntityKey) {
    let Some(target) = world.entities.get(key).map(|e| e.game.target.clone()) else {
        return;
    };
    if target.is_empty() {
        world.gi.dprintf("train_find: no target\n");
        return;
    }
    let Some(corner) = world.pick_target(&target) else {
        world
            .gi
            .dprintf(&format!("train_find: target {target} not found\n"));
        return;
    };
    let Some(c) = world.entities.get(corner) else {
        return;
    };
    let (c_origin, c_target) = (c.state.origin, c.game.target.clone());
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.target = c_target;
    ent.state.origin = c_origin - ent.mins;
    world.link_entity(key);

    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    // If not triggered, start immediately.
    if ent.game.targetname.is_empty() {
        ent.game.spawnflags |= TRAIN_START_ON;
    }
    if ent.game.spawnflags & TRAIN_START_ON != 0 {
        ent.game.nextthink = time_plus(time, FRAMETIME_C);
        ent.think = Some(train_next);
        ent.game.activator = Some(key);
    }
}

/// Start a parked train; a TOGGLE train that is running stops.
/// C ref: `train_use`.
pub fn train_use(world: &mut GameWorld, key: EntityKey, _other: EntityKey, activator: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.activator = Some(activator);
    if ent.game.spawnflags & TRAIN_START_ON != 0 {
        if ent.game.spawnflags & TRAIN_TOGGLE == 0 {
            return;
        }
        ent.game.spawnflags &= !TRAIN_START_ON;
        ent.velocity = Vec3f::ZERO;
        ent.game.nextthink = 0.0;
    } else if ent.game.target_ent.is_some() {
        train_resume(world, key);
    } else {
        train_next(world, key);
    }
}

// ---------------------------------------------------------------------------
// func_conveyor callbacks
// ---------------------------------------------------------------------------

/// func_conveyor spawnflags (C g_func.c).
const CONVEYOR_START_ON: i32 = 1;
const CONVEYOR_TOGGLE: i32 = 2;

/// Switch the belt on (to the speed saved in `count`) or off. Without
/// TOGGLE the saved speed is spent on the first use. pmove pushes at a
/// fixed 100 from the brush's CONTENTS_CURRENT_* (C PM_AddCurrents), so
/// `speed` only records the state. C ref: `func_conveyor_use`.
pub fn func_conveyor_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let g = &mut ent.game;
    if g.spawnflags & CONVEYOR_START_ON != 0 {
        g.speed = 0.0;
        g.spawnflags &= !CONVEYOR_START_ON;
    } else {
        g.speed = g.count as f32;
        g.spawnflags |= CONVEYOR_START_ON;
    }
    if g.spawnflags & CONVEYOR_TOGGLE == 0 {
        g.count = 0;
    }
}

// ---------------------------------------------------------------------------
// func_door_secret callbacks
// ---------------------------------------------------------------------------

/// func_door_secret spawnflags (C g_func.c).
const SECRET_ALWAYS_SHOOT: i32 = 1;
const SECRET_1ST_LEFT: i32 = 2;
const SECRET_1ST_DOWN: i32 = 4;

// A secret door's C pos1 (the first, sideways stop) and pos2 (the open
// position) live in `moveinfo.start_origin`/`end_origin`, which C leaves
// unused for these doors.

fn secret_positions(world: &GameWorld, key: EntityKey) -> Option<(Vec3f, Vec3f)> {
    let mi = world.entities.get(key)?.moveinfo.as_deref()?;
    Some((mi.start_origin, mi.end_origin))
}

/// Open, unless already moving (only a closed door sits at its origin).
/// C ref: `door_secret_use`.
pub fn door_secret_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.state.origin != Vec3f::ZERO)
    {
        return;
    }
    if let Some((pos1, _)) = secret_positions(world, key) {
        move_calc(world, key, pos1, door_secret_move1);
    }
    door_use_areaportals(world, key, true);
}

/// At the first stop: pause a second. C ref: `door_secret_move1`.
pub fn door_secret_move1(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + 1.0;
        ent.think = Some(door_secret_move2);
    }
}

/// Slide back to the open position. C ref: `door_secret_move2`.
pub fn door_secret_move2(world: &mut GameWorld, key: EntityKey) {
    if let Some((_, pos2)) = secret_positions(world, key) {
        move_calc(world, key, pos2, door_secret_move3);
    }
}

/// Open: hold for `wait`, or for good when it is -1.
/// C ref: `door_secret_move3`.
pub fn door_secret_move3(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.wait == -1.0 {
        return;
    }
    ent.game.nextthink = time + ent.game.wait;
    ent.think = Some(door_secret_move4);
}

/// Slide forward to the first stop. C ref: `door_secret_move4`.
pub fn door_secret_move4(world: &mut GameWorld, key: EntityKey) {
    if let Some((pos1, _)) = secret_positions(world, key) {
        move_calc(world, key, pos1, door_secret_move5);
    }
}

/// Back at the first stop: pause a second. C ref: `door_secret_move5`.
pub fn door_secret_move5(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + 1.0;
        ent.think = Some(door_secret_move6);
    }
}

/// Slide shut. C ref: `door_secret_move6`.
pub fn door_secret_move6(world: &mut GameWorld, key: EntityKey) {
    move_calc(world, key, Vec3f::ZERO, door_secret_done);
}

/// Closed: an untargeted or ALWAYS_SHOOT door can be shot open again.
/// C ref: `door_secret_done`.
pub fn door_secret_done(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if ent.game.targetname.is_empty() || ent.game.spawnflags & SECRET_ALWAYS_SHOOT != 0 {
            ent.game.health = 0;
            ent.game.takedamage = TakeDamage::Yes;
        }
    }
    door_use_areaportals(world, key, false);
}

/// C ref: `door_secret_die`.
pub fn door_secret_die(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.takedamage = TakeDamage::No;
    }
    door_secret_use(world, self_key, attacker, attacker);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

/// Spawn-only integer key (C `st.lip`, `st.height`), 0 when absent.
fn spawn_temp(fields: &HashMap<String, String>, key: &str) -> f32 {
    fields.get(key).map_or(0, |v| crate::spawn::strtol(v)) as f32
}

/// Distance a mover travels along `movedir`: its size along that axis less
/// the lip that stays visible.
fn move_distance(movedir: Vec3f, size: Vec3f, lip: f32) -> f32 {
    let abs = movedir.abs();
    abs.x * size.x + abs.y * size.y + abs.z * size.z - lip
}

/// C ref: `SP_func_plat`. Speeds are per frame (C scales map values by 0.1).
pub fn sp_func_plat(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = "func_plat".to_string();
    ent.state.angles = Vec3f::ZERO;
    ent.solid = Solid::Bsp;
    ent.game.movetype = MoveType::Push;
    let g = &mut ent.game;
    let tenth = |v: f32| (f64::from(v) * 0.1) as f32; // C `*= 0.1`, a double
    g.speed = if g.speed == 0.0 { 20.0 } else { tenth(g.speed) };
    g.accel = if g.accel == 0.0 { 5.0 } else { tenth(g.accel) };
    g.decel = if g.decel == 0.0 { 5.0 } else { tenth(g.decel) };
    if g.dmg == 0 {
        g.dmg = 2;
    }
    let mut lip = spawn_temp(fields, "lip");
    if lip == 0.0 {
        lip = 8.0;
    }

    // pos1 is the top position, pos2 the bottom.
    let pos1 = ent.state.origin;
    let mut pos2 = pos1;
    let height = spawn_temp(fields, "height");
    if height != 0.0 {
        pos2.z -= height;
    } else {
        pos2.z -= (ent.maxs.z - ent.mins.z) - lip;
    }
    ent.use_fn = Some(use_plat);
    ent.blocked = Some(plat_blocked);
    ent.moveinfo = Some(Box::new(MoveInfo {
        speed: ent.game.speed,
        accel: ent.game.accel,
        decel: ent.game.decel,
        wait: ent.game.wait,
        start_origin: pos1,
        end_origin: pos2,
        ..Default::default()
    }));
    let targeted = !ent.game.targetname.is_empty();
    plat_spawn_inside_trigger(storage, key, lip);

    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.moveinfo.as_deref_mut() else {
        return;
    };
    if targeted {
        mi.state = STATE_UP;
    } else {
        ent.state.origin = pos2;
        mi.state = STATE_BOTTOM;
    }
}

/// C ref: `SP_func_button`.
pub fn sp_func_button(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = "func_button".to_string();
    let movedir = GameWorld::set_movedir(ent.state.angles);
    ent.state.angles = Vec3f::ZERO;
    ent.game.movedir = movedir;
    ent.game.movetype = MoveType::Stop;
    ent.solid = Solid::Bsp;
    let g = &mut ent.game;
    if g.speed == 0.0 {
        g.speed = 40.0;
    }
    if g.accel == 0.0 {
        g.accel = g.speed;
    }
    if g.decel == 0.0 {
        g.decel = g.speed;
    }
    if g.wait == 0.0 {
        g.wait = 3.0;
    }
    let mut lip = spawn_temp(fields, "lip");
    if lip == 0.0 {
        lip = 4.0;
    }
    let pos1 = ent.state.origin;
    let pos2 = pos1 + movedir * move_distance(movedir, ent.maxs - ent.mins, lip);

    ent.state.effects |= EF_ANIM01;
    ent.use_fn = Some(button_use);
    if ent.game.health != 0 {
        ent.game.max_health = ent.game.health;
        ent.die = Some(button_killed);
        ent.game.takedamage = TakeDamage::Yes;
    } else if ent.game.targetname.is_empty() {
        ent.touch = Some(button_touch);
    }
    ent.moveinfo = Some(Box::new(MoveInfo {
        state: STATE_BOTTOM,
        speed: ent.game.speed,
        accel: ent.game.accel,
        decel: ent.game.decel,
        wait: ent.game.wait,
        start_origin: pos1,
        end_origin: pos2,
        ..Default::default()
    }));
}

/// C ref: `SP_func_door` (single player: no deathmatch speed-up; sounds
/// and `door_touch` are set by `finish_door`).
pub fn sp_func_door(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = "func_door".to_string();
    let movedir = GameWorld::set_movedir(ent.state.angles);
    ent.state.angles = Vec3f::ZERO;
    ent.game.movedir = movedir;
    ent.game.movetype = MoveType::Push;
    ent.solid = Solid::Bsp;
    ent.use_fn = Some(door_use);
    ent.blocked = Some(door_blocked);
    let g = &mut ent.game;
    if g.speed == 0.0 {
        g.speed = 100.0;
    }
    if g.accel == 0.0 {
        g.accel = g.speed;
    }
    if g.decel == 0.0 {
        g.decel = g.speed;
    }
    if g.wait == 0.0 {
        g.wait = 3.0;
    }
    if g.dmg == 0 {
        g.dmg = 2;
    }
    let mut lip = spawn_temp(fields, "lip");
    if lip == 0.0 {
        lip = 8.0;
    }

    // Calculate the second position.
    let mut pos1 = ent.state.origin;
    let distance = move_distance(movedir, ent.maxs - ent.mins, lip);
    let mut pos2 = pos1 + movedir * distance;
    // A door that starts open switches the positions.
    if ent.game.spawnflags & DOOR_START_OPEN != 0 {
        ent.state.origin = pos2;
        pos2 = pos1;
        pos1 = ent.state.origin;
    }

    if ent.game.health != 0 {
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(door_killed);
        ent.game.max_health = ent.game.health;
    }
    ent.moveinfo = Some(Box::new(MoveInfo {
        state: STATE_BOTTOM,
        speed: ent.game.speed,
        accel: ent.game.accel,
        decel: ent.game.decel,
        wait: ent.game.wait,
        distance,
        start_origin: pos1,
        end_origin: pos2,
        ..Default::default()
    }));
    if ent.game.spawnflags & DOOR_ANIMATED != 0 {
        ent.state.effects |= EF_ANIM_ALL;
    }
    if ent.game.spawnflags & DOOR_ANIMATED_FAST != 0 {
        ent.state.effects |= EF_ANIM_ALLFAST;
    }

    // Non-teamed doors are a team of one.
    if ent.game.team.is_empty() {
        ent.game.teammaster = Some(key);
    }
    ent.game.nextthink = FRAMETIME;
    ent.think = if ent.game.health != 0 || !ent.game.targetname.is_empty() {
        Some(think_calc_move_speed)
    } else {
        Some(think_spawn_door_trigger)
    };
}

/// C ref: `SP_func_rotating`.
pub fn sp_func_rotating(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    let flags = ent.game.spawnflags;
    ent.solid = Solid::Bsp;
    ent.game.movetype = if flags & ROTATING_STOP != 0 {
        MoveType::Stop
    } else {
        MoveType::Push
    };
    // Set the axis of rotation (Z_AXIS is the default, index 1 = yaw).
    ent.game.movedir = if flags & ROTATING_X_AXIS != 0 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else if flags & ROTATING_Y_AXIS != 0 {
        Vec3f::new(1.0, 0.0, 0.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    if flags & ROTATING_REVERSE != 0 {
        ent.game.movedir = -ent.game.movedir;
    }
    if ent.game.speed == 0.0 {
        ent.game.speed = 100.0;
    }
    if ent.game.dmg == 0 {
        ent.game.dmg = 2;
    }
    ent.use_fn = Some(rotating_use);
    ent.blocked = Some(rotating_blocked);
    if flags & ROTATING_START_ON != 0 {
        rotating_toggle(ent);
    }
    if flags & ANIMATED != 0 {
        ent.state.effects |= EF_ANIM_ALL;
    }
    if flags & ANIMATED_FAST != 0 {
        ent.state.effects |= EF_ANIM_ALLFAST;
    }
}

/// C ref: `SP_func_door_rotating` (sounds and `door_touch` are set by
/// `finish_door`).
pub fn sp_func_door_rotating(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = "func_door_rotating".to_string();
    ent.state.angles = Vec3f::ZERO;
    let flags = ent.game.spawnflags;
    // Set the axis of rotation (Z_AXIS is the default, index 1 = yaw).
    let mut movedir = if flags & DOOR_X_AXIS != 0 {
        Vec3f::new(0.0, 0.0, 1.0)
    } else if flags & DOOR_Y_AXIS != 0 {
        Vec3f::new(1.0, 0.0, 0.0)
    } else {
        Vec3f::new(0.0, 1.0, 0.0)
    };
    if flags & DOOR_REVERSE != 0 {
        movedir = -movedir;
    }
    let mut distance = spawn_temp(fields, "distance");
    if distance == 0.0 {
        tracing::debug!(
            "func_door_rotating at {:?} with no distance set",
            ent.state.origin
        );
        distance = 90.0;
    }
    let mut pos1 = ent.state.angles;
    let mut pos2 = pos1 + movedir * distance;

    ent.game.movetype = MoveType::Push;
    ent.solid = Solid::Bsp;
    ent.blocked = Some(door_blocked);
    ent.use_fn = Some(door_use);
    let g = &mut ent.game;
    if g.speed == 0.0 {
        g.speed = 100.0;
    }
    if g.accel == 0.0 {
        g.accel = g.speed;
    }
    if g.decel == 0.0 {
        g.decel = g.speed;
    }
    if g.wait == 0.0 {
        g.wait = 3.0;
    }
    if g.dmg == 0 {
        g.dmg = 2;
    }

    // A door that starts open switches the positions and turns back.
    if flags & DOOR_START_OPEN != 0 {
        ent.state.angles = pos2;
        pos2 = pos1;
        pos1 = ent.state.angles;
        movedir = -movedir;
    }
    ent.game.movedir = movedir;

    if ent.game.health != 0 {
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(door_killed);
        ent.game.max_health = ent.game.health;
    }
    ent.moveinfo = Some(Box::new(MoveInfo {
        state: STATE_BOTTOM,
        speed: ent.game.speed,
        accel: ent.game.accel,
        decel: ent.game.decel,
        wait: ent.game.wait,
        distance,
        start_origin: ent.state.origin,
        start_angles: pos1,
        end_origin: ent.state.origin,
        end_angles: pos2,
        ..Default::default()
    }));
    if flags & DOOR_ANIMATED != 0 {
        ent.state.effects |= EF_ANIM_ALL;
    }

    // Non-teamed doors are a team of one.
    if ent.game.team.is_empty() {
        ent.game.teammaster = Some(key);
    }
    ent.game.nextthink = FRAMETIME;
    ent.think = if ent.game.health != 0 || !ent.game.targetname.is_empty() {
        Some(think_calc_move_speed)
    } else {
        Some(think_spawn_door_trigger)
    };
}

/// A movable water brush: a door that only opens when targeted, toggles
/// by default and never crushes. Renamed func_door, as in C, so the door
/// callbacks slide it. C ref: `SP_func_water` (sounds: `finish_water`).
pub fn sp_func_water(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    let movedir = GameWorld::set_movedir(ent.state.angles);
    ent.state.angles = Vec3f::ZERO;
    ent.game.movedir = movedir;
    ent.game.movetype = MoveType::Push;
    ent.solid = Solid::Bsp;

    // Calculate the second position; func_water has no default lip.
    let mut pos1 = ent.state.origin;
    let distance = move_distance(movedir, ent.maxs - ent.mins, spawn_temp(fields, "lip"));
    let mut pos2 = pos1 + movedir * distance;
    if ent.game.spawnflags & DOOR_START_OPEN != 0 {
        ent.state.origin = pos2;
        pos2 = pos1;
        pos1 = ent.state.origin;
    }

    let g = &mut ent.game;
    if g.speed == 0.0 {
        g.speed = 25.0;
    }
    if g.wait == 0.0 {
        g.wait = -1.0;
    }
    if g.wait == -1.0 {
        g.spawnflags |= DOOR_TOGGLE;
    }
    ent.moveinfo = Some(Box::new(MoveInfo {
        state: STATE_BOTTOM,
        speed: ent.game.speed,
        accel: ent.game.speed,
        decel: ent.game.speed,
        wait: ent.game.wait,
        distance,
        start_origin: pos1,
        end_origin: pos2,
        ..Default::default()
    }));
    ent.use_fn = Some(door_use);
    ent.game.classname = "func_door".to_string();
}

/// C ref: `SP_func_train` (the `noise` sound: `finish_train`).
pub fn sp_func_train(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = "func_train".to_string();
    ent.game.movetype = MoveType::Push;
    ent.state.angles = Vec3f::ZERO;
    ent.blocked = Some(train_blocked);
    let g = &mut ent.game;
    if g.spawnflags & TRAIN_BLOCK_STOPS != 0 {
        g.dmg = 0;
    } else if g.dmg == 0 {
        g.dmg = 100;
    }
    ent.solid = Solid::Bsp;
    if ent.game.speed == 0.0 {
        ent.game.speed = 100.0;
    }
    let speed = ent.game.speed;
    ent.moveinfo = Some(Box::new(MoveInfo {
        speed,
        accel: speed,
        decel: speed,
        ..Default::default()
    }));
    ent.use_fn = Some(train_use);

    if ent.game.target.is_empty() {
        tracing::debug!("func_train without a target at {:?}", ent.absmin);
    } else {
        // Start trains on the second frame, to make sure their targets
        // have had a chance to spawn.
        ent.game.nextthink = FRAMETIME;
        ent.think = Some(func_train_find);
    }
}

/// Send the parked train to the corner the user's `pathtarget` names.
/// C ref: `trigger_elevator_use`.
pub fn trigger_elevator_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    other: EntityKey,
    _activator: EntityKey,
) {
    let Some(train) = world.entities.get(self_key).and_then(|e| e.game.movetarget) else {
        return;
    };
    if world
        .entities
        .get(train)
        .is_none_or(|t| t.game.nextthink != 0.0)
    {
        return;
    }
    let Some(pathtarget) = world.entities.get(other).map(|e| e.game.pathtarget.clone()) else {
        return;
    };
    if pathtarget.is_empty() {
        world.gi.dprintf("elevator used with no pathtarget\n");
        return;
    }
    let Some(target) = world.pick_target(&pathtarget) else {
        world.gi.dprintf(&format!(
            "elevator used with bad pathtarget: {pathtarget}\n"
        ));
        return;
    };
    if let Some(t) = world.entities.get_mut(train) {
        t.game.target_ent = Some(target);
    }
    train_resume(world, train);
}

/// Second-frame setup, once the train exists: only a `func_train` target
/// makes the elevator usable. C ref: `trigger_elevator_init`.
pub fn trigger_elevator_init(world: &mut GameWorld, key: EntityKey) {
    let Some(target) = world.entities.get(key).map(|e| e.game.target.clone()) else {
        return;
    };
    if target.is_empty() {
        world.gi.dprintf("trigger_elevator has no target\n");
        return;
    }
    let Some(train) = world.pick_target(&target) else {
        world.gi.dprintf(&format!(
            "trigger_elevator unable to find target {target}\n"
        ));
        return;
    };
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.movetarget = Some(train);
    }
    if world
        .entities
        .get(train)
        .is_none_or(|t| t.game.classname != "func_train")
    {
        world.gi.dprintf(&format!(
            "trigger_elevator target {target} is not a train\n"
        ));
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.use_fn = Some(trigger_elevator_use);
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
    }
}

/// C ref: `SP_trigger_elevator`.
pub fn sp_trigger_elevator(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.think = Some(trigger_elevator_init);
        ent.game.nextthink = FRAMETIME;
    }
}

/// A stationary belt brush; its CONTENTS_CURRENT_* surfaces move what
/// stands on it. C ref: `SP_func_conveyor` (movetype stays NONE).
pub fn sp_func_conveyor(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = "func_conveyor".to_string();
    let g = &mut ent.game;
    if g.speed == 0.0 {
        g.speed = 100.0;
    }
    if g.spawnflags & CONVEYOR_START_ON == 0 {
        g.count = g.speed as i32;
        g.speed = 0.0;
    }
    ent.use_fn = Some(func_conveyor_use);
    ent.solid = Solid::Bsp;
}

/// A door that slides sideways (or down) by its width, then back by its
/// length, and returns after `wait`. Renamed func_door, as in C.
/// C ref: `SP_func_door_secret` (sounds and `door_touch`:
/// `finish_door_secret`).
pub fn sp_func_door_secret(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Push;
    ent.solid = Solid::Bsp;
    ent.blocked = Some(train_blocked);
    ent.use_fn = Some(door_secret_use);
    let flags = ent.game.spawnflags;
    if ent.game.targetname.is_empty() || flags & SECRET_ALWAYS_SHOOT != 0 {
        ent.game.health = 0;
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(door_secret_die);
    }
    if ent.game.dmg == 0 {
        ent.game.dmg = 2;
    }
    if ent.game.wait == 0.0 {
        ent.game.wait = 5.0;
    }

    // Calculate positions.
    let (forward, right, up) = crate::physics::angle_vectors(ent.state.angles);
    ent.state.angles = Vec3f::ZERO;
    let size = ent.maxs - ent.mins;
    let side = 1.0 - (flags & SECRET_1ST_LEFT) as f32;
    let width = if flags & SECRET_1ST_DOWN != 0 {
        up.dot(size).abs()
    } else {
        right.dot(size).abs()
    };
    let length = forward.dot(size).abs();
    let pos1 = if flags & SECRET_1ST_DOWN != 0 {
        ent.state.origin - up * width
    } else {
        ent.state.origin + right * (side * width)
    };
    let pos2 = pos1 + forward * length;
    ent.moveinfo = Some(Box::new(MoveInfo {
        speed: 50.0,
        accel: 50.0,
        decel: 50.0,
        start_origin: pos1,
        end_origin: pos2,
        ..Default::default()
    }));

    if ent.game.health != 0 {
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(door_killed);
        ent.game.max_health = ent.game.health;
    }
    ent.game.classname = "func_door".to_string();
}

/// C ref: `SP_func_timer`; `start_func_timer` handles START_ON.
pub fn sp_func_timer(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    if ent.game.wait == 0.0 {
        ent.game.wait = 1.0;
    }
    ent.use_fn = Some(func_timer_use);
    ent.think = Some(func_timer_think);
    if ent.game.random >= ent.game.wait {
        ent.game.random = (f64::from(ent.game.wait) - FRAMETIME_C) as f32;
        tracing::debug!("func_timer at {:?} has random >= wait", ent.state.origin);
    }
    ent.svflags = q2_shared::constants::SVF_NOCLIENT;
}

/// C ref: `SP_func_explosive` (single player).
pub fn sp_func_explosive(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Push;
    let flags = ent.game.spawnflags;
    let explodes_on_use = flags & EXPLOSIVE_TRIGGER_SPAWN == 0 && !ent.game.targetname.is_empty();
    if flags & EXPLOSIVE_TRIGGER_SPAWN != 0 {
        ent.svflags |= q2_shared::constants::SVF_NOCLIENT;
        ent.solid = Solid::Not;
        ent.use_fn = Some(func_explosive_spawn);
    } else {
        ent.solid = Solid::Bsp;
        if explodes_on_use {
            ent.use_fn = Some(func_explosive_use);
        }
    }
    if flags & 2 != 0 {
        ent.state.effects |= EF_ANIM_ALL;
    }
    if flags & 4 != 0 {
        ent.state.effects |= EF_ANIM_ALLFAST;
    }
    if !explodes_on_use {
        if ent.game.health == 0 {
            ent.game.health = 100;
        }
        ent.die = Some(func_explosive_die);
        ent.game.takedamage = TakeDamage::Yes;
    }
}

/// C ref: `SP_func_killbox`: an invisible, non-solid volume.
pub fn sp_func_killbox(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(func_killbox_use);
        ent.svflags = q2_shared::constants::SVF_NOCLIENT;
    }
}

pub fn sp_func_generic(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "func".to_string());
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;
    }
}

/// func_areaportal (see `sp_func_areaportal`) and func_clock: C
/// SP_func_areaportal never sets a solid (a solid portal brush would close
/// every doorway it sits in), and func_clock is a point entity.
pub fn sp_func_nonsolid(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields.get("classname").cloned().unwrap_or_default();
        ent.state.modelindex = 0; // C never calls setmodel: nothing to draw
    }
}

/// C ref: `SP_func_areaportal` (g_misc.c): it always starts closed.
pub fn sp_func_areaportal(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    sp_func_nonsolid(storage, key, fields);
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(use_areaportal);
        ent.game.count = 0;
    }
}

/// Toggle the portal numbered by `style`. C ref: `Use_Areaportal`.
pub fn use_areaportal(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.count ^= 1;
    let (style, open) = (ent.game.style, ent.game.count != 0);
    world.gi.set_area_portal_state(style, open);
}

// ---------------------------------------------------------------------------
// Spawn finish functions: the sound registration of the C spawn functions,
// which needs `GameWorld` (spawn functions only see `EntityStorage`).
// ---------------------------------------------------------------------------

/// C ref: `SP_func_plat` (its sounds are fixed).
pub(crate) fn finish_plat(world: &mut GameWorld, key: EntityKey, _: &HashMap<String, String>) {
    set_mover_sounds(
        world,
        key,
        [
            "plats/pt1_strt.wav",
            "plats/pt1_mid.wav",
            "plats/pt1_end.wav",
        ],
    );
}

/// C ref: `SP_func_button`: `sounds` 1 is silent.
pub(crate) fn finish_button(
    world: &mut GameWorld,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if sounds_key(fields) != 1 {
        set_mover_sounds(world, key, ["switches/butn2.wav", "", ""]);
    }
}

/// A targeted door centerprints its message when touched: without
/// health for func_door and func_door_secret (C `else if`), with or
/// without for func_door_rotating (C `if`).
/// C ref: SP_func_door, SP_func_door_rotating and SP_func_door_secret.
fn finish_door_message(world: &mut GameWorld, key: EntityKey) {
    let has_touch = world.entities.get(key).is_some_and(|e| {
        (e.game.health == 0 || e.game.classname == "func_door_rotating")
            && !e.game.targetname.is_empty()
            && !e.game.message.is_empty()
    });
    if has_touch {
        world.gi.sound_index("misc/talk.wav");
        if let Some(ent) = world.entities.get_mut(key) {
            ent.touch = Some(door_touch);
        }
    }
}

/// C ref: `SP_func_door` and `SP_func_door_rotating`: `sounds` 1 is silent.
pub(crate) fn finish_door(world: &mut GameWorld, key: EntityKey, fields: &HashMap<String, String>) {
    if sounds_key(fields) != 1 {
        set_mover_sounds(world, key, DOOR_SOUNDS);
    }
    finish_door_message(world, key);
}

/// C ref: `SP_func_door_secret`: its sounds are fixed, and no secret door
/// callback plays them.
pub(crate) fn finish_door_secret(
    world: &mut GameWorld,
    key: EntityKey,
    _: &HashMap<String, String>,
) {
    set_mover_sounds(world, key, DOOR_SOUNDS);
    finish_door_message(world, key);
}

/// C ref: `SP_func_water`: `sounds` 1 (water) and 2 (lava) have start and
/// end sounds only.
pub(crate) fn finish_water(
    world: &mut GameWorld,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if matches!(sounds_key(fields), 1 | 2) {
        set_mover_sounds(world, key, ["world/mov_watr.wav", "", "world/stp_watr.wav"]);
    }
}

/// C ref: `SP_func_train`: `noise` loops while it moves.
pub(crate) fn finish_train(
    world: &mut GameWorld,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(noise) = fields.get("noise") {
        set_mover_sounds(world, key, ["", noise, ""]);
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::GameImport;
    use crate::world::test_world;

    use crate::entity::ClientData;
    use crate::world::test_world_recording;

    const DOOR_MINS: Vec3f = Vec3f::new(0.0, 0.0, 0.0);
    const DOOR_MAXS: Vec3f = Vec3f::new(16.0, 64.0, 96.0);
    const CAR_MINS: Vec3f = Vec3f::new(-32.0, -32.0, -8.0);

    /// A world spawned from `entities`, where `*1` is a 16x64x96 box, `*2`
    /// a 128x128x64 plat and `*3` a 64x64x16 car centred on its origin.
    fn spawned(entities: &str) -> GameWorld {
        spawned_gi(entities).0
    }

    /// `spawned`, with the mock import that recorded the spawn.
    fn spawned_gi(entities: &str) -> (GameWorld, std::rc::Rc<crate::world::MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        let mut bounds = gi.inline_bounds.borrow_mut();
        bounds.insert("*1".into(), (DOOR_MINS, DOOR_MAXS));
        bounds.insert("*2".into(), (Vec3f::ZERO, Vec3f::new(128.0, 128.0, 64.0)));
        bounds.insert("*3".into(), (CAR_MINS, -CAR_MINS));
        drop(bounds);
        world.init_game();
        world.spawn_entities(
            "test",
            &format!("{{ \"classname\" \"worldspawn\" }}\n{entities}"),
            "",
        );
        (world, gi)
    }

    fn player(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn_test_client(ClientData::default());
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = 100;
        key
    }

    fn run_for(world: &mut GameWorld, seconds: f32) {
        for _ in 0..(seconds / FRAMETIME).round() as i32 {
            world.run_frame();
        }
    }

    fn by_class(world: &GameWorld, classname: &str) -> EntityKey {
        world.find_by_classname(None, classname).expect(classname)
    }

    fn mover(world: &GameWorld, key: EntityKey) -> (Vec3f, i32) {
        let ent = world.entities.get(key).unwrap();
        (ent.state.origin, ent.moveinfo.as_ref().unwrap().state)
    }

    fn touch_field(world: &GameWorld, owner: EntityKey) -> Option<EntityKey> {
        world
            .entities
            .iter()
            .find(|(_, e)| e.in_use && e.owner == Some(owner) && e.solid == Solid::Trigger)
            .map(|(k, _)| k)
    }

    #[test]
    fn door_opens_its_size_along_movedir_less_the_lip() {
        let world = spawned("{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" }");
        let door = by_class(&world, "func_door");
        let ent = world.entities.get(door).unwrap();
        let mi = ent.moveinfo.as_ref().unwrap();
        assert_eq!(
            mi.end_origin,
            Vec3f::new(0.0, 0.0, 88.0),
            "96 up less lip 8"
        );
        assert_eq!(ent.state.angles, Vec3f::ZERO, "G_SetMovedir clears angles");
        assert_eq!(ent.solid, Solid::Bsp);
    }

    #[test]
    fn door_with_a_lip_key_and_yaw_opens_sideways() {
        let world = spawned(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"90\" \"lip\" \"16\" }",
        );
        let mi = world
            .entities
            .get(by_class(&world, "func_door"))
            .unwrap()
            .moveinfo
            .clone()
            .unwrap();
        assert!((mi.end_origin.y - 48.0).abs() < 1e-3, "{:?}", mi.end_origin);
        assert!(mi.end_origin.x.abs() < 1e-3);
    }

    #[test]
    fn untargeted_door_spawns_a_touch_field_60_wider_on_its_first_frame() {
        let mut world =
            spawned("{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" }");
        let door = by_class(&world, "func_door");
        assert!(touch_field(&world, door).is_none());
        world.run_frame();
        let field = touch_field(&world, door).expect("trigger spawned");
        let (d, f) = (
            world.entities.get(door).unwrap(),
            world.entities.get(field).unwrap(),
        );
        assert_eq!(f.mins.x, d.absmin.x - 60.0);
        assert_eq!(f.maxs.y, d.absmax.y + 60.0);
        assert_eq!(f.maxs.z, d.absmax.z, "not widened vertically");
    }

    #[test]
    fn touching_the_field_opens_the_door_fully_then_it_closes_after_wait() {
        let mut world =
            spawned("{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" }");
        let door = by_class(&world, "func_door");
        world.run_frame();
        let field = touch_field(&world, door).unwrap();
        let p = player(&mut world);
        let touch = world.entities.get(field).unwrap().touch.unwrap();
        touch(&mut world, field, p, None, None);
        assert_eq!(mover(&world, door).1, STATE_UP);

        run_for(&mut world, 1.5); // 88 units at 100/s
        let (origin, state) = mover(&world, door);
        assert_eq!(state, STATE_TOP);
        assert!((origin.z - 88.0).abs() < 1e-3, "lands exactly: {origin:?}");

        run_for(&mut world, 3.0 + 1.5);
        let (origin, state) = mover(&world, door);
        assert_eq!(state, STATE_BOTTOM);
        assert!(origin.z.abs() < 1e-3, "{origin:?}");
    }

    #[test]
    fn door_field_ignores_entities_that_are_neither_clients_nor_monsters() {
        let mut world = spawned("{ \"classname\" \"func_door\" \"model\" \"*1\" }");
        let door = by_class(&world, "func_door");
        world.run_frame();
        let field = touch_field(&world, door).unwrap();
        let barrel = world.spawn().unwrap();
        world.entities.get_mut(barrel).unwrap().game.health = 10;
        touch_door_trigger(&mut world, field, barrel, None, None);
        assert_eq!(mover(&world, door).1, STATE_BOTTOM);
    }

    #[test]
    fn targeted_door_has_no_field_and_opens_when_used() {
        let mut world = spawned(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" \"targetname\" \"d\" }",
        );
        let door = by_class(&world, "func_door");
        world.run_frame();
        assert!(touch_field(&world, door).is_none());
        let p = player(&mut world);
        door_use(&mut world, door, p, p);
        run_for(&mut world, 1.5);
        assert_eq!(mover(&world, door).1, STATE_TOP);
    }

    #[test]
    fn plat_starts_lowered_and_rises_to_its_top_when_its_center_is_touched() {
        let mut world = spawned("{ \"classname\" \"func_plat\" \"model\" \"*2\" }");
        let plat = by_class(&world, "func_plat");
        assert_eq!(
            mover(&world, plat),
            (Vec3f::new(0.0, 0.0, -56.0), STATE_BOTTOM)
        );

        let center = world
            .entities
            .iter()
            .find(|(_, e)| e.game.enemy == Some(plat))
            .map(|(k, _)| k)
            .expect("inside trigger");
        let c = world.entities.get(center).unwrap();
        assert!(c.linkcount > 0, "linked at spawn");
        assert_eq!((c.mins.x, c.maxs.x), (25.0, 103.0));
        assert_eq!(c.maxs.z, 72.0);
        assert_eq!(c.mins.z, 72.0 - (56.0 + 8.0));

        let p = player(&mut world);
        touch_plat_center(&mut world, center, p, None, None);
        run_for(&mut world, 1.0);
        let (origin, state) = mover(&world, plat);
        assert_eq!(state, STATE_TOP);
        assert!(
            origin.z.abs() < 1e-3,
            "accelerated move ends exactly: {origin:?}"
        );

        run_for(&mut world, 3.0 + 1.0); // plat_hit_top waits 3 s
        let (origin, state) = mover(&world, plat);
        assert_eq!(state, STATE_BOTTOM);
        assert!((origin.z + 56.0).abs() < 1e-3, "{origin:?}");
    }

    #[test]
    fn button_pressed_by_walking_into_it_fires_its_targets() {
        let mut world = spawned(concat!(
            "{ \"classname\" \"func_button\" \"model\" \"*1\" \"angle\" \"0\" \"target\" \"d\" }\n",
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" \"targetname\" \"d\" }",
        ));
        let button = by_class(&world, "func_button");
        let door = by_class(&world, "func_door");
        let p = player(&mut world);
        let touch = world
            .entities
            .get(button)
            .unwrap()
            .touch
            .expect("button_touch");
        touch(&mut world, button, p, None, None);
        run_for(&mut world, 1.0); // 12 units at 40/s
        assert_eq!(mover(&world, button).1, STATE_TOP);
        assert_ne!(
            mover(&world, door).1,
            STATE_BOTTOM,
            "door opened by the button"
        );
    }

    #[test]
    fn constant_speed_move_lands_on_its_destination() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = MoveType::Push;
            ent.moveinfo = Some(Box::new(MoveInfo {
                speed: 100.0,
                accel: 100.0,
                decel: 100.0,
                ..Default::default()
            }));
        }
        fn arrived(world: &mut GameWorld, key: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count = 1;
        }
        move_calc(&mut world, key, Vec3f::new(255.0, 0.0, 0.0), arrived);
        run_for(&mut world, 3.0);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.count, 1);
        assert!(
            (ent.state.origin.x - 255.0).abs() < 1e-3,
            "{:?}",
            ent.state.origin
        );
        assert_eq!(ent.velocity, Vec3f::ZERO);
    }

    #[test]
    fn areaportal_brush_is_not_solid_so_it_never_blocks_its_doorway() {
        let (mut world, gi) = crate::world::test_world_recording();
        gi.inline_bounds
            .borrow_mut()
            .insert("*3".into(), (Vec3f::ZERO, Vec3f::splat(64.0)));
        world.init_game();
        world.spawn_entities(
            "base1",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"func_areaportal\" \"model\" \"*3\" }",
            "",
        );
        let key = world.find_by_classname(None, "func_areaportal").unwrap();
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.solid, Solid::Not);
        assert_eq!(ent.state.modelindex, 0, "not drawn");
    }

    #[test]
    fn register_func_spawns() {
        let world = test_world();
        assert!(world.spawn_table.get("func_plat").is_some());
        assert!(world.spawn_table.get("func_door").is_some());
        assert!(world.spawn_table.get("func_button").is_some());
        assert!(world.spawn_table.get("func_train").is_some());
        assert!(world.spawn_table.get("func_explosive").is_some());
        assert!(world.spawn_table.get("func_timer").is_some());
        assert!(world.spawn_table.get("func_killbox").is_some());
        assert!(world.spawn_table.get("func_rotating").is_some());
        assert!(world.spawn_table.get("func_door_rotating").is_some());
        assert!(world.spawn_table.get("func_door_secret").is_some());
        assert!(world.spawn_table.get("func_water").is_some());
        assert!(world.spawn_table.get("func_conveyor").is_some());
    }

    #[test]
    fn door_use_toggles_state() {
        let mut world = test_world();
        world.level.time = 1.0;

        let door = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(door).unwrap();
            ent.game.classname = "func_door".to_string();
            ent.game.movetype = MoveType::Push;
            let mi = MoveInfo {
                speed: 100.0,
                wait: 3.0,
                state: STATE_BOTTOM,
                start_origin: Vec3f::ZERO,
                end_origin: Vec3f::new(0.0, 0.0, 64.0),
                ..Default::default()
            };
            ent.moveinfo = Some(Box::new(mi));
        }

        // Open the door.
        door_use(&mut world, door, door, door);

        let state = world
            .entities
            .get(door)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap()
            .state;
        assert_eq!(state, STATE_UP);
    }

    #[test]
    fn button_use_fires_and_returns() {
        fn test_use(world: &mut GameWorld, key: EntityKey, _o: EntityKey, _a: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();
        world.level.framenum = 10; // run_frame derives time from framenum
        world.level.time = 1.0;

        let button = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(button).unwrap();
            ent.game.classname = "func_button".to_string();
            ent.game.target = "tgt".to_string();
            let mi = MoveInfo {
                speed: 40.0,
                accel: 40.0,
                decel: 40.0,
                wait: 1.0,
                state: STATE_BOTTOM,
                ..Default::default()
            };
            ent.moveinfo = Some(Box::new(mi));
        }

        let target = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.targetname = "tgt".to_string();
            ent.use_fn = Some(test_use);
        }

        button_use(&mut world, button, button, button);
        assert!(
            world.entities.get(target).unwrap().game.count == 0,
            "targets fire once pressed in"
        );
        world.run_frame();
        assert!(world.entities.get(target).unwrap().game.count != 0);

        let state = world
            .entities
            .get(button)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap()
            .state;
        assert_eq!(state, STATE_TOP);
        // C: pressed shows the ANIM23 frames, back out the ANIM01 ones.
        use q2_shared::constants::{EF_ANIM01, EF_ANIM23};
        let effects = |world: &GameWorld| world.entities.get(button).unwrap().state.effects;
        assert_eq!(effects(&world) & (EF_ANIM01 | EF_ANIM23), EF_ANIM23);
        run_for(&mut world, 1.5);
        assert_eq!(mover(&world, button).1, STATE_BOTTOM);
        assert_eq!(effects(&world) & (EF_ANIM01 | EF_ANIM23), EF_ANIM01);
    }

    #[test]
    fn func_button_spawns_showing_its_idle_frames() {
        let world = spawned(r#"{ "classname" "func_button" "model" "*1" }"#);
        let button = world.entities.get(by_class(&world, "func_button")).unwrap();
        assert_ne!(button.state.effects & q2_shared::constants::EF_ANIM01, 0);
    }

    #[test]
    fn func_explosive_dies_with_damage() {
        let mut world = test_world();

        let explosive = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(explosive).unwrap();
            ent.game.dmg = 100;
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

        func_explosive_die(&mut world, explosive, attacker, attacker, 100, Vec3f::ZERO);

        assert!(world.entities.get(victim).unwrap().game.health < 200);
        assert!(!world.entities.get(explosive).is_some_and(|e| e.in_use));
    }

    #[test]
    fn func_timer_fires_periodically() {
        fn test_use(world: &mut GameWorld, key: EntityKey, _o: EntityKey, _a: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();
        world.level.time = 1.0;

        let timer = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(timer).unwrap();
            ent.game.classname = "func_timer".to_string();
            ent.game.wait = 2.0;
            ent.game.target = "tgt".to_string();
        }

        let target = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.targetname = "tgt".to_string();
            ent.use_fn = Some(test_use);
        }

        // Call think twice.
        func_timer_think(&mut world, timer);
        assert_eq!(world.entities.get(target).unwrap().game.count, 1);

        func_timer_think(&mut world, timer);
        assert_eq!(world.entities.get(target).unwrap().game.count, 2);

        // nextthink should be set for next firing.
        let nt = world.entities.get(timer).unwrap().game.nextthink;
        assert!(nt > world.level.time);
    }

    /// A mover at level time 1 with `remaining` units left at `speed`.
    fn moving(remaining: f32, speed: f32) -> (GameWorld, EntityKey) {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().moveinfo = Some(Box::new(MoveInfo {
            speed,
            remaining_distance: remaining,
            dir: Vec3f::X,
            ..Default::default()
        }));
        (world, key)
    }

    #[test]
    fn move_begin_counts_frames_with_c_double_frametime() {
        // C: floor((280 / 100) / 0.1) in double is 27 (2.8f is just under
        // 2.8); the float quotient rounds to 28.
        let (mut world, key) = moving(280.0, 100.0);
        move_begin(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.moveinfo.as_ref().unwrap().remaining_distance, 10.0);
        assert_eq!(ent.game.nextthink, (1.0f64 + 27.0 * 0.1) as f32);
    }

    #[test]
    fn move_final_and_think_times_add_the_double_frametime() {
        let remaining = 10.0f32 / 7.0; // / 0.1f rounds differently from / 0.1
        let (mut world, key) = moving(remaining, 100.0);
        world.level.time = 0.3;
        move_final(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.velocity.x, (f64::from(remaining) / 0.1) as f32);
        assert_eq!(ent.game.nextthink, (f64::from(0.3f32) + 0.1) as f32);
    }

    #[test]
    fn plat_speeds_scale_by_the_double_tenth() {
        // C `ent->speed *= 0.1` is a double multiply; 9 * 0.1f rounds to a
        // different float.
        let world = spawned(
            r#"{ "classname" "func_plat" "model" "*2" "speed" "9" "accel" "13" "decel" "18" }"#,
        );
        let mi = world
            .entities
            .get(by_class(&world, "func_plat"))
            .unwrap()
            .moveinfo
            .clone()
            .unwrap();
        assert_eq!(
            (mi.speed, mi.accel, mi.decel),
            (
                (9.0f64 * 0.1) as f32,
                (13.0f64 * 0.1) as f32,
                (18.0f64 * 0.1) as f32
            )
        );
    }

    #[test]
    fn func_timer_random_over_wait_drops_to_wait_minus_the_double_frametime() {
        let world = spawned(r#"{ "classname" "func_timer" "wait" "0.1" "random" "5" }"#);
        let timer = world.entities.get(by_class(&world, "func_timer")).unwrap();
        assert_eq!(timer.game.random, (f64::from(0.1f32) - 0.1) as f32); // 1.49e-9, not 0
    }

    /// (current_speed, remaining_distance, next_speed) bits after each
    /// Think_AccelMove of a mover starting `distance` away.
    fn accel_frames(speed: f32, accel: f32, decel: f32, distance: f32) -> Vec<(u32, u32, u32)> {
        let (mut world, key) = moving(distance, speed);
        if let Some(mi) = world.entities.get_mut(key).unwrap().moveinfo.as_deref_mut() {
            mi.accel = accel;
            mi.decel = decel;
        }
        let mut frames = Vec::new();
        loop {
            think_accel_move(&mut world, key);
            let ent = world.entities.get(key).unwrap();
            let mi = ent.moveinfo.as_ref().unwrap();
            frames.push((
                mi.current_speed.to_bits(),
                mi.remaining_distance.to_bits(),
                mi.next_speed.to_bits(),
            ));
            let again = ent.think.is_some_and(|t| {
                std::ptr::fn_addr_eq(t, think_accel_move as crate::entity::ThinkFn)
            });
            if !again {
                return frames;
            }
        }
    }

    fn speeds(frames: Vec<(u32, u32, u32)>) -> Vec<(u32, u32)> {
        frames.into_iter().map(|(c, r, _)| (c, r)).collect()
    }

    #[test]
    fn accelerated_moves_match_a_c_run_frame_by_frame() {
        // From the C plat_CalcAcceleratedMove / plat_Accelerate /
        // Think_AccelMove compiled with -ffp-contract=off.
        #[rustfmt::skip]
        let default_plat = [
            (0x40a00000, 0x42f00000), (0x41200000, 0x42e60000), (0x41700000, 0x42d20000),
            (0x41a00000, 0x42b40000), (0x41a00000, 0x428c0000), (0x41a00000, 0x42480000),
            (0x41700000, 0x41f00000), (0x41200000, 0x41700000), (0x40a00000, 0x40a00000),
        ];
        assert_eq!(speeds(accel_frames(20.0, 5.0, 5.0, 120.0)), default_plat);
        #[rustfmt::skip]
        let short = [
            (0x40a00000, 0x42140000), (0x41200000, 0x42000000), (0x4131eb51, 0x41b00000),
            (0x40fcf014, 0x412e14af), (0x4039e028, 0x403e7294), (0x4039e028, 0x3d924d80),
        ];
        assert_eq!(speeds(accel_frames(20.0, 5.0, 5.0, 37.0)), short);
        #[rustfmt::skip]
        let odd = [
            (0x40066666, 0x4226cccd), (0x40866666, 0x421e6667), (0x40c99999, 0x420d999a),
            (0x40e9999a, 0x41e8ccce), (0x40e9999a, 0x41ae6668), (0x40c5a7a3, 0x41680003),
            (0x408f413c, 0x41052c32), (0x4031b5ab, 0x40762e50), (0x3f89d1bc, 0x3f88f14a),
        ];
        assert_eq!(speeds(accel_frames(7.3, 2.1, 1.7, 41.7)), odd);
        // The double 1.0 in plat_Accelerate's p2_distance rounds next_speed
        // differently from float here.
        let tenth = |v: f64| (v * 0.1) as f32;
        #[rustfmt::skip]
        let decelerating = [
            (0x3f800000, 0x41a00000, 0), (0x3fc00000, 0x41980000, 0),
            (0x3fc00000, 0x418c0000, 0), (0x3fc00000, 0x41800000, 0),
            (0x3fc00000, 0x41680000, 0), (0x3fc00000, 0x41500000, 0),
            (0x3fc00000, 0x41380000, 0), (0x3fc00000, 0x41200000, 0),
            (0x3fc00000, 0x41080000, 0), (0x3fc00000, 0x40e00000, 0),
            (0x3fc00000, 0x40b00000, 0), (0x3fc00000, 0x40800000, 0),
            (0x3fc00000, 0x40200000, 0x3f6aaaab), (0x3f6aaaab, 0x3f800000, 0),
            (0x3f6aaaab, 0x3daaaaa8, 0),
        ];
        assert_eq!(
            accel_frames(tenth(15.0), tenth(10.0), tenth(10.0), 20.0),
            decelerating
        );
    }

    #[test]
    fn plat_movement_states() {
        let mut world = test_world();
        world.level.framenum = 10; // run_frame derives time from framenum
        world.level.time = 1.0;

        let plat = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(plat).unwrap();
            ent.state.origin = Vec3f::new(0.0, 0.0, 100.0);
            let mi = MoveInfo {
                speed: 200.0,
                accel: 200.0,
                decel: 200.0,
                state: STATE_TOP,
                start_origin: Vec3f::new(0.0, 0.0, 100.0),
                end_origin: Vec3f::new(0.0, 0.0, 0.0),
                ..Default::default()
            };
            ent.moveinfo = Some(Box::new(mi));
        }

        // Go down: Move_Calc starts the move on the plat's next think.
        plat_go_down(&mut world, plat);
        world.run_frame();
        let state = world
            .entities
            .get(plat)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap()
            .state;
        assert_eq!(state, STATE_DOWN);

        // Velocity should be downward.
        let vz = world.entities.get(plat).unwrap().velocity.z;
        assert!(vz < 0.0);
    }

    #[test]
    fn func_rotating_spins() {
        // C SP_func_rotating spins at spawn only with START_ON.
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.speed = 360.0;
        world.entities.get_mut(key).unwrap().game.spawnflags = ROTATING_START_ON;

        sp_func_rotating(&mut world.entities, key, &HashMap::new());

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.avelocity.y, 360.0);
    }

    #[test]
    fn func_rotating_waits_for_use_and_honours_axis_and_reverse_flags() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.spawnflags = ROTATING_X_AXIS | ROTATING_REVERSE;
        sp_func_rotating(&mut world.entities, key, &HashMap::new());
        assert_eq!(world.entities.get(key).unwrap().avelocity, Vec3f::ZERO);

        rotating_use(&mut world, key, key, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            ent.avelocity,
            Vec3f::new(0.0, 0.0, -100.0),
            "speed defaults to 100"
        );
        assert!(ent.blocked.is_some());
        rotating_use(&mut world, key, key, key);
        assert_eq!(world.entities.get(key).unwrap().avelocity, Vec3f::ZERO);
    }

    #[test]
    fn func_timer_is_off_until_used_unless_start_on() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "t",
            concat!(
                "{ \"classname\" \"worldspawn\" }\n",
                "{ \"classname\" \"func_timer\" \"targetname\" \"off\" }\n",
                "{ \"classname\" \"func_timer\" \"targetname\" \"on\" \"spawnflags\" \"1\" \"wait\" \"2\" }",
            ),
            "",
        );
        let off = world.find_by_targetname(None, "off").unwrap();
        let on = world.find_by_targetname(None, "on").unwrap();
        assert_eq!(world.entities.get(off).unwrap().game.nextthink, 0.0);
        // 1 + pausetime 0 + delay 0 + wait 2, random 0.
        assert!((world.entities.get(on).unwrap().game.nextthink - 3.0).abs() < 1e-5);

        func_timer_use(&mut world, off, off, off);
        assert!(world.entities.get(off).unwrap().game.nextthink > 0.0);
        func_timer_use(&mut world, off, off, off);
        assert_eq!(world.entities.get(off).unwrap().game.nextthink, 0.0);
    }

    #[test]
    fn func_explosive_blasts_from_its_centre_with_dmg_plus_40() {
        let mut world =
            spawned("{ \"classname\" \"func_explosive\" \"model\" \"*1\" \"dmg\" \"50\" }");
        let brush = by_class(&world, "func_explosive");
        let victim = world.spawn().unwrap();
        {
            let v = world.entities.get_mut(victim).unwrap();
            // 70 units from the brush centre (8, 32, 48): within dmg + 40.
            v.state.origin = Vec3f::new(78.0, 32.0, 48.0);
            v.game.health = 100;
            v.game.takedamage = TakeDamage::Yes;
            v.solid = Solid::Bbox; // findradius skips SOLID_NOT
        }
        let p = player(&mut world);
        func_explosive_die(&mut world, brush, p, p, 100, Vec3f::ZERO);
        assert!(world.entities.get(victim).unwrap().game.health < 100);
        assert!(!world.entities.is_live(brush));
    }

    #[test]
    fn killbox_telefrags_what_is_inside_and_is_hidden_from_clients() {
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities(
            "t",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"func_killbox\" \"targetname\" \"k\" \"count\" \"1\" }",
            "",
        );
        let kb = by_class(&world, "func_killbox");
        assert_eq!(world.entities.get(kb).unwrap().solid, Solid::Not);
        assert_ne!(
            world.entities.get(kb).unwrap().svflags & q2_shared::constants::SVF_NOCLIENT,
            0
        );
        let victim = world.spawn().unwrap();
        {
            let v = world.entities.get_mut(victim).unwrap();
            v.game.health = 100;
            v.game.takedamage = TakeDamage::Yes;
            v.solid = Solid::Bbox; // findradius skips SOLID_NOT
            v.solid = Solid::Bbox;
        }
        *gi.next_trace.borrow_mut() = Some(Trace {
            startsolid: true,
            ent_index: world.number_of(victim),
            ..Default::default()
        });
        func_killbox_use(&mut world, kb, kb, kb);
        assert!(world.entities.get(victim).unwrap().game.health <= 0);
        assert!(
            world.entities.get(kb).unwrap().think.is_some(),
            "removed after its last use"
        );
    }
    // -- teams and SV_Push --

    #[test]
    fn doors_sharing_a_team_key_form_one_team_and_open_together() {
        let mut world = spawned(concat!(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" \"team\" \"d\" }\n",
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-2\" \"team\" \"d\" }",
        ));
        let master = by_class(&world, "func_door");
        let slave = world.find_by_classname(Some(master), "func_door").unwrap();
        let s = world.entities.get(slave).unwrap();
        assert!(s.game.flags.contains(EntityFlags::TEAMSLAVE));
        assert_eq!(s.game.teammaster, Some(master));
        assert_eq!(
            world.entities.get(master).unwrap().game.teamchain,
            Some(slave)
        );

        world.run_frame();
        let p = player(&mut world);
        door_use(&mut world, master, p, p);
        run_for(&mut world, 1.5);
        assert!(mover(&world, master).0.z > 80.0);
        assert!(
            mover(&world, slave).0.z < -80.0,
            "the slave moves with its master"
        );
        // One touch field for the whole team.
        assert!(touch_field(&world, slave).is_none());
    }

    #[test]
    fn plat_carries_the_entity_riding_it() {
        let mut world = test_world();
        let plat = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(plat).unwrap();
            e.game.movetype = MoveType::Push;
            e.solid = Solid::Bsp;
            e.maxs = Vec3f::new(64.0, 64.0, 8.0);
            e.velocity = Vec3f::new(0.0, 0.0, 100.0);
        }
        world.link_entity(plat);
        let rider = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(rider).unwrap();
            e.game.movetype = MoveType::Step;
            e.game.ground_entity = Some(plat);
            e.state.origin = Vec3f::new(32.0, 32.0, 32.0);
        }
        world.link_entity(rider);
        world.physics_pusher(plat);
        assert_eq!(world.entities.get(plat).unwrap().state.origin.z, 10.0);
        assert_eq!(world.entities.get(rider).unwrap().state.origin.z, 42.0);
    }

    #[test]
    fn blocked_pusher_backs_out_and_calls_its_blocked_function() {
        fn note_block(world: &mut GameWorld, key: EntityKey, other: EntityKey) {
            world.entities.get_mut(key).unwrap().game.enemy = Some(other);
        }
        let (mut world, gi) = test_world_recording();
        let plat = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(plat).unwrap();
            e.game.movetype = MoveType::Push;
            e.solid = Solid::Bsp;
            e.velocity = Vec3f::new(0.0, 0.0, 100.0);
            e.game.nextthink = 5.0;
            e.blocked = Some(note_block);
        }
        world.link_entity(plat);
        let rider = world.spawn().unwrap();
        world.entities.get_mut(rider).unwrap().game.movetype = MoveType::Step;
        world.entities.get_mut(rider).unwrap().game.ground_entity = Some(plat);
        world.link_entity(rider);
        // The rider is stuck both at the pushed and at its old position.
        let stuck = Trace {
            startsolid: true,
            ..Default::default()
        };
        gi.queued_traces.borrow_mut().extend([stuck.clone(), stuck]);
        world.physics_pusher(plat);
        let e = world.entities.get(plat).unwrap();
        assert_eq!(e.state.origin, Vec3f::ZERO, "move backed out");
        assert_eq!(e.game.enemy, Some(rider));
        assert!(
            (e.game.nextthink - 5.1).abs() < 1e-5,
            "think delayed a frame"
        );
        assert_eq!(world.entities.get(rider).unwrap().state.origin, Vec3f::ZERO);
    }

    #[test]
    fn door_blocked_while_closing_reopens_and_crushes_items() {
        let mut world =
            spawned("{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" }");
        let door = by_class(&world, "func_door");
        world
            .entities
            .get_mut(door)
            .unwrap()
            .moveinfo
            .as_mut()
            .unwrap()
            .state = STATE_DOWN;
        let p = player(&mut world);
        world.entities.get_mut(p).unwrap().game.takedamage = TakeDamage::Aim;
        door_blocked(&mut world, door, p);
        assert_eq!(mover(&world, door).1, STATE_UP);
        assert!(
            world.entities.get(p).unwrap().game.health < 100,
            "crush damage"
        );

        let item = world.spawn().unwrap();
        door_blocked(&mut world, door, item);
        assert!(!world.entities.is_live(item));
    }

    /// Dying turns the func_plat round, as a deathtarget re-using it could.
    fn die_reversing_the_plat(
        world: &mut GameWorld,
        _key: EntityKey,
        _inflictor: EntityKey,
        _attacker: EntityKey,
        _damage: i32,
        _point: Vec3f,
    ) {
        let plat = by_class(world, "func_plat");
        moveinfo_mut(world, plat).unwrap().state = STATE_DOWN;
    }

    #[test]
    fn blocked_plat_reverses_on_its_state_after_the_crush_damage() {
        // C plat_blocked reads moveinfo.state after T_Damage.
        let mut world = spawned(r#"{ "classname" "func_plat" "model" "*2" }"#);
        let plat = by_class(&world, "func_plat");
        moveinfo_mut(&mut world, plat).unwrap().state = STATE_UP;
        let victim = player(&mut world);
        {
            let v = world.entities.get_mut(victim).unwrap();
            v.game.takedamage = TakeDamage::Aim;
            v.game.health = 1;
            v.die = Some(die_reversing_the_plat);
        }
        plat_blocked(&mut world, plat, victim);
        assert_eq!(mover(&world, plat).1, STATE_UP, "plat_go_up from DOWN");
    }

    /// Dying sends the func_door up, as a deathtarget re-using it could.
    fn die_opening_the_door(
        world: &mut GameWorld,
        _key: EntityKey,
        _inflictor: EntityKey,
        _attacker: EntityKey,
        _damage: i32,
        _point: Vec3f,
    ) {
        let door = by_class(world, "func_door");
        moveinfo_mut(world, door).unwrap().state = STATE_UP;
    }

    #[test]
    fn blocked_door_reverses_on_its_state_after_the_crush_damage() {
        // C door_blocked reads moveinfo.state after T_Damage.
        let mut world = spawned(r#"{ "classname" "func_door" "model" "*1" "angle" "-1" }"#);
        let door = by_class(&world, "func_door");
        moveinfo_mut(&mut world, door).unwrap().state = STATE_DOWN;
        let victim = player(&mut world);
        {
            let v = world.entities.get_mut(victim).unwrap();
            v.game.takedamage = TakeDamage::Aim;
            v.game.health = 1;
            v.die = Some(die_opening_the_door);
        }
        door_blocked(&mut world, door, victim);
        assert_eq!(mover(&world, door).1, STATE_DOWN, "door_go_down from UP");
    }

    // -- func_door_rotating --

    fn angles_of(world: &GameWorld, key: EntityKey) -> Vec3f {
        world.entities.get(key).unwrap().state.angles
    }

    fn near(a: Vec3f, b: Vec3f) -> bool {
        (a - b).abs().max_element() < 1e-2
    }

    fn use_it(world: &mut GameWorld, key: EntityKey, activator: EntityKey) {
        let use_fn = world.entities.get(key).unwrap().use_fn.expect("use");
        use_fn(world, key, activator, activator);
    }

    #[test]
    fn rotating_door_ignores_its_angle_key_and_turns_distance_about_the_x_axis() {
        // demo2's t5 door pair (*49), plus an `angle` key C clears.
        let world = spawned(concat!(
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"origin\" \"-888 180 -156\" ",
            "\"distance\" \"-90\" \"spawnflags\" \"64\" \"speed\" \"35\" \"wait\" \"-1\" ",
            "\"targetname\" \"t5\" \"angle\" \"45\" }",
        ));
        let door = by_class(&world, "func_door_rotating");
        let ent = world.entities.get(door).unwrap();
        let mi = ent.moveinfo.as_ref().unwrap();
        let origin = Vec3f::new(-888.0, 180.0, -156.0);
        assert_eq!(ent.state.angles, Vec3f::ZERO);
        assert_eq!(ent.game.movedir, Vec3f::new(0.0, 0.0, 1.0), "X_AXIS rolls");
        assert_eq!(mi.start_angles, Vec3f::ZERO);
        assert_eq!(mi.end_angles, Vec3f::new(0.0, 0.0, -90.0));
        assert_eq!((mi.start_origin, mi.end_origin), (origin, origin));
        assert_eq!(mi.distance, -90.0);
        assert_eq!((mi.state, mi.speed, mi.wait), (STATE_BOTTOM, 35.0, -1.0));
        assert_eq!(ent.game.movetype, MoveType::Push);
        assert_eq!(ent.solid, Solid::Bsp);
        assert!(ent.use_fn.is_some() && ent.blocked.is_some());
    }

    #[test]
    fn rotating_door_defaults_to_90_degrees_of_yaw_and_reverse_negates_it() {
        let world = spawned(concat!(
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"spawnflags\" \"2\" }\n",
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"spawnflags\" \"144\" }",
        ));
        let yaw = by_class(&world, "func_door_rotating");
        let pitch = world
            .find_by_classname(Some(yaw), "func_door_rotating")
            .unwrap();
        let e = world.entities.get(yaw).unwrap();
        let mi = e.moveinfo.as_ref().unwrap();
        assert_eq!(e.game.movedir, Vec3f::new(0.0, -1.0, 0.0));
        assert_eq!(mi.end_angles, Vec3f::new(0.0, -90.0, 0.0));
        assert_eq!((mi.speed, mi.accel, mi.decel), (100.0, 100.0, 100.0));
        assert_eq!((mi.wait, e.game.dmg), (3.0, 2));
        assert_eq!(e.state.effects & EF_ANIM_ALL, 0);
        // demo2's *51: Y_AXIS | ANIMATED.
        let p = world.entities.get(pitch).unwrap();
        assert_eq!(p.game.movedir, Vec3f::new(1.0, 0.0, 0.0));
        assert_eq!(
            p.moveinfo.as_ref().unwrap().end_angles,
            Vec3f::new(90.0, 0.0, 0.0)
        );
        assert_ne!(p.state.effects & EF_ANIM_ALL, 0);
    }

    #[test]
    fn func_door_animates_its_texture_with_spawnflags_16_and_64() {
        // C SP_func_door: 16 is EF_ANIM_ALL, 64 EF_ANIM_ALLFAST.
        for (flags, effects) in [(0, 0), (16, EF_ANIM_ALL), (64, EF_ANIM_ALLFAST)] {
            let world = spawned(&format!(
                "{{ \"classname\" \"func_door\" \"model\" \"*1\" \"spawnflags\" \"{flags}\" }}"
            ));
            let door = world.entities.get(by_class(&world, "func_door")).unwrap();
            assert_eq!(
                door.state.effects & (EF_ANIM_ALL | EF_ANIM_ALLFAST),
                effects
            );
        }
    }

    #[test]
    fn rotating_door_starting_open_begins_turned_and_swings_back_the_other_way() {
        let world = spawned(
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"spawnflags\" \"1\" \"distance\" \"90\" }",
        );
        let e = world
            .entities
            .get(by_class(&world, "func_door_rotating"))
            .unwrap();
        let mi = e.moveinfo.as_ref().unwrap();
        assert_eq!(e.state.angles, Vec3f::new(0.0, 90.0, 0.0));
        assert_eq!(mi.start_angles, Vec3f::new(0.0, 90.0, 0.0));
        assert_eq!(mi.end_angles, Vec3f::ZERO);
        assert_eq!(e.game.movedir, Vec3f::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn used_rotating_door_swings_open_then_swings_shut_after_wait() {
        let mut world = spawned(
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"speed\" \"90\" \"targetname\" \"d\" }",
        );
        let door = by_class(&world, "func_door_rotating");
        world.run_frame();
        let p = player(&mut world);
        use_it(&mut world, door, p);
        assert_eq!(mover(&world, door).1, STATE_UP);
        run_for(&mut world, 0.5);
        let half = angles_of(&world, door).y;
        assert!(half > 30.0 && half < 60.0, "mid-swing: {half}");

        run_for(&mut world, 0.8);
        assert_eq!(mover(&world, door).1, STATE_TOP);
        assert!(near(angles_of(&world, door), Vec3f::new(0.0, 90.0, 0.0)));
        assert_eq!(world.entities.get(door).unwrap().avelocity, Vec3f::ZERO);
        assert_eq!(mover(&world, door).0, Vec3f::ZERO, "rotates in place");

        run_for(&mut world, 3.0 + 1.3);
        assert_eq!(mover(&world, door).1, STATE_BOTTOM);
        assert!(near(angles_of(&world, door), Vec3f::ZERO));
    }

    #[test]
    fn untargeted_rotating_door_opens_from_its_touch_field() {
        let mut world = spawned("{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" }");
        let door = by_class(&world, "func_door_rotating");
        world.run_frame();
        let field = touch_field(&world, door).expect("trigger spawned");
        let p = player(&mut world);
        touch_door_trigger(&mut world, field, p, None, None);
        run_for(&mut world, 1.2);
        assert_eq!(mover(&world, door).1, STATE_TOP);
        assert!(near(angles_of(&world, door), Vec3f::new(0.0, 90.0, 0.0)));
    }

    #[test]
    fn rotating_door_blocked_while_opening_swings_back_shut() {
        let mut world = spawned(
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"targetname\" \"d\" }",
        );
        let door = by_class(&world, "func_door_rotating");
        world.run_frame();
        {
            let e = world.entities.get_mut(door).unwrap();
            e.state.angles.y = 45.0;
            e.moveinfo.as_mut().unwrap().state = STATE_UP;
        }
        let p = player(&mut world);
        world.entities.get_mut(p).unwrap().game.takedamage = TakeDamage::Aim;
        let blocked = world.entities.get(door).unwrap().blocked.unwrap();
        blocked(&mut world, door, p);
        assert_eq!(mover(&world, door).1, STATE_DOWN);
        run_for(&mut world, 1.0);
        assert_eq!(mover(&world, door).1, STATE_BOTTOM);
        assert!(near(angles_of(&world, door), Vec3f::ZERO));
    }

    #[test]
    fn shot_rotating_door_swings_open() {
        let mut world =
            spawned("{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"health\" \"5\" }");
        let door = by_class(&world, "func_door_rotating");
        world.run_frame();
        assert!(touch_field(&world, door).is_none(), "health doors are shot");
        let e = world.entities.get(door).unwrap();
        assert_eq!((e.game.takedamage, e.game.max_health), (TakeDamage::Yes, 5));
        let die = e.die.expect("door_killed");
        let p = player(&mut world);
        die(&mut world, door, p, p, 5, Vec3f::ZERO);
        run_for(&mut world, 1.2);
        assert_eq!(mover(&world, door).1, STATE_TOP);
    }

    // -- func_water --

    #[test]
    fn func_water_starting_open_sits_lowered_and_behaves_as_a_door() {
        // demo3's *21.
        let world = spawned(concat!(
            "{ \"classname\" \"func_water\" \"model\" \"*1\" \"wait\" \"60\" \"speed\" \"50\" ",
            "\"lip\" \"32\" \"angle\" \"-2\" \"spawnflags\" \"1\" \"targetname\" \"t22\" }",
        ));
        let water = by_class(&world, "func_door");
        let e = world.entities.get(water).unwrap();
        let mi = e.moveinfo.as_ref().unwrap();
        assert_eq!(
            e.state.origin,
            Vec3f::new(0.0, 0.0, -64.0),
            "96 down less 32"
        );
        assert_eq!(mi.start_origin, Vec3f::new(0.0, 0.0, -64.0));
        assert_eq!(mi.end_origin, Vec3f::ZERO);
        assert_eq!((mi.speed, mi.accel, mi.decel), (50.0, 50.0, 50.0));
        assert_eq!((mi.wait, mi.state), (60.0, STATE_BOTTOM));
        assert_eq!(e.game.spawnflags & DOOR_TOGGLE, 0);
        assert_eq!(e.state.angles, Vec3f::ZERO);
        assert!(e.blocked.is_none() && e.think.is_none());
        assert_eq!((e.solid, e.game.movetype), (Solid::Bsp, MoveType::Push));
    }

    #[test]
    fn func_water_defaults_to_a_slow_toggle_with_no_lip() {
        let mut world =
            spawned("{ \"classname\" \"func_water\" \"model\" \"*1\" \"targetname\" \"w\" }");
        let water = by_class(&world, "func_door");
        let e = world.entities.get(water).unwrap();
        let mi = e.moveinfo.as_ref().unwrap();
        assert_eq!(mi.end_origin, Vec3f::new(16.0, 0.0, 0.0), "east, no lip");
        assert_eq!((mi.speed, mi.wait), (25.0, -1.0));
        assert_ne!(e.game.spawnflags & DOOR_TOGGLE, 0);

        let p = player(&mut world);
        use_it(&mut world, water, p);
        run_for(&mut world, 1.0);
        assert_eq!(
            mover(&world, water),
            (Vec3f::new(16.0, 0.0, 0.0), STATE_TOP)
        );
        run_for(&mut world, 5.0);
        assert_eq!(mover(&world, water).1, STATE_TOP, "a toggle never returns");
        use_it(&mut world, water, p);
        run_for(&mut world, 1.0);
        assert_eq!(mover(&world, water), (Vec3f::ZERO, STATE_BOTTOM));
    }

    // -- func_door_secret --

    #[test]
    fn secret_door_slides_sideways_by_its_width_then_back_by_its_length() {
        let world = spawned(concat!(
            "{ \"classname\" \"func_door_secret\" \"model\" \"*1\" }\n",
            "{ \"classname\" \"func_door_secret\" \"model\" \"*1\" \"spawnflags\" \"2\" ",
            "\"targetname\" \"s\" }\n",
            "{ \"classname\" \"func_door_secret\" \"model\" \"*1\" \"spawnflags\" \"5\" ",
            "\"targetname\" \"s\" \"angle\" \"90\" }",
        ));
        let right = by_class(&world, "func_door");
        let left = world.find_by_classname(Some(right), "func_door").unwrap();
        let down = world.find_by_classname(Some(left), "func_door").unwrap();
        let positions = |k: EntityKey| {
            let mi = world.entities.get(k).unwrap().moveinfo.clone().unwrap();
            (mi.start_origin, mi.end_origin)
        };
        // Yaw 0: forward +x (length 16), right -y (width 64).
        let (p1, p2) = positions(right);
        assert!(near(p1, Vec3f::new(0.0, -64.0, 0.0)), "{p1:?}");
        assert!(near(p2, Vec3f::new(16.0, -64.0, 0.0)), "{p2:?}");
        let (p1, _) = positions(left);
        assert!(near(p1, Vec3f::new(0.0, 64.0, 0.0)), "1st_left: {p1:?}");
        // Yaw 90: forward +y (length 64); 1st_down drops by the height.
        let (p1, p2) = positions(down);
        assert!(near(p1, Vec3f::new(0.0, 0.0, -96.0)), "1st_down: {p1:?}");
        assert!(near(p2, Vec3f::new(0.0, 64.0, -96.0)), "{p2:?}");

        let e = world.entities.get(right).unwrap();
        assert_eq!(e.state.angles, Vec3f::ZERO);
        let mi = e.moveinfo.as_ref().unwrap();
        assert_eq!((mi.speed, mi.accel, mi.decel), (50.0, 50.0, 50.0));
        assert_eq!((e.game.wait, e.game.dmg), (5.0, 2));
        assert!(e.blocked.is_some() && e.use_fn.is_some());
    }

    #[test]
    fn shootable_secret_door_opens_holds_closes_and_becomes_shootable_again() {
        let mut world = spawned("{ \"classname\" \"func_door_secret\" \"model\" \"*1\" }");
        let door = by_class(&world, "func_door");
        let e = world.entities.get(door).unwrap();
        assert_eq!((e.game.takedamage, e.game.health), (TakeDamage::Yes, 0));
        let die = e.die.expect("door_secret_die");
        let p = player(&mut world);
        die(&mut world, door, p, p, 1, Vec3f::ZERO);
        assert_eq!(
            world.entities.get(door).unwrap().game.takedamage,
            TakeDamage::No
        );

        let at = |w: &GameWorld| w.entities.get(door).unwrap().state.origin;
        run_for(&mut world, 2.0); // 64 units at 50/s, then a 1s pause
        assert!(
            near(at(&world), Vec3f::new(0.0, -64.0, 0.0)),
            "{:?}",
            at(&world)
        );
        run_for(&mut world, 1.5); // 16 units back
        assert!(
            near(at(&world), Vec3f::new(16.0, -64.0, 0.0)),
            "{:?}",
            at(&world)
        );
        // Used again while open: ignored (it is not at its origin).
        use_it(&mut world, door, p);
        run_for(&mut world, 4.0);
        assert!(
            near(at(&world), Vec3f::new(16.0, -64.0, 0.0)),
            "holds for wait 5"
        );
        run_for(&mut world, 1.5 + 1.0 + 1.5);
        assert!(near(at(&world), Vec3f::ZERO), "{:?}", at(&world));
        let e = world.entities.get(door).unwrap();
        assert_eq!((e.game.takedamage, e.game.health), (TakeDamage::Yes, 0));
    }

    #[test]
    fn targeted_secret_door_is_not_shootable_and_wait_minus_one_holds_it_open() {
        let mut world = spawned(
            "{ \"classname\" \"func_door_secret\" \"model\" \"*1\" \"targetname\" \"s\" \"wait\" \"-1\" }",
        );
        let door = by_class(&world, "func_door");
        let e = world.entities.get(door).unwrap();
        assert_eq!(e.game.takedamage, TakeDamage::No);
        assert!(e.die.is_none());
        let p = player(&mut world);
        use_it(&mut world, door, p);
        run_for(&mut world, 20.0);
        let e = world.entities.get(door).unwrap();
        assert!(near(e.state.origin, Vec3f::new(16.0, -64.0, 0.0)));
        assert_eq!(e.game.takedamage, TakeDamage::No);
    }

    #[test]
    fn always_shoot_secret_door_is_shootable_though_targeted() {
        let world = spawned(
            "{ \"classname\" \"func_door_secret\" \"model\" \"*1\" \"targetname\" \"s\" \"spawnflags\" \"1\" }",
        );
        let e = world.entities.get(by_class(&world, "func_door")).unwrap();
        assert_eq!(e.game.takedamage, TakeDamage::Yes);
        assert!(e.die.is_some());
    }

    #[test]
    fn secret_door_blocked_removes_debris_and_hurts_players_twice_a_second() {
        let mut world = spawned("{ \"classname\" \"func_door_secret\" \"model\" \"*1\" }");
        let door = by_class(&world, "func_door");
        let blocked = world.entities.get(door).unwrap().blocked.unwrap();
        let item = world.spawn().unwrap();
        blocked(&mut world, door, item);
        assert!(!world.entities.is_live(item));

        let p = player(&mut world);
        world.entities.get_mut(p).unwrap().game.takedamage = TakeDamage::Aim;
        blocked(&mut world, door, p);
        blocked(&mut world, door, p);
        assert_eq!(world.entities.get(p).unwrap().game.health, 98, "dmg 2 once");
        world.level.time += 0.5;
        blocked(&mut world, door, p);
        assert_eq!(world.entities.get(p).unwrap().game.health, 96);
    }

    // -- func_conveyor --

    #[test]
    fn conveyor_off_at_spawn_turns_on_once_unless_it_toggles() {
        let mut world = spawned(concat!(
            "{ \"classname\" \"func_conveyor\" \"model\" \"*1\" \"speed\" \"50\" }\n",
            "{ \"classname\" \"func_conveyor\" \"model\" \"*1\" \"spawnflags\" \"2\" }",
        ));
        let once = by_class(&world, "func_conveyor");
        let toggle = world
            .find_by_classname(Some(once), "func_conveyor")
            .unwrap();
        let speed = |w: &GameWorld, k: EntityKey| {
            let e = w.entities.get(k).unwrap();
            (e.game.speed, e.game.count)
        };
        let e = world.entities.get(once).unwrap();
        assert_eq!((e.solid, e.game.movetype), (Solid::Bsp, MoveType::None));
        assert_eq!(speed(&world, once), (0.0, 50));
        let p = player(&mut world);
        use_it(&mut world, once, p);
        assert_eq!(speed(&world, once), (50.0, 0), "on; count spent");
        use_it(&mut world, once, p);
        use_it(&mut world, once, p);
        assert_eq!(speed(&world, once).0, 0.0, "nothing left to turn on");

        assert_eq!(speed(&world, toggle), (0.0, 100));
        use_it(&mut world, toggle, p);
        use_it(&mut world, toggle, p);
        use_it(&mut world, toggle, p);
        assert_eq!(speed(&world, toggle), (100.0, 100));
    }

    #[test]
    fn conveyor_starting_on_runs_at_its_speed() {
        let mut world =
            spawned("{ \"classname\" \"func_conveyor\" \"model\" \"*1\" \"spawnflags\" \"1\" }");
        let belt = by_class(&world, "func_conveyor");
        assert_eq!(world.entities.get(belt).unwrap().game.speed, 100.0);
        let p = player(&mut world);
        use_it(&mut world, belt, p);
        let e = world.entities.get(belt).unwrap();
        assert_eq!((e.game.speed, e.game.spawnflags & 1), (0.0, 0));
    }

    // -- func_train --

    const LOOP: &str = concat!(
        "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"target\" \"p2\" \"origin\" \"100 0 0\" \"wait\" \"2\" }\n",
        "{ \"classname\" \"path_corner\" \"targetname\" \"p2\" \"target\" \"p1\" \"origin\" \"100 200 0\" \"wait\" \"2\" }\n",
    );

    fn train_world(train_keys: &str, corners: &str) -> (GameWorld, EntityKey) {
        let world = spawned(&format!(
            "{corners}{{ \"classname\" \"func_train\" \"model\" \"*3\" \"target\" \"p1\" {train_keys} }}"
        ));
        let train = by_class(&world, "func_train");
        (world, train)
    }

    /// Where the train sits at corner `c`: its mins on the corner.
    fn at_corner(c: Vec3f) -> Vec3f {
        c - CAR_MINS
    }

    fn origin_of(world: &GameWorld, key: EntityKey) -> Vec3f {
        world.entities.get(key).unwrap().state.origin
    }

    #[test]
    fn train_spawns_unrotated_and_starts_from_its_first_corner_on_the_second_frame() {
        let (mut world, train) = train_world("\"angle\" \"90\"", LOOP);
        let e = world.entities.get(train).unwrap();
        assert_eq!(e.state.angles, Vec3f::ZERO);
        assert_eq!(
            (e.game.dmg, e.solid, e.game.movetype),
            (100, Solid::Bsp, MoveType::Push)
        );
        let mi = e.moveinfo.as_ref().unwrap();
        assert_eq!((mi.speed, mi.accel, mi.decel), (100.0, 100.0, 100.0));
        assert!(e.blocked.is_some() && e.use_fn.is_some());
        assert_eq!(e.state.origin, Vec3f::ZERO, "corners may not exist yet");

        world.run_frame();
        let e = world.entities.get(train).unwrap();
        assert_eq!(e.state.origin, at_corner(Vec3f::new(100.0, 0.0, 0.0)));
        assert_eq!(e.game.target, "p2", "heads for the corner's target");
        assert_ne!(e.game.spawnflags & 1, 0, "untargeted trains start on");
    }

    #[test]
    fn untargeted_train_drives_its_loop_and_pauses_at_corners_with_a_wait() {
        let (mut world, train) = train_world("", LOOP);
        let (p1, p2) = (
            at_corner(Vec3f::new(100.0, 0.0, 0.0)),
            at_corner(Vec3f::new(100.0, 200.0, 0.0)),
        );
        run_for(&mut world, 1.2);
        let mid = origin_of(&world, train);
        assert!(mid.y > 50.0 && mid.y < 150.0 && mid.x == p1.x, "{mid:?}");
        run_for(&mut world, 1.3); // 200 units at 100/s
        assert!(near(origin_of(&world, train), p2));
        assert_eq!(world.entities.get(train).unwrap().velocity, Vec3f::ZERO);
        run_for(&mut world, 1.5);
        assert!(near(origin_of(&world, train), p2), "waits 2s at p2");
        run_for(&mut world, 3.0);
        assert!(
            near(origin_of(&world, train), p1),
            "{:?}",
            origin_of(&world, train)
        );
    }

    #[test]
    fn train_keeps_the_corner_it_drives_to_in_target_ent_not_goalentity() {
        let (mut world, train) = train_world("", LOOP);
        run_for(&mut world, 1.2);
        let p2 = world.find_by_targetname(None, "p2").unwrap();
        let e = world.entities.get(train).unwrap();
        assert_eq!(e.game.target_ent, Some(p2));
        assert_eq!(e.game.goalentity, None);
    }

    #[test]
    fn targeted_train_waits_for_use_and_ignores_later_uses_unless_toggle() {
        let (mut world, train) = train_world("\"targetname\" \"t91\"", LOOP);
        run_for(&mut world, 1.0);
        let p1 = at_corner(Vec3f::new(100.0, 0.0, 0.0));
        assert_eq!(origin_of(&world, train), p1, "parked until used");
        let p = player(&mut world);
        use_it(&mut world, train, p);
        assert_eq!(world.entities.get(train).unwrap().game.activator, Some(p));
        run_for(&mut world, 0.5);
        let moving = world.entities.get(train).unwrap().velocity;
        assert_ne!(moving, Vec3f::ZERO);
        // Move_Calc would clear the velocity until the train's next think.
        world.level.current_entity = None;
        use_it(&mut world, train, p);
        assert_eq!(
            world.entities.get(train).unwrap().velocity,
            moving,
            "a second use is ignored"
        );
    }

    #[test]
    fn toggle_train_stops_when_used_and_resumes_toward_the_same_corner() {
        let (mut world, train) = train_world("\"targetname\" \"t\" \"spawnflags\" \"2\"", LOOP);
        world.run_frame();
        let p = player(&mut world);
        use_it(&mut world, train, p);
        run_for(&mut world, 0.5);
        use_it(&mut world, train, p);
        let e = world.entities.get(train).unwrap();
        assert_eq!((e.velocity, e.game.nextthink), (Vec3f::ZERO, 0.0));
        assert_eq!(e.game.spawnflags & 1, 0);
        let stopped = e.state.origin;
        run_for(&mut world, 1.0);
        assert_eq!(origin_of(&world, train), stopped);
        use_it(&mut world, train, p);
        run_for(&mut world, 2.0);
        assert!(near(
            origin_of(&world, train),
            at_corner(Vec3f::new(100.0, 200.0, 0.0))
        ));
    }

    #[test]
    fn train_fires_a_corners_pathtarget_on_arrival_and_keeps_its_target() {
        let corners = concat!(
            "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"target\" \"p2\" \"origin\" \"100 0 0\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p2\" \"target\" \"p1\" \"origin\" \"100 20 0\" ",
            "\"pathtarget\" \"belt\" \"wait\" \"-1\" }\n",
            "{ \"classname\" \"func_conveyor\" \"model\" \"*1\" \"targetname\" \"belt\" }\n",
        );
        let (mut world, _train) = train_world("", corners);
        let belt = by_class(&world, "func_conveyor");
        run_for(&mut world, 0.2);
        assert_eq!(world.entities.get(belt).unwrap().game.speed, 0.0);
        run_for(&mut world, 0.5);
        assert_eq!(
            world.entities.get(belt).unwrap().game.speed,
            100.0,
            "used once"
        );
        let corner = world.find_by_targetname(None, "p2").unwrap();
        assert_eq!(world.entities.get(corner).unwrap().game.target, "p1");
    }

    #[test]
    fn corner_with_wait_minus_one_parks_the_train_for_good_unless_it_toggles() {
        let corners = concat!(
            "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"target\" \"p2\" \"origin\" \"100 0 0\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p2\" \"target\" \"p3\" \"origin\" \"100 20 0\" \"wait\" \"-1\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p3\" \"target\" \"p1\" \"origin\" \"100 40 0\" \"wait\" \"5\" }\n",
        );
        let p2 = at_corner(Vec3f::new(100.0, 20.0, 0.0));
        let p3 = at_corner(Vec3f::new(100.0, 40.0, 0.0));

        let (mut world, train) = train_world("", corners);
        run_for(&mut world, 1.0);
        assert!(near(origin_of(&world, train), p2));
        let p = player(&mut world);
        use_it(&mut world, train, p);
        run_for(&mut world, 1.0);
        assert!(
            near(origin_of(&world, train), p2),
            "START_ON stays set: use is ignored"
        );

        let (mut world, train) = train_world("\"spawnflags\" \"2\"", corners);
        run_for(&mut world, 1.0);
        assert!(near(origin_of(&world, train), p2));
        let e = world.entities.get(train).unwrap();
        assert_eq!((e.game.spawnflags & 1, e.velocity), (0, Vec3f::ZERO));
        let p = player(&mut world);
        use_it(&mut world, train, p);
        run_for(&mut world, 1.0);
        assert!(
            near(origin_of(&world, train), p3),
            "resumes toward the next corner"
        );
    }

    #[test]
    fn teleport_corner_moves_the_train_there_at_once_and_flags_the_event() {
        let corners = concat!(
            "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"target\" \"p2\" \"origin\" \"100 0 0\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p2\" \"target\" \"p3\" \"origin\" \"900 0 0\" \"spawnflags\" \"1\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p3\" \"target\" \"p1\" \"origin\" \"900 100 0\" \"wait\" \"5\" }\n",
        );
        let (mut world, train) = train_world("", corners);
        run_for(&mut world, 0.2); // find, then train_next
        let e = world.entities.get(train).unwrap();
        let jumped = at_corner(Vec3f::new(900.0, 0.0, 0.0));
        assert_eq!(e.state.origin, jumped);
        assert_eq!(e.state.old_origin, jumped);
        assert_eq!(e.state.event, q2_shared::constants::EV_OTHER_TELEPORT);
        assert_eq!(e.game.target, "p1", "already heading past p3");
        run_for(&mut world, 1.2);
        assert!(near(
            origin_of(&world, train),
            at_corner(Vec3f::new(900.0, 100.0, 0.0))
        ));
    }

    #[test]
    fn connected_teleport_corners_leave_the_train_at_the_first() {
        let corners = concat!(
            "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"target\" \"p2\" \"origin\" \"100 0 0\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p2\" \"target\" \"p3\" \"origin\" \"900 0 0\" \"spawnflags\" \"1\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p3\" \"target\" \"p1\" \"origin\" \"900 100 0\" \"spawnflags\" \"1\" }\n",
        );
        let (mut world, train) = train_world("", corners);
        run_for(&mut world, 1.0);
        let e = world.entities.get(train).unwrap();
        assert_eq!(e.state.origin, at_corner(Vec3f::new(900.0, 0.0, 0.0)));
        assert_eq!(e.velocity, Vec3f::ZERO);
    }

    #[test]
    fn train_without_a_target_never_thinks() {
        let world = spawned("{ \"classname\" \"func_train\" \"model\" \"*3\" }");
        let e = world.entities.get(by_class(&world, "func_train")).unwrap();
        assert!(e.think.is_none());
        assert_eq!(e.game.nextthink, 0.0);
    }

    #[test]
    fn train_blocked_removes_debris_and_crushes_players_but_block_stops_spares_them() {
        let (mut world, train) = train_world("", LOOP);
        let blocked = world.entities.get(train).unwrap().blocked.unwrap();
        let gib = world.spawn().unwrap();
        blocked(&mut world, train, gib);
        assert!(!world.entities.is_live(gib));
        let p = player(&mut world);
        {
            let e = world.entities.get_mut(p).unwrap();
            e.game.takedamage = TakeDamage::Aim;
            e.game.health = 1000;
        }
        blocked(&mut world, train, p);
        blocked(&mut world, train, p);
        assert_eq!(
            world.entities.get(p).unwrap().game.health,
            900,
            "once per 0.5s"
        );

        let (mut world, train) = train_world("\"spawnflags\" \"4\" \"dmg\" \"50\"", LOOP);
        assert_eq!(world.entities.get(train).unwrap().game.dmg, 0);
        let p = player(&mut world);
        world.entities.get_mut(p).unwrap().game.takedamage = TakeDamage::Aim;
        let blocked = world.entities.get(train).unwrap().blocked.unwrap();
        blocked(&mut world, train, p);
        assert_eq!(world.entities.get(p).unwrap().game.health, 100);
    }

    #[test]
    fn rotating_door_turning_less_than_a_frame_finishes_in_one_frame() {
        let mut world = spawned(
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"distance\" \"5\" \"targetname\" \"d\" }",
        );
        let door = by_class(&world, "func_door_rotating");
        world.run_frame();
        let p = player(&mut world);
        use_it(&mut world, door, p);
        run_for(&mut world, 0.3);
        assert_eq!(mover(&world, door).1, STATE_TOP);
        assert!(near(angles_of(&world, door), Vec3f::new(0.0, 5.0, 0.0)));
    }

    #[test]
    fn rotating_door_turning_less_than_a_frame_never_overshoots() {
        // C AngleMove_Begin: under one frame of travel goes straight to
        // AngleMove_Final, which turns exactly the rest in one frame.
        let mut world = spawned(
            "{ \"classname\" \"func_door_rotating\" \"model\" \"*3\" \"distance\" \"5\" \"targetname\" \"d\" }",
        );
        let door = by_class(&world, "func_door_rotating");
        world.run_frame();
        let p = player(&mut world);
        use_it(&mut world, door, p);
        for _ in 0..3 {
            world.run_frame();
            let yaw = angles_of(&world, door).y;
            assert!(yaw <= 5.0 + 1e-3, "turned to {yaw}");
        }
    }

    #[test]
    fn targeted_secret_door_with_health_ignores_being_shot_as_in_c() {
        // door_killed has no team master to open.
        let mut world = spawned(
            "{ \"classname\" \"func_door_secret\" \"model\" \"*1\" \"targetname\" \"s\" \"health\" \"10\" }",
        );
        let door = by_class(&world, "func_door");
        let e = world.entities.get(door).unwrap();
        assert_eq!(
            (e.game.takedamage, e.game.max_health),
            (TakeDamage::Yes, 10)
        );
        let die = e.die.expect("door_killed");
        let p = player(&mut world);
        die(&mut world, door, p, p, 10, Vec3f::ZERO);
        let e = world.entities.get(door).unwrap();
        assert_eq!(e.game.takedamage, TakeDamage::Yes, "no team to reset");
        run_for(&mut world, 2.0);
        assert_eq!(origin_of(&world, door), Vec3f::ZERO);
    }

    #[test]
    fn train_stays_put_when_its_corner_is_missing_or_leads_nowhere() {
        let (mut world, lost) = train_world("", "");
        run_for(&mut world, 1.0);
        assert_eq!(
            origin_of(&world, lost),
            Vec3f::ZERO,
            "train_find: not found"
        );

        let dead_end = "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"target\" \"gone\" \"origin\" \"100 0 0\" }\n";
        let (mut world, train) = train_world("", dead_end);
        run_for(&mut world, 1.0);
        let e = world.entities.get(train).unwrap();
        assert_eq!(e.state.origin, at_corner(Vec3f::new(100.0, 0.0, 0.0)));
        assert_eq!(e.velocity, Vec3f::ZERO, "train_next: bad target");

        let last =
            "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"origin\" \"100 0 0\" }\n";
        let (mut world, train) = train_world("", last);
        run_for(&mut world, 1.0);
        assert_eq!(world.entities.get(train).unwrap().velocity, Vec3f::ZERO);
    }

    #[test]
    fn train_killed_by_its_corners_pathtarget_stops_there() {
        let corners = concat!(
            "{ \"classname\" \"path_corner\" \"targetname\" \"p1\" \"target\" \"p2\" \"origin\" \"100 0 0\" }\n",
            "{ \"classname\" \"path_corner\" \"targetname\" \"p2\" \"target\" \"p1\" \"origin\" \"100 20 0\" ",
            "\"pathtarget\" \"boom\" }\n",
            "{ \"classname\" \"trigger_relay\" \"targetname\" \"boom\" \"killtarget\" \"car\" }\n",
        );
        let (mut world, train) = train_world("\"targetname\" \"car\"", corners);
        world.run_frame();
        let p = player(&mut world);
        use_it(&mut world, train, p);
        run_for(&mut world, 1.0);
        assert!(!world.entities.is_live(train));
    }

    /// A parked lift (`targetname` lift) on corners `l1` -> `l2` and a
    /// `trigger_elevator` aimed at `elevator_target`, after its init frame.
    fn elevator_world(elevator_target: &str) -> (GameWorld, EntityKey, EntityKey) {
        let mut world = spawned(&format!(
            concat!(
                "{{ \"classname\" \"path_corner\" \"targetname\" \"l1\" \"target\" \"l2\" \"origin\" \"0 0 0\" }}\n",
                "{{ \"classname\" \"path_corner\" \"targetname\" \"l2\" \"origin\" \"0 0 200\" }}\n",
                "{{ \"classname\" \"func_train\" \"model\" \"*3\" \"target\" \"l1\" \"targetname\" \"lift\" }}\n",
                "{{ \"classname\" \"func_wall\" \"model\" \"*1\" \"targetname\" \"wall\" }}\n",
                "{{ \"classname\" \"trigger_elevator\" \"target\" \"{}\" }}\n",
            ),
            elevator_target
        ));
        run_for(&mut world, 0.2);
        let elevator = by_class(&world, "trigger_elevator");
        let train = by_class(&world, "func_train");
        (world, elevator, train)
    }

    /// An entity that uses the elevator, asking for corner `pathtarget`.
    fn caller(world: &mut GameWorld, pathtarget: &str) -> EntityKey {
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.pathtarget = pathtarget.to_string();
        key
    }

    #[test]
    fn elevator_finds_its_train_on_the_next_frame_and_hides_from_clients() {
        let (world, elevator, train) = elevator_world("lift");
        let e = world.entities.get(elevator).unwrap();
        assert_eq!(e.game.movetarget, Some(train));
        assert!(e.use_fn.is_some());
        assert_eq!(
            e.svflags & q2_shared::constants::SVF_NOCLIENT,
            q2_shared::constants::SVF_NOCLIENT
        );
    }

    #[test]
    fn elevator_without_a_train_target_is_never_usable() {
        for target in ["", "nothing", "wall"] {
            let (world, elevator, _) = elevator_world(target);
            assert!(
                world.entities.get(elevator).unwrap().use_fn.is_none(),
                "target {target:?}"
            );
        }
    }

    #[test]
    fn elevator_sends_its_parked_train_to_the_callers_pathtarget() {
        let (mut world, elevator, train) = elevator_world("lift");
        let l2 = world.find_by_targetname(None, "l2").unwrap();
        let from = caller(&mut world, "l2");
        use_it_from(&mut world, elevator, from);
        assert_eq!(world.entities.get(train).unwrap().game.target_ent, Some(l2));
        run_for(&mut world, 2.5);
        assert!(near(
            origin_of(&world, train),
            at_corner(Vec3f::new(0.0, 0.0, 200.0))
        ));
    }

    #[test]
    fn elevator_ignores_callers_while_the_train_moves_or_without_a_good_pathtarget() {
        let (mut world, elevator, train) = elevator_world("lift");
        for pathtarget in ["", "nowhere"] {
            let from = caller(&mut world, pathtarget);
            use_it_from(&mut world, elevator, from);
            assert_eq!(world.entities.get(train).unwrap().game.nextthink, 0.0);
        }
        let from = caller(&mut world, "l2");
        use_it_from(&mut world, elevator, from);
        let l1 = world.find_by_targetname(None, "l1").unwrap();
        let back = caller(&mut world, "l1");
        world.entities.get_mut(train).unwrap().game.nextthink = world.level.time + 1.0;
        use_it_from(&mut world, elevator, back);
        assert_ne!(world.entities.get(train).unwrap().game.target_ent, Some(l1));
    }

    fn use_it_from(world: &mut GameWorld, key: EntityKey, other: EntityKey) {
        let use_fn = world.entities.get(key).unwrap().use_fn.expect("use");
        use_fn(world, key, other, other);
    }

    // -- mover sounds --

    use crate::world::SoundCall;

    /// A C `gi.sound(ent, CHAN_NO_PHS_ADD + CHAN_VOICE, index, 1,
    /// ATTN_STATIC, 0)` call, as every mover makes.
    fn mover_sound(world: &GameWorld, key: EntityKey, index: i32) -> SoundCall {
        (world.number_of(key), 10, index, 1.0, 3.0, 0.0)
    }

    fn take_sounds(gi: &crate::world::MockGameImport) -> Vec<SoundCall> {
        std::mem::take(&mut *gi.sound_calls.borrow_mut())
    }

    fn looping(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().state.sound
    }

    fn sounds_of(world: &GameWorld, key: EntityKey) -> (i32, i32, i32) {
        let mi = world.entities.get(key).unwrap().moveinfo.clone().unwrap();
        (mi.sound_start, mi.sound_middle, mi.sound_end)
    }

    #[test]
    fn door_plays_start_loops_middle_while_moving_and_plays_end_where_it_stops() {
        let (mut world, gi) = spawned_gi(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" \"targetname\" \"d\" }",
        );
        let door = by_class(&world, "func_door");
        let (start, middle, end) = (
            gi.sound_index("doors/dr1_strt.wav"),
            gi.sound_index("doors/dr1_mid.wav"),
            gi.sound_index("doors/dr1_end.wav"),
        );
        assert_eq!(sounds_of(&world, door), (start, middle, end));
        take_sounds(&gi);
        let p = player(&mut world);
        door_use(&mut world, door, p, p);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, door, start)]);
        assert_eq!(looping(&world, door), middle);

        run_for(&mut world, 1.5);
        assert_eq!(mover(&world, door).1, STATE_TOP);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, door, end)]);
        assert_eq!(looping(&world, door), 0);

        run_for(&mut world, 3.0); // wait, then door_go_down
        assert_eq!(mover(&world, door).1, STATE_DOWN);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, door, start)]);
        assert_eq!(looping(&world, door), middle);
        run_for(&mut world, 1.5);
        assert_eq!(mover(&world, door).1, STATE_BOTTOM);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, door, end)]);
        assert_eq!(looping(&world, door), 0);
    }

    #[test]
    fn door_with_sounds_1_is_silent() {
        let (mut world, gi) = spawned_gi(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"targetname\" \"d\" \"sounds\" \"1\" }",
        );
        let door = by_class(&world, "func_door");
        assert_eq!(sounds_of(&world, door), (0, 0, 0));
        let p = player(&mut world);
        door_use(&mut world, door, p, p);
        run_for(&mut world, 1.5);
        assert!(take_sounds(&gi).is_empty());
        assert_eq!(looping(&world, door), 0);
    }

    #[test]
    fn rotating_door_gets_the_door_sounds_unless_sounds_is_1() {
        let (world, gi) = spawned_gi("{ \"classname\" \"func_door_rotating\" \"model\" \"*1\" }");
        let door = by_class(&world, "func_door_rotating");
        let expected = (
            gi.sound_index("doors/dr1_strt.wav"),
            gi.sound_index("doors/dr1_mid.wav"),
            gi.sound_index("doors/dr1_end.wav"),
        );
        assert_eq!(sounds_of(&world, door), expected);
        let world =
            spawned("{ \"classname\" \"func_door_rotating\" \"model\" \"*1\" \"sounds\" \"1\" }");
        let door = by_class(&world, "func_door_rotating");
        assert_eq!(sounds_of(&world, door), (0, 0, 0));
    }

    #[test]
    fn team_slave_door_moves_silently_with_its_master() {
        let (mut world, gi) = spawned_gi(concat!(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" \"targetname\" \"d\" \"team\" \"t\" }\n",
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" \"team\" \"t\" }\n",
        ));
        let master = by_class(&world, "func_door");
        let slave = world.find_by_classname(Some(master), "func_door").unwrap();
        assert!(world
            .entities
            .get(slave)
            .unwrap()
            .game
            .flags
            .contains(EntityFlags::TEAMSLAVE));
        take_sounds(&gi);
        let p = player(&mut world);
        door_use(&mut world, master, p, p);
        run_for(&mut world, 1.5);
        let calls = take_sounds(&gi);
        assert_eq!(calls.len(), 2, "start and end, master only: {calls:?}");
        assert!(calls.iter().all(|c| c.0 == world.number_of(master)));
        assert_eq!(mover(&world, slave).1, STATE_TOP);
        assert_eq!(looping(&world, slave), 0);
    }

    #[test]
    fn plat_plays_its_sounds_going_up_and_down() {
        let (mut world, gi) = spawned_gi("{ \"classname\" \"func_plat\" \"model\" \"*2\" }");
        let plat = by_class(&world, "func_plat");
        let (start, middle, end) = (
            gi.sound_index("plats/pt1_strt.wav"),
            gi.sound_index("plats/pt1_mid.wav"),
            gi.sound_index("plats/pt1_end.wav"),
        );
        assert_eq!(sounds_of(&world, plat), (start, middle, end));
        take_sounds(&gi);
        plat_go_up(&mut world, plat);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, plat, start)]);
        assert_eq!(looping(&world, plat), middle);
        let run_until = |world: &mut GameWorld, state: i32| {
            for _ in 0..100 {
                if mover(world, plat).1 == state {
                    return;
                }
                world.run_frame();
            }
            panic!("plat never reached state {state}");
        };
        run_until(&mut world, STATE_TOP);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, plat, end)]);
        assert_eq!(looping(&world, plat), 0);
        run_until(&mut world, STATE_DOWN); // plat_hit_top waits 3 seconds
        assert_eq!(take_sounds(&gi), [mover_sound(&world, plat, start)]);
        assert_eq!(looping(&world, plat), middle);
        run_until(&mut world, STATE_BOTTOM);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, plat, end)]);
        assert_eq!(looping(&world, plat), 0);
    }

    #[test]
    fn button_plays_only_a_start_sound_when_pressed_unless_sounds_is_1() {
        let (mut world, gi) = spawned_gi("{ \"classname\" \"func_button\" \"model\" \"*1\" }");
        let button = by_class(&world, "func_button");
        let start = gi.sound_index("switches/butn2.wav");
        assert_eq!(sounds_of(&world, button), (start, 0, 0));
        take_sounds(&gi);
        button_fire(&mut world, button);
        assert_eq!(take_sounds(&gi), [mover_sound(&world, button, start)]);
        assert_eq!(looping(&world, button), 0);
        button_fire(&mut world, button);
        assert!(take_sounds(&gi).is_empty(), "already pressed");

        let world = spawned("{ \"classname\" \"func_button\" \"model\" \"*1\" \"sounds\" \"1\" }");
        let button = by_class(&world, "func_button");
        assert_eq!(sounds_of(&world, button), (0, 0, 0));
    }

    #[test]
    fn water_with_sounds_1_or_2_moves_with_water_sounds_and_no_loop() {
        for sounds in ["1", "2"] {
            let (world, gi) = spawned_gi(&format!(
                "{{ \"classname\" \"func_water\" \"model\" \"*1\" \"sounds\" \"{sounds}\" }}"
            ));
            let water = by_class(&world, "func_door");
            let expected = (
                gi.sound_index("world/mov_watr.wav"),
                0,
                gi.sound_index("world/stp_watr.wav"),
            );
            assert_eq!(sounds_of(&world, water), expected, "sounds {sounds}");
        }
        for sounds in ["0", "3"] {
            let world = spawned(&format!(
                "{{ \"classname\" \"func_water\" \"model\" \"*1\" \"sounds\" \"{sounds}\" }}"
            ));
            let water = by_class(&world, "func_door");
            assert_eq!(sounds_of(&world, water), (0, 0, 0), "sounds {sounds}");
        }
    }

    #[test]
    fn train_loops_its_noise_between_corners_and_stops_it_while_waiting() {
        let (mut world, train) = train_world("\"noise\" \"world/train.wav\"", LOOP);
        let noise = world.gi.sound_index("world/train.wav");
        assert_eq!(sounds_of(&world, train), (0, noise, 0));
        run_for(&mut world, 0.2); // find at p1, then train_next toward p2
        assert_eq!(looping(&world, train), noise);
        run_for(&mut world, 2.2); // 200 units at 100/s
        assert!(near(
            origin_of(&world, train),
            at_corner(Vec3f::new(100.0, 200.0, 0.0))
        ));
        assert_eq!(looping(&world, train), 0, "waiting 2s at p2");
        run_for(&mut world, 2.0);
        assert_eq!(looping(&world, train), noise, "back to p1");

        let (world, train) = train_world("", LOOP);
        assert_eq!(sounds_of(&world, train), (0, 0, 0));
    }

    #[test]
    fn secret_door_precaches_the_door_sounds_but_never_plays_them() {
        let (mut world, gi) = spawned_gi(
            "{ \"classname\" \"func_door_secret\" \"model\" \"*1\" \"targetname\" \"s\" }",
        );
        let door = by_class(&world, "func_door");
        let expected = (
            gi.sound_index("doors/dr1_strt.wav"),
            gi.sound_index("doors/dr1_mid.wav"),
            gi.sound_index("doors/dr1_end.wav"),
        );
        assert_eq!(sounds_of(&world, door), expected);
        take_sounds(&gi);
        let p = player(&mut world);
        door_secret_use(&mut world, door, p, p);
        run_for(&mut world, 10.0);
        assert!(take_sounds(&gi).is_empty());
        assert_eq!(looping(&world, door), 0);
    }

    // -- door messages --

    #[test]
    fn touching_a_targeted_door_centerprints_its_message_at_most_every_5_seconds() {
        let (mut world, gi) = spawned_gi(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"targetname\" \"d\" \"message\" \"Locked\" }",
        );
        let door = by_class(&world, "func_door");
        assert!(gi.sounds.borrow().iter().any(|s| s == "misc/talk.wav"));
        let p = player(&mut world);
        let touch = world.entities.get(door).unwrap().touch.expect("door_touch");
        take_sounds(&gi);
        touch(&mut world, door, p, None, None);
        let number = world.number_of(p);
        assert_eq!(*gi.centerprints.borrow(), [(number, "Locked".to_string())]);
        let talk = gi.sound_index("misc/talk1.wav");
        assert_eq!(take_sounds(&gi), [(number, 0, talk, 1.0, 1.0, 0.0)]);

        run_for(&mut world, 4.9);
        touch(&mut world, door, p, None, None);
        assert_eq!(gi.centerprints.borrow().len(), 1, "debounced");
        run_for(&mut world, 0.2);
        touch(&mut world, door, p, None, None);
        assert_eq!(gi.centerprints.borrow().len(), 2);
    }

    #[test]
    fn door_message_is_not_printed_to_non_clients_and_goes_once_the_door_is_used() {
        let (mut world, gi) = spawned_gi(
            "{ \"classname\" \"func_door\" \"model\" \"*1\" \"targetname\" \"d\" \"message\" \"Locked\" }",
        );
        let door = by_class(&world, "func_door");
        let monster = world.spawn().unwrap();
        door_touch(&mut world, door, monster, None, None);
        assert!(gi.centerprints.borrow().is_empty());
        let p = player(&mut world);
        door_use(&mut world, door, p, p);
        let e = world.entities.get(door).unwrap();
        assert!(e.touch.is_none() && e.game.message.is_empty());
        assert!(
            gi.centerprints.borrow().is_empty(),
            "use_targets saw no message"
        );
    }

    #[test]
    fn targeted_doors_get_a_message_touch_as_c_decides() {
        // func_door and func_door_secret use `else if`, so a shootable door
        // gets no message touch; SP_func_door_rotating a plain `if`.
        let shootable = "\"targetname\" \"d\" \"message\" \"m\" \"health\" \"10\"";
        let world = spawned(&format!(
            "{{ \"classname\" \"func_door\" \"model\" \"*1\" {shootable} }}"
        ));
        let touch = world
            .entities
            .get(by_class(&world, "func_door"))
            .unwrap()
            .touch;
        assert!(
            touch.is_none_or(|t| !std::ptr::fn_addr_eq(t, door_touch as crate::entity::TouchFn))
        );
        for keys in ["\"message\" \"m\"", "\"targetname\" \"d\""] {
            for class in ["func_door", "func_door_rotating"] {
                let world = spawned(&format!(
                    "{{ \"classname\" \"{class}\" \"model\" \"*1\" {keys} }}"
                ));
                let door = by_class(&world, class);
                let touch = world.entities.get(door).unwrap().touch;
                assert!(
                    touch.is_none_or(|t| !std::ptr::fn_addr_eq(
                        t,
                        door_touch as crate::entity::TouchFn
                    )),
                    "{class} {keys}"
                );
            }
        }
        for (class, health) in [
            ("func_door_rotating", ""),
            ("func_door_rotating", "\"health\" \"10\""),
            ("func_door_secret", ""),
        ] {
            let world = spawned(&format!(
                "{{ \"classname\" \"{class}\" \"model\" \"*1\" \"targetname\" \"d\" \"message\" \"m\" {health} }}"
            ));
            let door = world.find_by_targetname(None, "d").unwrap();
            let touch = world.entities.get(door).unwrap().touch;
            assert!(
                touch
                    .is_some_and(|t| std::ptr::fn_addr_eq(t, door_touch as crate::entity::TouchFn)),
                "{class} {health}"
            );
        }
    }

    // -- area portals --

    fn portal_states(gi: &crate::world::MockGameImport) -> Vec<(i32, bool)> {
        std::mem::take(&mut *gi.portal_states.borrow_mut())
    }

    #[test]
    fn areaportal_starts_closed_and_each_use_toggles_its_portal() {
        let (mut world, gi) = spawned_gi(
            "{ \"classname\" \"func_areaportal\" \"targetname\" \"ap\" \"style\" \"3\" \"count\" \"1\" }",
        );
        let portal = by_class(&world, "func_areaportal");
        assert_eq!(world.entities.get(portal).unwrap().game.count, 0);
        let p = player(&mut world);
        use_it(&mut world, portal, p);
        assert_eq!(portal_states(&gi), [(3, true)]);
        use_it(&mut world, portal, p);
        assert_eq!(portal_states(&gi), [(3, false)]);
    }

    const PORTAL_DOOR: &str = concat!(
        "{ \"classname\" \"func_areaportal\" \"targetname\" \"ap\" \"style\" \"2\" }\n",
        "{ \"classname\" \"info_notnull\" \"targetname\" \"ap\" \"style\" \"7\" }\n",
    );

    #[test]
    fn door_opens_its_targeted_portals_as_it_opens_and_shuts_them_at_the_bottom() {
        let (mut world, gi) = spawned_gi(&format!(
            "{PORTAL_DOOR}{{ \"classname\" \"func_door\" \"model\" \"*1\" \"angle\" \"-1\" \"target\" \"ap\" }}"
        ));
        let door = by_class(&world, "func_door");
        world.run_frame();
        assert!(portal_states(&gi).is_empty());
        let p = player(&mut world);
        door_use(&mut world, door, p, p);
        // C G_UseTargets leaves a door's portals to door_use_areaportals.
        assert_eq!(portal_states(&gi), [(2, true)]);
        run_for(&mut world, 1.5 + 3.0);
        assert_eq!(mover(&world, door).1, STATE_DOWN);
        assert!(portal_states(&gi).is_empty(), "open until the door is shut");
        run_for(&mut world, 1.5);
        assert_eq!(mover(&world, door).1, STATE_BOTTOM);
        assert_eq!(portal_states(&gi), [(2, false)]);
        door_use(&mut world, door, p, p);
        assert_eq!(portal_states(&gi), [(2, true)]);
    }

    #[test]
    fn only_an_untargeted_start_open_door_opens_its_portals_at_spawn() {
        let (mut world, gi) = spawned_gi(&format!(
            "{PORTAL_DOOR}{{ \"classname\" \"func_door\" \"model\" \"*1\" \"target\" \"ap\" \"spawnflags\" \"1\" }}"
        ));
        world.run_frame();
        assert_eq!(portal_states(&gi), [(2, true)]);

        for keys in [
            "\"target\" \"ap\" \"spawnflags\" \"1\" \"targetname\" \"d\"",
            "\"target\" \"ap\"",
            "\"spawnflags\" \"1\"",
        ] {
            let (mut world, gi) = spawned_gi(&format!(
                "{PORTAL_DOOR}{{ \"classname\" \"func_door\" \"model\" \"*1\" {keys} }}"
            ));
            world.run_frame();
            assert!(portal_states(&gi).is_empty(), "{keys}");
        }
    }

    #[test]
    fn secret_door_opens_its_portals_when_used_and_shuts_them_when_closed() {
        let (mut world, gi) = spawned_gi(&format!(
            "{PORTAL_DOOR}{{ \"classname\" \"func_door_secret\" \"model\" \"*1\" \"target\" \"ap\" \"targetname\" \"s\" }}"
        ));
        let door = world.find_by_targetname(None, "s").unwrap();
        let p = player(&mut world);
        door_secret_use(&mut world, door, p, p);
        assert_eq!(portal_states(&gi), [(2, true)]);
        run_for(&mut world, 20.0);
        assert_eq!(world.entities.get(door).unwrap().state.origin, Vec3f::ZERO);
        assert_eq!(portal_states(&gi), [(2, false)]);
    }
}
