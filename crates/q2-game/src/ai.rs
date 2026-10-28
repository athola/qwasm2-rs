//! Monster AI and start-up — target selection, attack decisions, movement,
//! animation, and the world effects a monster feels each frame.
//!
//! Port of `g_ai.c` and of `g_monster.c` except `monster_fire_*`; monsters
//! step, turn and chase through `monster::movement` (C `m_move.c`).
//! Deviations from C: a player is always lit well enough to be seen (C
//! `light_level`, which nothing sets yet), heard noises skip the area
//! portal check (no `AreasConnected` import), and a monster starts on the
//! first frame of its move rather than a random one.
//!
//! # AI state machine
//! Monsters cycle: stand → (spot player) → run → (in range) → attack → pain → die.
//! Each state is a `MonsterMove`: a frame range, one `MonsterFrame` (AI
//! function, distance, think function) per frame, and an end function.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_ai.c`, `~/Qwasm2/src/game/g_monster.c`

use q2_shared::constants::{
    CONTENTS_MONSTER, CONTENTS_SOLID, CONTENTS_WINDOW, MASK_PLAYERSOLID, RF_FRAMELERP,
};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{Entity, EntityKey, MonsterInfo, ThinkFn};
use crate::monster::movement;
use crate::world::GameWorld;

/// C `MELEE_DISTANCE` (local.h).
pub const MELEE_DISTANCE: f32 = 80.0;

/// C `entity_state_t` effects and render flags (shared.h).
const EF_COLOR_SHELL: u32 = 0x0000_0100;
const EF_POWERSCREEN: u32 = 0x0000_0200;
pub const EF_FLIES: u32 = 0x0000_4000;
const RF_SHELL_RED: i32 = 1024;
const RF_SHELL_GREEN: i32 = 2048;
const RF_SHELL_BLUE: i32 = 4096;

/// Monster spawnflags (C uses the bare numbers): wake only on really
/// seeing a player, spawn when triggered, and the flag `monster_start`
/// turns into the first for monsters that are not good guys.
pub const SPAWNFLAG_AMBUSH: i32 = 1;
pub const SPAWNFLAG_TRIGGER_SPAWN: i32 = 2;
const SPAWNFLAG_SIGHT: i32 = 4;

/// C sets `pausetime` to this to stand until something happens.
const PAUSE_FOREVER: f32 = 100_000_000.0;

/// What `ai_checkattack` found out about the enemy this frame, for
/// `ai_run` and the default `checkattack`. C ref: the g_ai.c statics
/// `enemy_vis`, `enemy_infront`, `enemy_range` and `enemy_yaw`.
#[derive(Debug, Clone, Copy)]
pub struct EnemyCheck {
    pub vis: bool,
    pub infront: bool,
    pub range: Range,
    pub yaw: f32,
}

impl Default for EnemyCheck {
    /// C's zeroed statics: `enemy_range` 0 is RANGE_MELEE.
    fn default() -> Self {
        Self {
            vis: false,
            infront: false,
            range: Range::Melee,
            yaw: 0.0,
        }
    }
}

fn monsterinfo(world: &GameWorld, key: EntityKey) -> Option<&MonsterInfo> {
    world.entities.get(key)?.monsterinfo.as_deref()
}

fn monsterinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MonsterInfo> {
    world.entities.get_mut(key)?.monsterinfo.as_deref_mut()
}

fn aiflags(world: &GameWorld, key: EntityKey) -> AiFlags {
    monsterinfo(world, key).map_or(AiFlags::empty(), |mi| mi.aiflags)
}

/// Call one of the monster's state callbacks (`stand`, `walk`, `run`, ...)
/// if it has one; C calls them unchecked.
fn call(world: &mut GameWorld, key: EntityKey, pick: fn(&MonsterInfo) -> Option<ThinkFn>) {
    if let Some(callback) = monsterinfo(world, key).and_then(pick) {
        callback(world, key);
    }
}

fn origin(world: &GameWorld, key: EntityKey) -> Vec3f {
    world
        .entities
        .get(key)
        .map_or(Vec3f::ZERO, |e| e.state.origin)
}

fn is_client(world: &GameWorld, key: EntityKey) -> bool {
    world.entities.get(key).is_some_and(|e| e.client.is_some())
}

fn has_flag(world: &GameWorld, key: EntityKey, flag: EntityFlags) -> bool {
    world
        .entities
        .get(key)
        .is_some_and(|e| e.game.flags.contains(flag))
}

/// C `vtos`: "(x y z)" in whole units.
fn vtos(v: Vec3f) -> String {
    format!("({} {} {})", v.x as i32, v.y as i32, v.z as i32)
}

// ---------------------------------------------------------------------------
// AI movement functions — per-frame callbacks used in MonsterMove sequences
// ---------------------------------------------------------------------------

/// Stand, looking for players; a stand-ground monster with an enemy turns
/// to it and may attack, others walk off once `pausetime` passes or play
/// their idle sound now and then. C ref: `ai_stand` (g_ai.c).
pub fn ai_stand(world: &mut GameWorld, key: EntityKey, dist: f32) {
    if dist != 0.0 {
        let yaw = world.entities.get(key).map_or(0.0, |e| e.state.angles.y);
        movement::m_walkmove(world, key, yaw, dist);
    }

    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        let enemy = world.entities.get(key).and_then(|e| e.game.enemy);
        if let Some(enemy) = enemy {
            let enemy_origin = origin(world, enemy);
            let Some(ent) = world.entities.get_mut(key) else {
                return;
            };
            ent.game.ideal_yaw = movement::vectoyaw(enemy_origin - ent.state.origin);
            let turning = ent.state.angles.y != ent.game.ideal_yaw;
            if let Some(mi) = ent.monsterinfo.as_mut() {
                if turning && mi.aiflags.contains(AiFlags::TEMP_STAND_GROUND) {
                    mi.aiflags
                        .remove(AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND);
                    call(world, key, |mi| mi.run);
                }
            }
            movement::m_change_yaw(world, key);
            ai_checkattack(world, key);
        } else {
            find_target(world, key);
        }
        return;
    }

    if find_target(world, key) {
        return;
    }

    let time = world.level.time;
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let ambush = ent.game.spawnflags & SPAWNFLAG_AMBUSH != 0;
    let Some(mi) = ent.monsterinfo.as_ref() else {
        return;
    };
    if time > mi.pausetime {
        call(world, key, |mi| mi.walk);
        return;
    }
    if !ambush && mi.idle.is_some() && time > mi.idle_time {
        idle_sound(world, key, |mi| mi.idle);
    }
}

/// Play the idle (or search) sound callback no more than every 15 to 30
/// seconds; the first call only schedules it. Shared by C `ai_stand` and
/// `ai_walk`.
fn idle_sound(world: &mut GameWorld, key: EntityKey, pick: fn(&MonsterInfo) -> Option<ThinkFn>) {
    let time = world.level.time;
    let first = monsterinfo(world, key).is_none_or(|mi| mi.idle_time == 0.0);
    if !first {
        call(world, key, pick);
    }
    let delay = if first { 0.0 } else { 15.0 } + world.random() * 15.0;
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.idle_time = time + delay;
    }
}

/// Walk toward the goal, watching for players and now and then playing
/// the search sound. C ref: `ai_walk` (g_ai.c).
pub fn ai_walk(world: &mut GameWorld, key: EntityKey, dist: f32) {
    movement::m_move_to_goal(world, key, dist);

    // Check for noticing a player.
    if find_target(world, key) {
        return;
    }

    let time = world.level.time;
    if monsterinfo(world, key).is_some_and(|mi| mi.search.is_some() && time > mi.idle_time) {
        idle_sound(world, key, |mi| mi.search);
    }
}

/// Run at the enemy: straight to a combat point, after a heard noise,
/// into an attack, sideways when sliding, or after it along the player
/// trail once it is out of sight. C ref: `ai_run` (g_ai.c).
pub fn ai_run(world: &mut GameWorld, key: EntityKey, dist: f32) {
    let flags = aiflags(world, key);

    // If we're going to a combat point, just proceed.
    if flags.contains(AiFlags::COMBAT_POINT) {
        movement::m_move_to_goal(world, key, dist);
        return;
    }

    if flags.contains(AiFlags::SOUND_TARGET) {
        // C's special case: a projectile that made the noise is gone, so
        // only a remaining enemy close by makes the monster stop.
        let enemy = world.entities.get(key).and_then(|e| e.game.enemy);
        if let Some(enemy) = enemy {
            if (origin(world, key) - origin(world, enemy)).length() < 64.0 {
                if let Some(mi) = monsterinfo_mut(world, key) {
                    mi.aiflags |= AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND;
                }
                call(world, key, |mi| mi.stand);
                return;
            }
        }
        movement::m_move_to_goal(world, key, dist);
        if !find_target(world, key) {
            return;
        }
    }

    if ai_checkattack(world, key) {
        return;
    }

    if monsterinfo(world, key).is_some_and(|mi| mi.attack_state == AttackState::Sliding) {
        ai_run_slide(world, key, dist);
        return;
    }

    let time = world.level.time;
    if world.enemy_check.vis {
        movement::m_move_to_goal(world, key, dist);
        let enemy_origin = world
            .entities
            .get(key)
            .and_then(|e| e.game.enemy)
            .map(|e| origin(world, e));
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.aiflags.remove(AiFlags::LOST_SIGHT);
            if let Some(enemy_origin) = enemy_origin {
                mi.last_sighting = enemy_origin;
            }
            mi.trail_time = time;
        }
        return;
    }

    let search_time = monsterinfo(world, key).map_or(0.0, |mi| mi.search_time);
    if search_time != 0.0 && time > search_time + 20.0 {
        movement::m_move_to_goal(world, key, dist);
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.search_time = 0.0;
        }
        return;
    }

    pursue_lost_enemy(world, key, dist);
}

/// Head for the last sighting, then along the player trail, sidestepping
/// a blocked straight line. The lost-sight half of C `ai_run`; C points
/// `goalentity` at a temporary edict, here the goal is just its origin.
fn pursue_lost_enemy(world: &mut GameWorld, key: EntityKey, mut dist: f32) {
    /// Which C `PlayerTrail_Pick*` the flags ask for.
    enum Pick {
        None,
        First,
        Next(f32),
    }
    let time = world.level.time;
    let Some(mi) = world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_mut())
    else {
        return;
    };

    let mut new = false;
    if !mi.aiflags.contains(AiFlags::LOST_SIGHT) {
        // Just lost sight of the player: decide where to go first.
        mi.aiflags |= AiFlags::LOST_SIGHT | AiFlags::PURSUIT_LAST_SEEN;
        mi.aiflags
            .remove(AiFlags::PURSUE_NEXT | AiFlags::PURSUE_TEMP);
        new = true;
    }

    let mut pick = Pick::None;
    if mi.aiflags.contains(AiFlags::PURSUE_NEXT) {
        mi.aiflags.remove(AiFlags::PURSUE_NEXT);
        // Give ourself more time since we got this far.
        mi.search_time = time + 5.0;
        if mi.aiflags.contains(AiFlags::PURSUE_TEMP) {
            mi.aiflags.remove(AiFlags::PURSUE_TEMP);
            mi.last_sighting = mi.saved_goal;
            new = true;
        } else if mi.aiflags.contains(AiFlags::PURSUIT_LAST_SEEN) {
            mi.aiflags.remove(AiFlags::PURSUIT_LAST_SEEN);
            pick = Pick::First;
        } else {
            pick = Pick::Next(mi.trail_time);
        }
    }
    let marker = match pick {
        Pick::None => None,
        Pick::First => world.player_trail_pick_first(key),
        Pick::Next(trail_time) => world.trail.pick_next(trail_time),
    };

    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.monsterinfo.as_mut() else {
        return;
    };
    if let Some(marker) = marker {
        mi.last_sighting = marker.origin;
        mi.trail_time = marker.timestamp;
        ent.state.angles.y = marker.yaw;
        ent.game.ideal_yaw = marker.yaw;
        new = true;
    }

    let d1 = (ent.state.origin - mi.last_sighting).length();
    if d1 <= dist {
        mi.aiflags |= AiFlags::PURSUE_NEXT;
        dist = d1;
    }
    let mut goal = mi.last_sighting;

    if new {
        goal = sidestep_goal(world, key, goal);
    }

    movement::m_move_toward(world, key, Some(goal), dist);
}

/// When the straight line to `goal` is blocked, aim for a point to the
/// left or right of it that is clearer, remembering `goal` to come back
/// to (AI_PURSUE_TEMP). The `if (new)` block of C `ai_run`.
fn sidestep_goal(world: &mut GameWorld, key: EntityKey, goal: Vec3f) -> Vec3f {
    let Some(ent) = world.entities.get(key) else {
        return goal;
    };
    let (origin, mins, maxs) = (ent.state.origin, ent.mins, ent.maxs);
    let pass = world.number_of(key);
    let trace = |end: Vec3f| {
        world
            .gi
            .trace(origin, mins, maxs, end, pass, MASK_PLAYERSOLID)
            .fraction
    };
    let center = trace(goal);
    if center >= 1.0 {
        return goal;
    }

    let to_goal = goal - origin;
    let d1 = to_goal.length();
    let d2 = d1 * ((center + 1.0) / 2.0);
    let yaw = movement::vectoyaw(to_goal);
    let mut angles = world
        .entities
        .get(key)
        .map_or(Vec3f::ZERO, |e| e.state.angles);
    angles.y = yaw;
    let (forward, right, _) = crate::physics::angle_vectors(angles);
    // C G_ProjectSource with a zero up offset.
    let project = |x: f32, y: f32| origin + forward * x + right * y;

    let left_target = project(d2, -16.0);
    let left = trace(left_target);
    let right_target = project(d2, 16.0);
    let right_frac = trace(right_target);
    let center = (d1 * center) / d2;

    let side = if left >= center && left > right_frac {
        Some(if left < 1.0 {
            project(d2 * left * 0.5, -16.0)
        } else {
            left_target
        })
    } else if right_frac >= center && right_frac > left {
        Some(if right_frac < 1.0 {
            project(d2 * right_frac * 0.5, 16.0)
        } else {
            right_target
        })
    } else {
        None
    };

    let Some(ent) = world.entities.get_mut(key) else {
        return goal;
    };
    let Some(target) = side else {
        ent.state.angles.y = yaw;
        ent.game.ideal_yaw = yaw;
        return goal;
    };
    let side_yaw = movement::vectoyaw(target - origin);
    ent.state.angles.y = side_yaw;
    ent.game.ideal_yaw = side_yaw;
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.saved_goal = mi.last_sighting;
        mi.aiflags |= AiFlags::PURSUE_TEMP;
        mi.last_sighting = target;
    }
    target
}

/// Step along the facing without looking for players, as pain and death
/// frames do. C ref: `ai_move` (g_ai.c).
pub fn ai_move(world: &mut GameWorld, key: EntityKey, dist: f32) {
    let yaw = world.entities.get(key).map_or(0.0, |e| e.state.angles.y);
    movement::m_walkmove(world, key, yaw, dist);
}

/// Face the enemy and close in. C ref: `ai_charge` (g_ai.c).
pub fn ai_charge(world: &mut GameWorld, key: EntityKey, dist: f32) {
    let enemy_origin = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .and_then(|k| world.entities.get(k))
        .map(|e| e.state.origin);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if let Some(enemy_origin) = enemy_origin {
        ent.game.ideal_yaw = movement::vectoyaw(enemy_origin - ent.state.origin);
    }
    movement::m_change_yaw(world, key);
    if dist != 0.0 {
        let yaw = world.entities.get(key).map_or(0.0, |e| e.state.angles.y);
        movement::m_walkmove(world, key, yaw, dist);
    }
}

/// Step along the facing, watch for players, turn toward `ideal_yaw`.
/// C ref: `ai_turn` (g_ai.c).
pub fn ai_turn(world: &mut GameWorld, key: EntityKey, dist: f32) {
    if dist != 0.0 {
        let yaw = world.entities.get(key).map_or(0.0, |e| e.state.angles.y);
        movement::m_walkmove(world, key, yaw, dist);
    }
    if find_target(world, key) {
        return;
    }
    movement::m_change_yaw(world, key);
}

// ---------------------------------------------------------------------------
// AI utility functions
// ---------------------------------------------------------------------------

/// How far `other` is from `key`: melee (hostile even behind), near (seen
/// if in front or hostile), mid (seen only in front), far.
/// C ref: `range` (g_ai.c).
pub fn range(world: &GameWorld, key: EntityKey, other: EntityKey) -> Range {
    let len = (origin(world, key) - origin(world, other)).length();
    if len < MELEE_DISTANCE {
        Range::Melee
    } else if len < 500.0 {
        Range::Near
    } else if len < 1000.0 {
        Range::Mid
    } else {
        Range::Far
    }
}

/// Line of sight eye to eye, whichever way `key` faces.
/// C ref: `visible` (g_ai.c).
pub fn visible(world: &GameWorld, key: EntityKey, other: EntityKey) -> bool {
    let Some(them) = world.entities.get(other) else {
        return false;
    };
    let eye = them.state.origin + Vec3f::new(0.0, 0.0, them.game.viewheight);
    visible_point(world, key, eye)
}

/// Line of sight from `key`'s eye to `point`: C `visible` against an edict
/// with no view height, such as a player trail node.
pub(crate) fn visible_point(world: &GameWorld, key: EntityKey, point: Vec3f) -> bool {
    let Some(me) = world.entities.get(key) else {
        return false;
    };
    let trace = world.gi.trace(
        me.state.origin + Vec3f::new(0.0, 0.0, me.game.viewheight),
        Vec3f::ZERO,
        Vec3f::ZERO,
        point,
        world.number_of(key),
        MASK_OPAQUE,
    );
    trace.fraction == 1.0
}

/// Whether `other` is within about 72 degrees of `key`'s facing.
/// C ref: `infront` (g_ai.c).
pub fn infront(world: &GameWorld, key: EntityKey, other: EntityKey) -> bool {
    let (Some(me), Some(them)) = (world.entities.get(key), world.entities.get(other)) else {
        return false;
    };
    let (forward, _, _) = crate::physics::angle_vectors(me.state.angles);
    let vec = (them.state.origin - me.state.origin).normalize_or_zero();
    vec.dot(forward) > 0.3
}

/// Whether the monster faces within 45 degrees of `ideal_yaw`.
/// C ref: `FacingIdeal` (g_ai.c).
pub fn facing_ideal(world: &GameWorld, key: EntityKey) -> bool {
    let Some(ent) = world.entities.get(key) else {
        return false;
    };
    let delta = movement::anglemod(ent.state.angles.y - ent.game.ideal_yaw);
    !(delta > 45.0 && delta < 315.0)
}

