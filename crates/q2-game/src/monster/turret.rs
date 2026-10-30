//! Turrets: a `turret_breach` gun (pitch and yaw) teamed with a
//! `turret_base` (yaw), aimed and fired by the `turret_driver` that
//! targets the breach.
//!
//! The driver is a monster only in name: it never animates or moves on
//! its own; the breach drags it along by velocity.
//! C ref: `~/Qwasm2/src/game/g_turret.c`.

use q2_common::cvar::parse_value as strtod;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterInfo, MoveInfo};
use crate::monster::infantry;
use crate::weapons::vectoangles;
use crate::world::GameWorld;
use std::collections::HashMap;

/// Set on the breach by its driver to fire on the breach's next think.
const SPAWNFLAG_FIRE: i32 = 65536;

/// C ref: `AnglesNormalize` (pitch and yaw into 0..=360).
pub fn angles_normalize(mut v: Vec3f) -> Vec3f {
    while v.x > 360.0 {
        v.x -= 360.0;
    }
    while v.x < 0.0 {
        v.x += 360.0;
    }
    while v.y > 360.0 {
        v.y -= 360.0;
    }
    while v.y < 0.0 {
        v.y += 360.0;
    }
    v
}

/// Round to the nearest eighth of a unit, halves away from zero.
/// C ref: `SnapToEights`.
pub fn snap_to_eights(x: f32) -> f32 {
    let x = x * 8.0;
    let x = if x > 0.0 { x + 0.5 } else { x - 0.5 };
    0.125 * (x as i32) as f32
}

/// C `pos1` / `pos2` (the breach's pitch and yaw limits). The entity has
/// no such fields, so the breach keeps them in its `moveinfo`.
fn limits(world: &GameWorld, key: EntityKey) -> (Vec3f, Vec3f) {
    world
        .entities
        .get(key)
        .and_then(|e| e.moveinfo.as_ref())
        .map_or((Vec3f::ZERO, Vec3f::ZERO), |mi| {
            (mi.start_angles, mi.end_angles)
        })
}

/// Crush whatever blocks the turret, credited to the driver.
/// C ref: `turret_blocked`.
pub fn turret_blocked(world: &mut GameWorld, key: EntityKey, other: EntityKey) {
    let Some(other_ent) = world.entities.get(other) else {
        return;
    };
    if other_ent.game.takedamage == TakeDamage::No {
        return;
    }
    let point = other_ent.state.origin;
    let Some(master) = world.entities.get(key).and_then(|e| e.game.teammaster) else {
        return;
    };
    let Some(master_ent) = world.entities.get(master) else {
        return;
    };
    let attacker = master_ent.owner.unwrap_or(master);
    let dmg = master_ent.game.dmg;
    world.t_damage(
        other,
        key,
        attacker,
        Vec3f::ZERO,
        point,
        Vec3f::ZERO,
        dmg,
        10,
        DamageFlags::empty(),
        MeansOfDeath::Crush,
    );
}

/// A rocket from the muzzle, owned by the driver. C ref: `turret_breach_fire`.
pub fn turret_breach_fire(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (f, r, u) = crate::physics::angle_vectors(ent.state.angles);
    let mo = ent.game.move_origin;
    let start = ent.state.origin + f * mo.x + r * mo.y + u * mo.z;
    let Some(owner) = ent
        .game
        .teammaster
        .and_then(|m| world.entities.get(m))
        .and_then(|m| m.owner)
    else {
        return;
    };
    let damage = (100.0 + world.random() * 50.0) as i32;
    let speed = 550 + 50 * world.game.skill;
    world.fire_rocket(owner, start, f, damage, speed, 150.0, damage);
    let index = world.gi.sound_index("weapons/rocklf1a.wav");
    if let Some(number) = world.number_of(key) {
        world
            .gi
            .positioned_sound(start, number, CHAN_WEAPON, index, 1.0, ATTN_NORM, 0.0);
    }
}

/// Wrap a delta angle into -180..=180 (C does it once, not in a loop).
fn wrap180(d: f32) -> f32 {
    if d < -180.0 {
        d + 360.0
    } else if d > 180.0 {
        d - 360.0
    } else {
        d
    }
}

/// C's two ifs, not `f32::clamp`: a map's negative `speed` makes the
/// bounds cross, which would panic `clamp` and leaves C at `-step`.
fn cap_turn(delta: f32, step: f32) -> f32 {
    let delta = if delta > step { step } else { delta };
    if delta < -step {
        -step
    } else {
        delta
    }
}

