//! Functional entities — doors, platforms, buttons, trains, rotating objects.
//!
//! Port of `g_func.c` (3,012 lines, 16 func_* entity types).
//! These entities use `MoveInfo` for smooth acceleration/deceleration.
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
pub const DOOR_NOMONSTER: i32 = 8;
pub const DOOR_TOGGLE: i32 = 32;
/// func_plat spawnflag.
pub const PLAT_LOW_TRIGGER: i32 = 1;

fn moveinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MoveInfo> {
    world
        .entities
        .get_mut(key)
        .and_then(|e| e.moveinfo.as_deref_mut())
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
            ent.game.nextthink = time + FRAMETIME;
            ent.think = Some(move_begin);
        }
    } else {
        mi.current_speed = 0.0;
        ent.think = Some(think_accel_move);
        ent.game.nextthink = time + FRAMETIME;
    }
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
    if mi.speed * FRAMETIME >= mi.remaining_distance {
        move_final(world, key);
        return;
    }
    ent.velocity = mi.dir * mi.speed;
    let frames = ((mi.remaining_distance / mi.speed) / FRAMETIME).floor();
    mi.remaining_distance -= frames * mi.speed * FRAMETIME;
    ent.game.nextthink = time + frames * FRAMETIME;
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
    ent.velocity = mi.dir * (mi.remaining_distance / FRAMETIME);
    ent.think = Some(move_done_think);
    ent.game.nextthink = time + FRAMETIME;
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

pub fn move_done_think(world: &mut GameWorld, key: EntityKey) {
    move_done(world, key);
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
        let p2_distance = mi.move_speed * (1.0 - (p1_distance / mi.move_speed));
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
        let p2_distance = mi.move_speed * (1.0 - (p1_distance / p1_speed));
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
    ent.game.nextthink = time + FRAMETIME;
    ent.think = Some(think_accel_move);
}

/// `self`, then each `teamchain` member (C `for (ent = self; ent; ent =
/// ent->teamchain)`).
fn team_members(world: &GameWorld, first: EntityKey) -> Vec<EntityKey> {
    let mut members = vec![first];
    let mut next = world.entities.get(first).and_then(|e| e.game.teamchain);
    while let Some(k) = next {
        if members.contains(&k) {
            break;
        }
        members.push(k);
        next = world.entities.get(k).and_then(|e| e.game.teamchain);
    }
    members
}

// ---------------------------------------------------------------------------
// Platform (func_plat) callbacks
// ---------------------------------------------------------------------------

/// C ref: `plat_hit_top`.
pub fn plat_hit_top(world: &mut GameWorld, key: EntityKey) {
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
    if let Some(mi) = moveinfo_mut(world, key) {
        mi.state = STATE_BOTTOM;
    }
}

/// C ref: `plat_go_down`.
pub fn plat_go_down(world: &mut GameWorld, key: EntityKey) {
    let Some(mi) = moveinfo_mut(world, key) else {
        return;
    };
    mi.state = STATE_DOWN;
    let end = mi.end_origin;
    move_calc(world, key, end, plat_hit_bottom);
}

/// C ref: `plat_go_up`.
pub fn plat_go_up(world: &mut GameWorld, key: EntityKey) {
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
    let members = team_members(world, self_key);
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

/// C ref: `door_go_up` (func_door; func_door_rotating's AngleMove is not
/// ported).
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
    move_calc(world, key, end, door_hit_top);
    world.use_targets(key, activator);
}

/// C ref: `door_go_down`.
pub fn door_go_down(world: &mut GameWorld, key: EntityKey) {
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
    move_calc(world, key, start, door_hit_bottom);
}