/// Hold off the next missile attack for `time` seconds.
/// C ref: `AttackFinished` (g_monster.c).
pub fn attack_finished(world: &mut GameWorld, key: EntityKey, time: f32) {
    let until = world.level.time + time;
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.attack_finished = until;
    }
}

// ---------------------------------------------------------------------------
// Target selection
// ---------------------------------------------------------------------------

/// Go after the enemy: stand or run, face it when visible, and wait a
/// second before a first attack. C ref: `HuntTarget` (g_ai.c); where C
/// reads an unset vector for an unseen enemy, `ideal_yaw` is kept.
pub fn hunt_target(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let enemy = ent.game.enemy;
    ent.game.goalentity = enemy;
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        call(world, key, |mi| mi.stand);
    } else {
        call(world, key, |mi| mi.run);
    }
    if let Some(enemy) = enemy.filter(|&e| visible(world, key, e)) {
        let yaw = movement::vectoyaw(origin(world, enemy) - origin(world, key));
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.ideal_yaw = yaw;
        }
    }
    // Wait a while before first attack.
    if !aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        attack_finished(world, key, 1.0);
    }
}

/// The monster has a new enemy: wake other monsters, remember where it
/// was seen, and hunt it or first run to the combat point the mapper set.
/// C ref: `FoundTarget` (g_ai.c), without lighting up the sighting
/// monster (`light_level`, not ported).
pub fn found_target(world: &mut GameWorld, key: EntityKey) {
    let Some(enemy) = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .filter(|&e| world.entities.get(e).is_some_and(|e| e.in_use))
    else {
        return;
    };
    let (time, framenum) = (world.level.time, world.level.framenum);

    // Let other monsters see this monster for a while.
    if is_client(world, enemy) {
        world.level.sight_entity = Some(key);
        world.level.sight_entity_framenum = framenum;
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.light_level = 128;
        }
    }

    let enemy_origin = origin(world, enemy);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let combattarget = ent.game.combattarget.clone();
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.show_hostile = time + 1.0; // wake up other monsters
        mi.last_sighting = enemy_origin;
        mi.trail_time = time;
    }

    if combattarget.is_empty() {
        hunt_target(world, key);
        return;
    }

    let point = world.pick_target(&combattarget);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(point) = point else {
        ent.game.goalentity = Some(enemy);
        ent.game.movetarget = Some(enemy);
        let message = format!(
            "{} at {}, combattarget {} not found\n",
            ent.game.classname,
            vtos(ent.state.origin),
            combattarget
        );
        hunt_target(world, key);
        world.gi.dprintf(&message);
        return;
    };
    ent.game.goalentity = Some(point);
    ent.game.movetarget = Some(point);
    // Clear out our combattarget, these are a one shot deal.
    ent.game.combattarget.clear();
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.aiflags |= AiFlags::COMBAT_POINT;
        mi.pausetime = 0.0;
    }
    // Clear the targetname, that point is ours!
    if let Some(point) = world.entities.get_mut(point) {
        point.game.targetname.clear();
    }
    // Run for it.
    call(world, key, |mi| mi.run);
}

/// Look for a player to attack: the one other monsters just saw, a noise
/// a player made, or this frame's sight client, within range, in light
/// and line of sight, and in front unless it is close and hostile.
/// Returns whether an enemy was found. C ref: `FindTarget` (g_ai.c).
/// Deviations: no darkness check and no area portal check (see the
/// module docs), and an unset sight or sound entity never takes its
/// branch (C would follow a null pointer).
pub fn find_target(world: &mut GameWorld, key: EntityKey) -> bool {
    let Some(ent) = world.entities.get(key) else {
        return false;
    };
    let flags = aiflags(world, key);
    // Good guys never look; going to a combat point, just proceed.
    if flags.intersects(AiFlags::GOOD_GUY | AiFlags::COMBAT_POINT) {
        return false;
    }
    // An ambusher wakes up only on really seeing the player, not on
    // another monster getting angry or on hearing something.
    let ambush = ent.game.spawnflags & SPAWNFLAG_AMBUSH != 0;
    let own_enemy = ent.game.enemy;
    let level = &world.level;
    let recent = |framenum: i32| framenum >= level.framenum - 1;

    let mut heardit = false;
    let client = if recent(level.sight_entity_framenum) && !ambush && level.sight_entity.is_some() {
        let sight_entity = level.sight_entity;
        let their_enemy = sight_entity
            .and_then(|e| world.entities.get(e))
            .and_then(|e| e.game.enemy);
        if their_enemy == own_enemy {
            return false;
        }
        sight_entity
    } else if recent(level.sound_entity_framenum) && level.sound_entity.is_some() {
        heardit = true;
        level.sound_entity
    } else if own_enemy.is_none()
        && recent(level.sound2_entity_framenum)
        && !ambush
        && level.sound2_entity.is_some()
    {
        heardit = true;
        level.sound2_entity
    } else {
        level.sight_client_key
    };

    // If the entity went away, forget it.
    let Some((client, them)) = client
        .and_then(|c| world.entities.get(c).map(|e| (c, e)))
        .filter(|(_, e)| e.in_use)
    else {
        return false;
    };
    if them.client.is_some() && world.level.intermission_time != 0.0 {
        return false;
    }
    if Some(client) == own_enemy {
        return true;
    }

    let notarget =
        |k: Option<EntityKey>| k.is_some_and(|k| has_flag(world, k, EntityFlags::NOTARGET));
    if them.client.is_some() {
        if them.game.flags.contains(EntityFlags::NOTARGET) {
            return false;
        }
    } else if them.svflags & SvFlags::MONSTER.bits() != 0 {
        if them.game.enemy.is_none() || notarget(them.game.enemy) {
            return false;
        }
    } else if heardit {
        if notarget(them.owner) {
            return false;
        }
    } else {
        return false;
    }

    let time = world.level.time;
    if !heardit {
        let r = range(world, key, client);
        if r == Range::Far {
            return false;
        }
        // Is the client in a spot too dark to be seen?
        if them.game.light_level <= 5 {
            return false;
        }
        if !visible(world, key, client) {
            return false;
        }
        let show_hostile = them.monsterinfo.as_ref().map_or(0.0, |mi| mi.show_hostile);
        let seen = match r {
            Range::Near => show_hostile >= time || infront(world, key, client),
            Range::Mid => infront(world, key, client),
            _ => true,
        };
        if !seen {
            return false;
        }

        let is_noise = them.game.classname == "player_noise";
        let (their_enemy, client_is_client) = (them.game.enemy, them.client.is_some());
        let mut enemy = Some(client);
        if !is_noise {
            if !client_is_client {
                // A monster that saw a player: the player is the enemy.
                enemy = their_enemy.filter(|&e| is_client(world, e));
            }
            if let Some(mi) = monsterinfo_mut(world, key) {
                mi.aiflags.remove(AiFlags::SOUND_TARGET);
            }
        }
        let Some(ent) = world.entities.get_mut(key) else {
            return false;
        };
        ent.game.enemy = enemy;
        if enemy.is_none() {
            return false;
        }
    } else {
        let client_origin = them.state.origin;
        let my_origin = origin(world, key);
        if ambush {
            if !visible(world, key, client) {
                return false;
            }
        } else if !world.gi.in_phs(my_origin, client_origin) {
            return false;
        }
        let temp = client_origin - my_origin;
        if temp.length() > 1000.0 {
            return false; // too far to hear
        }
        // Check area portals: if they are different and not connected
        // then we can't hear it.
        let (my_area, their_area) = (
            world.entities.get(key).map_or(0, |e| e.areanum),
            them.areanum,
        );
        if their_area != my_area && !world.gi.areas_connected(my_area, their_area) {
            return false;
        }
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.ideal_yaw = movement::vectoyaw(temp);
        }
        movement::m_change_yaw(world, key);
        // Hunt the sound for a bit; hopefully find the real player.
        let Some(ent) = world.entities.get_mut(key) else {
            return false;
        };
        ent.game.enemy = Some(client);
        if let Some(mi) = ent.monsterinfo.as_mut() {
            mi.aiflags |= AiFlags::SOUND_TARGET;
        }
    }

    found_target(world, key);

    let sight = monsterinfo(world, key)
        .filter(|mi| !mi.aiflags.contains(AiFlags::SOUND_TARGET))
        .and_then(|mi| mi.sight);
    let enemy = world.entities.get(key).and_then(|e| e.game.enemy);
    if let (Some(sight), Some(enemy)) = (sight, enemy) {
        sight(world, key, enemy);
    }
    true
}

/// Pick the next live client, round-robin, for monsters to check this
/// frame; none if no client is alive. C ref: `AI_SetSightClient` (g_ai.c).
pub fn ai_set_sight_client(world: &mut GameWorld) {
    let maxclients = world.game.maxclients.max(0) as usize;
    let start = world
        .level
        .sight_client_key
        .and_then(|k| world.number_of(k))
        .filter(|&n| (1..=maxclients).contains(&n))
        .unwrap_or(maxclients);
    world.level.sight_client_key = (1..=maxclients)
        .map(|i| (start + i - 1) % maxclients + 1)
        .filter_map(|n| world.entities.key_for_number(n))
        .find(|&k| {
            world.entities.get(k).is_some_and(|e| {
                e.in_use
                    && e.client.is_some()
                    && e.game.health > 0
                    && !e.game.flags.contains(EntityFlags::NOTARGET)
            })
        });
}

// ---------------------------------------------------------------------------
// Attack decisions
// ---------------------------------------------------------------------------

/// The default `checkattack`: with a clear shot, melee in melee range
/// (on easy only one time in four), otherwise fire a missile by a chance
/// set by range and skill; a flyer that holds fire may slide sideways.
/// C ref: `M_CheckAttack` (g_ai.c).
pub fn m_check_attack(world: &mut GameWorld, key: EntityKey) -> bool {
    let Some(enemy) = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .filter(|&e| world.entities.get(e).is_some_and(|e| e.in_use))
    else {
        return false;
    };
    let (Some(me), Some(them)) = (world.entities.get(key), world.entities.get(enemy)) else {
        return false;
    };

    if them.game.health > 0 {
        // See if any entities are in the way of the shot.
        let spot1 = me.state.origin + Vec3f::new(0.0, 0.0, me.game.viewheight);
        let spot2 = them.state.origin + Vec3f::new(0.0, 0.0, them.game.viewheight);
        let tr = world.gi.trace(
            spot1,
            Vec3f::ZERO,
            Vec3f::ZERO,
            spot2,
            world.number_of(key),
            CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_SLIME | CONTENTS_LAVA | CONTENTS_WINDOW,
        );
        // Do we have a clear shot?
        if tr.ent_index != world.number_of(enemy) {
            return false;
        }
    }

    let skill = world.game.skill;
    let enemy_range = world.enemy_check.range;
    if enemy_range == Range::Melee {
        // Don't always melee in easy mode.
        if skill == SKILL_EASY && world.random_u32() & 3 != 0 {
            return false;
        }
        let Some(mi) = monsterinfo_mut(world, key) else {
            return false;
        };
        mi.attack_state = if mi.melee.is_some() {
            AttackState::Melee
        } else {
            AttackState::Missile
        };
        return true;
    }

    // Missile attack.
    let time = world.level.time;
    let Some(mi) = monsterinfo(world, key) else {
        return false;
    };
    if mi.attack.is_none() || time < mi.attack_finished || enemy_range == Range::Far {
        return false;
    }
    let mut chance = if mi.aiflags.contains(AiFlags::STAND_GROUND) {
        0.4
    } else if enemy_range == Range::Near {
        0.1
    } else if enemy_range == Range::Mid {
        0.02
    } else {
        return false;
    };
    if skill == SKILL_EASY {
        chance *= 0.5;
    } else if skill >= SKILL_HARD {
        chance *= 2.0;
    }

    if world.random() < chance {
        let wait = 2.0 * world.random();
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.attack_state = AttackState::Missile;
            mi.attack_finished = time + wait;
        }
        return true;
    }

    if has_flag(world, key, EntityFlags::FLY) {
        let state = if world.random() < 0.3 {
            AttackState::Sliding
        } else {
            AttackState::Straight
        };
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.attack_state = state;
        }
    }
    false
}

/// Turn to the enemy and start the melee attack once facing it.
/// C ref: `ai_run_melee` (g_ai.c).
pub fn ai_run_melee(world: &mut GameWorld, key: EntityKey) {
    face_enemy_then(world, key, |mi| mi.melee);
}

/// Turn in place to the enemy and fire once facing it.
/// C ref: `ai_run_missile` (g_ai.c).
pub fn ai_run_missile(world: &mut GameWorld, key: EntityKey) {
    face_enemy_then(world, key, |mi| mi.attack);
}

fn face_enemy_then(
    world: &mut GameWorld,
    key: EntityKey,
    pick: fn(&MonsterInfo) -> Option<ThinkFn>,
) {
    let yaw = world.enemy_check.yaw;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.ideal_yaw = yaw;
    movement::m_change_yaw(world, key);
    if !facing_ideal(world, key) {
        return;
    }
    if let Some(attack) = monsterinfo(world, key).and_then(pick) {
        attack(world, key);
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.attack_state = AttackState::Straight;
        }
    }
}

/// Strafe sideways at about the same range, switching sides when blocked.
/// C ref: `ai_run_slide` (g_ai.c).
pub fn ai_run_slide(world: &mut GameWorld, key: EntityKey, distance: f32) {
    let yaw = world.enemy_check.yaw;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.ideal_yaw = yaw;
    movement::m_change_yaw(world, key);
    let lefty = monsterinfo(world, key).is_some_and(|mi| mi.lefty);
    let ofs = if lefty { 90.0 } else { -90.0 };
    if movement::m_walkmove(world, key, yaw + ofs, distance) {
        return;
    }
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.lefty = !mi.lefty;
    }
    movement::m_walkmove(world, key, yaw - ofs, distance);
}

/// Whether the enemy is gone or dead enough: a medic's patient is "dead"
/// once healed, a brutal monster keeps hitting a corpse until it gibs.
/// C ref: `hesDeadJim` (g_ai.c).
fn hes_dead_jim(world: &GameWorld, key: EntityKey) -> bool {
    let Some(enemy) = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .and_then(|e| world.entities.get(e))
        .filter(|e| e.in_use)
    else {
        return true;
    };
    let flags = aiflags(world, key);
    if flags.contains(AiFlags::MEDIC) {
        return enemy.game.health > 0;
    }
    if enemy.client.is_some() && world.level.intermission_time != 0.0 {
        return true;
    }
    if flags.contains(AiFlags::BRUTAL) {
        return enemy.game.health <= -80;
    }
    enemy.game.health <= 0
}

/// C `edict_t.last_sound_time` of a player noise. C sets it to
/// `level.time` exactly when it records the noise's level framenum, and
/// `level.time` is `framenum * FRAMETIME`, so the framenum gives it back;
/// anything that is not the current noise reads as C's never-set zero.
fn last_sound_time(world: &GameWorld, noise: EntityKey) -> f32 {
    let level = &world.level;
    if level.sound_entity == Some(noise) {
        level.sound_entity_framenum as f32 * FRAMETIME
    } else if level.sound2_entity == Some(noise) {
        level.sound2_entity_framenum as f32 * FRAMETIME
    } else {
        0.0
    }
}

/// Decide whether to attack, or what else to do, and record what is
/// known about the enemy in `world.enemy_check`. Returns whether the
/// monster acted (attacked, changed enemy or gave up). Used by `ai_run`
/// and stand-ground `ai_stand`. C ref: `ai_checkattack` (g_ai.c).
pub fn ai_checkattack(world: &mut GameWorld, key: EntityKey) -> bool {
    let time = world.level.time;
    let Some(ent) = world.entities.get(key) else {
        world.enemy_check.vis = false;
        return false;
    };
    let (goal, enemy, movetarget) = (ent.game.goalentity, ent.game.enemy, ent.game.movetarget);

    // This causes monsters to run blindly to the combat point w/o firing.
    if let Some(goal) = goal {
        let flags = aiflags(world, key);
        if flags.contains(AiFlags::COMBAT_POINT) {
            return false;
        }
        if flags.contains(AiFlags::SOUND_TARGET) && !visible(world, key, goal) {
            let heard = enemy.map_or(0.0, |e| last_sound_time(world, e));
            let Some(ent) = world.entities.get_mut(key) else {
                return false;
            };
            if time - heard > 5.0 {
                if ent.game.goalentity == ent.game.enemy {
                    ent.game.goalentity = movetarget;
                }
                if let Some(mi) = ent.monsterinfo.as_mut() {
                    mi.aiflags.remove(AiFlags::SOUND_TARGET);
                    if mi.aiflags.contains(AiFlags::TEMP_STAND_GROUND) {
                        mi.aiflags
                            .remove(AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND);
                    }
                }
            } else {
                if let Some(mi) = ent.monsterinfo.as_mut() {
                    mi.show_hostile = time + 1.0;
                }
                return false;
            }
        }
    }

    world.enemy_check.vis = false;

    // See if the enemy is dead.
    if hes_dead_jim(world, key) {
        let Some(ent) = world.entities.get_mut(key) else {
            return false;
        };
        ent.game.enemy = None;
        if let Some(mi) = ent.monsterinfo.as_mut() {
            mi.aiflags.remove(AiFlags::MEDIC);
        }
        let oldenemy = ent
            .game
            .oldenemy
            .filter(|&o| world.entities.get(o).is_some_and(|o| o.game.health > 0));
        let Some(ent) = world.entities.get_mut(key) else {
            return false;
        };
        if let Some(oldenemy) = oldenemy {
            ent.game.enemy = Some(oldenemy);
            ent.game.oldenemy = None;
            hunt_target(world, key);
        } else {
            if let Some(movetarget) = ent.game.movetarget {
                ent.game.goalentity = Some(movetarget);
                call(world, key, |mi| mi.walk);
            } else {
                // Without the pausetime the stand code would just revert
                // to walking with no target, hunting the world entity.
                if let Some(mi) = ent.monsterinfo.as_mut() {
                    mi.pausetime = time + PAUSE_FOREVER;
                }
                call(world, key, |mi| mi.stand);
            }
            return true;
        }
    }

    // Wake up other monsters.
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.show_hostile = time + 1.0;
    }

    // Check knowledge of enemy.
    let enemy = world.entities.get(key).and_then(|e| e.game.enemy);
    let vis = enemy.is_some_and(|e| visible(world, key, e));
    world.enemy_check.vis = vis;
    if let (true, Some(enemy)) = (vis, enemy) {
        let enemy_origin = origin(world, enemy);
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.search_time = time + 5.0;
            mi.last_sighting = enemy_origin;
        }
    }

    // Look for other coop players here.
    if world.game.coop
        && monsterinfo(world, key).is_some_and(|mi| mi.search_time < time)
        && find_target(world, key)
    {
        return true;
    }

    if let Some(enemy) = world.entities.get(key).and_then(|e| e.game.enemy) {
        world.enemy_check.infront = infront(world, key, enemy);
        world.enemy_check.range = range(world, key, enemy);
        world.enemy_check.yaw = movement::vectoyaw(origin(world, enemy) - origin(world, key));
    }

    match monsterinfo(world, key).map(|mi| mi.attack_state) {
        Some(AttackState::Missile) => {
            ai_run_missile(world, key);
            return true;
        }
        Some(AttackState::Melee) => {
            ai_run_melee(world, key);
            return true;
        }
        _ => {}
    }

    // If enemy is not currently visible, we will never attack.
    if !world.enemy_check.vis {
        return false;
    }
    let checkattack = monsterinfo(world, key)
        .and_then(|mi| mi.checkattack)
        .unwrap_or(m_check_attack);
    checkattack(world, key)
}