/// Turn toward `move_angles` within the pitch/yaw limits at `speed`
/// degrees a second, turn the base with it, drag the driver along, and
/// fire when the driver asked. C ref: `turret_breach_think`.
pub fn turret_breach_think(world: &mut GameWorld, key: EntityKey) {
    let (pos1, pos2) = limits(world, key);
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let current_angles = angles_normalize(ent.state.angles);
    let mut aim = angles_normalize(ent.game.move_angles);
    if aim.x > 180.0 {
        aim.x -= 360.0;
    }

    // Clamp angles to mins & maxs.
    if aim.x > pos1.x {
        aim.x = pos1.x;
    } else if aim.x < pos2.x {
        aim.x = pos2.x;
    }
    if aim.y < pos1.y || aim.y > pos2.y {
        let dmin = wrap180((pos1.y - aim.y).abs());
        let dmax = wrap180((pos2.y - aim.y).abs());
        aim.y = if dmin.abs() < dmax.abs() {
            pos1.y
        } else {
            pos2.y
        };
    }
    ent.game.move_angles = aim;

    let step = ent.game.speed * FRAMETIME;
    let mut delta = aim - current_angles;
    delta.x = cap_turn(wrap180(delta.x), step);
    delta.y = cap_turn(wrap180(delta.y), step);
    delta.z = 0.0;
    ent.avelocity = delta * (1.0 / FRAMETIME);
    ent.game.nextthink = time + FRAMETIME;
    let (avelocity, angles, origin) = (ent.avelocity, ent.state.angles, ent.state.origin);
    let (master, driver, fire) = (
        ent.game.teammaster,
        ent.owner,
        ent.game.spawnflags & SPAWNFLAG_FIRE != 0,
    );

    let mut member = master;
    while let Some(m) = member {
        let Some(e) = world.entities.get_mut(m) else {
            break;
        };
        e.avelocity.y = avelocity.y;
        member = e.game.teamchain;
    }

    // If we have a driver, adjust his velocities.
    let Some(driver) = driver else {
        return;
    };
    let Some(d) = world.entities.get_mut(driver) else {
        return;
    };
    // Angular is easy, just copy ours.
    d.avelocity.x = avelocity.x;
    d.avelocity.y = avelocity.y;

    // X and y.
    let angle = (angles.y + d.game.move_origin.y).to_radians();
    let target_x = snap_to_eights(origin.x + angle.cos() * d.game.move_origin.x);
    let target_y = snap_to_eights(origin.y + angle.sin() * d.game.move_origin.x);
    d.velocity.x = (target_x - d.state.origin.x) / FRAMETIME;
    d.velocity.y = (target_y - d.state.origin.y) / FRAMETIME;

    // Z.
    let angle = angles.x.to_radians();
    let target_z =
        snap_to_eights(origin.z + d.game.move_origin.x * angle.tan() + d.game.move_origin.z);
    d.velocity.z = (target_z - d.state.origin.z) / FRAMETIME;

    if fire {
        turret_breach_fire(world, key);
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.spawnflags &= !SPAWNFLAG_FIRE;
        }
    }
}

/// Measure the muzzle from the breach's target (then remove it), hand the
/// crush damage to the team master, and start aiming.
/// C ref: `turret_breach_finish_init`.
pub fn turret_breach_finish_init(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (target, origin) = (ent.game.target.clone(), ent.state.origin);
    // Get and save info for muzzle location.
    if target.is_empty() {
        let message = format!(
            "{} at ({} {} {}) needs a target\n",
            ent.game.classname, origin.x as i32, origin.y as i32, origin.z as i32
        );
        world.gi.dprintf(&message);
    } else if let Some(muzzle) = world.pick_target(&target) {
        let at = world
            .entities
            .get(muzzle)
            .map_or(origin, |m| m.state.origin);
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.target_ent = Some(muzzle);
            ent.game.move_origin = at - origin;
        }
        world.free_entity(muzzle);
    }
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let (dmg, master) = (ent.game.dmg, ent.game.teammaster);
    ent.think = Some(turret_breach_think);
    if let Some(m) = master.and_then(|m| world.entities.get_mut(m)) {
        m.game.dmg = dmg;
    }
    turret_breach_think(world, key);
}

/// C `st.*` floats: 0 when absent, as the C spawn temp leaves them.
fn st_float(fields: &HashMap<String, String>, name: &str) -> f32 {
    fields.get(name).map_or(0.0, |v| strtod(v))
}

/// The `EntityStorage` half of C `SP_turret_breach` (its brush model is
/// set before the spawn function runs).
pub fn sp_turret_breach(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.solid = Solid::Bsp;
    ent.game.movetype = MoveType::Push;
    if ent.game.speed == 0.0 {
        ent.game.speed = 50.0;
    }
    if ent.game.dmg == 0 {
        ent.game.dmg = 10;
    }
    let or = |v: f32, default: f32| if v == 0.0 { default } else { v };
    let minpitch = or(st_float(fields, "minpitch"), -30.0);
    let maxpitch = or(st_float(fields, "maxpitch"), 30.0);
    let minyaw = st_float(fields, "minyaw");
    let maxyaw = or(st_float(fields, "maxyaw"), 360.0);
    ent.moveinfo = Some(Box::new(MoveInfo {
        start_angles: Vec3f::new(-minpitch, minyaw, 0.0),
        end_angles: Vec3f::new(-maxpitch, maxyaw, 0.0),
        ..Default::default()
    }));
    ent.game.ideal_yaw = ent.state.angles.y;
    ent.game.move_angles.y = ent.game.ideal_yaw;
    ent.blocked = Some(turret_blocked);
    ent.think = Some(turret_breach_finish_init);
}