/// C ref: `door_hit_top`.
pub fn door_hit_top(world: &mut GameWorld, key: EntityKey) {
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

/// C ref: `door_hit_bottom` (area portals are not ported).
pub fn door_hit_bottom(world: &mut GameWorld, key: EntityKey) {
    if let Some(mi) = moveinfo_mut(world, key) {
        mi.state = STATE_BOTTOM;
    }
}

/// C ref: `door_killed`: a shot door opens with its whole team.
pub fn door_killed(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let master = world
        .entities
        .get(self_key)
        .and_then(|e| e.game.teammaster)
        .unwrap_or(self_key);
    for member in team_members(world, master) {
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

/// Make a team finish its moves together. C ref: `Think_CalcMoveSpeed`.
pub fn think_calc_move_speed(world: &mut GameWorld, key: EntityKey) {
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.game.flags.contains(EntityFlags::TEAMSLAVE))
    {
        return; // only the team master does this
    }
    let members = team_members(world, key);
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
    for member in team_members(world, key).into_iter().skip(1) {
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
    think_calc_move_speed(world, key);
}

// ---------------------------------------------------------------------------
// Button callbacks
// ---------------------------------------------------------------------------

/// C ref: `button_done`.
pub fn button_done(world: &mut GameWorld, key: EntityKey) {
    if let Some(mi) = moveinfo_mut(world, key) {
        mi.state = STATE_BOTTOM;
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
// Timer callbacks
// ---------------------------------------------------------------------------

pub fn func_timer_think(world: &mut GameWorld, key: EntityKey) {
    world.use_targets(key, key);

    let (wait, random) = {
        let Some(ent) = world.entities.get(key) else {
            return;
        };
        (ent.game.wait, ent.game.random)
    };

    // Schedule next firing.
    if let Some(ent) = world.entities.get_mut(key) {
        let delay = wait.max(FRAMETIME);
        // Random variation (simplified — no true randomness in deterministic game).
        ent.game.nextthink = world.level.time + delay + random * 0.5;
    }
}

// ---------------------------------------------------------------------------
// Explosive callbacks
// ---------------------------------------------------------------------------

pub fn func_explosive_die(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let dmg = world
        .entities
        .get(self_key)
        .map(|e| e.game.dmg)
        .unwrap_or(0);

    if dmg > 0 {
        world.t_radius_damage(
            self_key,
            attacker,
            dmg as f32,
            None,
            dmg as f32,
            MeansOfDeath::Explosive,
        );
    }

    world.use_targets(self_key, attacker);
    world.free_entity(self_key);
}

// ---------------------------------------------------------------------------
// Killbox callback
// ---------------------------------------------------------------------------

pub fn func_killbox_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    // Kill everything inside the killbox's volume.
    // Simplified: in full impl, uses box_edicts to find entities inside.
    let _ = world.entities.get(self_key);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Register all func_* spawn functions.
    pub fn register_func_spawns(&mut self) {
        self.spawn_table
            .insert("func_plat".to_string(), sp_func_plat);
        self.spawn_table
            .insert("func_button".to_string(), sp_func_button);
        self.spawn_table
            .insert("func_door".to_string(), sp_func_door);
        self.spawn_table
            .insert("func_door_secret".to_string(), sp_func_generic);
        self.spawn_table
            .insert("func_door_rotating".to_string(), sp_func_generic);
        self.spawn_table
            .insert("func_rotating".to_string(), sp_func_rotating);
        self.spawn_table
            .insert("func_train".to_string(), sp_func_train);
        self.spawn_table
            .insert("func_water".to_string(), sp_func_generic);
        self.spawn_table
            .insert("func_conveyor".to_string(), sp_func_generic);
        self.spawn_table
            .insert("func_areaportal".to_string(), sp_func_nonsolid);
        self.spawn_table
            .insert("func_clock".to_string(), sp_func_nonsolid);
        self.spawn_table
            .insert("func_wall".to_string(), sp_func_generic);
        self.spawn_table
            .insert("func_object".to_string(), sp_func_generic);
        self.spawn_table
            .insert("func_timer".to_string(), sp_func_timer);
        self.spawn_table
            .insert("func_explosive".to_string(), sp_func_explosive);
        self.spawn_table
            .insert("func_killbox".to_string(), sp_func_killbox);
    }
}

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
fn sp_func_plat(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = "func_plat".to_string();
    ent.state.angles = Vec3f::ZERO;
    ent.solid = Solid::Bsp;
    ent.game.movetype = MoveType::Push;
    let g = &mut ent.game;
    g.speed = if g.speed == 0.0 { 20.0 } else { g.speed * 0.1 };
    g.accel = if g.accel == 0.0 { 5.0 } else { g.accel * 0.1 };
    g.decel = if g.decel == 0.0 { 5.0 } else { g.decel * 0.1 };
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
fn sp_func_button(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
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

/// C ref: `SP_func_door` (single player: no deathmatch speed-up; the
/// message-only `door_touch` is not ported).
fn sp_func_door(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
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

fn sp_func_rotating(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_rotating".to_string();
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;

        let speed = fields
            .get("speed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100.0);
        ent.avelocity = Vec3f::new(0.0, speed, 0.0); // rotate around Y axis.
    }
}

#[allow(clippy::field_reassign_with_default)]
fn sp_func_train(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_train".to_string();
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;

        let speed = fields
            .get("speed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100.0);

        let mut mi = MoveInfo::default();
        mi.speed = speed;
        ent.moveinfo = Some(Box::new(mi));

        if let Some(t) = fields.get("target") {
            ent.game.target = t.clone();
        }
    }
}

fn sp_func_timer(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_timer".to_string();

        ent.game.wait = fields
            .get("wait")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);
        ent.game.random = fields
            .get("random")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);

        ent.think = Some(func_timer_think);
        ent.game.nextthink = fields
            .get("delay")
            .and_then(|v| v.parse().ok())
            .map(|d: f32| d)
            .unwrap_or(1.0);
    }
}

fn sp_func_explosive(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_explosive".to_string();
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;
        ent.game.health = fields
            .get("health")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        ent.game.dmg = fields.get("dmg").and_then(|v| v.parse().ok()).unwrap_or(0);
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(func_explosive_die);
    }
}

fn sp_func_killbox(storage: &mut EntityStorage, key: EntityKey, _fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "func_killbox".to_string();
        ent.solid = Solid::Trigger;
        ent.use_fn = Some(func_killbox_use);
    }
}

fn sp_func_generic(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "func".to_string());
        ent.game.movetype = MoveType::Push;
        ent.solid = Solid::Bsp;
    }
}