// ---------------------------------------------------------------------------
// Monster animation system
// ---------------------------------------------------------------------------

/// Advance the current animation frame, then run that frame's AI function
/// with its distance times `monsterinfo.scale` and its think function.
/// Called each game tick. C ref: `M_MoveFrame` (g_monster.c). A frame
/// past the end of the move's table runs nothing (C would read past it).
pub fn monster_move_frame(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    // First, so an end function can stop the thinking.
    ent.game.nextthink = time + FRAMETIME;
    let Some(mi) = ent.monsterinfo.as_mut() else {
        return;
    };
    let Some(cm) = mi.currentmove.clone() else {
        return;
    };

    if mi.nextframe != 0 && (cm.firstframe..=cm.lastframe).contains(&mi.nextframe) {
        if ent.state.frame != mi.nextframe {
            ent.state.frame = mi.nextframe;
            mi.aiflags.remove(AiFlags::HOLD_FRAME);
        }
        mi.nextframe = 0;
    } else {
        // Keep a nextframe outside this move from leaking into a later one.
        mi.nextframe = 0;
        if ent.state.frame == cm.lastframe {
            if let Some(endfunc) = cm.endfunc {
                endfunc(world, key);
                if world
                    .entities
                    .get(key)
                    .is_none_or(|e| e.svflags & SvFlags::DEADMONSTER.bits() != 0)
                {
                    return;
                }
            }
        }
        // The end function very likely changed the move.
        let Some(ent) = world.entities.get_mut(key) else {
            return;
        };
        let Some(mi) = ent.monsterinfo.as_mut() else {
            return;
        };
        let Some(cm) = mi.currentmove.as_ref() else {
            return;
        };
        if !(cm.firstframe..=cm.lastframe).contains(&ent.state.frame) {
            mi.aiflags.remove(AiFlags::HOLD_FRAME);
            ent.state.frame = cm.firstframe;
        } else if !mi.aiflags.contains(AiFlags::HOLD_FRAME) {
            ent.state.frame += 1;
            if ent.state.frame > cm.lastframe {
                ent.state.frame = cm.firstframe;
            }
        }
    }

    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let Some(mi) = ent.monsterinfo.as_ref() else {
        return;
    };
    let Some(frame) = mi.currentmove.as_ref().and_then(|cm| {
        usize::try_from(ent.state.frame - cm.firstframe)
            .ok()
            .and_then(|index| cm.frames.get(index))
            .copied()
    }) else {
        return;
    };
    let dist = if mi.aiflags.contains(AiFlags::HOLD_FRAME) {
        0.0
    } else {
        frame.dist * mi.scale
    };
    if let Some(aifunc) = frame.aifunc {
        aifunc(world, key, dist);
    }
    if let Some(thinkfunc) = frame.thinkfunc {
        thinkfunc(world, key);
    }
}

/// Animate, then react to where the monster now is: find the ground
/// after it was moved, check the water it is in, drown or burn, and set
/// its shell effects. The `think` of every started monster.
/// C ref: `monster_think` (g_monster.c).
pub fn monster_think(world: &mut GameWorld, key: EntityKey) {
    monster_move_frame(world, key);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let linkcount = ent.linkcount;
    let moved = ent
        .monsterinfo
        .as_mut()
        .filter(|mi| mi.linkcount != linkcount)
        .map(|mi| mi.linkcount = linkcount)
        .is_some();
    if moved {
        world.m_check_ground(key);
    }
    m_catagorize_position(world, key);
    m_world_effects(world, key);
    m_set_effects(world, key);
}

// ---------------------------------------------------------------------------
// World effects
// ---------------------------------------------------------------------------

/// Set `waterlevel` (0 dry, 1 feet, 2 waist, 3 under) and `watertype`
/// from the contents at the bottom of the box and 26 and 48 units up.
/// C ref: `M_CatagorizePosition` (g_monster.c).
pub fn m_catagorize_position(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let mut point = Vec3f::new(
        (ent.absmax.x + ent.absmin.x) / 2.0,
        (ent.absmax.y + ent.absmin.y) / 2.0,
        ent.absmin.z + 2.0,
    );
    let mut levels = (0, 0);
    let cont = world.gi.point_contents(point);
    if cont & MASK_WATER != 0 {
        levels = (1, cont);
        point.z += 26.0;
        if world.gi.point_contents(point) & MASK_WATER != 0 {
            levels.0 = 2;
            point.z += 22.0;
            if world.gi.point_contents(point) & MASK_WATER != 0 {
                levels.0 = 3;
            }
        }
    }
    if let Some(ent) = world.entities.get_mut(key) {
        (ent.game.waterlevel, ent.game.watertype) = levels;
    }
}

/// Damage from the world itself (C `T_Damage(ent, world, world, ...)`).
pub(crate) fn world_damage(
    world: &mut GameWorld,
    key: EntityKey,
    damage: i32,
    dflags: DamageFlags,
    mod_: MeansOfDeath,
) {
    let Some(worldspawn) = world.entities.key_for_number(0) else {
        return;
    };
    let origin = origin(world, key);
    world.t_damage(
        key,
        worldspawn,
        worldspawn,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        damage,
        0,
        dflags,
        mod_,
    );
}

fn body_sound(world: &mut GameWorld, key: EntityKey, name: &str) {
    let sound = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), CHAN_BODY, sound, 1.0, ATTN_NORM, 0.0);
}

/// Drown a monster too long under water, or suffocate a swimmer out of
/// it; burn it in lava and slime, and splash on entering and leaving.
/// C ref: `M_WorldEffects` (g_monster.c).
pub fn m_world_effects(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.health > 0 {
        let swims = ent.game.flags.contains(EntityFlags::SWIM);
        // Air lasts 12 s for a walker under water, 9 s for a swimmer out of it.
        let breathing = if swims {
            ent.game.waterlevel > 0
        } else {
            ent.game.waterlevel < 3
        };
        let air = if swims { 9.0 } else { 12.0 };
        let pain_debounce = ent.game.pain_debounce_time;
        let mut drown = None;
        if let Some(mi) = ent.monsterinfo.as_mut() {
            if breathing {
                mi.air_finished = time + air;
            } else if mi.air_finished < time && pain_debounce < time {
                drown = Some((2 + 2 * (time - mi.air_finished).floor() as i32).min(15));
            }
        }
        if let Some(dmg) = drown {
            // Drown (or suffocate)! The pain callback sees the old debounce.
            world_damage(world, key, dmg, DamageFlags::NO_ARMOR, MeansOfDeath::Water);
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.pain_debounce_time = time + 1.0;
            }
        }
    }

    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (waterlevel, watertype, flags) = (ent.game.waterlevel, ent.game.watertype, ent.game.flags);
    if waterlevel == 0 {
        if flags.contains(EntityFlags::INWATER) {
            body_sound(world, key, "player/watr_out.wav");
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.flags.remove(EntityFlags::INWATER);
            }
        }
        return;
    }

    let dead = ent.svflags & SvFlags::DEADMONSTER.bits() != 0;
    if watertype & CONTENTS_LAVA != 0 && !flags.contains(EntityFlags::IMMUNE_LAVA) {
        burn(world, key, 0.2, 10 * waterlevel, MeansOfDeath::Lava);
    }
    if watertype & CONTENTS_SLIME != 0 && !flags.contains(EntityFlags::IMMUNE_SLIME) && !dead {
        burn(world, key, 1.0, 4 * waterlevel, MeansOfDeath::Slime);
    }

    if !flags.contains(EntityFlags::INWATER) {
        let dead = world
            .entities
            .get(key)
            .is_some_and(|e| e.svflags & SvFlags::DEADMONSTER.bits() != 0);
        if !dead {
            if watertype & CONTENTS_LAVA != 0 {
                let name = if world.random() <= 0.5 {
                    "player/lava1.wav"
                } else {
                    "player/lava2.wav"
                };
                body_sound(world, key, name);
            } else if watertype & (CONTENTS_SLIME | CONTENTS_WATER) != 0 {
                body_sound(world, key, "player/watr_in.wav");
            }
        }
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.flags |= EntityFlags::INWATER;
            ent.game.damage_debounce_time = 0.0;
        }
    }
}

/// Lava or slime damage, at most once per `every` seconds.
fn burn(world: &mut GameWorld, key: EntityKey, every: f32, damage: i32, mod_: MeansOfDeath) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.damage_debounce_time < time {
        ent.game.damage_debounce_time = time + every;
        world_damage(world, key, damage, DamageFlags::empty(), mod_);
    }
}

/// Drop a monster placed in the air onto the floor below, up to 256
/// units down. C ref: `M_droptofloor` (g_monster.c).
pub fn m_droptofloor(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.origin.z += 1.0;
    let (start, mins, maxs) = (ent.state.origin, ent.mins, ent.maxs);
    let end = start - Vec3f::new(0.0, 0.0, 256.0);
    let trace = world.gi.trace(
        start,
        mins,
        maxs,
        end,
        world.number_of(key),
        MASK_MONSTERSOLID,
    );
    if trace.fraction == 1.0 || trace.allsolid {
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.origin = trace.endpos;
    }
    world.link_entity(key);
    world.m_check_ground(key);
    m_catagorize_position(world, key);
}

/// The red shell of a monster being resurrected, and the power screen or
/// green shell for 0.2 s after power armor saved a hit. C ref:
/// `M_SetEffects` (g_monster.c).
pub fn m_set_effects(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.effects &= !(EF_COLOR_SHELL | EF_POWERSCREEN);
    ent.state.renderfx &= !(RF_SHELL_RED | RF_SHELL_GREEN | RF_SHELL_BLUE);
    let resurrecting = ent
        .monsterinfo
        .as_ref()
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::RESURRECTING));
    if resurrecting {
        ent.state.effects |= EF_COLOR_SHELL;
        ent.state.renderfx |= RF_SHELL_RED;
    }
    if ent.game.health <= 0 || ent.game.powerarmor_time <= world.level.time {
        return;
    }
    match ent.monsterinfo.as_ref().map(|mi| mi.power_armor_type) {
        Some(POWER_ARMOR_SCREEN) => ent.state.effects |= EF_POWERSCREEN,
        Some(POWER_ARMOR_SHIELD) => {
            ent.state.effects |= EF_COLOR_SHELL;
            ent.state.renderfx |= RF_SHELL_GREEN;
        }
        _ => {}
    }
}

/// Stop the flies buzzing round a corpse. C ref: `M_FliesOff`.
pub fn m_flies_off(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.effects &= !EF_FLIES;
        ent.state.sound = 0;
    }
}

/// Flies buzz round a dry corpse for a minute. C ref: `M_FliesOn`.
pub fn m_flies_on(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.game.waterlevel != 0)
    {
        return;
    }
    let sound = world.gi.sound_index("infantry/inflies1.wav");
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.effects |= EF_FLIES;
        ent.state.sound = sound;
        ent.think = Some(m_flies_off);
        ent.game.nextthink = time + 60.0;
    }
}

/// Half of dry corpses draw flies 5 to 15 seconds later; a death move's
/// think. C ref: `M_FlyCheck` (g_monster.c).
pub fn m_fly_check(world: &mut GameWorld, key: EntityKey) {
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.game.waterlevel != 0)
    {
        return;
    }
    if world.random() > 0.5 {
        return;
    }
    let delay = 5.0 + 10.0 * world.random();
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.think = Some(m_flies_on);
        ent.game.nextthink = time + delay;
    }
}

/// End function of a death move: the corpse stops animating and falls.
/// C ref: the `*_dead` functions (e.g. `soldier_dead`), without their
/// per-monster corpse boxes.
pub fn monster_dead(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Toss;
    ent.svflags |= SvFlags::DEADMONSTER.bits();
    ent.game.nextthink = 0.0;
    world.link_entity(key);
}

// ---------------------------------------------------------------------------
// Using and trigger-spawning monsters
// ---------------------------------------------------------------------------

/// Using a monster makes it angry at the player (or good guy) who used
/// it. C ref: `monster_use` (g_monster.c).
pub fn monster_use(world: &mut GameWorld, key: EntityKey, _other: EntityKey, activator: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.enemy.is_some() || ent.game.health <= 0 {
        return;
    }
    let Some(them) = world.entities.get(activator) else {
        return;
    };
    if them.game.flags.contains(EntityFlags::NOTARGET) {
        return;
    }
    let good_guy = them
        .monsterinfo
        .as_ref()
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::GOOD_GUY));
    if them.client.is_none() && !good_guy {
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.enemy = Some(activator);
    }
    found_target(world, key);
}

/// Appear: telefrag what is in the way, become solid and visible, start
/// as usual, and go after the player who triggered the spawn (unless an
/// ambusher). C ref: `monster_triggered_spawn` (g_monster.c).
pub fn monster_triggered_spawn(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.origin.z += 1.0;
    }
    world.kill_box(key);
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.solid = Solid::Bbox;
    ent.game.movetype = MoveType::Step;
    ent.svflags &= !SvFlags::NOCLIENT.bits();
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.air_finished = time + 12.0;
    }
    world.link_entity(key);

    monster_start_go(world, key);

    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let ambush = ent.game.spawnflags & SPAWNFLAG_AMBUSH != 0;
    let enemy = ent.game.enemy;
    if enemy.is_some_and(|e| !ambush && !has_flag(world, e, EntityFlags::NOTARGET)) {
        found_target(world, key);
    } else if let Some(ent) = world.entities.get_mut(key) {
        ent.game.enemy = None;
    }
}

/// The `use` of a monster waiting to be trigger-spawned: spawn next
/// frame, so as not to telefrag the player who triggered it, and be an
/// ordinary monster from then on. C ref: `monster_triggered_spawn_use`.
pub fn monster_triggered_spawn_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let time = world.level.time;
    let activator_is_client = is_client(world, activator);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.think = Some(monster_triggered_spawn);
    ent.game.nextthink = time + FRAMETIME;
    if activator_is_client {
        ent.game.enemy = Some(activator);
    }
    ent.use_fn = Some(monster_use);
}

/// Hide a trigger-spawned monster until it is used.
/// C ref: `monster_triggered_start` (g_monster.c).
pub fn monster_triggered_start(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.solid = Solid::Not;
    ent.game.movetype = MoveType::None;
    ent.svflags |= SvFlags::NOCLIENT.bits();
    ent.game.nextthink = 0.0;
    ent.use_fn = Some(monster_triggered_spawn_use);
}

// ---------------------------------------------------------------------------
// Monster initialization
// ---------------------------------------------------------------------------

/// Common monster setup, run by `GameWorld::call_spawn` after a monster's
/// spawn function (C calls it from `walkmonster_start` and friends at the
/// end of `SP_monster_*`). Removes monsters in deathmatch and returns
/// whether the monster stays. The first think is the one the spawn
/// function chose with [`walkmonster_start`], [`flymonster_start`] or
/// [`swimmonster_start`], `walkmonster_start_go` if none.
/// C ref: `monster_start` (g_monster.c). Kept from before: movetype STEP,
/// SOLID_BBOX and a mass of 200 by default (C's `SP_*` set these), and
/// `pausetime` set already so a monster that thinks before
/// `monster_start_go` stands. Not here: the random start frame (see the
/// module docs), and `st.item`, which `call_spawn` resolves.
pub fn monster_start(world: &mut GameWorld, key: EntityKey) -> bool {
    if world.game.deathmatch {
        world.free_entity(key);
        return false;
    }
    let time = world.level.time;
    let good_guy = aiflags(world, key).contains(AiFlags::GOOD_GUY);
    let Some(ent) = world.entities.get_mut(key) else {
        return false;
    };

    if ent.game.spawnflags & SPAWNFLAG_SIGHT != 0 && !good_guy {
        ent.game.spawnflags &= !SPAWNFLAG_SIGHT;
        ent.game.spawnflags |= SPAWNFLAG_AMBUSH;
    }
    // C clears the flag only with g_fix_triggered, which defaults to 0.
    if ent.game.spawnflags & SPAWNFLAG_TRIGGER_SPAWN != 0 && ent.game.targetname.is_empty() {
        let message = format!(
            "triggered {} at {} has no targetname\n",
            ent.game.classname,
            vtos(ent.state.origin)
        );
        world.gi.dprintf(&message);
    }
    if !good_guy {
        world.level.total_monsters += 1;
    }

    let Some(ent) = world.entities.get_mut(key) else {
        return false;
    };
    ent.game.nextthink = time + FRAMETIME;
    ent.svflags |= SvFlags::MONSTER.bits();
    ent.state.renderfx |= RF_FRAMELERP;
    ent.game.takedamage = TakeDamage::Aim;
    ent.use_fn = Some(monster_use);
    if ent.game.max_health == 0 {
        ent.game.max_health = ent.game.health;
    }
    ent.clipmask = MASK_MONSTERSOLID;
    ent.game.deadflag = DeadFlag::No;
    ent.svflags &= !SvFlags::DEADMONSTER.bits();
    ent.state.old_origin = ent.state.origin;

    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    if ent.game.mass == 0 {
        ent.game.mass = 200;
    }
    if ent.think.is_none() {
        ent.think = Some(walkmonster_start_go);
    }
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.air_finished = time + 12.0;
        if mi.checkattack.is_none() {
            mi.checkattack = Some(m_check_attack);
        }
        mi.pausetime = PAUSE_FOREVER;
    }
    true
}