/// The `GameWorld` half of C `SP_turret_breach`: finish next frame, once
/// `G_FindTeams` has made the breach's team.
pub fn turret_breach_finish(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + FRAMETIME;
    }
}

/// C ref: `SP_turret_base`.
pub fn sp_turret_base(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.solid = Solid::Bsp;
    ent.game.movetype = MoveType::Push;
    ent.blocked = Some(turret_blocked);
}

/// Level the gun, leave the turret's team, and die as an infantry
/// soldier. C ref: `turret_driver_die`, which ends in `infantry_die`.
pub fn turret_driver_die(
    world: &mut GameWorld,
    key: EntityKey,
    inflictor: EntityKey,
    attacker: EntityKey,
    damage: i32,
    point: Vec3f,
) {
    // C dereferences target_ent unchecked. A driver that never linked
    // (misnamed target) has no gun or team; one whose breach is gone
    // still leaves the team it joined. Either way the driver dies.
    let breach = world.entities.get(key).and_then(|e| e.game.target_ent);
    if let Some(b) = breach.and_then(|b| world.entities.get_mut(b)) {
        // Level the gun.
        b.game.move_angles.x = 0.0;
        b.owner = None;
    }
    turret_driver_leave_team(world, key);
    // C: a driver's gib_health is 0, so infantry_die always gibs it.
    infantry::infantry_die(world, key, inflictor, attacker, damage, point);
}

/// Take the driver off the end of the turret's team (the breach's team,
/// which the driver joined in `turret_driver_link`).
fn turret_driver_leave_team(world: &mut GameWorld, key: EntityKey) {
    let master = world.entities.get(key).and_then(|e| e.game.teammaster);

    // Remove the driver from the end of the team chain.
    let mut member = master;
    while let Some(m) = member {
        let Some(e) = world.entities.get_mut(m) else {
            break;
        };
        if e.game.teamchain == Some(key) {
            e.game.teamchain = None;
            break;
        }
        member = e.game.teamchain;
    }
    if let Some(m) = master.and_then(|m| world.entities.get_mut(m)) {
        m.owner = None;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.teammaster = None;
        ent.game.flags &= !EntityFlags::TEAMSLAVE;
    }
}

/// Track an enemy and tell the breach where to aim, and to fire once the
/// enemy has been in sight for the skill's reaction time.
/// C ref: `turret_driver_think`.
pub fn turret_driver_think(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.nextthink = time + FRAMETIME;
    let enemy = ent.game.enemy.filter(|&e| {
        world
            .entities
            .get(e)
            .is_some_and(|e| e.in_use && e.game.health > 0)
    });
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.enemy = enemy;
    }

    match enemy {
        None => {
            if !ai::find_target(world, key) {
                return;
            }
            if let Some(mi) = monsterinfo_mut(world, key) {
                mi.trail_time = time;
                mi.aiflags.remove(AiFlags::LOST_SIGHT);
            }
        }
        Some(enemy) => {
            if ai::visible(world, key, enemy) {
                if let Some(mi) = monsterinfo_mut(world, key) {
                    if mi.aiflags.contains(AiFlags::LOST_SIGHT) {
                        mi.trail_time = time;
                        mi.aiflags.remove(AiFlags::LOST_SIGHT);
                    }
                }
            } else {
                if let Some(mi) = monsterinfo_mut(world, key) {
                    mi.aiflags |= AiFlags::LOST_SIGHT;
                }
                return;
            }
        }
    }

    // Let the turret know where we want it to aim.
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (Some(enemy), Some(breach)) = (ent.game.enemy, ent.game.target_ent) else {
        return;
    };
    let (Some(e), Some(b)) = (world.entities.get(enemy), world.entities.get(breach)) else {
        return;
    };
    let target = e.state.origin + Vec3f::new(0.0, 0.0, e.game.viewheight);
    let aim = vectoangles(target - b.state.origin);
    if let Some(b) = world.entities.get_mut(breach) {
        b.game.move_angles = aim;
    }

    // Decide if we should shoot.
    let Some(mi) = world.entities.get(key).and_then(|e| e.monsterinfo.as_ref()) else {
        return;
    };
    if time < mi.attack_finished {
        return;
    }
    let reaction_time = (3 - world.game.skill) as f32;
    if time - mi.trail_time < reaction_time {
        return;
    }
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.attack_finished = time + reaction_time + 1.0;
    }
    if let Some(b) = world.entities.get_mut(breach) {
        b.game.spawnflags |= SPAWNFLAG_FIRE;
    }
}

fn monsterinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MonsterInfo> {
    world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_deref_mut())
}