/// func_areaportal and func_clock: C SP_func_areaportal never sets a solid
/// (a solid portal brush would close every doorway it sits in), and
/// func_clock is a point entity.
fn sp_func_nonsolid(storage: &mut EntityStorage, key: EntityKey, fields: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields.get("classname").cloned().unwrap_or_default();
        ent.state.modelindex = 0; // C never calls setmodel: nothing to draw
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    use crate::entity::ClientData;
    use crate::world::test_world_recording;

    const DOOR_MINS: Vec3f = Vec3f::new(0.0, 0.0, 0.0);
    const DOOR_MAXS: Vec3f = Vec3f::new(16.0, 64.0, 96.0);

    /// A world spawned from `entities`, where `*1` is a 16x64x96 box and
    /// `*2` a 128x128x64 plat.
    fn spawned(entities: &str) -> GameWorld {
        let (mut world, gi) = test_world_recording();
        let mut bounds = gi.inline_bounds.borrow_mut();
        bounds.insert("*1".into(), (DOOR_MINS, DOOR_MAXS));
        bounds.insert("*2".into(), (Vec3f::ZERO, Vec3f::new(128.0, 128.0, 64.0)));
        drop(bounds);
        world.init_game();
        world.spawn_entities(
            "test",
            &format!("{{ \"classname\" \"worldspawn\" }}\n{entities}"),
            "",
        );
        world
    }

    fn player(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.client = Some(ClientData::default());
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
        let mut world = test_world();
        world.register_func_spawns();
        assert!(world.spawn_table.get("func_plat").is_some());
        assert!(world.spawn_table.get("func_door").is_some());
        assert!(world.spawn_table.get("func_button").is_some());
        assert!(world.spawn_table.get("func_train").is_some());
        assert!(world.spawn_table.get("func_explosive").is_some());
        assert!(world.spawn_table.get("func_timer").is_some());
        assert!(world.spawn_table.get("func_killbox").is_some());
        assert!(world.spawn_table.get("func_rotating").is_some());
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
        use std::sync::atomic::{AtomicBool, Ordering};
        static FIRED: AtomicBool = AtomicBool::new(false);

        fn test_use(_w: &mut GameWorld, _s: EntityKey, _o: EntityKey, _a: EntityKey) {
            FIRED.store(true, Ordering::Relaxed);
        }

        FIRED.store(false, Ordering::Relaxed);

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
            !FIRED.load(Ordering::Relaxed),
            "targets fire once pressed in"
        );
        world.run_frame();
        assert!(FIRED.load(Ordering::Relaxed));

        let state = world
            .entities
            .get(button)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap()
            .state;
        assert_eq!(state, STATE_TOP);
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
        }

        let attacker = world.spawn().unwrap();

        func_explosive_die(&mut world, explosive, attacker, attacker, 100, Vec3f::ZERO);

        assert!(world.entities.get(victim).unwrap().game.health < 200);
        assert!(!world.entities.get(explosive).is_some_and(|e| e.in_use));
    }

    #[test]
    fn func_timer_fires_periodically() {
        use std::sync::atomic::{AtomicI32, Ordering};
        static COUNT: AtomicI32 = AtomicI32::new(0);

        fn test_use(_w: &mut GameWorld, _s: EntityKey, _o: EntityKey, _a: EntityKey) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }

        COUNT.store(0, Ordering::Relaxed);

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
        assert_eq!(COUNT.load(Ordering::Relaxed), 1);

        func_timer_think(&mut world, timer);
        assert_eq!(COUNT.load(Ordering::Relaxed), 2);

        // nextthink should be set for next firing.
        let nt = world.entities.get(timer).unwrap().game.nextthink;
        assert!(nt > world.level.time);
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
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let mut fields = HashMap::new();
        fields.insert("speed".to_string(), "360".to_string());

        sp_func_rotating(&mut world.entities, key, &fields);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.avelocity.y, 360.0);
    }
}