/// Start the monster on its path: turn a `target` of combat points into
/// its `combattarget`, then walk to a `path_corner` target or stand.
/// C ref: `monster_start_go` (g_monster.c).
pub fn monster_start_go(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.health <= 0 {
        return;
    }
    let (classname, at) = (ent.game.classname.clone(), ent.state.origin);
    let target = ent.game.target.clone();

    // Check for target to combat_point and change to combattarget.
    if !target.is_empty() {
        let (mut notcombat, mut fixup) = (false, false);
        let mut search = None;
        while let Some(t) = world.find_by_targetname(search, &target) {
            search = Some(t);
            if world
                .entities
                .get(t)
                .is_some_and(|t| t.game.classname == "point_combat")
            {
                fixup = true;
            } else {
                notcombat = true;
            }
        }
        let Some(ent) = world.entities.get_mut(key) else {
            return;
        };
        if fixup {
            ent.game.combattarget = target.clone();
        }
        if notcombat && !ent.game.combattarget.is_empty() {
            world.gi.dprintf(&format!(
                "{classname} at {} has target with mixed types\n",
                vtos(at)
            ));
        }
        if fixup {
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.target.clear();
            }
        }
    }

    // Validate combattarget.
    let combattarget = world
        .entities
        .get(key)
        .map(|e| e.game.combattarget.clone())
        .unwrap_or_default();
    if !combattarget.is_empty() {
        let mut search = None;
        while let Some(t) = world.find_by_targetname(search, &combattarget) {
            search = Some(t);
            let Some(point) = world.entities.get(t) else {
                continue;
            };
            if point.game.classname != "point_combat" {
                let p = point.state.origin;
                world.gi.dprintf(&format!(
                    "{classname} at ({} {} {}) has a bad combattarget {combattarget} : {} at ({} {} {})\n",
                    at.x as i32, at.y as i32, at.z as i32,
                    point.game.classname, p.x as i32, p.y as i32, p.z as i32
                ));
            }
        }
    }

    let target = world
        .entities
        .get(key)
        .map(|e| e.game.target.clone())
        .unwrap_or_default();
    let time = world.level.time;
    let mut stand = true;
    if !target.is_empty() {
        let movetarget = world.pick_target(&target);
        let path_corner = movetarget
            .and_then(|t| world.entities.get(t))
            .filter(|t| t.game.classname == "path_corner")
            .map(|t| t.state.origin);
        let Some(ent) = world.entities.get_mut(key) else {
            return;
        };
        ent.game.goalentity = movetarget;
        ent.game.movetarget = movetarget;
        if movetarget.is_none() {
            world.gi.dprintf(&format!(
                "{classname} can't find target {target} at {}\n",
                vtos(at)
            ));
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.target.clear();
            }
        } else if let Some(corner) = path_corner {
            let yaw = movement::vectoyaw(corner - ent.state.origin);
            ent.game.ideal_yaw = yaw;
            ent.state.angles.y = yaw;
            call(world, key, |mi| mi.walk);
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.target.clear();
            }
            stand = false;
        } else {
            ent.game.goalentity = None;
            ent.game.movetarget = None;
        }
    }
    if stand {
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.pausetime = PAUSE_FOREVER;
        }
        call(world, key, |mi| mi.stand);
    }

    if let Some(ent) = world.entities.get_mut(key) {
        ent.think = Some(monster_think);
        ent.game.nextthink = time + FRAMETIME;
    }
}

/// Spawn-function half of C `walkmonster_start`: the first think drops
/// the monster to the floor and starts it. `monster_start` runs after
/// the spawn function, in `GameWorld::call_spawn`.
pub fn walkmonster_start(ent: &mut Entity) {
    ent.think = Some(walkmonster_start_go);
}

/// Spawn-function half of C `flymonster_start`: the monster flies.
pub fn flymonster_start(ent: &mut Entity) {
    ent.game.flags |= EntityFlags::FLY;
    ent.think = Some(flymonster_start_go);
}

/// Spawn-function half of C `swimmonster_start`: the monster swims.
pub fn swimmonster_start(ent: &mut Entity) {
    ent.game.flags |= EntityFlags::SWIM;
    ent.think = Some(swimmonster_start_go);
}

/// Drop a walker placed at map load to the floor, then start it (or hide
/// it until triggered). C ref: `walkmonster_start_go` (g_monster.c).
pub fn walkmonster_start_go(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.spawnflags & SPAWNFLAG_TRIGGER_SPAWN == 0 && world.level.time < 1.0 {
        m_droptofloor(world, key);
        let grounded = world
            .entities
            .get(key)
            .is_some_and(|e| e.game.ground_entity.is_some());
        if grounded && !movement::m_walkmove(world, key, 0.0, 0.0) {
            report_in_solid(world, key);
        }
    }
    start_go(world, key, 20.0, 25.0);
}

/// C ref: `flymonster_start_go` (g_monster.c).
pub fn flymonster_start_go(world: &mut GameWorld, key: EntityKey) {
    if !movement::m_walkmove(world, key, 0.0, 0.0) {
        report_in_solid(world, key);
    }
    start_go(world, key, 10.0, 25.0);
}

/// C ref: `swimmonster_start_go` (g_monster.c).
pub fn swimmonster_start_go(world: &mut GameWorld, key: EntityKey) {
    start_go(world, key, 10.0, 10.0);
}

fn report_in_solid(world: &GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get(key) {
        world.gi.dprintf(&format!(
            "{} in solid at {}\n",
            ent.game.classname,
            vtos(ent.state.origin)
        ));
    }
}