/// Take the seat: own the breach (and its team), face its way, record
/// where the driver sits relative to it, and join the end of its team.
/// C ref: `turret_driver_link`. Also puts back the MOVETYPE_PUSH that
/// `monster_start` (which C's SP_turret_driver does not call) replaced.
pub fn turret_driver_link(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Push;
    ent.think = Some(turret_driver_think);
    ent.game.nextthink = time + FRAMETIME;
    let (target, origin) = (ent.game.target.clone(), ent.state.origin);

    let Some(breach) = world.pick_target(&target) else {
        return;
    };
    let Some(b) = world.entities.get_mut(breach) else {
        return;
    };
    b.owner = Some(key);
    let (master, b_origin, b_angles) = (b.game.teammaster, b.state.origin, b.state.angles);
    if let Some(m) = master.and_then(|m| world.entities.get_mut(m)) {
        m.owner = Some(key);
    }

    let flat = Vec3f::new(b_origin.x - origin.x, b_origin.y - origin.y, 0.0);
    let seat = angles_normalize(vectoangles(origin - b_origin));
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.target_ent = Some(breach);
        ent.state.angles = b_angles;
        ent.game.move_origin = Vec3f::new(flat.length(), seat.y, origin.z - b_origin.z);
    }

    // Add the driver to the end of the team chain.
    let mut last = master;
    while let Some(m) = last {
        match world.entities.get(m).and_then(|e| e.game.teamchain) {
            Some(next) => last = Some(next),
            None => break,
        }
    }
    if let Some(e) = last.and_then(|m| world.entities.get_mut(m)) {
        e.game.teamchain = Some(key);
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.teammaster = master;
        ent.game.flags |= EntityFlags::TEAMSLAVE;
    }
}