/// The shared tail of the `*monster_start_go` functions: default turn
/// speed and eye height, then start, or wait to be trigger-spawned.
fn start_go(world: &mut GameWorld, key: EntityKey, yaw_speed: f32, viewheight: f32) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.yaw_speed == 0.0 {
        ent.game.yaw_speed = yaw_speed;
    }
    if ent.game.viewheight == 0.0 {
        ent.game.viewheight = viewheight;
    }
    if ent.game.spawnflags & SPAWNFLAG_TRIGGER_SPAWN != 0 {
        monster_triggered_start(world, key);
    } else {
        monster_start_go(world, key);
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_target_sight_trace_passes_the_monster_itself() {
        let (mut world, gi) = crate::world::test_world_recording();
        let monster = world.spawn().unwrap();
        // C FindTarget only looks at a client (or a monster or a noise).
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        world.entities.get_mut(player).unwrap().game.light_level = 128; // lit
        world.level.sight_client_key = Some(player);
        find_target(&mut world, monster);
        assert_eq!(
            gi.trace_pass_ents.borrow().first(),
            Some(&world.number_of(monster))
        );
    }

    #[test]
    fn sight_trace_is_blocked_by_slime_and_lava_like_c_mask_opaque() {
        let (mut world, gi) = crate::world::test_world_recording();
        let monster = world.spawn().unwrap();
        let player = world.spawn().unwrap();
        visible(&world, monster, player);
        assert_eq!(
            *gi.trace_masks.borrow(),
            [q2_shared::constants::MASK_OPAQUE]
        );
    }
    use crate::entity::{MonsterFrame, MonsterMove};
    use crate::monster::test_support::{floor_world, walker};
    use crate::world::test_world;

    /// A walker at the origin whose enemy stands behind a wall at x = 200.
    fn monster_with_hidden_enemy() -> (GameWorld, EntityKey) {
        let mut world = floor_world(Some(200.0));
        let key = walker(&mut world, 0.0, 0.0);
        let enemy = walker(&mut world, 300.0, 0.0);
        world.entities.get_mut(enemy).unwrap().monsterinfo = None;
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        world.level.time = 10.0;
        (world, key)
    }

    fn monsterinfo(world: &GameWorld, key: EntityKey) -> &crate::entity::MonsterInfo {
        world
            .entities
            .get(key)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap()
    }

    #[test]
    fn ai_run_marks_lost_sight_and_heads_for_the_last_sighting_when_the_enemy_is_hidden() {
        let (mut world, key) = monster_with_hidden_enemy();
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .last_sighting = Vec3f::new(5.0, 0.0, 24.0);

        ai_run(&mut world, key, 10.0);

        let mi = monsterinfo(&world, key);
        assert!(mi
            .aiflags
            .contains(AiFlags::LOST_SIGHT | AiFlags::PURSUIT_LAST_SEEN));
        // The last sighting is within this step: pursue the trail next.
        assert!(mi.aiflags.contains(AiFlags::PURSUE_NEXT));
    }

    #[test]
    fn ai_run_with_a_visible_enemy_records_the_sighting_and_clears_lost_sight() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        let enemy = walker(&mut world, 300.0, 0.0);
        world.level.time = 10.0;
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.enemy = Some(enemy);
            ent.monsterinfo.as_mut().unwrap().aiflags = AiFlags::LOST_SIGHT;
        }

        ai_run(&mut world, key, 10.0);

        let mi = monsterinfo(&world, key);
        assert!(!mi.aiflags.contains(AiFlags::LOST_SIGHT));
        assert_eq!(mi.last_sighting, Vec3f::new(300.0, 0.0, 24.0));
        assert_eq!(mi.trail_time, 10.0);
        assert_eq!(mi.search_time, 15.0);
    }

    #[test]
    fn ai_run_picks_the_next_trail_node_after_reaching_the_last_sighting() {
        let (mut world, key) = monster_with_hidden_enemy();
        world.trail.init();
        world.trail.add(Vec3f::new(50.0, 0.0, 24.0), 1.0);
        world.trail.add(Vec3f::new(50.0, 80.0, 24.0), 2.0);
        {
            let mi = world
                .entities
                .get_mut(key)
                .unwrap()
                .monsterinfo
                .as_mut()
                .unwrap();
            mi.aiflags = AiFlags::LOST_SIGHT | AiFlags::PURSUE_NEXT;
            mi.trail_time = 1.0;
        }

        ai_run(&mut world, key, 10.0);

        let mi = monsterinfo(&world, key);
        assert_eq!(mi.last_sighting, Vec3f::new(50.0, 80.0, 24.0));
        assert_eq!(mi.trail_time, 2.0);
        assert_eq!(mi.search_time, 15.0);
        // The node's yaw (from the previous node, +Y) is the new heading.
        assert_eq!(world.entities.get(key).unwrap().game.ideal_yaw, 90.0);
    }

    #[test]
    fn ai_run_returns_to_the_saved_goal_after_a_sidestep() {
        let (mut world, key) = monster_with_hidden_enemy();
        {
            let mi = world
                .entities
                .get_mut(key)
                .unwrap()
                .monsterinfo
                .as_mut()
                .unwrap();
            mi.aiflags = AiFlags::LOST_SIGHT | AiFlags::PURSUE_NEXT | AiFlags::PURSUE_TEMP;
            mi.saved_goal = Vec3f::new(0.0, 120.0, 24.0);
        }

        ai_run(&mut world, key, 10.0);

        let mi = monsterinfo(&world, key);
        assert_eq!(mi.last_sighting, Vec3f::new(0.0, 120.0, 24.0));
        assert!(!mi.aiflags.contains(AiFlags::PURSUE_TEMP));
    }

    #[test]
    fn ai_run_sidesteps_when_the_way_to_the_last_sighting_is_blocked() {
        let (mut world, key) = monster_with_hidden_enemy();
        // Straight ahead through the wall at x = 200.
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .last_sighting = Vec3f::new(400.0, 0.0, 24.0);

        ai_run(&mut world, key, 10.0);

        let mi = monsterinfo(&world, key);
        // Both probes hit the same wall as the centre line, so the monster
        // only turns toward the goal (C: no side is clearer).
        assert!(!mi.aiflags.contains(AiFlags::PURSUE_TEMP));
        assert_eq!(world.entities.get(key).unwrap().game.ideal_yaw, 0.0);
    }

    #[test]
    fn ai_run_sidesteps_toward_the_clearer_side_of_a_slanted_block() {
        let (mut world, key) = monster_with_hidden_enemy();
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .last_sighting = Vec3f::new(400.0, 200.0, 24.0);

        ai_run(&mut world, key, 10.0);

        // Worked by hand from C ai_run: centre fraction 0.46 at yaw 26,
        // probes 326.5 ahead: left 0.6425 beats centre 0.630 and right
        // 0.6125, so head for the left point at half the left fraction.
        let mi = monsterinfo(&world, key);
        assert!(mi.aiflags.contains(AiFlags::PURSUE_TEMP));
        assert_eq!(mi.saved_goal, Vec3f::new(400.0, 200.0, 24.0));
        let side = mi.last_sighting;
        assert!(
            (side - Vec3f::new(87.24, 60.35, 24.0)).length() < 0.05,
            "{side}"
        );
        assert_eq!(world.entities.get(key).unwrap().game.ideal_yaw, 34.0);
    }

    #[test]
    fn ai_run_gives_up_the_search_twenty_seconds_after_it_expired() {
        let (mut world, key) = monster_with_hidden_enemy();
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .search_time = 1.0;
        world.level.time = 30.0;

        ai_run(&mut world, key, 10.0);

        let mi = monsterinfo(&world, key);
        assert_eq!(mi.search_time, 0.0);
        assert!(!mi.aiflags.contains(AiFlags::LOST_SIGHT));
    }

    #[test]
    fn ai_stand_without_enemy_does_not_panic() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        ai_stand(&mut world, key, 0.0);
    }

    #[test]
    fn ai_walk_steps_along_the_floor() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0); // facing +X

        ai_walk(&mut world, key, 10.0);

        let after = world.entities.get(key).unwrap().state.origin.x;
        assert!((after - 10.0).abs() < 0.01, "{after}");
    }

    #[test]
    fn ai_run_without_enemy_returns_to_stand() {
        fn test_stand(world: &mut GameWorld, key: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                stand: Some(test_stand),
                ..Default::default()
            }));
        }

        ai_run(&mut world, key, 10.0);

        assert_eq!(world.entities.get(key).unwrap().game.count, 1);
    }

    #[test]
    fn monster_move_frame_advances() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.frame = 0;
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 5,
                    frames: &[],
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.frame, 1);
    }

    #[test]
    fn monster_move_frame_clears_a_stale_out_of_range_nextframe_and_advances() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.frame = 2;
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 5,
                    frames: &[],
                    endfunc: None,
                }),
                nextframe: 50,
                ..Default::default()
            }));
        }

        monster_move_frame(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 3, "the move advances as if unset");
        assert_eq!(ent.monsterinfo.as_ref().unwrap().nextframe, 0);
    }

    #[test]
    fn monster_move_frame_loops() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.frame = 5; // at lastframe
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 5,
                    frames: &[],
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.frame, 0); // looped
    }

    #[test]
    fn monster_move_frame_calls_frame_fn() {
        fn test_frame(world: &mut GameWorld, key: EntityKey, _dist: f32) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.frame = 0;
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 3,
                    frames: &[MonsterFrame {
                        aifunc: Some(test_frame),
                        dist: 8.0,
                        thinkfunc: None,
                    }; 4],
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().game.count, 1);
    }

    #[test]
    fn monster_start_starts_the_lerp_where_the_monster_stands() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.monsterinfo = Some(Box::default());
            ent.state.origin = Vec3f::new(64.0, 32.0, 24.0);
        }
        monster_start(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.old_origin, Vec3f::new(64.0, 32.0, 24.0));
    }

    #[test]
    fn monster_start_sets_defaults() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.monsterinfo = Some(Box::default());
        }

        monster_start(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert_eq!(ent.game.takedamage, TakeDamage::Aim); // C DAMAGE_AIM
        assert!(ent.svflags & SvFlags::MONSTER.bits() != 0);
        assert_eq!(ent.game.mass, 200);
        assert!(ent.think.is_some());
    }

    #[test]
    fn monster_think_schedules_next() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 3,
                    frames: &[],
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_think(&mut world, key);

        let nt = world.entities.get(key).unwrap().game.nextthink;
        assert!((nt - 1.1).abs() < 0.01); // level.time + FRAMETIME
    }

    #[test]
    fn ai_set_sight_client_finds_player() {
        let mut world = test_world();
        let player = world.connect_client(1, "").unwrap();

        ai_set_sight_client(&mut world);

        assert_eq!(world.level.sight_client_key, Some(player));
    }

    #[test]
    fn find_target_sets_enemy() {
        let mut world = test_world();

        // Create player as sight client: C FindTarget needs a client.
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        world.entities.get_mut(player).unwrap().game.classname = "player".to_string();
        world.entities.get_mut(player).unwrap().state.origin = Vec3f::new(100.0, 0.0, 0.0);
        world.entities.get_mut(player).unwrap().game.light_level = 128; // lit
        world.level.sight_client_key = Some(player);

        // Create monster.
        let monster = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.state.origin = Vec3f::ZERO;
            ent.monsterinfo = Some(Box::default());
        }

        // MockGameImport trace returns fraction=1.0 (clear LOS).
        let found = find_target(&mut world, monster);

        assert!(found);
        assert_eq!(
            world.entities.get(monster).unwrap().game.enemy,
            Some(player)
        );
    }

    #[test]
    fn ai_charge_turns_toward_the_enemy_by_at_most_yaw_speed() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        let enemy = walker(&mut world, 0.0, 100.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(enemy);

        ai_charge(&mut world, monster, 0.0);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.ideal_yaw, 90.0);
        assert!((ent.state.angles.y - 20.0).abs() < 0.01);
    }

    fn with_move(world: &mut GameWorld, key: EntityKey, frame: i32, cm: MonsterMove) {
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.frame = frame;
        ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
            currentmove: Some(cm),
            scale: 1.0,
            ..Default::default()
        }));
    }

    fn run_move(world: &mut GameWorld, key: EntityKey) {
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .currentmove = Some(MonsterMove {
            firstframe: 97,
            lastframe: 104,
            frames: &[],
            endfunc: None,
        });
    }

    fn record_dist(world: &mut GameWorld, key: EntityKey, dist: f32) {
        world.entities.get_mut(key).unwrap().game.speed = dist;
    }

    #[test]
    fn monster_move_frame_starts_the_move_an_endfunc_switches_to() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let pain = MonsterMove {
            firstframe: 60,
            lastframe: 63,
            frames: &[],
            endfunc: Some(run_move),
        };
        with_move(&mut world, key, 63, pain);

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.frame, 97);
    }

    #[test]
    fn monster_think_stops_on_the_last_frame_of_a_death_move() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        let death = MonsterMove {
            firstframe: 178,
            lastframe: 213,
            frames: &[],
            endfunc: Some(monster_dead),
        };
        with_move(&mut world, key, 213, death);

        monster_think(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 213);
        assert_eq!(ent.game.nextthink, 0.0);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
        assert_eq!(ent.game.movetype, MoveType::Toss);
    }

    #[test]
    fn monster_move_frame_holds_the_frame_and_moves_nowhere_with_hold_frame() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let walk = MonsterMove {
            firstframe: 0,
            lastframe: 5,
            frames: &[MonsterFrame {
                aifunc: Some(record_dist),
                dist: 8.0,
                thinkfunc: None,
            }; 6],
            endfunc: None,
        };
        with_move(&mut world, key, 2, walk);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::HOLD_FRAME;
        world.entities.get_mut(key).unwrap().game.speed = -1.0;

        monster_move_frame(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 2);
        assert_eq!(ent.game.speed, 0.0);
    }

    #[test]
    fn monster_move_frame_jumps_to_a_requested_nextframe() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let walk = MonsterMove {
            firstframe: 0,
            lastframe: 5,
            ..Default::default()
        };
        with_move(&mut world, key, 1, walk);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .nextframe = 4;

        monster_move_frame(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 4);
        assert_eq!(ent.monsterinfo.as_ref().unwrap().nextframe, 0);
    }

    #[test]
    fn monster_start_counts_the_monster_and_clips_like_c() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().monsterinfo = Some(Box::default());
        let good = world.spawn().unwrap();
        world.entities.get_mut(good).unwrap().monsterinfo =
            Some(Box::new(crate::entity::MonsterInfo {
                aiflags: AiFlags::GOOD_GUY,
                ..Default::default()
            }));

        monster_start(&mut world, key);
        monster_start(&mut world, good);

        assert_eq!(world.level.total_monsters, 1);
        // C MASK_MONSTERSOLID: SOLID | WINDOW | MONSTERCLIP | MONSTER.
        assert_eq!(world.entities.get(key).unwrap().clipmask, 0x0202_0003);
    }

    #[test]
    fn ai_set_sight_client_never_picks_a_map_marker() {
        let mut world = test_world();
        let start = world.spawn().unwrap();
        world.entities.get_mut(start).unwrap().game.classname = "info_player_start".to_string();

        ai_set_sight_client(&mut world);

        assert_eq!(world.level.sight_client_key, None);
    }

    #[test]
    fn ai_set_sight_client_skips_a_dead_client() {
        let mut world = test_world();
        let player = world.connect_client(1, "").unwrap();
        world.entities.get_mut(player).unwrap().game.health = 0;

        ai_set_sight_client(&mut world);

        assert_eq!(world.level.sight_client_key, None);
    }

    #[test]
    fn monster_switched_from_stand_to_run_jumps_to_the_run_firstframe() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let stand = MonsterMove {
            firstframe: 0,
            lastframe: 29,
            ..Default::default()
        };
        with_move(&mut world, key, 3, stand);
        run_move(&mut world, key);

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.frame, 97);
    }

    /// USER-1: the soldier faced away from the player, ran straight on and
    /// never turned.
    #[test]
    fn ai_run_turns_toward_the_enemy_and_closes_in() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 336.0, -16.0);
        let player = walker(&mut world, 128.0, -320.0);
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.state.angles.y = 90.0;
            ent.game.enemy = Some(player);
        }
        // C steers by the goal and heading HuntTarget sets on sighting.
        hunt_target(&mut world, monster);
        let distance = |w: &GameWorld| {
            let a = w.entities.get(monster).unwrap().state.origin;
            let b = w.entities.get(player).unwrap().state.origin;
            (a - b).length()
        };
        let start = distance(&world);

        let mut yaw = 90.0;
        for _ in 0..20 {
            ai_run(&mut world, monster, 10.0);
            let now = world.entities.get(monster).unwrap().state.angles.y;
            let turned = (now - yaw + 540.0).rem_euclid(360.0) - 180.0;
            assert!(turned.abs() <= 20.01, "turned {turned}");
            yaw = now;
        }

        assert!(
            distance(&world) < start - 50.0,
            "{} -> {}",
            start,
            distance(&world)
        );
    }

    #[test]
    fn ai_run_does_not_pass_through_a_wall() {
        let mut world = floor_world(Some(100.0));
        let monster = walker(&mut world, 60.0, 0.0);
        let player = walker(&mut world, 300.0, 0.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);

        for _ in 0..20 {
            ai_run(&mut world, monster, 10.0);
        }

        let x = world.entities.get(monster).unwrap().state.origin.x;
        assert!(x + 16.0 <= 100.0, "{x}");
    }

    fn count_speed(world: &mut GameWorld, key: EntityKey) {
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.count = ent.game.speed as i32 + 100;
    }

    /// A walk table whose frames each carry their own distance; frame 2
    /// has no AI function and frame 3 a think function.
    static PER_FRAME: [MonsterFrame; 4] = [
        MonsterFrame {
            aifunc: Some(record_dist),
            dist: 1.0,
            thinkfunc: None,
        },
        MonsterFrame {
            aifunc: Some(record_dist),
            dist: 3.0,
            thinkfunc: None,
        },
        MonsterFrame {
            aifunc: None,
            dist: 50.0,
            thinkfunc: None,
        },
        MonsterFrame {
            aifunc: Some(record_dist),
            dist: 8.0,
            thinkfunc: Some(count_speed),
        },
    ];

    fn per_frame_monster(world: &mut GameWorld, frame: i32, scale: f32) -> EntityKey {
        let key = world.spawn().unwrap();
        let cm = MonsterMove {
            firstframe: 10,
            lastframe: 13,
            frames: &PER_FRAME,
            endfunc: None,
        };
        with_move(world, key, frame, cm);
        let ent = world.entities.get_mut(key).unwrap();
        ent.monsterinfo.as_mut().unwrap().scale = scale;
        ent.game.speed = -1.0;
        key
    }

    #[test]
    fn monster_move_frame_calls_each_frames_own_aifunc_with_its_dist_times_scale() {
        let mut world = test_world();
        let key = per_frame_monster(&mut world, 10, 2.0);

        monster_move_frame(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 11);
        assert_eq!(ent.game.speed, 6.0);
    }

    #[test]
    fn monster_move_frame_moves_nothing_on_a_frame_without_aifunc() {
        let mut world = test_world();
        let key = per_frame_monster(&mut world, 11, 1.0);

        monster_move_frame(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 12);
        assert_eq!(ent.game.speed, -1.0);
    }

    #[test]
    fn monster_move_frame_runs_the_frames_thinkfunc_after_its_aifunc() {
        let mut world = test_world();
        let key = per_frame_monster(&mut world, 12, 1.2);

        monster_move_frame(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 13);
        // 8 * 1.2 written by the aifunc, read by the thinkfunc.
        assert_eq!(ent.game.count, 109);
    }

    #[test]
    fn monster_move_frame_runs_no_thinkfunc_on_frames_without_one() {
        let mut world = test_world();
        let key = per_frame_monster(&mut world, 10, 1.0);

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().game.count, 0);
    }

    #[test]
    fn monster_move_frame_with_hold_frame_passes_zero_and_still_thinks() {
        let mut world = test_world();
        let key = per_frame_monster(&mut world, 13, 1.0);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::HOLD_FRAME;

        monster_move_frame(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.frame, 13);
        assert_eq!(ent.game.speed, 0.0);
        assert_eq!(ent.game.count, 100);
    }

    #[test]
    fn ai_move_steps_along_the_facing_forward_and_back() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().state.angles.y = 90.0;

        ai_move(&mut world, key, 10.0);
        let forward = world.entities.get(key).unwrap().state.origin;
        ai_move(&mut world, key, -13.0);
        let back = world.entities.get(key).unwrap().state.origin;

        assert!(
            (forward - Vec3f::new(0.0, 10.0, 24.0)).length() < 0.01,
            "{forward}"
        );
        assert!(
            (back - Vec3f::new(0.0, -3.0, 24.0)).length() < 0.01,
            "{back}"
        );
    }

    // -- C g_ai.c / g_monster.c branch tests --

    fn mi_mut(world: &mut GameWorld, key: EntityKey) -> &mut crate::entity::MonsterInfo {
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
    }

    /// A live client (entity 1) standing on the floor at (x, y), this
    /// frame's sight client.
    fn player_at(world: &mut GameWorld, x: f32, y: f32) -> EntityKey {
        let key = world.spawn_test_client(crate::entity::ClientData::default());
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.classname = "player".to_string();
        ent.state.origin = Vec3f::new(x, y, 24.0);
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
        ent.game.health = 100;
        ent.game.light_level = 128; // lit, as the browser client reports
        world.link_entity(key);
        world.level.sight_client_key = Some(key);
        key
    }

    /// Callbacks that count their calls in `game.count` digits: stand 1,
    /// walk 10, run 100, sight 1000, idle/search 10000.
    fn count_stand(world: &mut GameWorld, key: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 1;
    }
    fn count_walk(world: &mut GameWorld, key: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 10;
    }
    fn count_run(world: &mut GameWorld, key: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 100;
    }
    fn count_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 1000;
    }
    fn count_idle(world: &mut GameWorld, key: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 10000;
    }

    /// A walker (facing +X) with counting stand/walk/run/sight callbacks.
    fn counting_monster(world: &mut GameWorld, x: f32, y: f32) -> EntityKey {
        let key = walker(world, x, y);
        let mi = mi_mut(world, key);
        mi.stand = Some(count_stand);
        mi.walk = Some(count_walk);
        mi.run = Some(count_run);
        mi.sight = Some(count_sight);
        key
    }

    fn count(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().game.count
    }

    fn enemy_of(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
        world.entities.get(key).unwrap().game.enemy
    }

    #[test]
    fn find_target_sees_a_player_in_front_at_mid_range_and_calls_sight() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 600.0, 0.0);

        assert!(find_target(&mut world, monster));

        assert_eq!(enemy_of(&world, monster), Some(player));
        // FoundTarget -> HuntTarget runs, then the sight callback.
        assert_eq!(count(&world, monster), 1100);
    }

    #[test]
    fn find_target_does_not_see_a_player_standing_in_the_dark() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 600.0, 0.0);
        world.entities.get_mut(player).unwrap().game.light_level = 5;
        assert!(!find_target(&mut world, monster));

        world.entities.get_mut(player).unwrap().game.light_level = 6;
        assert!(find_target(&mut world, monster));
    }

    #[test]
    fn find_target_hears_a_noise_made_in_the_dark() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        world.entities.get_mut(player).unwrap().game.light_level = 0;
        let noise = noise_at(&mut world, 0.0, -300.0, player);
        hear(&mut world, noise);
        assert!(find_target(&mut world, monster));
    }

    #[test]
    fn find_target_ignores_a_player_behind_it_at_mid_range() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        player_at(&mut world, -600.0, 0.0);

        assert!(!find_target(&mut world, monster));
        assert_eq!(enemy_of(&world, monster), None);
    }

    #[test]
    fn find_target_ignores_a_player_1000_units_away() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        player_at(&mut world, 1000.0, 0.0);

        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_at_near_range_sees_only_in_front_unless_the_player_is_hostile() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        player_at(&mut world, -200.0, 0.0);

        // A player's show_hostile is C's zero, already past.
        world.level.time = 1.0;
        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_at_melee_range_sees_a_player_behind_it() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -60.0, 0.0);

        assert!(find_target(&mut world, monster));
        assert_eq!(enemy_of(&world, monster), Some(player));
    }

    #[test]
    fn find_target_ignores_a_notarget_player() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 100.0, 0.0);
        world.entities.get_mut(player).unwrap().game.flags |= EntityFlags::NOTARGET;

        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_ignores_a_sight_client_that_is_not_a_client() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let thing = walker(&mut world, 100.0, 0.0);
        world.entities.get_mut(thing).unwrap().monsterinfo = None;
        world.level.sight_client_key = Some(thing);

        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_never_looks_for_good_guys_or_on_the_way_to_a_combat_point() {
        for flag in [AiFlags::GOOD_GUY, AiFlags::COMBAT_POINT] {
            let mut world = floor_world(None);
            let monster = counting_monster(&mut world, 0.0, 0.0);
            player_at(&mut world, 100.0, 0.0);
            mi_mut(&mut world, monster).aiflags = flag;

            assert!(!find_target(&mut world, monster), "{flag:?}");
        }
    }

    #[test]
    fn find_target_reports_its_current_enemy_as_found_without_looking() {
        let mut world = floor_world(Some(50.0));
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 300.0, 0.0); // behind the wall
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);

        assert!(find_target(&mut world, monster));
        assert_eq!(count(&world, monster), 0);
    }

    #[test]
    fn find_target_ignores_players_during_intermission() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        player_at(&mut world, 100.0, 0.0);
        world.level.intermission_time = 5.0;

        assert!(!find_target(&mut world, monster));
    }

    /// A monster at (x, y) that just saw `player` (C level.sight_entity).
    fn sighting_monster(world: &mut GameWorld, x: f32, y: f32, player: EntityKey) -> EntityKey {
        let other = walker(world, x, y);
        let ent = world.entities.get_mut(other).unwrap();
        ent.svflags |= SvFlags::MONSTER.bits();
        ent.game.enemy = Some(player);
        ent.game.light_level = 128; // as FoundTarget leaves it
        world.level.sight_entity = Some(other);
        world.level.sight_entity_framenum = world.level.framenum;
        other
    }

    #[test]
    fn find_target_takes_the_player_another_monster_just_saw() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0); // out of range
        world.level.framenum = 10;
        sighting_monster(&mut world, 100.0, 0.0, player);

        assert!(find_target(&mut world, monster));
        assert_eq!(enemy_of(&world, monster), Some(player));
    }

    #[test]
    fn find_target_ignores_a_sighting_older_than_last_frame() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        world.level.framenum = 10;
        sighting_monster(&mut world, 100.0, 0.0, player);
        world.level.framenum = 12;

        // Falls back to the sight client, who is out of range.
        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_ignores_a_sighting_of_its_own_enemy() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        let other = sighting_monster(&mut world, 100.0, 0.0, player);
        // Both have no enemy: C compares the pointers, NULL == NULL.
        world.entities.get_mut(other).unwrap().game.enemy = None;

        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_ambusher_ignores_another_monsters_sighting() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.entities.get_mut(monster).unwrap().game.spawnflags = SPAWNFLAG_AMBUSH;
        let player = player_at(&mut world, -2000.0, 0.0);
        sighting_monster(&mut world, 100.0, 0.0, player);

        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_ignores_a_sighting_monster_whose_enemy_is_notarget() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        sighting_monster(&mut world, 100.0, 0.0, player);
        world.entities.get_mut(player).unwrap().game.flags |= EntityFlags::NOTARGET;

        assert!(!find_target(&mut world, monster));
    }

    /// A player noise at (x, y) made by `player` (C PlayerNoise).
    fn noise_at(world: &mut GameWorld, x: f32, y: f32, player: EntityKey) -> EntityKey {
        let noise = world.spawn().unwrap();
        let ent = world.entities.get_mut(noise).unwrap();
        ent.game.classname = "player_noise".to_string();
        ent.state.origin = Vec3f::new(x, y, 24.0);
        ent.owner = Some(player);
        noise
    }

    fn hear(world: &mut GameWorld, noise: EntityKey) {
        world.level.sound_entity = Some(noise);
        world.level.sound_entity_framenum = world.level.framenum;
    }

    #[test]
    fn find_target_hunts_a_heard_player_noise_as_a_sound_target() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        let noise = noise_at(&mut world, 0.0, -300.0, player);
        hear(&mut world, noise);

        assert!(find_target(&mut world, monster));

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(noise));
        // C vectoyaw gives -90 straight down the Y axis.
        assert_eq!(ent.game.ideal_yaw, -90.0);
        assert!(
            (ent.state.angles.y - 340.0).abs() < 0.01,
            "turned toward it"
        );
        assert!(monsterinfo(&world, monster)
            .aiflags
            .contains(AiFlags::SOUND_TARGET));
        // HuntTarget ran, but no sight callback for a noise.
        assert_eq!(count(&world, monster), 100);
    }

    #[test]
    fn find_target_does_not_hear_a_noise_over_1000_units_away() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        let noise = noise_at(&mut world, 0.0, -1001.0, player);
        hear(&mut world, noise);

        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_does_not_hear_a_notarget_players_noise() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        world.entities.get_mut(player).unwrap().game.flags |= EntityFlags::NOTARGET;
        let noise = noise_at(&mut world, 0.0, -300.0, player);
        hear(&mut world, noise);

        assert!(!find_target(&mut world, monster));
    }

    #[test]
    fn find_target_ambusher_hears_only_a_noise_it_can_see() {
        let mut world = floor_world(Some(100.0));
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.entities.get_mut(monster).unwrap().game.spawnflags = SPAWNFLAG_AMBUSH;
        let player = player_at(&mut world, -2000.0, 0.0);
        let behind_wall = noise_at(&mut world, 300.0, 0.0, player);
        hear(&mut world, behind_wall);
        assert!(!find_target(&mut world, monster));

        let in_view = noise_at(&mut world, 0.0, 300.0, player);
        hear(&mut world, in_view);
        assert!(find_target(&mut world, monster));
    }

    #[test]
    fn find_target_does_not_hear_a_noise_in_an_area_its_own_is_cut_off_from() {
        let (mut world, gi) = floor_world_with(FloorImport::default());
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        let noise = noise_at(&mut world, 0.0, -300.0, player);
        for (key, area) in [(monster, 1), (noise, 2)] {
            let number = world.number_of(key).unwrap();
            gi.inner.areas.borrow_mut().insert(number, (area, 0));
            world.link_entity(key);
        }
        gi.inner.disconnected_areas.borrow_mut().push((1, 2));
        hear(&mut world, noise);
        assert!(!find_target(&mut world, monster), "closed portal between");

        gi.inner.disconnected_areas.borrow_mut().clear();
        assert!(find_target(&mut world, monster), "portal open");
        assert_eq!(enemy_of(&world, monster), Some(noise));
    }

    #[test]
    fn find_target_hears_a_noise_in_its_own_area_without_asking_the_portals() {
        let (mut world, gi) = floor_world_with(FloorImport::default());
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        let noise = noise_at(&mut world, 0.0, -300.0, player);
        for key in [monster, noise] {
            let number = world.number_of(key).unwrap();
            gi.inner.areas.borrow_mut().insert(number, (3, 0));
            world.link_entity(key);
        }
        // C asks AreasConnected only for different areas.
        gi.inner.disconnected_areas.borrow_mut().push((3, 3));
        hear(&mut world, noise);
        assert!(find_target(&mut world, monster));
    }

    #[test]
    fn find_target_hears_an_impact_noise_only_without_an_enemy() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, -2000.0, 0.0);
        let noise = noise_at(&mut world, 0.0, 300.0, player);
        world.level.sound2_entity = Some(noise);
        world.level.sound2_entity_framenum = world.level.framenum;
        let dead = walker(&mut world, 500.0, 500.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(dead);

        assert!(!find_target(&mut world, monster));

        world.entities.get_mut(monster).unwrap().game.enemy = None;
        assert!(find_target(&mut world, monster));
        assert_eq!(enemy_of(&world, monster), Some(noise));
    }

    #[test]
    fn found_target_marks_the_sighting_and_hunts_the_player() {
        let mut world = floor_world(None);
        world.level.time = 3.0;
        world.level.framenum = 30;
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 200.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);

        found_target(&mut world, monster);

        assert_eq!(world.level.sight_entity, Some(monster));
        assert_eq!(world.level.sight_entity_framenum, 30);
        // Lit up, so other monsters can see it in the dark.
        assert_eq!(world.entities.get(monster).unwrap().game.light_level, 128);
        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.goalentity, Some(player));
        assert_eq!(ent.game.ideal_yaw, 90.0);
        assert_eq!(count(&world, monster), 100);
        let mi = monsterinfo(&world, monster);
        assert_eq!(mi.show_hostile, 4.0);
        assert_eq!(mi.last_sighting, Vec3f::new(0.0, 200.0, 24.0));
        assert_eq!(mi.trail_time, 3.0);
        assert_eq!(mi.attack_finished, 4.0, "waits a second to attack");
    }

    #[test]
    fn found_target_runs_to_its_combat_point_once() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 200.0);
        let point = world.spawn().unwrap();
        world.entities.get_mut(point).unwrap().game.targetname = "cp".to_string();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.game.enemy = Some(player);
            ent.game.combattarget = "cp".to_string();
        }
        mi_mut(&mut world, monster).pausetime = 50.0;

        found_target(&mut world, monster);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.goalentity, Some(point));
        assert_eq!(ent.game.movetarget, Some(point));
        assert!(ent.game.combattarget.is_empty());
        assert!(world
            .entities
            .get(point)
            .unwrap()
            .game
            .targetname
            .is_empty());
        let mi = monsterinfo(&world, monster);
        assert!(mi.aiflags.contains(AiFlags::COMBAT_POINT));
        assert_eq!(mi.pausetime, 0.0);
        assert_eq!(count(&world, monster), 100);
    }

    #[test]
    fn found_target_hunts_the_enemy_when_its_combat_point_is_missing() {
        let (mut world, gi) = crate::monster::test_support::floor_world_with(Default::default());
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 200.0);
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.game.classname = "monster_soldier".to_string();
            ent.game.enemy = Some(player);
            ent.game.combattarget = "nowhere".to_string();
        }

        found_target(&mut world, monster);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.goalentity, Some(player));
        assert_eq!(ent.game.movetarget, Some(player));
        assert!(gi
            .inner
            .dprints
            .borrow()
            .iter()
            .any(|m| m == "monster_soldier at (0 0 24), combattarget nowhere not found\n"));
    }

    #[test]
    fn hunt_target_stand_ground_monster_stands_and_may_attack_at_once() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 200.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);
        mi_mut(&mut world, monster).aiflags = AiFlags::STAND_GROUND;

        hunt_target(&mut world, monster);

        assert_eq!(count(&world, monster), 1);
        assert_eq!(monsterinfo(&world, monster).attack_finished, 0.0);
    }

    #[test]
    fn hunt_target_keeps_its_heading_for_an_enemy_out_of_sight() {
        let mut world = floor_world(Some(50.0));
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 300.0, 200.0);
        let ent = world.entities.get_mut(monster).unwrap();
        ent.game.enemy = Some(player);
        ent.game.ideal_yaw = 180.0;

        hunt_target(&mut world, monster);

        assert_eq!(world.entities.get(monster).unwrap().game.ideal_yaw, 180.0);
    }

    // -- ai_stand / ai_walk --

    #[test]
    fn ai_stand_walks_off_once_pausetime_passes() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.level.time = 10.0;
        mi_mut(&mut world, monster).pausetime = 5.0;

        ai_stand(&mut world, monster, 0.0);

        assert_eq!(count(&world, monster), 10);
    }

    #[test]
    fn ai_stand_waits_until_pausetime() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.level.time = 10.0;
        mi_mut(&mut world, monster).pausetime = 100_000_000.0;

        ai_stand(&mut world, monster, 0.0);

        assert_eq!(count(&world, monster), 0);
    }

    #[test]
    fn ai_stand_schedules_the_idle_sound_then_plays_it_every_15_to_30_seconds() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.level.time = 10.0;
        {
            let mi = mi_mut(&mut world, monster);
            mi.pausetime = 100_000_000.0;
            mi.idle = Some(count_idle);
        }

        ai_stand(&mut world, monster, 0.0);
        let first = monsterinfo(&world, monster).idle_time;
        assert_eq!(count(&world, monster), 0, "the first call only schedules");
        assert!((10.0..=25.0).contains(&first), "{first}");

        world.level.time = first + 0.1;
        ai_stand(&mut world, monster, 0.0);
        let next = monsterinfo(&world, monster).idle_time;
        assert_eq!(count(&world, monster), 10000);
        assert!((first + 15.1..=first + 30.1).contains(&next), "{next}");
    }

    #[test]
    fn ai_stand_ambusher_plays_no_idle_sound() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.entities.get_mut(monster).unwrap().game.spawnflags = SPAWNFLAG_AMBUSH;
        world.level.time = 10.0;
        {
            let mi = mi_mut(&mut world, monster);
            mi.pausetime = 100_000_000.0;
            mi.idle = Some(count_idle);
            mi.idle_time = 1.0;
        }

        ai_stand(&mut world, monster, 0.0);

        assert_eq!(count(&world, monster), 0);
        assert_eq!(monsterinfo(&world, monster).idle_time, 1.0);
    }

    #[test]
    fn ai_stand_steps_its_frame_distance_along_its_facing() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        mi_mut(&mut world, monster).pausetime = 100_000_000.0;

        ai_stand(&mut world, monster, 4.0);

        let x = world.entities.get(monster).unwrap().state.origin.x;
        assert!((x - 4.0).abs() < 0.01, "{x}");
    }

    fn checkattack_counts(world: &mut GameWorld, key: EntityKey) -> bool {
        world.entities.get_mut(key).unwrap().game.style += 1;
        false
    }

    #[test]
    fn ai_stand_holding_ground_turns_to_its_enemy_and_checks_for_an_attack() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 200.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);
        {
            let mi = mi_mut(&mut world, monster);
            mi.aiflags = AiFlags::STAND_GROUND;
            mi.checkattack = Some(checkattack_counts);
        }

        ai_stand(&mut world, monster, 0.0);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.ideal_yaw, 90.0);
        assert!((ent.state.angles.y - 20.0).abs() < 0.01);
        assert_eq!(ent.game.style, 1, "checkattack ran");
        assert_eq!(count(&world, monster), 0, "still holding ground");
    }

    #[test]
    fn ai_stand_temporarily_holding_ground_runs_when_the_enemy_moves_off_its_facing() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 200.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);
        mi_mut(&mut world, monster).aiflags = AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND;

        ai_stand(&mut world, monster, 0.0);

        let mi = monsterinfo(&world, monster);
        assert!(!mi
            .aiflags
            .intersects(AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND));
        assert_eq!(count(&world, monster), 100);
    }

    #[test]
    fn ai_stand_holding_ground_without_an_enemy_looks_for_one() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 200.0, 0.0);
        mi_mut(&mut world, monster).aiflags = AiFlags::STAND_GROUND;

        ai_stand(&mut world, monster, 0.0);

        assert_eq!(enemy_of(&world, monster), Some(player));
    }

    #[test]
    fn ai_walk_schedules_then_plays_the_search_sound() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.level.time = 10.0;
        mi_mut(&mut world, monster).search = Some(count_idle);

        ai_walk(&mut world, monster, 0.0);
        assert_eq!(count(&world, monster), 0);
        let first = monsterinfo(&world, monster).idle_time;

        world.level.time = first + 0.1;
        ai_walk(&mut world, monster, 0.0);
        assert_eq!(count(&world, monster), 10000);
    }

    #[test]
    fn ai_walk_that_spots_a_player_plays_no_search_sound() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        player_at(&mut world, 200.0, 0.0);
        world.level.time = 10.0;
        {
            let mi = mi_mut(&mut world, monster);
            mi.search = Some(count_idle);
            mi.idle_time = 1.0;
        }

        ai_walk(&mut world, monster, 0.0);

        assert_eq!(count(&world, monster), 1100);
    }

    #[test]
    fn monster_start_in_deathmatch_removes_the_monster() {
        let mut world = test_world();
        world.game.deathmatch = true;
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().monsterinfo = Some(Box::default());

        assert!(!monster_start(&mut world, key));

        assert!(!world.entities.is_live(key));
        assert_eq!(world.level.total_monsters, 0);
    }

    // -- range, infront, FacingIdeal, AttackFinished --

    #[test]
    fn range_uses_the_c_melee_near_and_mid_distances() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        let other = walker(&mut world, 0.0, 0.0);
        for (x, want) in [
            (79.9, Range::Melee),
            (80.0, Range::Near),
            (499.9, Range::Near),
            (500.0, Range::Mid),
            (999.9, Range::Mid),
            (1000.0, Range::Far),
        ] {
            world.entities.get_mut(other).unwrap().state.origin.x = x;
            assert_eq!(range(&world, monster, other), want, "{x}");
        }
    }

    #[test]
    fn infront_is_within_a_dot_product_of_0_3_of_the_facing() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        let other = walker(&mut world, 0.0, 0.0);
        for (y, want) in [(0.0, true), (310.0, true), (330.0, false), (-330.0, false)] {
            world.entities.get_mut(other).unwrap().state.origin = Vec3f::new(100.0, y, 24.0);
            assert_eq!(infront(&world, monster, other), want, "{y}");
        }
        world.entities.get_mut(other).unwrap().state.origin = Vec3f::new(-100.0, 0.0, 24.0);
        assert!(!infront(&world, monster, other));
    }

    #[test]
    fn facing_ideal_allows_45_degrees_either_way() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        for (ideal, want) in [(45.0, true), (46.0, false), (315.0, true), (314.0, false)] {
            world.entities.get_mut(monster).unwrap().game.ideal_yaw = ideal;
            assert_eq!(facing_ideal(&world, monster), want, "{ideal}");
        }
    }

    #[test]
    fn attack_finished_holds_attacks_for_the_time_from_now() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        world.level.time = 7.0;

        attack_finished(&mut world, monster, 1.5);

        assert_eq!(monsterinfo(&world, monster).attack_finished, 8.5);
    }

    // -- M_CheckAttack --

    fn attack_counts(world: &mut GameWorld, key: EntityKey) {
        world.entities.get_mut(key).unwrap().game.dmg += 1;
    }

    /// A monster at the origin and a live enemy at `x`, in a world whose
    /// next trace (the shot check) hits the enemy, with the enemy's range
    /// already recorded as `ai_checkattack` does.
    fn shooter(x: f32) -> (GameWorld, EntityKey, EntityKey) {
        let (mut world, gi) = crate::world::test_world_recording();
        let monster = world.spawn().unwrap();
        let enemy = world.spawn().unwrap();
        world.entities.get_mut(enemy).unwrap().game.health = 100;
        world.entities.get_mut(enemy).unwrap().state.origin.x = x;
        let ent = world.entities.get_mut(monster).unwrap();
        ent.game.enemy = Some(enemy);
        ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
            attack: Some(attack_counts),
            ..Default::default()
        }));
        world.enemy_check.range = range(&world, monster, enemy);
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ent_index: world.number_of(enemy),
            ..Trace::default()
        });
        (world, monster, enemy)
    }

    /// A random-number state whose next `random()` lies in `lo..hi`.
    fn seed_for_random_in(lo: f32, hi: f32) -> u32 {
        (0..)
            .find(|&seed| {
                let mut probe = test_world();
                probe.level.rand_state = seed;
                (lo..hi).contains(&probe.random())
            })
            .unwrap()
    }

    #[test]
    fn m_check_attack_needs_a_clear_shot_at_the_enemy() {
        let (mut world, monster, _) = shooter(60.0);
        // Use up the queued hit: the shot check then hits nothing.
        world
            .gi
            .trace(Vec3f::ZERO, Vec3f::ZERO, Vec3f::ZERO, Vec3f::ZERO, None, 0);

        assert!(!m_check_attack(&mut world, monster));
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Straight
        );
    }

    #[test]
    fn m_check_attack_in_melee_range_melees_or_shoots() {
        let (mut world, monster, _) = shooter(60.0);
        assert!(m_check_attack(&mut world, monster));
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Missile
        );

        let (mut world, monster, _) = shooter(60.0);
        mi_mut(&mut world, monster).melee = Some(attack_counts);
        assert!(m_check_attack(&mut world, monster));
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Melee
        );
    }

    #[test]
    fn m_check_attack_on_easy_melees_only_one_time_in_four() {
        // C: `(randk() & 3)` nonzero means no melee this time.
        let seed_where = |zero: bool| {
            (0..)
                .find(|&seed| {
                    let mut probe = test_world();
                    probe.level.rand_state = seed;
                    (probe.random_u32() & 3 == 0) == zero
                })
                .unwrap()
        };
        for zero in [true, false] {
            let (mut world, monster, _) = shooter(60.0);
            world.game.skill = SKILL_EASY;
            world.level.rand_state = seed_where(zero);
            assert_eq!(m_check_attack(&mut world, monster), zero);
        }
    }

    #[test]
    fn m_check_attack_missile_chance_holding_ground_is_0_4_halved_on_easy() {
        let seed = seed_for_random_in(0.2, 0.4);
        for (skill, fires) in [(SKILL_EASY, false), (1, true)] {
            let (mut world, monster, _) = shooter(300.0);
            world.game.skill = skill;
            world.level.rand_state = seed;
            mi_mut(&mut world, monster).aiflags = AiFlags::STAND_GROUND;
            assert_eq!(m_check_attack(&mut world, monster), fires, "skill {skill}");
        }
    }

    #[test]
    fn m_check_attack_near_chance_is_0_1_doubled_on_hard() {
        let seed = seed_for_random_in(0.1, 0.2);
        for (skill, fires) in [(1, false), (SKILL_HARD, true), (SKILL_HARDPLUS, true)] {
            let (mut world, monster, _) = shooter(300.0);
            world.game.skill = skill;
            world.level.rand_state = seed;
            assert_eq!(m_check_attack(&mut world, monster), fires, "skill {skill}");
        }
    }

    #[test]
    fn m_check_attack_mid_chance_is_0_02() {
        let seed = seed_for_random_in(0.0, 0.02);
        let (mut world, monster, _) = shooter(700.0);
        world.level.rand_state = seed;
        assert!(m_check_attack(&mut world, monster));

        let seed = seed_for_random_in(0.02, 0.04);
        let (mut world, monster, _) = shooter(700.0);
        world.level.rand_state = seed;
        assert!(!m_check_attack(&mut world, monster));
    }

    #[test]
    fn m_check_attack_firing_sets_missile_and_waits_up_to_two_seconds() {
        let seed = seed_for_random_in(0.0, 0.1);
        let (mut world, monster, _) = shooter(300.0);
        world.level.time = 5.0;
        world.level.rand_state = seed;

        assert!(m_check_attack(&mut world, monster));

        let mi = monsterinfo(&world, monster);
        assert_eq!(mi.attack_state, AttackState::Missile);
        assert!((5.0..=7.0).contains(&mi.attack_finished));
    }

    #[test]
    fn m_check_attack_never_fires_too_soon_too_far_or_without_an_attack() {
        let seed = seed_for_random_in(0.0, 0.01);
        let (mut world, monster, _) = shooter(300.0);
        world.level.rand_state = seed;
        world.level.time = 1.0;
        mi_mut(&mut world, monster).attack_finished = 2.0;
        assert!(!m_check_attack(&mut world, monster), "too soon");

        let (mut world, monster, _) = shooter(1200.0);
        world.level.rand_state = seed;
        assert!(!m_check_attack(&mut world, monster), "too far");

        let (mut world, monster, _) = shooter(300.0);
        world.level.rand_state = seed;
        mi_mut(&mut world, monster).attack = None;
        assert!(!m_check_attack(&mut world, monster), "no attack");
    }

    #[test]
    fn m_check_attack_flyer_holding_fire_slides_three_times_in_ten() {
        // First draw misses the 0.1 near chance, second picks the slide.
        let seed = (0..)
            .find(|&seed| {
                let mut probe = test_world();
                probe.level.rand_state = seed;
                probe.random() >= 0.1 && probe.random() < 0.3
            })
            .unwrap();
        let (mut world, monster, _) = shooter(300.0);
        world.level.rand_state = seed;
        world.entities.get_mut(monster).unwrap().game.flags |= EntityFlags::FLY;

        assert!(!m_check_attack(&mut world, monster));
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Sliding
        );
    }

    // -- ai_run_melee / ai_run_missile / ai_run_slide --

    #[test]
    fn ai_run_missile_fires_once_facing_the_enemy_and_goes_straight() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        {
            let mi = mi_mut(&mut world, monster);
            mi.attack = Some(attack_counts);
            mi.attack_state = AttackState::Missile;
        }
        world.enemy_check.yaw = 10.0;

        ai_run_missile(&mut world, monster);

        assert_eq!(world.entities.get(monster).unwrap().game.dmg, 1);
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Straight
        );
    }

    #[test]
    fn ai_run_missile_only_turns_while_facing_away() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        {
            let mi = mi_mut(&mut world, monster);
            mi.attack = Some(attack_counts);
            mi.attack_state = AttackState::Missile;
        }
        world.enemy_check.yaw = 180.0;

        ai_run_missile(&mut world, monster);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.dmg, 0);
        assert!(
            (ent.state.angles.y - 20.0).abs() < 0.01 || (ent.state.angles.y - 340.0).abs() < 0.01
        );
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Missile
        );
    }

    #[test]
    fn ai_run_melee_strikes_once_facing_the_enemy() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        {
            let mi = mi_mut(&mut world, monster);
            mi.melee = Some(attack_counts);
            mi.attack_state = AttackState::Melee;
        }
        world.enemy_check.yaw = 30.0;

        ai_run_melee(&mut world, monster);

        assert_eq!(world.entities.get(monster).unwrap().game.dmg, 1);
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Straight
        );
    }

    #[test]
    fn ai_run_slide_strafes_and_switches_side_when_blocked() {
        let mut world = floor_world(Some(20.0));
        let monster = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(monster).unwrap().state.angles.y = 90.0;
        world.enemy_check.yaw = 90.0;

        // Right of +Y is +X, into the wall: slide left instead.
        ai_run_slide(&mut world, monster, 10.0);

        let x = world.entities.get(monster).unwrap().state.origin.x;
        assert!((x + 10.0).abs() < 0.01, "{x}");
        assert!(monsterinfo(&world, monster).lefty);
    }

    // -- ai_checkattack --

    /// A counting monster at the origin with a live player enemy at (x, y).
    fn monster_and_enemy(wall_x: Option<f32>, x: f32, y: f32) -> (GameWorld, EntityKey, EntityKey) {
        let mut world = floor_world(wall_x);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, x, y);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);
        mi_mut(&mut world, monster).checkattack = Some(checkattack_counts);
        (world, monster, player)
    }

    #[test]
    fn ai_checkattack_after_killing_its_enemy_stands_until_something_happens() {
        let (mut world, monster, player) = monster_and_enemy(None, 200.0, 0.0);
        world.entities.get_mut(player).unwrap().game.health = 0;
        world.level.time = 3.0;

        assert!(ai_checkattack(&mut world, monster));

        assert_eq!(enemy_of(&world, monster), None);
        assert_eq!(monsterinfo(&world, monster).pausetime, 3.0 + 100_000_000.0);
        assert_eq!(count(&world, monster), 1);
    }

    #[test]
    fn ai_checkattack_after_killing_its_enemy_walks_back_to_its_path() {
        let (mut world, monster, player) = monster_and_enemy(None, 200.0, 0.0);
        world.entities.get_mut(player).unwrap().game.health = -5;
        let corner = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().game.movetarget = Some(corner);

        assert!(ai_checkattack(&mut world, monster));

        assert_eq!(
            world.entities.get(monster).unwrap().game.goalentity,
            Some(corner)
        );
        assert_eq!(count(&world, monster), 10);
    }

    #[test]
    fn ai_checkattack_after_killing_its_enemy_goes_back_to_a_live_old_enemy() {
        let (mut world, monster, player) = monster_and_enemy(None, 200.0, 0.0);
        world.entities.get_mut(player).unwrap().game.health = 0;
        let old = walker(&mut world, 0.0, 300.0);
        world.entities.get_mut(monster).unwrap().game.oldenemy = Some(old);

        ai_checkattack(&mut world, monster);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(old));
        assert_eq!(ent.game.oldenemy, None);
        assert_eq!(ent.game.goalentity, Some(old), "HuntTarget");
    }

    #[test]
    fn ai_checkattack_brutal_monster_keeps_at_a_corpse_until_it_gibs() {
        let (mut world, monster, player) = monster_and_enemy(None, 200.0, 0.0);
        world.entities.get_mut(player).unwrap().game.health = -79;
        mi_mut(&mut world, monster).aiflags = AiFlags::BRUTAL;

        ai_checkattack(&mut world, monster);
        assert_eq!(enemy_of(&world, monster), Some(player));

        world.entities.get_mut(player).unwrap().game.health = -80;
        ai_checkattack(&mut world, monster);
        assert_eq!(enemy_of(&world, monster), None);
    }

    #[test]
    fn ai_checkattack_medic_is_done_once_its_patient_lives() {
        let (mut world, monster, player) = monster_and_enemy(None, 200.0, 0.0);
        world.entities.get_mut(player).unwrap().game.health = 0;
        mi_mut(&mut world, monster).aiflags = AiFlags::MEDIC;
        ai_checkattack(&mut world, monster);
        assert_eq!(enemy_of(&world, monster), Some(player), "still dead");

        world.entities.get_mut(player).unwrap().game.health = 10;
        ai_checkattack(&mut world, monster);
        assert_eq!(enemy_of(&world, monster), None);
        assert!(!monsterinfo(&world, monster)
            .aiflags
            .contains(AiFlags::MEDIC));
    }

    #[test]
    fn ai_checkattack_gives_up_a_player_enemy_at_intermission() {
        let (mut world, monster, _) = monster_and_enemy(None, 200.0, 0.0);
        world.level.intermission_time = 1.0;

        ai_checkattack(&mut world, monster);

        assert_eq!(enemy_of(&world, monster), None);
    }

    #[test]
    fn ai_checkattack_records_a_visible_enemy_and_asks_checkattack() {
        let (mut world, monster, _) = monster_and_enemy(None, 0.0, 300.0);
        world.level.time = 2.0;

        assert!(!ai_checkattack(&mut world, monster));

        let check = world.enemy_check;
        assert!(check.vis && !check.infront);
        assert_eq!(check.range, Range::Near);
        assert_eq!(check.yaw, 90.0);
        let mi = monsterinfo(&world, monster);
        assert_eq!(mi.search_time, 7.0);
        assert_eq!(mi.show_hostile, 3.0);
        assert_eq!(mi.last_sighting, Vec3f::new(0.0, 300.0, 24.0));
        assert_eq!(world.entities.get(monster).unwrap().game.style, 1);
    }

    #[test]
    fn ai_checkattack_never_attacks_an_unseen_enemy() {
        let (mut world, monster, _) = monster_and_enemy(Some(100.0), 300.0, 0.0);

        assert!(!ai_checkattack(&mut world, monster));

        assert!(!world.enemy_check.vis);
        assert_eq!(world.entities.get(monster).unwrap().game.style, 0);
    }

    #[test]
    fn ai_checkattack_carries_out_a_chosen_missile_attack() {
        let (mut world, monster, _) = monster_and_enemy(Some(100.0), 300.0, 0.0);
        {
            let mi = mi_mut(&mut world, monster);
            mi.attack_state = AttackState::Missile;
            mi.attack = Some(attack_counts);
        }

        assert!(ai_checkattack(&mut world, monster));

        assert_eq!(world.entities.get(monster).unwrap().game.dmg, 1);
    }

    #[test]
    fn ai_checkattack_carries_out_a_chosen_melee_attack() {
        let (mut world, monster, _) = monster_and_enemy(None, 60.0, 0.0);
        {
            let mi = mi_mut(&mut world, monster);
            mi.attack_state = AttackState::Melee;
            mi.melee = Some(attack_counts);
        }

        assert!(ai_checkattack(&mut world, monster));

        assert_eq!(world.entities.get(monster).unwrap().game.dmg, 1);
    }

    #[test]
    fn ai_checkattack_runs_blindly_to_a_combat_point() {
        let (mut world, monster, _) = monster_and_enemy(None, 60.0, 0.0);
        let point = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().game.goalentity = Some(point);
        mi_mut(&mut world, monster).aiflags = AiFlags::COMBAT_POINT;

        assert!(!ai_checkattack(&mut world, monster));
        assert_eq!(world.entities.get(monster).unwrap().game.style, 0);
    }

    /// A monster hunting a noise behind a wall, made at framenum 10.
    fn hunting_a_noise() -> (GameWorld, EntityKey, EntityKey) {
        let mut world = floor_world(Some(100.0));
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 2000.0, 0.0);
        let noise = noise_at(&mut world, 300.0, 0.0, player);
        world.level.framenum = 10;
        hear(&mut world, noise);
        let ent = world.entities.get_mut(monster).unwrap();
        ent.game.enemy = Some(noise);
        ent.game.goalentity = Some(noise);
        mi_mut(&mut world, monster).aiflags =
            AiFlags::SOUND_TARGET | AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND;
        (world, monster, noise)
    }

    #[test]
    fn ai_checkattack_keeps_hunting_a_noise_heard_in_the_last_five_seconds() {
        let (mut world, monster, _) = hunting_a_noise();
        world.level.time = 6.0;

        assert!(!ai_checkattack(&mut world, monster));

        let mi = monsterinfo(&world, monster);
        assert!(mi.aiflags.contains(AiFlags::SOUND_TARGET));
        assert_eq!(mi.show_hostile, 7.0);
    }

    #[test]
    fn ai_checkattack_forgets_a_noise_after_five_seconds() {
        let (mut world, monster, _) = hunting_a_noise();
        let corner = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().game.movetarget = Some(corner);
        world.level.time = 6.1;

        ai_checkattack(&mut world, monster);

        assert_eq!(
            world.entities.get(monster).unwrap().game.goalentity,
            Some(corner)
        );
        assert!(!monsterinfo(&world, monster).aiflags.intersects(
            AiFlags::SOUND_TARGET | AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND
        ));
    }

    #[test]
    fn ai_checkattack_in_coop_looks_for_another_player_once_the_search_lapses() {
        let (mut world, monster, _) = monster_and_enemy(Some(250.0), 300.0, 0.0);
        world.game.coop = true;
        world.level.time = 10.0;
        // The sight client this frame is a second player in plain view.
        let other = walker(&mut world, 200.0, 30.0);
        world.entities.get_mut(other).unwrap().client = Some(0);
        world.entities.get_mut(other).unwrap().game.light_level = 128;
        world.level.sight_client_key = Some(other);

        assert!(ai_checkattack(&mut world, monster));
        assert_eq!(enemy_of(&world, monster), Some(other));
    }

    #[test]
    fn ai_checkattack_outside_coop_never_switches_players() {
        let (mut world, monster, player) = monster_and_enemy(Some(250.0), 300.0, 0.0);
        world.level.time = 10.0;
        let other = walker(&mut world, 200.0, 30.0);
        world.entities.get_mut(other).unwrap().client = Some(0);
        world.entities.get_mut(other).unwrap().game.light_level = 128;
        world.level.sight_client_key = Some(other);

        ai_checkattack(&mut world, monster);

        assert_eq!(enemy_of(&world, monster), Some(player));
    }

    #[test]
    fn ai_checkattack_uses_m_check_attack_when_the_monster_has_none() {
        let (mut world, monster, _) = monster_and_enemy(None, 60.0, 0.0);
        mi_mut(&mut world, monster).checkattack = None;

        // FloorImport traces report the world, not the enemy: no clear shot.
        assert!(!ai_checkattack(&mut world, monster));
        assert_eq!(
            monsterinfo(&world, monster).attack_state,
            AttackState::Straight
        );
    }

    // -- ai_run --

    #[test]
    fn ai_run_heads_straight_for_a_combat_point() {
        let (mut world, monster, _) = monster_and_enemy(None, 300.0, 0.0);
        let point = walker(&mut world, 0.0, 300.0);
        world.entities.get_mut(monster).unwrap().game.goalentity = Some(point);
        mi_mut(&mut world, monster).aiflags = AiFlags::COMBAT_POINT;

        ai_run(&mut world, monster, 10.0);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.style, 0, "no attack check");
        assert_ne!(ent.state.origin, Vec3f::new(0.0, 0.0, 24.0));
    }

    #[test]
    fn ai_run_holds_ground_on_reaching_a_heard_noise() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 2000.0, 0.0);
        let noise = noise_at(&mut world, 40.0, 0.0, player);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(noise);
        mi_mut(&mut world, monster).aiflags = AiFlags::SOUND_TARGET;

        ai_run(&mut world, monster, 10.0);

        let mi = monsterinfo(&world, monster);
        assert!(mi
            .aiflags
            .contains(AiFlags::STAND_GROUND | AiFlags::TEMP_STAND_GROUND));
        assert_eq!(count(&world, monster), 1);
    }

    #[test]
    fn ai_run_after_a_noise_stops_when_it_finds_nothing() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 2000.0, 0.0);
        let noise = noise_at(&mut world, 400.0, 0.0, player);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(noise);
        {
            let mi = mi_mut(&mut world, monster);
            mi.aiflags = AiFlags::SOUND_TARGET;
            mi.checkattack = Some(checkattack_counts);
        }

        ai_run(&mut world, monster, 10.0);

        assert_eq!(world.entities.get(monster).unwrap().game.style, 0);
    }

    fn checkattack_attacks(world: &mut GameWorld, key: EntityKey) -> bool {
        world.entities.get_mut(key).unwrap().game.style += 1;
        true
    }

    #[test]
    fn ai_run_does_not_move_on_a_frame_it_attacks() {
        let (mut world, monster, _) = monster_and_enemy(None, 300.0, 0.0);
        mi_mut(&mut world, monster).checkattack = Some(checkattack_attacks);

        ai_run(&mut world, monster, 10.0);

        assert_eq!(world.entities.get(monster).unwrap().state.origin.x, 0.0);
    }

    #[test]
    fn ai_run_slides_when_the_attack_state_says_so() {
        let (mut world, monster, _) = monster_and_enemy(None, 300.0, 0.0);
        mi_mut(&mut world, monster).attack_state = AttackState::Sliding;

        ai_run(&mut world, monster, 10.0);

        // Facing the enemy at yaw 0, a right-hand slide is toward -Y.
        let origin = world.entities.get(monster).unwrap().state.origin;
        assert!((origin.y + 10.0).abs() < 0.01, "{origin}");
    }

    // -- monster_think and world effects (g_monster.c) --

    use crate::monster::test_support::{floor_world_with, FloorImport};
    use q2_shared::constants::{CONTENTS_LAVA, CONTENTS_SLIME, CONTENTS_WATER};

    fn water_world(top: f32, contents: i32) -> (GameWorld, std::rc::Rc<FloorImport>) {
        floor_world_with(FloorImport {
            water: Some((top, contents)),
            ..Default::default()
        })
    }

    #[test]
    fn monster_think_finds_the_ground_after_the_monster_was_relinked() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().game.ground_entity = None;
        mi_mut(&mut world, key).currentmove = Some(MonsterMove {
            firstframe: 0,
            lastframe: 0,
            ..Default::default()
        });

        monster_think(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.monsterinfo.as_ref().unwrap().linkcount, ent.linkcount);
        assert!(ent.game.ground_entity.is_some());
    }

    #[test]
    fn monster_think_checks_the_ground_only_when_the_linkcount_changed() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        let linkcount = world.entities.get(key).unwrap().linkcount;
        world.entities.get_mut(key).unwrap().game.ground_entity = None;
        mi_mut(&mut world, key).linkcount = linkcount;

        monster_think(&mut world, key);

        assert!(world
            .entities
            .get(key)
            .unwrap()
            .game
            .ground_entity
            .is_none());
    }

    #[test]
    fn m_catagorize_position_measures_feet_waist_and_head_deep() {
        // The walker's box spans z = -1..57 once linked: probes at 1, 27, 49.
        for (top, level) in [(0.5, 0), (10.0, 1), (30.0, 2), (60.0, 3)] {
            let (mut world, _) = water_world(top, CONTENTS_WATER);
            let key = walker(&mut world, 0.0, 0.0);

            m_catagorize_position(&mut world, key);

            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.waterlevel, level, "top {top}");
            let want = if level > 0 { CONTENTS_WATER } else { 0 };
            assert_eq!(ent.game.watertype, want, "top {top}");
        }
    }

    /// A walker that takes damage, `waterlevel` deep in `watertype`.
    fn bather(watertype: i32, waterlevel: i32) -> (GameWorld, EntityKey, std::rc::Rc<FloorImport>) {
        let (mut world, gi) = floor_world_with(FloorImport::default());
        let key = walker(&mut world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.takedamage = TakeDamage::Aim;
        ent.game.watertype = watertype;
        ent.game.waterlevel = waterlevel;
        world.level.time = 20.0;
        mi_mut(&mut world, key).air_finished = 32.0; // not drowning
        (world, key, gi)
    }

    fn health(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().game.health
    }

    #[test]
    fn m_world_effects_drowns_a_walker_whose_air_ran_out() {
        let (mut world, key, _) = bather(CONTENTS_WATER, 3);
        mi_mut(&mut world, key).air_finished = 17.5;

        m_world_effects(&mut world, key);

        // 2 + 2 * floor(20 - 17.5).
        assert_eq!(health(&world, key), 94);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            21.0
        );
    }

    #[test]
    fn m_world_effects_drowning_hurts_at_most_15() {
        let (mut world, key, _) = bather(CONTENTS_WATER, 3);
        mi_mut(&mut world, key).air_finished = 1.0;

        m_world_effects(&mut world, key);

        assert_eq!(health(&world, key), 85);
    }

    #[test]
    fn m_world_effects_drowns_at_most_once_a_second() {
        let (mut world, key, _) = bather(CONTENTS_WATER, 3);
        mi_mut(&mut world, key).air_finished = 19.0;
        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 20.5;

        m_world_effects(&mut world, key);

        assert_eq!(health(&world, key), 100);
    }

    #[test]
    fn m_world_effects_gives_a_walker_12_seconds_of_air_until_its_head_is_under() {
        let (mut world, key, _) = bather(CONTENTS_WATER, 2);

        m_world_effects(&mut world, key);

        assert_eq!(monsterinfo(&world, key).air_finished, 32.0);
        assert_eq!(health(&world, key), 100);
    }

    #[test]
    fn m_world_effects_suffocates_a_swimmer_out_of_water_after_9_seconds() {
        let (mut world, key, _) = bather(CONTENTS_WATER, 1);
        world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::SWIM;
        m_world_effects(&mut world, key);
        assert_eq!(monsterinfo(&world, key).air_finished, 29.0);

        world.entities.get_mut(key).unwrap().game.waterlevel = 0;
        world.level.time = 29.5;
        m_world_effects(&mut world, key);
        assert_eq!(health(&world, key), 98);
    }

    #[test]
    fn m_world_effects_dead_monsters_do_not_drown() {
        let (mut world, key, _) = bather(CONTENTS_WATER, 3);
        world.entities.get_mut(key).unwrap().game.health = 0;
        mi_mut(&mut world, key).air_finished = 1.0;

        m_world_effects(&mut world, key);

        assert_eq!(health(&world, key), 0);
    }

    #[test]
    fn m_world_effects_burns_10_per_lava_level_and_splashes_in() {
        let (mut world, key, gi) = bather(CONTENTS_LAVA, 2);

        m_world_effects(&mut world, key);

        assert_eq!(health(&world, key), 80);
        let ent = world.entities.get(key).unwrap();
        assert!(ent.game.flags.contains(EntityFlags::INWATER));
        // Entering resets the debounce, so the next frame burns too.
        assert_eq!(ent.game.damage_debounce_time, 0.0);
        let sounds = gi.inner.sounds.borrow();
        assert!(sounds
            .iter()
            .any(|s| s == "player/lava1.wav" || s == "player/lava2.wav"));
    }

    #[test]
    fn m_world_effects_burns_in_lava_five_times_a_second() {
        let (mut world, key, _) = bather(CONTENTS_LAVA, 1);
        world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::INWATER;

        m_world_effects(&mut world, key);
        m_world_effects(&mut world, key);

        assert_eq!(health(&world, key), 90);
        assert_eq!(
            world.entities.get(key).unwrap().game.damage_debounce_time,
            20.2
        );
    }

    #[test]
    fn m_world_effects_spares_a_lava_immune_monster() {
        let (mut world, key, _) = bather(CONTENTS_LAVA, 3);
        world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::IMMUNE_LAVA;

        m_world_effects(&mut world, key);

        assert_eq!(health(&world, key), 100);
    }

    #[test]
    fn m_world_effects_slime_hurts_4_per_level_once_a_second_but_not_a_corpse() {
        let (mut world, key, _) = bather(CONTENTS_SLIME, 3);
        world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::INWATER;
        m_world_effects(&mut world, key);
        assert_eq!(health(&world, key), 88);
        assert_eq!(
            world.entities.get(key).unwrap().game.damage_debounce_time,
            21.0
        );

        let (mut world, key, _) = bather(CONTENTS_SLIME, 3);
        world.entities.get_mut(key).unwrap().svflags |= SvFlags::DEADMONSTER.bits();
        m_world_effects(&mut world, key);
        assert_eq!(health(&world, key), 100);
    }

    #[test]
    fn m_world_effects_splashes_into_water_and_out_again() {
        let (mut world, key, gi) = bather(CONTENTS_WATER, 1);
        m_world_effects(&mut world, key);
        assert!(gi
            .inner
            .sounds
            .borrow()
            .iter()
            .any(|s| s == "player/watr_in.wav"));

        world.entities.get_mut(key).unwrap().game.waterlevel = 0;
        m_world_effects(&mut world, key);
        assert!(gi
            .inner
            .sounds
            .borrow()
            .iter()
            .any(|s| s == "player/watr_out.wav"));
        assert!(!world
            .entities
            .get(key)
            .unwrap()
            .game
            .flags
            .contains(EntityFlags::INWATER));
    }

    #[test]
    fn m_world_effects_corpse_sinks_without_a_splash() {
        let (mut world, key, gi) = bather(CONTENTS_WATER, 1);
        world.entities.get_mut(key).unwrap().svflags |= SvFlags::DEADMONSTER.bits();

        m_world_effects(&mut world, key);

        assert!(gi.inner.sounds.borrow().is_empty());
        assert!(world
            .entities
            .get(key)
            .unwrap()
            .game
            .flags
            .contains(EntityFlags::INWATER));
    }

    #[test]
    fn m_droptofloor_drops_the_monster_onto_the_floor() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin.z = 100.0;
        ent.game.ground_entity = None;

        m_droptofloor(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert!(
            (ent.state.origin.z - 24.0).abs() < 0.01,
            "{}",
            ent.state.origin
        );
        assert!(ent.game.ground_entity.is_some());
    }

    #[test]
    fn m_droptofloor_with_no_floor_in_256_units_only_rises_a_unit() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().state.origin.z = 400.0;

        m_droptofloor(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.origin.z, 401.0);
    }

    #[test]
    fn m_set_effects_shows_a_red_shell_only_while_resurrecting() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().state.renderfx = RF_SHELL_GREEN | RF_FRAMELERP;
        mi_mut(&mut world, key).aiflags = AiFlags::RESURRECTING;

        m_set_effects(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.effects, EF_COLOR_SHELL);
        assert_eq!(ent.state.renderfx, RF_SHELL_RED | RF_FRAMELERP);

        mi_mut(&mut world, key).aiflags = AiFlags::empty();
        m_set_effects(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.effects, 0);
        assert_eq!(ent.state.renderfx, RF_FRAMELERP);
    }

    #[test]
    fn m_set_effects_shows_power_armor_for_a_fifth_of_a_second_after_a_save() {
        // C M_SetEffects: screen -> EF_POWERSCREEN, shield -> green shell,
        // while powerarmor_time > level.time and the monster is alive.
        use crate::constants::{POWER_ARMOR_SCREEN, POWER_ARMOR_SHIELD};
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.level.time = 2.0;
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.health = 10;
            ent.game.powerarmor_time = 2.1;
        }
        mi_mut(&mut world, key).power_armor_type = POWER_ARMOR_SCREEN;
        m_set_effects(&mut world, key);
        assert_eq!(
            world.entities.get(key).unwrap().state.effects,
            EF_POWERSCREEN
        );

        mi_mut(&mut world, key).power_armor_type = POWER_ARMOR_SHIELD;
        m_set_effects(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.effects, EF_COLOR_SHELL);
        assert_eq!(ent.state.renderfx & RF_SHELL_GREEN, RF_SHELL_GREEN);

        // Expired, or dead: nothing.
        world.level.time = 2.1;
        m_set_effects(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().state.effects, 0);
        world.level.time = 2.0;
        world.entities.get_mut(key).unwrap().game.health = 0;
        m_set_effects(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.effects, 0);
        assert_eq!(ent.state.renderfx & RF_SHELL_GREEN, 0);
    }

    #[test]
    fn m_flies_on_buzzes_round_a_dry_corpse_for_a_minute() {
        let (mut world, gi) = floor_world_with(FloorImport::default());
        let key = walker(&mut world, 0.0, 0.0);
        world.level.time = 5.0;

        m_flies_on(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.effects & EF_FLIES, EF_FLIES);
        assert_eq!(
            gi.inner.sounds.borrow().as_slice(),
            ["infantry/inflies1.wav"]
        );
        assert_eq!(ent.game.nextthink, 65.0);
        let off = ent.think.unwrap();

        off(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.effects & EF_FLIES, 0);
        assert_eq!(ent.state.sound, 0);
    }

    #[test]
    fn m_flies_on_and_m_fly_check_leave_a_corpse_in_water_alone() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().game.waterlevel = 1;

        m_flies_on(&mut world, key);
        m_fly_check(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.effects, 0);
        assert!(ent.think.is_none());
    }

    #[test]
    fn m_fly_check_brings_flies_to_half_of_dry_corpses_in_5_to_15_seconds() {
        let seed = |low: bool| {
            (0..)
                .find(|&seed| {
                    let mut probe = test_world();
                    probe.level.rand_state = seed;
                    (probe.random() <= 0.5) == low
                })
                .unwrap()
        };
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.level.rand_state = seed(false);
        m_fly_check(&mut world, key);
        assert!(world.entities.get(key).unwrap().think.is_none());

        world.level.rand_state = seed(true);
        m_fly_check(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert!(ent.think.is_some());
        assert!(
            (5.0..=15.0).contains(&ent.game.nextthink),
            "{}",
            ent.game.nextthink
        );
    }

    // -- monster_use and trigger spawning --

    #[test]
    fn monster_use_makes_the_monster_angry_at_the_player_who_used_it() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 300.0);

        monster_use(&mut world, monster, player, player);

        assert_eq!(enemy_of(&world, monster), Some(player));
        assert_eq!(
            world.entities.get(monster).unwrap().game.goalentity,
            Some(player)
        );
    }

    #[test]
    fn monster_use_ignores_non_players_notarget_and_monsters_already_angry_or_dead() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 300.0);
        let button = world.spawn().unwrap();
        monster_use(&mut world, monster, button, button);
        assert_eq!(enemy_of(&world, monster), None, "not a client");

        world.entities.get_mut(player).unwrap().game.flags |= EntityFlags::NOTARGET;
        monster_use(&mut world, monster, player, player);
        assert_eq!(enemy_of(&world, monster), None, "notarget");
        world.entities.get_mut(player).unwrap().game.flags = EntityFlags::empty();

        world.entities.get_mut(monster).unwrap().game.health = 0;
        monster_use(&mut world, monster, player, player);
        assert_eq!(enemy_of(&world, monster), None, "dead");
        world.entities.get_mut(monster).unwrap().game.health = 100;

        let other = walker(&mut world, 0.0, 500.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(other);
        monster_use(&mut world, monster, player, player);
        assert_eq!(enemy_of(&world, monster), Some(other), "already angry");
    }

    #[test]
    fn monster_use_by_a_good_guy_monster_counts() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let good = walker(&mut world, 0.0, 300.0);
        mi_mut(&mut world, good).aiflags = AiFlags::GOOD_GUY;

        monster_use(&mut world, monster, good, good);

        assert_eq!(enemy_of(&world, monster), Some(good));
    }

    #[test]
    fn monster_triggered_start_hides_the_monster_until_used() {
        let mut world = floor_world(None);
        let monster = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(monster).unwrap().game.nextthink = 3.0;

        monster_triggered_start(&mut world, monster);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.solid, Solid::Not);
        assert_eq!(ent.game.movetype, MoveType::None);
        assert!(ent.svflags & SvFlags::NOCLIENT.bits() != 0);
        assert_eq!(ent.game.nextthink, 0.0);
        assert!(ent.use_fn.is_some());
    }

    #[test]
    fn monster_triggered_spawn_use_spawns_next_frame_angry_at_the_player() {
        let mut world = floor_world(None);
        world.level.time = 4.0;
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 300.0);
        monster_triggered_start(&mut world, monster);

        let use_fn = world.entities.get(monster).unwrap().use_fn.unwrap();
        use_fn(&mut world, monster, player, player);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.game.enemy, Some(player));
        assert!((ent.game.nextthink - 4.1).abs() < 1e-4);
        assert!(ent.think.is_some());
        // Using it again just makes it angry (monster_use).
        let button = world.spawn().unwrap();
        let again = world.entities.get(monster).unwrap().use_fn.unwrap();
        world.entities.get_mut(monster).unwrap().game.enemy = None;
        again(&mut world, monster, button, button);
        assert_eq!(enemy_of(&world, monster), None, "a button is not a player");
    }

    #[test]
    fn monster_triggered_spawn_use_by_a_non_player_spawns_without_an_enemy() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let relay = world.spawn().unwrap();
        monster_triggered_start(&mut world, monster);

        monster_triggered_spawn_use(&mut world, monster, relay, relay);

        assert_eq!(enemy_of(&world, monster), None);
    }

    #[test]
    fn monster_triggered_spawn_appears_and_goes_after_the_player() {
        let mut world = floor_world(None);
        world.level.time = 4.0;
        let monster = counting_monster(&mut world, 0.0, 0.0);
        let player = player_at(&mut world, 0.0, 300.0);
        monster_triggered_start(&mut world, monster);
        monster_triggered_spawn_use(&mut world, monster, player, player);

        monster_triggered_spawn(&mut world, monster);

        let ent = world.entities.get(monster).unwrap();
        assert_eq!(ent.solid, Solid::Bbox);
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert_eq!(ent.svflags & SvFlags::NOCLIENT.bits(), 0);
        assert_eq!(ent.state.origin.z, 25.0);
        assert_eq!(ent.game.goalentity, Some(player), "FoundTarget");
        let mi = monsterinfo(&world, monster);
        assert_eq!(mi.air_finished, 16.0);
        // monster_start_go stood it (1), then FoundTarget ran it (100).
        assert_eq!(count(&world, monster), 101);
    }

    #[test]
    fn monster_triggered_spawn_ambusher_forgets_the_player() {
        let mut world = floor_world(None);
        let monster = counting_monster(&mut world, 0.0, 0.0);
        world.entities.get_mut(monster).unwrap().game.spawnflags = SPAWNFLAG_AMBUSH;
        let player = player_at(&mut world, 0.0, 300.0);
        world.entities.get_mut(monster).unwrap().game.enemy = Some(player);

        monster_triggered_spawn(&mut world, monster);

        assert_eq!(enemy_of(&world, monster), None);
    }

    // -- monster_start, monster_start_go, walk/fly/swim starts --

    #[test]
    fn monster_start_turns_spawnflag_4_into_ambush_except_for_good_guys() {
        let mut world = floor_world(None);
        let bad = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(bad).unwrap().game.spawnflags = 4;
        let good = walker(&mut world, 0.0, 100.0);
        world.entities.get_mut(good).unwrap().game.spawnflags = 4;
        mi_mut(&mut world, good).aiflags = AiFlags::GOOD_GUY;

        monster_start(&mut world, bad);
        monster_start(&mut world, good);

        assert_eq!(
            world.entities.get(bad).unwrap().game.spawnflags,
            SPAWNFLAG_AMBUSH
        );
        assert_eq!(world.entities.get(good).unwrap().game.spawnflags, 4);
    }

    #[test]
    fn monster_start_reports_a_trigger_spawned_monster_without_a_targetname() {
        let (mut world, gi) = floor_world_with(FloorImport::default());
        let key = walker(&mut world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.classname = "monster_tank".to_string();
        ent.game.spawnflags = SPAWNFLAG_TRIGGER_SPAWN;

        monster_start(&mut world, key);

        assert!(gi
            .inner
            .dprints
            .borrow()
            .iter()
            .any(|m| m == "triggered monster_tank at (0 0 24) has no targetname\n"));
        // g_fix_triggered is 0: it still waits to be triggered.
        assert_eq!(
            world.entities.get(key).unwrap().game.spawnflags,
            SPAWNFLAG_TRIGGER_SPAWN
        );
    }

    #[test]
    fn monster_start_sets_c_defaults_and_thinks_next_frame() {
        let mut world = test_world();
        world.level.time = 2.0;
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().monsterinfo = Some(Box::default());

        assert!(monster_start(&mut world, key));

        let ent = world.entities.get(key).unwrap();
        assert!((ent.game.nextthink - 2.1).abs() < 1e-4);
        assert_eq!(ent.state.renderfx & RF_FRAMELERP, RF_FRAMELERP);
        assert!(ent.use_fn.is_some());
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(mi.checkattack.is_some());
        assert_eq!(mi.air_finished, 14.0);
    }

    #[test]
    fn monster_start_walker_by_default_starts_with_yaw_speed_20_and_eye_25() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().game.yaw_speed = 0.0;
        monster_start(&mut world, key);

        let think = world.entities.get(key).unwrap().think.unwrap();
        think(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.yaw_speed, 20.0);
        assert_eq!(ent.game.viewheight, 25.0);
        assert!(!ent
            .game
            .flags
            .intersects(EntityFlags::FLY | EntityFlags::SWIM));
    }

    #[test]
    fn flymonster_start_flies_with_yaw_speed_10_and_eye_25() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.yaw_speed = 0.0;
        flymonster_start(ent);
        monster_start(&mut world, key);

        let think = world.entities.get(key).unwrap().think.unwrap();
        think(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert!(ent.game.flags.contains(EntityFlags::FLY));
        assert_eq!(ent.game.yaw_speed, 10.0);
        assert_eq!(ent.game.viewheight, 25.0);
    }

    #[test]
    fn swimmonster_start_swims_with_yaw_speed_10_and_eye_10() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.yaw_speed = 0.0;
        swimmonster_start(ent);
        monster_start(&mut world, key);

        let think = world.entities.get(key).unwrap().think.unwrap();
        think(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert!(ent.game.flags.contains(EntityFlags::SWIM));
        assert_eq!(ent.game.yaw_speed, 10.0);
        assert_eq!(ent.game.viewheight, 10.0);
    }

    #[test]
    fn walkmonster_start_go_drops_a_monster_to_the_floor_at_map_load() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().state.origin.z = 80.0;

        walkmonster_start_go(&mut world, key);

        let z = world.entities.get(key).unwrap().state.origin.z;
        assert!((z - 24.0).abs() < 0.01, "{z}");
    }

    #[test]
    fn walkmonster_start_go_after_the_first_second_leaves_the_monster_where_it_is() {
        let mut world = floor_world(None);
        world.level.time = 1.0;
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().state.origin.z = 80.0;

        walkmonster_start_go(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.origin.z, 80.0);
    }

    #[test]
    fn walkmonster_start_go_hides_a_trigger_spawned_monster_without_dropping_it() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin.z = 80.0;
        ent.game.spawnflags = SPAWNFLAG_TRIGGER_SPAWN;

        walkmonster_start_go(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.origin.z, 80.0);
        assert_eq!(ent.solid, Solid::Not);
    }

    #[test]
    fn monster_start_go_without_a_target_stands_until_something_happens() {
        let mut world = floor_world(None);
        world.level.time = 3.0;
        let key = counting_monster(&mut world, 0.0, 0.0);

        monster_start_go(&mut world, key);

        assert_eq!(count(&world, key), 1);
        assert_eq!(monsterinfo(&world, key).pausetime, 100_000_000.0);
        let ent = world.entities.get(key).unwrap();
        assert!((ent.game.nextthink - 3.1).abs() < 1e-4);
    }

    #[test]
    fn monster_start_go_of_a_dead_monster_does_nothing() {
        let mut world = floor_world(None);
        let key = counting_monster(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().game.health = 0;

        monster_start_go(&mut world, key);

        assert_eq!(count(&world, key), 0);
    }

    fn targeted(world: &mut GameWorld, classname: &str, name: &str, x: f32, y: f32) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.classname = classname.to_string();
        ent.game.targetname = name.to_string();
        ent.state.origin = Vec3f::new(x, y, 24.0);
        key
    }

    #[test]
    fn monster_start_go_walks_toward_a_path_corner_target() {
        let mut world = floor_world(None);
        let key = counting_monster(&mut world, 0.0, 0.0);
        let corner = targeted(&mut world, "path_corner", "p1", 0.0, 200.0);
        world.entities.get_mut(key).unwrap().game.target = "p1".to_string();

        monster_start_go(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.goalentity, Some(corner));
        assert_eq!(ent.game.movetarget, Some(corner));
        assert_eq!(ent.game.ideal_yaw, 90.0);
        assert_eq!(ent.state.angles.y, 90.0);
        assert!(ent.game.target.is_empty());
        assert_eq!(count(&world, key), 10);
    }

    #[test]
    fn monster_start_go_makes_a_combat_point_target_its_combattarget() {
        let mut world = floor_world(None);
        let key = counting_monster(&mut world, 0.0, 0.0);
        targeted(&mut world, "point_combat", "cp", 0.0, 200.0);
        world.entities.get_mut(key).unwrap().game.target = "cp".to_string();

        monster_start_go(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.combattarget, "cp");
        assert!(ent.game.target.is_empty());
        assert_eq!(count(&world, key), 1);
    }

    #[test]
    fn monster_start_go_reports_mixed_and_bad_combat_targets() {
        let (mut world, gi) = floor_world_with(FloorImport::default());
        let key = counting_monster(&mut world, 0.0, 0.0);
        targeted(&mut world, "point_combat", "mix", 0.0, 200.0);
        targeted(&mut world, "path_corner", "mix", 0.0, 300.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.classname = "monster_gunner".to_string();
        ent.game.target = "mix".to_string();

        monster_start_go(&mut world, key);

        let dprints = gi.inner.dprints.borrow();
        assert!(dprints
            .iter()
            .any(|m| m == "monster_gunner at (0 0 24) has target with mixed types\n"));
        assert!(dprints.iter().any(|m| {
            m
            == "monster_gunner at (0 0 24) has a bad combattarget mix : path_corner at (0 300 24)\n"
        }));
    }

    #[test]
    fn monster_start_go_with_a_missing_target_says_so_and_stands() {
        let (mut world, gi) = floor_world_with(FloorImport::default());
        let key = counting_monster(&mut world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.classname = "monster_gunner".to_string();
        ent.game.target = "gone".to_string();

        monster_start_go(&mut world, key);

        assert!(gi
            .inner
            .dprints
            .borrow()
            .iter()
            .any(|m| m == "monster_gunner can't find target gone at (0 0 24)\n"));
        let ent = world.entities.get(key).unwrap();
        assert!(ent.game.target.is_empty());
        assert_eq!(count(&world, key), 1);
    }

    #[test]
    fn monster_start_go_with_another_kind_of_target_stands_without_a_goal() {
        let mut world = floor_world(None);
        let key = counting_monster(&mut world, 0.0, 0.0);
        targeted(&mut world, "info_notnull", "spot", 0.0, 200.0);
        world.entities.get_mut(key).unwrap().game.target = "spot".to_string();

        monster_start_go(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.goalentity, None);
        assert_eq!(ent.game.movetarget, None);
        assert_eq!(ent.game.target, "spot");
        assert_eq!(count(&world, key), 1);
    }
}