/// The `EntityStorage` half of C `SP_turret_driver`; `GameWorld::call_spawn`
/// then runs `monster_start`, which does the rest of C's inline monster
/// setup (deathmatch removal, `total_monsters`, SVF_MONSTER, RF_FRAMELERP,
/// DAMAGE_AIM, `monster_use`, MASK_MONSTERSOLID, `old_origin`) and keeps
/// the `turret_driver_link` think. It also does what C's driver never
/// gets: MOVETYPE_STEP until `turret_driver_link` puts PUSH back,
/// `max_health`, `air_finished`, the `M_CheckAttack` checkattack, a
/// forever `pausetime` and the SIGHT-to-AMBUSH spawnflag rewrite. The
/// stand is `infantry_stand`, which C's HuntTarget calls for this
/// STAND_GROUND monster once FindTarget succeeds.
pub fn sp_turret_driver(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Push;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
    ent.game.health = 100;
    ent.game.gib_health = 0;
    ent.game.mass = 200;
    ent.game.viewheight = 24.0;
    ent.die = Some(turret_driver_die);
    ent.game.flags |= EntityFlags::NO_KNOCKBACK;
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        aiflags: AiFlags::STAND_GROUND | AiFlags::DUCKED,
        stand: Some(infantry::infantry_stand),
        ..Default::default()
    }));
    ent.think = Some(turret_driver_link);
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ThinkFn;
    use crate::monster::test_support::{mi_mut, player_at};
    use crate::world::{test_world, test_world_recording};

    fn fields(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|&(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// A breach at `origin` teamed with a base (the master), plus a driver
    /// already linked to it, 32 units behind and 16 below.
    struct Turret {
        breach: EntityKey,
        base: EntityKey,
        driver: EntityKey,
    }

    fn turret(world: &mut GameWorld) -> Turret {
        let base = world.spawn().unwrap();
        sp_turret_base(&mut world.entities, base, &HashMap::new());
        let breach = world.spawn().unwrap();
        world.entities.get_mut(breach).unwrap().game.targetname = "gun".to_string();
        sp_turret_breach(&mut world.entities, breach, &HashMap::new());
        let base_ent = world.entities.get_mut(base).unwrap();
        base_ent.game.teamchain = Some(breach);
        base_ent.game.teammaster = Some(base);
        let b = world.entities.get_mut(breach).unwrap();
        b.game.teammaster = Some(base);
        b.game.flags |= EntityFlags::TEAMSLAVE;
        b.state.origin = Vec3f::new(0.0, 0.0, 64.0);
        let driver = world.spawn().unwrap();
        world.entities.get_mut(driver).unwrap().game.target = "gun".to_string();
        sp_turret_driver(&mut world.entities, driver, &HashMap::new());
        world.entities.get_mut(driver).unwrap().state.origin = Vec3f::new(-32.0, 0.0, 48.0);
        turret_driver_link(world, driver);
        Turret {
            breach,
            base,
            driver,
        }
    }

    #[test]
    fn angles_normalize_and_snap_to_eights_match_c() {
        assert_eq!(
            angles_normalize(Vec3f::new(-30.0, 725.0, -5.0)),
            Vec3f::new(330.0, 5.0, -5.0)
        );
        assert_eq!(
            angles_normalize(Vec3f::new(360.0, 0.0, 0.0)),
            Vec3f::new(360.0, 0.0, 0.0)
        );
        assert_eq!(snap_to_eights(1.06), 1.0);
        assert_eq!(snap_to_eights(1.07), 1.125);
        assert_eq!(snap_to_eights(-1.07), -1.125);
        assert_eq!(snap_to_eights(0.0), 0.0);
    }

    #[test]
    fn turret_breach_spawns_with_c_defaults() {
        let mut world = test_world();
        world.level.time = 2.0;
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().state.angles.y = 90.0;
        sp_turret_breach(&mut world.entities, key, &HashMap::new());
        turret_breach_finish(&mut world, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert!(ent.monsterinfo.is_none());
        assert_eq!((ent.solid, ent.game.movetype), (Solid::Bsp, MoveType::Push));
        assert_eq!((ent.game.speed, ent.game.dmg), (50.0, 10));
        assert_eq!(
            limits(&world, key),
            (Vec3f::new(30.0, 0.0, 0.0), Vec3f::new(-30.0, 360.0, 0.0))
        );
        assert_eq!((ent.game.ideal_yaw, ent.game.move_angles.y), (90.0, 90.0));
        assert!(ent.blocked.is_some());
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            turret_breach_finish_init as ThinkFn
        ));
        assert_eq!(ent.game.nextthink, 2.1);
    }

    #[test]
    fn turret_breach_takes_its_limits_speed_and_damage_from_the_map() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.speed = 20.0;
        ent.game.dmg = 3;
        let keys = fields(&[
            ("minpitch", "-10"),
            ("maxpitch", "45"),
            ("minyaw", "90"),
            ("maxyaw", "270"),
        ]);
        sp_turret_breach(&mut world.entities, key, &keys);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.speed, ent.game.dmg), (20.0, 3));
        assert_eq!(
            limits(&world, key),
            (Vec3f::new(10.0, 90.0, 0.0), Vec3f::new(-45.0, 270.0, 0.0))
        );
    }

    #[test]
    fn turret_base_is_a_blocking_pusher() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_turret_base(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.solid, ent.game.movetype), (Solid::Bsp, MoveType::Push));
        assert!(ent.blocked.is_some() && ent.monsterinfo.is_none());
    }

    #[test]
    fn turret_breach_finish_init_measures_and_removes_the_muzzle_target() {
        let mut world = test_world();
        let t = turret(&mut world);
        let muzzle = world.spawn().unwrap();
        let m = world.entities.get_mut(muzzle).unwrap();
        m.game.targetname = "tip".to_string();
        m.state.origin = Vec3f::new(40.0, 2.0, 70.0);
        let b = world.entities.get_mut(t.breach).unwrap();
        b.game.target = "tip".to_string();
        b.game.dmg = 7;
        turret_breach_finish_init(&mut world, t.breach);
        let b = world.entities.get(t.breach).unwrap();
        assert_eq!(b.game.move_origin, Vec3f::new(40.0, 2.0, 6.0));
        assert!(!world.entities.is_live(muzzle));
        assert_eq!(world.entities.get(t.base).unwrap().game.dmg, 7);
        assert!(std::ptr::fn_addr_eq(
            b.think.unwrap(),
            turret_breach_think as ThinkFn
        ));
    }

    #[test]
    fn turret_breach_without_a_target_says_so() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.classname = "turret_breach".to_string();
        sp_turret_breach(&mut world.entities, key, &HashMap::new());
        turret_breach_finish_init(&mut world, key);
        assert_eq!(
            gi.dprints.borrow().as_slice(),
            ["turret_breach at (0 0 0) needs a target\n"]
        );
    }

    #[test]
    fn turret_breach_think_turns_at_speed_within_its_limits_and_turns_the_base() {
        let mut world = test_world();
        world.level.time = 1.0;
        let t = turret(&mut world);
        world.entities.get_mut(t.breach).unwrap().game.move_angles = Vec3f::new(-50.0, 90.0, 0.0);
        turret_breach_think(&mut world, t.breach);
        let b = world.entities.get(t.breach).unwrap();
        // Pitch -50 (310) clamps to pos2 (-30); 50 deg/s caps both turns.
        assert_eq!(b.game.move_angles.x, -30.0);
        assert_eq!(b.avelocity, Vec3f::new(-50.0, 50.0, 0.0));
        assert!((b.game.nextthink - 1.1).abs() < 1e-6);
        assert_eq!(world.entities.get(t.base).unwrap().avelocity.y, 50.0);
        let d = world.entities.get(t.driver).unwrap();
        assert_eq!((d.avelocity.x, d.avelocity.y), (-50.0, 50.0));
    }

    #[test]
    fn turret_breach_think_clamps_yaw_to_the_nearer_limit() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let keys = fields(&[("minyaw", "90"), ("maxyaw", "180")]);
        sp_turret_breach(&mut world.entities, key, &keys);
        for (want, clamped) in [(80.0, 90.0), (200.0, 180.0)] {
            world.entities.get_mut(key).unwrap().game.move_angles = Vec3f::new(0.0, want, 0.0);
            turret_breach_think(&mut world, key);
            assert_eq!(
                world.entities.get(key).unwrap().game.move_angles.y,
                clamped,
                "{want}"
            );
        }
        // A delta past 180 turns the short way round.
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.angles.y = 350.0;
        ent.game.move_angles.y = 100.0;
        ent.game.speed = 2000.0;
        turret_breach_think(&mut world, key);
        assert!((world.entities.get(key).unwrap().avelocity.y - 1100.0).abs() < 1e-3);
    }

    #[test]
    fn turret_breach_think_with_a_negative_map_speed_turns_like_c() {
        // "speed" "-50" survives the spawn (only 0 is replaced). C's two
        // ifs then always leave the turn at +5 degrees a frame.
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.speed = -50.0;
        sp_turret_breach(&mut world.entities, key, &HashMap::new());
        world.entities.get_mut(key).unwrap().game.move_angles = Vec3f::new(-20.0, 90.0, 0.0);
        turret_breach_think(&mut world, key);
        assert_eq!(
            world.entities.get(key).unwrap().avelocity,
            Vec3f::new(50.0, 50.0, 0.0)
        );
    }

    #[test]
    fn turret_breach_think_drags_the_driver_to_its_seat() {
        let mut world = test_world();
        let t = turret(&mut world);
        let d = world.entities.get(t.driver).unwrap();
        assert_eq!(d.game.move_origin, Vec3f::new(32.0, 180.0, -16.0));
        // Move the driver off its seat; the breach pulls it back in a frame.
        world.entities.get_mut(t.driver).unwrap().state.origin = Vec3f::new(-30.0, 1.0, 40.0);
        world.entities.get_mut(t.breach).unwrap().game.move_angles = Vec3f::ZERO;
        turret_breach_think(&mut world, t.breach);
        let d = world.entities.get(t.driver).unwrap();
        assert!(
            (d.velocity - Vec3f::new(-20.0, -10.0, 80.0)).length() < 1e-3,
            "{:?}",
            d.velocity
        );
    }

    #[test]
    fn turret_breach_fires_a_rocket_when_its_driver_asks() {
        let (mut world, gi) = test_world_recording();
        let t = turret(&mut world);
        let b = world.entities.get_mut(t.breach).unwrap();
        b.game.move_origin = Vec3f::new(40.0, 0.0, 4.0);
        b.game.spawnflags |= SPAWNFLAG_FIRE;
        turret_breach_think(&mut world, t.breach);
        let rocket = world.find_by_classname(None, "rocket").unwrap();
        let r = world.entities.get(rocket).unwrap();
        assert_eq!(r.owner, Some(t.driver));
        assert!((r.state.origin - Vec3f::new(40.0, 0.0, 68.0)).length() < 1e-3);
        assert!(
            (r.velocity.length() - 600.0).abs() < 1e-2,
            "skill 1: 550 + 50"
        );
        assert!((100..150).contains(&r.game.dmg), "{}", r.game.dmg);
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.spawnflags & SPAWNFLAG_FIRE,
            0
        );
        let (_, ent, chan, index) = gi.positioned_sounds.borrow()[0];
        assert_eq!((Some(ent), chan), (world.number_of(t.breach), CHAN_WEAPON));
        assert_eq!(
            gi.sounds.borrow()[index as usize - 1],
            "weapons/rocklf1a.wav"
        );

        // Without a driver nothing fires.
        world.entities.get_mut(t.breach).unwrap().owner = None;
        world.entities.get_mut(t.breach).unwrap().game.spawnflags |= SPAWNFLAG_FIRE;
        turret_breach_think(&mut world, t.breach);
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.spawnflags & SPAWNFLAG_FIRE,
            SPAWNFLAG_FIRE
        );
    }

    #[test]
    fn turret_blocked_crushes_for_the_masters_damage_credited_to_the_driver() {
        let mut world = test_world();
        let t = turret(&mut world);
        world.entities.get_mut(t.base).unwrap().game.dmg = 25;
        let victim = world.spawn().unwrap();
        let v = world.entities.get_mut(victim).unwrap();
        v.game.takedamage = TakeDamage::Yes;
        v.game.health = 100;
        turret_blocked(&mut world, t.breach, victim);
        assert_eq!(world.entities.get(victim).unwrap().game.health, 75);

        let wall = world.spawn().unwrap();
        world.entities.get_mut(wall).unwrap().game.health = 100;
        turret_blocked(&mut world, t.breach, wall);
        assert_eq!(
            world.entities.get(wall).unwrap().game.health,
            100,
            "no takedamage"
        );
    }

    #[test]
    fn turret_driver_spawns_as_a_pusher_waiting_to_link() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_turret_driver(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.movetype, ent.solid),
            (MoveType::Push, Solid::Bbox)
        );
        assert_eq!(
            (ent.game.health, ent.game.mass, ent.game.viewheight),
            (100, 200, 24.0)
        );
        assert!(ent.game.flags.contains(EntityFlags::NO_KNOCKBACK));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert_eq!(mi.aiflags, AiFlags::STAND_GROUND | AiFlags::DUCKED);
        // HuntTarget calls it for a STAND_GROUND monster once FindTarget hits.
        assert!(mi.stand.is_some_and(|f| std::ptr::fn_addr_eq(
            f,
            infantry::infantry_stand as crate::entity::MonsterStandFn
        )));
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            turret_driver_link as ThinkFn
        ));
    }

    #[test]
    fn turret_driver_link_takes_the_seat_and_joins_the_team() {
        let mut world = test_world();
        world.level.time = 3.0;
        let t = turret(&mut world);
        let d = world.entities.get(t.driver).unwrap();
        assert_eq!(d.game.movetype, MoveType::Push);
        assert_eq!(d.game.target_ent, Some(t.breach));
        assert_eq!(d.game.teammaster, Some(t.base));
        assert!(d.game.flags.contains(EntityFlags::TEAMSLAVE));
        assert!(std::ptr::fn_addr_eq(
            d.think.unwrap(),
            turret_driver_think as ThinkFn
        ));
        assert_eq!(d.game.nextthink, 3.1);
        assert_eq!(world.entities.get(t.breach).unwrap().owner, Some(t.driver));
        assert_eq!(world.entities.get(t.base).unwrap().owner, Some(t.driver));
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.teamchain,
            Some(t.driver)
        );
    }

    #[test]
    fn turret_driver_aims_the_breach_and_fires_after_its_reaction_time() {
        let mut world = test_world();
        world.level.time = 1.0;
        let t = turret(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 42.0));
        world.entities.get_mut(t.driver).unwrap().game.enemy = Some(player);
        mi_mut(&mut world, t.driver).trail_time = 1.0;

        turret_driver_think(&mut world, t.driver);
        let aim = world.entities.get(t.breach).unwrap().game.move_angles;
        assert_eq!(
            aim,
            crate::weapons::vectoangles(Vec3f::new(300.0, 0.0, 0.0))
        );
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.spawnflags & SPAWNFLAG_FIRE,
            0
        );

        // Skill 1: two seconds in sight before the first shot.
        world.level.time = 3.0;
        turret_driver_think(&mut world, t.driver);
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.spawnflags & SPAWNFLAG_FIRE,
            SPAWNFLAG_FIRE
        );
        let mi = world
            .entities
            .get(t.driver)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap();
        assert_eq!(mi.attack_finished, 6.0);
    }

    #[test]
    fn turret_driver_finds_a_player_and_starts_its_reaction_clock() {
        let mut world = test_world();
        world.level.time = 4.0;
        let t = turret(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 42.0));
        mi_mut(&mut world, t.driver).aiflags |= AiFlags::LOST_SIGHT;
        turret_driver_think(&mut world, t.driver);
        assert_eq!(
            world.entities.get(t.driver).unwrap().game.enemy,
            Some(player)
        );
        let mi = mi_mut(&mut world, t.driver);
        assert_eq!(mi.trail_time, 4.0);
        assert!(!mi.aiflags.contains(AiFlags::LOST_SIGHT));
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.move_angles,
            crate::weapons::vectoangles(Vec3f::new(300.0, 0.0, 0.0))
        );
    }

    #[test]
    fn turret_driver_holds_fire_until_attack_finished() {
        let mut world = test_world();
        world.level.time = 10.0;
        let t = turret(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 42.0));
        world.entities.get_mut(t.driver).unwrap().game.enemy = Some(player);
        let mi = mi_mut(&mut world, t.driver);
        mi.trail_time = 0.0;
        mi.attack_finished = 10.5;
        turret_driver_think(&mut world, t.driver);
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.spawnflags & SPAWNFLAG_FIRE,
            0
        );
        assert_eq!(mi_mut(&mut world, t.driver).attack_finished, 10.5);

        world.level.time = 10.5;
        turret_driver_think(&mut world, t.driver);
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.spawnflags & SPAWNFLAG_FIRE,
            SPAWNFLAG_FIRE
        );
    }

    #[test]
    fn turret_driver_loses_sight_and_forgets_a_dead_enemy() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 5.0;
        let t = turret(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 42.0));
        world.entities.get_mut(t.driver).unwrap().game.enemy = Some(player);
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        turret_driver_think(&mut world, t.driver);
        assert!(mi_mut(&mut world, t.driver)
            .aiflags
            .contains(AiFlags::LOST_SIGHT));
        assert_eq!(
            world.entities.get(t.breach).unwrap().game.move_angles.x,
            0.0,
            "not aimed"
        );

        turret_driver_think(&mut world, t.driver);
        let mi = mi_mut(&mut world, t.driver);
        assert!(!mi.aiflags.contains(AiFlags::LOST_SIGHT), "back in sight");
        assert_eq!(mi.trail_time, 5.0);

        world.entities.get_mut(player).unwrap().game.health = 0;
        world.level.sight_client_key = None;
        turret_driver_think(&mut world, t.driver);
        assert_eq!(world.entities.get(t.driver).unwrap().game.enemy, None);
    }

    #[test]
    fn turret_driver_die_levels_the_gun_leaves_the_team_and_gibs() {
        let (mut world, gi) = test_world_recording();
        let t = turret(&mut world);
        world.entities.get_mut(t.breach).unwrap().game.move_angles.x = 20.0;
        // Killed: T_Damage calls die only at 0 health or less.
        world.entities.get_mut(t.driver).unwrap().game.health = -10;
        let count = world.entities.count();
        turret_driver_die(&mut world, t.driver, t.driver, t.driver, 50, Vec3f::ZERO);
        let b = world.entities.get(t.breach).unwrap();
        assert_eq!(
            (b.game.move_angles.x, b.owner, b.game.teamchain),
            (0.0, None, None)
        );
        assert_eq!(world.entities.get(t.base).unwrap().owner, None);
        let d = world.entities.get(t.driver).unwrap();
        assert_eq!(d.game.teammaster, None);
        assert!(!d.game.flags.contains(EntityFlags::TEAMSLAVE));
        assert_eq!(d.game.deadflag, DeadFlag::Dead);
        assert_eq!(world.entities.count(), count + 6, "2 bones, 4 meat");
        assert_eq!(gi.sounds.borrow()[0], "misc/udeath.wav");
    }

    const TURRET_MAP: &str = r#"
{ "classname" "worldspawn" }
{ "classname" "turret_base" "model" "*1" "team" "t1" }
{ "classname" "turret_breach" "model" "*2" "team" "t1" "targetname" "gun" "target" "tip" "origin" "0 0 64" }
{ "classname" "info_notnull" "targetname" "tip" "origin" "40 0 64" }
{ "classname" "turret_driver" "target" "gun" "origin" "-32 0 48" }
"#;

    #[test]
    #[ignore = "needs monster_start fix from the ai.rs branch (monstersB-4)"]
    fn turret_driver_does_not_fall_before_it_links() {
        // C SP_turret_driver is MOVETYPE_PUSH from the start, so the seat
        // offset turret_driver_link records is taken from the map origin.
        let mut world = test_world();
        world.spawn_entities("turret", TURRET_MAP, "");
        let driver = world.find_by_classname(None, "turret_driver").unwrap();
        let seat = world.entities.get(driver).unwrap().state.origin;
        world.run_frame();
        assert_eq!(world.entities.get(driver).unwrap().state.origin.z, seat.z);
    }

    #[test]
    fn turret_driver_without_a_breach_still_dies() {
        // A misnamed target leaves target_ent unset; C would dereference
        // NULL, the port skips the unlinking and still dies.
        let mut world = test_world();
        let driver = world.spawn().unwrap();
        world.entities.get_mut(driver).unwrap().game.target = "nothing".to_string();
        sp_turret_driver(&mut world.entities, driver, &HashMap::new());
        turret_driver_link(&mut world, driver);
        assert_eq!(world.entities.get(driver).unwrap().game.target_ent, None);
        world.entities.get_mut(driver).unwrap().game.health = -10;
        let count = world.entities.count();
        turret_driver_die(&mut world, driver, driver, driver, 50, Vec3f::ZERO);
        assert_eq!(
            world.entities.get(driver).unwrap().game.deadflag,
            DeadFlag::Dead
        );
        assert_eq!(world.entities.count(), count + 6, "2 bones, 4 meat");
    }

    #[test]
    fn turret_driver_whose_breach_is_gone_still_leaves_the_team_and_dies() {
        let mut world = test_world();
        let t = turret(&mut world);
        world.free_entity(t.breach);
        world.entities.get_mut(t.driver).unwrap().game.health = -10;
        turret_driver_die(&mut world, t.driver, t.driver, t.driver, 50, Vec3f::ZERO);
        let d = world.entities.get(t.driver).unwrap();
        assert_eq!(d.game.deadflag, DeadFlag::Dead);
        assert_eq!(d.game.teammaster, None);
        assert!(!d.game.flags.contains(EntityFlags::TEAMSLAVE));
        assert_eq!(world.entities.get(t.base).unwrap().owner, None);
    }

    /// A map turret (breach + base team, muzzle target, driver) with a
    /// player in view: the driver links, sights the player, aims the
    /// breach and fires a rocket at it.
    #[test]
    fn turret_on_a_map_sights_a_player_and_fires_a_rocket() {
        let mut world = test_world();
        world.spawn_entities("turret", TURRET_MAP, "");
        let driver = world.find_by_classname(None, "turret_driver").unwrap();
        assert_eq!(world.level.total_monsters, 1);
        let player = player_at(&mut world, Vec3f::new(400.0, 0.0, 42.0));

        let mut fired = None;
        for _ in 0..60 {
            // The real frame: only physics_pusher's team loop runs the
            // TEAMSLAVE driver's think once it has linked.
            world.run_frame();
            if let Some(rocket) = world.find_by_classname(None, "rocket") {
                fired = Some(rocket);
                break;
            }
        }
        assert_eq!(world.entities.get(driver).unwrap().game.enemy, Some(player));
        let rocket = fired.expect("the turret fired");
        assert_eq!(world.entities.get(rocket).unwrap().owner, Some(driver));
        assert!(
            world.entities.get(rocket).unwrap().velocity.x > 0.0,
            "toward the player"
        );
    }
}
