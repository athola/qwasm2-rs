//! Gladiator: a cleaver in melee and a charged railgun at range.
//! C ref: `~/Qwasm2/src/game/monster/gladiator/gladiator.c`.

use q2_shared::constants::MZ2_GLADIATOR_RAILGUN_1;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::monster::fire::monster_fire_railgun;
use crate::monster::flash::monster_flash_source;
use crate::monster::{footstep, frame, set_move, sound, throw_organic_gibs, MONSTER_FOOTSTEPS};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: gladiator.h.
// ---------------------------------------------------------------------------

const FRAME_STAND1: i32 = 0;
const FRAME_STAND7: i32 = 6;
const FRAME_WALK1: i32 = 7;
const FRAME_WALK16: i32 = 22;
const FRAME_RUN1: i32 = 23;
const FRAME_RUN6: i32 = 28;
const FRAME_MELEE1: i32 = 29;
const FRAME_MELEE17: i32 = 45;
const FRAME_ATTACK1: i32 = 46;
const FRAME_ATTACK9: i32 = 54;
const FRAME_PAIN1: i32 = 55;
const FRAME_PAIN6: i32 = 60;
const FRAME_DEATH1: i32 = 61;
const FRAME_DEATH22: i32 = 82;
const FRAME_PAINUP1: i32 = 83;
const FRAME_PAINUP7: i32 = 89;

/// C `MODEL_SCALE` (gladiator.h).
const MODEL_SCALE: f32 = 1.0;

// C SP_monster_gladiator values.
const HEALTH: i32 = 400;
const MINS: Vec3f = Vec3f::new(-32.0, -32.0, -24.0);
const MAXS: Vec3f = Vec3f::new(32.0, 32.0, 64.0);
const MASS: i32 = 400;

const SOUND_PAIN1: &str = "gladiator/pain.wav";
const SOUND_PAIN2: &str = "gladiator/gldpain2.wav";
const SOUND_DIE: &str = "gladiator/glddeth2.wav";
const SOUND_GUN: &str = "gladiator/railgun.wav";
const SOUND_CLEAVER_SWING: &str = "gladiator/melee1.wav";
const SOUND_CLEAVER_HIT: &str = "gladiator/melee2.wav";
const SOUND_CLEAVER_MISS: &str = "gladiator/melee3.wav";
const SOUND_IDLE: &str = "gladiator/gldidle1.wav";
const SOUND_SEARCH: &str = "gladiator/gldsrch1.wav";
pub(crate) const SOUND_STEPS: [&str; 2] = ["gladiator/step1.wav", "gladiator/step2.wav"];
const SOUND_SIGHT: &str = "gladiator/sight.wav";

/// The sounds C `SP_monster_gladiator` precaches, in its order.
const PRECACHE: [&str; 10] = [
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_DIE,
    SOUND_GUN,
    SOUND_CLEAVER_SWING,
    SOUND_CLEAVER_HIT,
    SOUND_CLEAVER_MISS,
    SOUND_IDLE,
    SOUND_SEARCH,
    SOUND_SIGHT,
];

// ---------------------------------------------------------------------------
// Moves. C ref: gladiator.c `gladiator_frames_*` / `gladiator_move_*`.
// ---------------------------------------------------------------------------

static GLADIATOR_FRAMES_STAND: [MonsterFrame; 7] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
];

static GLADIATOR_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND1,
    lastframe: FRAME_STAND7,
    frames: &GLADIATOR_FRAMES_STAND,
    endfunc: None,
};

static GLADIATOR_FRAMES_WALK: [MonsterFrame; 16] = [
    frame(ai::ai_walk, 15.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 2.0, Some(gladiator_footstep)),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 8.0, None),
    frame(ai::ai_walk, 12.0, None),
    frame(ai::ai_walk, 8.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 2.0, Some(gladiator_footstep)),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 1.0, None),
    frame(ai::ai_walk, 8.0, None),
];

static GLADIATOR_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK1,
    lastframe: FRAME_WALK16,
    frames: &GLADIATOR_FRAMES_WALK,
    endfunc: None,
};

static GLADIATOR_FRAMES_RUN: [MonsterFrame; 6] = [
    frame(ai::ai_run, 23.0, None),
    frame(ai::ai_run, 14.0, None),
    frame(ai::ai_run, 14.0, Some(gladiator_footstep)),
    frame(ai::ai_run, 21.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 13.0, Some(gladiator_footstep)),
];

static GLADIATOR_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN1,
    lastframe: FRAME_RUN6,
    frames: &GLADIATOR_FRAMES_RUN,
    endfunc: None,
};

static GLADIATOR_FRAMES_ATTACK_MELEE: [MonsterFrame; 17] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gladiator_cleaver_swing)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gladiator_melee_hit)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gladiator_cleaver_swing)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gladiator_melee_hit)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

static GLADIATOR_MOVE_ATTACK_MELEE: MonsterMove = MonsterMove {
    firstframe: FRAME_MELEE1,
    lastframe: FRAME_MELEE17,
    frames: &GLADIATOR_FRAMES_ATTACK_MELEE,
    endfunc: Some(gladiator_run),
};

static GLADIATOR_FRAMES_ATTACK_GUN: [MonsterFrame; 9] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gladiator_gun)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

static GLADIATOR_MOVE_ATTACK_GUN: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK1,
    lastframe: FRAME_ATTACK9,
    frames: &GLADIATOR_FRAMES_ATTACK_GUN,
    endfunc: Some(gladiator_run),
};

static GLADIATOR_FRAMES_PAIN: [MonsterFrame; 6] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static GLADIATOR_MOVE_PAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN1,
    lastframe: FRAME_PAIN6,
    frames: &GLADIATOR_FRAMES_PAIN,
    endfunc: Some(gladiator_run),
};

static GLADIATOR_FRAMES_PAIN_AIR: [MonsterFrame; 7] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static GLADIATOR_MOVE_PAIN_AIR: MonsterMove = MonsterMove {
    firstframe: FRAME_PAINUP1,
    lastframe: FRAME_PAINUP7,
    frames: &GLADIATOR_FRAMES_PAIN_AIR,
    endfunc: Some(gladiator_run),
};

static GLADIATOR_FRAMES_DEATH: [MonsterFrame; 22] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static GLADIATOR_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH1,
    lastframe: FRAME_DEATH22,
    frames: &GLADIATOR_FRAMES_DEATH,
    endfunc: Some(gladiator_dead),
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// C `self->monsterinfo.currentmove == &move`: the frame tables are
/// statics, so their addresses identify a move.
fn is_current(world: &GameWorld, key: EntityKey, mv: &MonsterMove) -> bool {
    world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .and_then(|mi| mi.currentmove.as_ref())
        .is_some_and(|cm| std::ptr::eq(cm.frames, mv.frames))
}

fn aiflags(world: &GameWorld, key: EntityKey) -> AiFlags {
    world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .map_or(AiFlags::empty(), |mi| mi.aiflags)
}

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

/// C ref: `gladiator_footstep`; silent unless `g_monsterfootsteps` is set.
pub fn gladiator_footstep(world: &mut GameWorld, key: EntityKey) {
    footstep(world, key, MONSTER_FOOTSTEPS, &SOUND_STEPS, 1.0);
}

/// C ref: `gladiator_idle`.
pub fn gladiator_idle(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
}

/// C ref: `gladiator_sight`.
pub fn gladiator_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `gladiator_search`.
pub fn gladiator_search(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SEARCH, ATTN_NORM);
}

/// C ref: `gladiator_cleaver_swing`.
pub fn gladiator_cleaver_swing(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_CLEAVER_SWING, ATTN_NORM);
}

/// C ref: `gladiator_stand`.
pub fn gladiator_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &GLADIATOR_MOVE_STAND);
}

/// C ref: `gladiator_walk`.
pub fn gladiator_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &GLADIATOR_MOVE_WALK);
}

/// C ref: `gladiator_run`.
pub fn gladiator_run(world: &mut GameWorld, key: EntityKey) {
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        set_move(world, key, &GLADIATOR_MOVE_STAND);
    } else {
        set_move(world, key, &GLADIATOR_MOVE_RUN);
    }
}

/// A cleaver blow of 20 to 24 damage, with a hit or a miss sound.
/// C ref: `GaldiatorMelee` (sic).
pub fn gladiator_melee_hit(world: &mut GameWorld, key: EntityKey) {
    let Some(mins_x) = world.entities.get(key).map(|e| e.mins.x) else {
        return;
    };
    let aim = Vec3f::new(ai::MELEE_DISTANCE, mins_x, -4.0);
    let damage = 20 + (world.random_u32() % 5) as i32;
    if world.fire_hit(key, aim, damage, 300) {
        sound(world, key, CHAN_AUTO, SOUND_CLEAVER_HIT, ATTN_NORM);
    } else {
        sound(world, key, CHAN_AUTO, SOUND_CLEAVER_MISS, ATTN_NORM);
    }
}

/// C ref: `gladiator_melee`.
pub fn gladiator_melee(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &GLADIATOR_MOVE_ATTACK_MELEE);
}

/// Fire the railgun at the spot `gladiator_attack` saved, not where the
/// enemy is now. C ref: `GladiatorGun`.
pub fn gladiator_gun(world: &mut GameWorld, key: EntityKey) {
    let Some((start, _, _)) = monster_flash_source(world, key, MZ2_GLADIATOR_RAILGUN_1) else {
        return;
    };
    let Some(pos1) = world.entities.get(key).map(|e| e.game.move_origin) else {
        return;
    };
    let dir = (pos1 - start).normalize_or_zero();
    monster_fire_railgun(world, key, start, dir, 50, 100, MZ2_GLADIATOR_RAILGUN_1);
}

/// Charge the railgun at the enemy's eyes, except inside a small safe zone
/// (not for stand-ground gladiators: players could abuse it). C keeps the
/// aim in `self->pos1`; the entity has no such field (needs-owner), so it
/// lives in `game.move_origin`, which only movers use.
/// C ref: `gladiator_attack`.
pub fn gladiator_attack(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let Some(enemy) = ent.game.enemy.and_then(|e| world.entities.get(e)) else {
        return;
    };
    let (origin, enemy_origin) = (ent.state.origin, enemy.state.origin);
    let aim = enemy_origin + Vec3f::new(0.0, 0.0, enemy.game.viewheight);
    if !aiflags(world, key).contains(AiFlags::STAND_GROUND)
        && (origin - enemy_origin).length() <= ai::MELEE_DISTANCE + 32.0
    {
        return;
    }
    sound(world, key, CHAN_WEAPON, SOUND_GUN, ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.move_origin = aim;
    }
    set_move(world, key, &GLADIATOR_MOVE_ATTACK_GUN);
}

/// C ref: `gladiator_pain`.
pub fn gladiator_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    _damage: i32,
) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.health < ent.game.max_health / 2 {
        ent.state.skinnum = 1;
    }
    let knocked_up = ent.velocity.z > 100.0;
    if time < ent.game.pain_debounce_time {
        if knocked_up && is_current(world, key, &GLADIATOR_MOVE_PAIN) {
            set_move(world, key, &GLADIATOR_MOVE_PAIN_AIR);
        }
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.pain_debounce_time = time + 3.0;
    }
    if world.random() < 0.5 {
        sound(world, key, CHAN_VOICE, SOUND_PAIN1, ATTN_NORM);
    } else {
        sound(world, key, CHAN_VOICE, SOUND_PAIN2, ATTN_NORM);
    }
    if world.game.skill == SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    if knocked_up {
        set_move(world, key, &GLADIATOR_MOVE_PAIN_AIR);
    } else {
        set_move(world, key, &GLADIATOR_MOVE_PAIN);
    }
}

/// The corpse box, then stop. C ref: `gladiator_dead`.
pub fn gladiator_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// Gib at `gib_health`, else fall over. C ref: `gladiator_die`.
pub fn gladiator_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.health <= ent.game.gib_health {
        sound(world, key, CHAN_VOICE, "misc/udeath.wav", ATTN_NORM);
        throw_organic_gibs(world, key, damage);
        return;
    }
    if ent.game.deadflag == DeadFlag::Dead {
        return;
    }
    sound(world, key, CHAN_VOICE, SOUND_DIE, ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    set_move(world, key, &GLADIATOR_MOVE_DEATH);
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_gladiator`; `monster_start`
/// (run by `call_spawn`) removes it in deathmatch.
pub fn sp_monster_gladiator(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = fields
        .get("classname")
        .cloned()
        .unwrap_or_else(|| "monster_gladiator".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = MINS;
    ent.maxs = MAXS;
    ent.game.health = HEALTH;
    ent.game.gib_health = -175;
    ent.game.mass = MASS;
    ent.pain = Some(gladiator_pain);
    ent.die = Some(gladiator_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(gladiator_stand),
        walk: Some(gladiator_walk),
        run: Some(gladiator_run),
        dodge: None,
        attack: Some(gladiator_attack),
        melee: Some(gladiator_melee),
        sight: Some(gladiator_sight),
        idle: Some(gladiator_idle),
        search: Some(gladiator_search),
        currentmove: Some(GLADIATOR_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_gladiator`: its sound precaches.
pub fn sp_monster_gladiator_finish(
    world: &mut GameWorld,
    _key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    for name in PRECACHE {
        world.gi.sound_index(name);
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{AnimFrameFn, ThinkFn};
    use crate::misc::wire::{wire_world, Wire};
    use crate::monster::test_support::{monster_arena, run, sounds};
    use crate::world::{test_world, test_world_recording};
    use q2_shared::protocol::SvcOp;

    type CFrame = (AnimFrameFn, f32, Option<ThinkFn>);
    type CMove = (&'static MonsterMove, i32, i32, Vec<CFrame>, Option<ThinkFn>);

    /// (move, firstframe, lastframe, [(aifunc, dist, thinkfunc)], endfunc) from C.
    #[rustfmt::skip]
    fn c_moves() -> Vec<CMove> {
        vec![
            (&GLADIATOR_MOVE_STAND, 0, 6, vec![
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
            ], None),
            (&GLADIATOR_MOVE_WALK, 7, 22, vec![
                (ai::ai_walk as AnimFrameFn, 15.0, None),
                (ai::ai_walk as AnimFrameFn, 7.0, None),
                (ai::ai_walk as AnimFrameFn, 6.0, None),
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, 2.0, Some(gladiator_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 0.0, None),
                (ai::ai_walk as AnimFrameFn, 2.0, None),
                (ai::ai_walk as AnimFrameFn, 8.0, None),
                (ai::ai_walk as AnimFrameFn, 12.0, None),
                (ai::ai_walk as AnimFrameFn, 8.0, None),
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, 2.0, Some(gladiator_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 2.0, None),
                (ai::ai_walk as AnimFrameFn, 1.0, None),
                (ai::ai_walk as AnimFrameFn, 8.0, None),
            ], None),
            (&GLADIATOR_MOVE_RUN, 23, 28, vec![
                (ai::ai_run as AnimFrameFn, 23.0, None),
                (ai::ai_run as AnimFrameFn, 14.0, None),
                (ai::ai_run as AnimFrameFn, 14.0, Some(gladiator_footstep as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 21.0, None),
                (ai::ai_run as AnimFrameFn, 12.0, None),
                (ai::ai_run as AnimFrameFn, 13.0, Some(gladiator_footstep as ThinkFn)),
            ], None),
            (&GLADIATOR_MOVE_ATTACK_MELEE, 29, 45, vec![
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(gladiator_cleaver_swing as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(gladiator_melee_hit as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(gladiator_cleaver_swing as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(gladiator_melee_hit as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
            ], Some(gladiator_run as ThinkFn)),
            (&GLADIATOR_MOVE_ATTACK_GUN, 46, 54, vec![
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(gladiator_gun as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
            ], Some(gladiator_run as ThinkFn)),
            (&GLADIATOR_MOVE_PAIN, 55, 60, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(gladiator_run as ThinkFn)),
            (&GLADIATOR_MOVE_PAIN_AIR, 83, 89, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(gladiator_run as ThinkFn)),
            (&GLADIATOR_MOVE_DEATH, 61, 82, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(gladiator_dead as ThinkFn)),
        ]
    }

    fn same_think(a: Option<ThinkFn>, b: Option<ThinkFn>) -> bool {
        match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => std::ptr::fn_addr_eq(a, b),
            _ => false,
        }
    }

    #[test]
    fn gladiator_moves_match_c_frame_by_frame() {
        for (mv, first, last, frames, end) in c_moves() {
            assert_eq!((mv.firstframe, mv.lastframe), (first, last));
            assert_eq!(mv.frames.len(), frames.len(), "move at {first}");
            assert_eq!(mv.frames.len() as i32, last - first + 1);
            for (i, (got, (ai, dist, think))) in mv.frames.iter().zip(&frames).enumerate() {
                let at = format!("move at {first}, frame {i}");
                assert!(
                    got.aifunc.is_some_and(|a| std::ptr::fn_addr_eq(a, *ai)),
                    "{at}"
                );
                assert_eq!(got.dist, *dist, "{at}");
                assert!(same_think(got.thinkfunc, *think), "{at}");
            }
            assert!(same_think(mv.endfunc, end), "move at {first}");
        }
    }

    fn spawned(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        sp_monster_gladiator(&mut world.entities, key, &HashMap::new());
        key
    }

    fn mi_mut(world: &mut GameWorld, key: EntityKey) -> &mut MonsterInfo {
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
    }

    fn is_move(world: &GameWorld, key: EntityKey, mv: &MonsterMove) -> bool {
        is_current(world, key, mv)
    }

    /// Step the RNG until the next `random()` satisfies `pred`.
    fn seed_random(world: &mut GameWorld, pred: impl Fn(f32) -> bool) {
        loop {
            let state = world.level.rand_state;
            let r = world.random();
            world.level.rand_state = state;
            if pred(r) {
                return;
            }
            world.random_u32();
        }
    }

    /// Step the RNG until the next `random_u32()` (C `randk()`) satisfies
    /// `pred`.
    fn seed_randk(world: &mut GameWorld, pred: impl Fn(u32) -> bool) {
        loop {
            let state = world.level.rand_state;
            let r = world.random_u32();
            world.level.rand_state = state;
            if pred(r) {
                return;
            }
            world.random_u32();
        }
    }

    /// A monster enemy `x` units in front, with eyes 22 up.
    fn enemy_at(world: &mut GameWorld, key: EntityKey, x: f32) -> EntityKey {
        let enemy = world.spawn().unwrap();
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = Vec3f::new(x, 0.0, 0.0);
        e.mins = Vec3f::new(-16.0, -16.0, -24.0);
        e.maxs = Vec3f::new(16.0, 16.0, 32.0);
        e.game.health = 100;
        e.game.viewheight = 22.0;
        e.game.takedamage = TakeDamage::Yes;
        e.svflags |= SvFlags::MONSTER.bits();
        world.link_entity(enemy);
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        enemy
    }

    #[test]
    fn gladiator_spawns_with_the_c_values_and_callbacks() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "monster_gladiator");
        assert_eq!((ent.game.health, ent.game.mass), (400, 400));
        assert_eq!((ent.mins, ent.maxs), (MINS, MAXS));
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(std::ptr::fn_addr_eq(
            ent.pain.unwrap(),
            gladiator_pain as crate::entity::PainFn
        ));
        for (got, want) in [
            (mi.walk, gladiator_walk as ThinkFn),
            (mi.run, gladiator_run),
            (mi.melee, gladiator_melee),
        ] {
            assert!(std::ptr::fn_addr_eq(got.unwrap(), want));
        }
        assert_eq!(mi.scale, 1.0);
        assert!(mi.dodge.is_none());
        assert!(std::ptr::fn_addr_eq(
            mi.attack.unwrap(),
            gladiator_attack as ThinkFn
        ));
        assert!(std::ptr::fn_addr_eq(
            mi.idle.unwrap(),
            gladiator_idle as ThinkFn
        ));
        assert!(mi.sight.is_some() && mi.search.is_some());
        assert!(is_move(&world, key, &GLADIATOR_MOVE_STAND));
    }

    #[test]
    fn gladiator_finish_precaches_the_c_sounds_and_the_spawn_row_uses_it() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        let row = crate::spawn::find_spawn("monster_gladiator").unwrap();
        assert_eq!(row.2, Some("models/monsters/gladiatr/tris.md2"));
        row.3.unwrap()(&mut world, key, &HashMap::new());
        assert_eq!(*gi.sounds.borrow(), PRECACHE);
    }

    #[test]
    fn gladiator_idle_sight_search_and_swing_play_their_sounds() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        gladiator_idle(&mut world, key);
        gladiator_sight(&mut world, key, key);
        gladiator_search(&mut world, key);
        gladiator_cleaver_swing(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [
                (CHAN_VOICE, SOUND_IDLE.to_string(), ATTN_IDLE),
                (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_SEARCH.to_string(), ATTN_NORM),
                (CHAN_WEAPON, SOUND_CLEAVER_SWING.to_string(), ATTN_NORM),
            ]
        );
    }

    #[test]
    fn gladiator_walks_runs_and_melees_unless_holding_ground() {
        let mut world = test_world();
        let key = spawned(&mut world);
        gladiator_walk(&mut world, key);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_WALK));
        gladiator_run(&mut world, key);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_RUN));
        gladiator_melee(&mut world, key);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_ATTACK_MELEE));
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        gladiator_run(&mut world, key);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_STAND));
    }

    #[test]
    fn gladiator_cleaver_hits_for_20_to_24_with_the_hit_sound() {
        for k in 0..5 {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            let enemy = enemy_at(&mut world, key, 40.0);
            *gi.next_trace.borrow_mut() = Some(Trace {
                fraction: 0.5,
                ent_index: world.number_of(enemy),
                ..Trace::default()
            });
            seed_randk(&mut world, |r| r % 5 == k);
            gladiator_melee_hit(&mut world, key);
            let health = world.entities.get(enemy).unwrap().game.health;
            assert_eq!(health, 100 - (20 + k as i32));
            assert_eq!(
                sounds(&gi),
                [(CHAN_AUTO, SOUND_CLEAVER_HIT.to_string(), ATTN_NORM)]
            );
        }
    }

    #[test]
    fn gladiator_cleaver_out_of_reach_plays_the_miss_sound() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        let enemy = enemy_at(&mut world, key, 200.0);
        gladiator_melee_hit(&mut world, key);
        assert_eq!(world.entities.get(enemy).unwrap().game.health, 100);
        assert_eq!(
            sounds(&gi),
            [(CHAN_AUTO, SOUND_CLEAVER_MISS.to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn gladiator_attack_inside_the_safe_zone_does_nothing() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        enemy_at(&mut world, key, 112.0);
        gladiator_attack(&mut world, key);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_STAND));
        assert!(sounds(&gi).is_empty());
    }

    #[test]
    fn gladiator_attack_charges_the_railgun_at_the_enemy_eyes() {
        for (x, stand_ground) in [(113.0, false), (50.0, true)] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            enemy_at(&mut world, key, x);
            if stand_ground {
                mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
            }
            gladiator_attack(&mut world, key);
            assert!(is_move(&world, key, &GLADIATOR_MOVE_ATTACK_GUN));
            assert_eq!(
                sounds(&gi),
                [(CHAN_WEAPON, SOUND_GUN.to_string(), ATTN_NORM)]
            );
            let pos1 = world.entities.get(key).unwrap().game.move_origin;
            assert_eq!(pos1, Vec3f::new(x, 0.0, 22.0));
        }
    }

    #[test]
    fn gladiator_gun_fires_its_rail_flash_from_the_c_offset() {
        let (mut world, gi) = wire_world();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.move_origin = Vec3f::new(500.0, 0.0, 0.0);
        gladiator_gun(&mut world, key);
        let number = world.number_of(key).unwrap() as i32;
        let wire = gi.wire.borrow();
        let start = monster_flash_source(&world, key, MZ2_GLADIATOR_RAILGUN_1)
            .unwrap()
            .0;
        assert_eq!(
            wire[wire.len() - 4..],
            [
                Wire::Byte(SvcOp::MuzzleFlash2 as i32),
                Wire::Short(number),
                Wire::Byte(MZ2_GLADIATOR_RAILGUN_1),
                Wire::Multicast(start, Multicast::PVS),
            ]
        );
    }

    #[test]
    fn gladiator_gun_hits_the_saved_spot_and_misses_an_enemy_that_moved() {
        let (mut world, gi, key, player) =
            monster_arena(sp_monster_gladiator, sp_monster_gladiator_finish, 300.0);
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        gladiator_attack(&mut world, key);
        gladiator_gun(&mut world, key);
        assert_eq!(world.entities.get(player).unwrap().game.health, 50);

        gladiator_attack(&mut world, key);
        let (number, absmin, absmax) = gi.target.get().unwrap();
        let side = Vec3f::new(0.0, 200.0, 0.0);
        gi.target.set(Some((number, absmin + side, absmax + side)));
        world.entities.get_mut(player).unwrap().state.origin += side;
        gladiator_gun(&mut world, key);
        assert_eq!(world.entities.get(player).unwrap().game.health, 50);
    }

    #[test]
    fn gladiator_pain_debounces_three_seconds_with_one_of_two_sounds() {
        for (roll, name) in [(0.49_f32, SOUND_PAIN1), (0.5, SOUND_PAIN2)] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            world.level.time = 5.0;
            world.entities.get_mut(key).unwrap().game.max_health = 400;
            seed_random(&mut world, |r| if roll < 0.5 { r < 0.5 } else { r >= 0.5 });
            gladiator_pain(&mut world, key, key, 0.0, 10);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.pain_debounce_time, 8.0);
            assert_eq!(ent.state.skinnum, 0);
            assert_eq!(sounds(&gi), [(CHAN_VOICE, name.to_string(), ATTN_NORM)]);
            assert!(is_move(&world, key, &GLADIATOR_MOVE_PAIN));
        }
    }

    #[test]
    fn gladiator_pain_knocked_upward_plays_the_air_pain() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().velocity.z = 101.0;
        gladiator_pain(&mut world, key, key, 0.0, 10);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_PAIN_AIR));
    }

    #[test]
    fn gladiator_pain_while_debounced_only_lifts_a_ground_pain_into_the_air() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.level.time = 5.0;
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.max_health = 400;
        ent.game.health = 199;
        ent.game.pain_debounce_time = 6.0;
        ent.velocity.z = 101.0;
        gladiator_pain(&mut world, key, key, 0.0, 10);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_STAND));
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);

        set_move(&mut world, key, &GLADIATOR_MOVE_PAIN);
        gladiator_pain(&mut world, key, key, 0.0, 10);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_PAIN_AIR));
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            6.0
        );
        assert!(sounds(&gi).is_empty());

        set_move(&mut world, key, &GLADIATOR_MOVE_PAIN);
        world.entities.get_mut(key).unwrap().velocity.z = 100.0;
        gladiator_pain(&mut world, key, key, 0.0, 10);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_PAIN));
    }

    #[test]
    fn gladiator_pain_in_nightmare_plays_the_sound_without_an_animation() {
        let (mut world, gi) = test_world_recording();
        world.game.skill = 3;
        let key = spawned(&mut world);
        gladiator_pain(&mut world, key, key, 0.0, 50);
        assert_eq!(sounds(&gi).len(), 1);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_STAND));
    }

    #[test]
    fn gladiator_at_its_c_gib_health_throws_six_gibs() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -175;
        let before = world.entities.count();
        let die = world.entities.get(key).unwrap().die.unwrap();
        die(&mut world, key, key, key, 100, Vec3f::ZERO);
        assert_eq!(world.entities.count(), before + 6);
        assert_eq!(
            sounds(&gi),
            [(CHAN_VOICE, "misc/udeath.wav".to_string(), ATTN_NORM)]
        );
        assert_eq!(
            world.entities.get(key).unwrap().game.deadflag,
            DeadFlag::Dead
        );
    }

    #[test]
    fn gladiator_above_gib_health_falls_dead_only_once() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -174;
        let before = world.entities.count();
        gladiator_die(&mut world, key, key, key, 100, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert_eq!(world.entities.count(), before);
        assert!(is_move(&world, key, &GLADIATOR_MOVE_DEATH));
        assert_eq!(
            sounds(&gi),
            [(CHAN_VOICE, SOUND_DIE.to_string(), ATTN_NORM)]
        );
        gladiator_die(&mut world, key, key, key, 10, Vec3f::ZERO);
        assert_eq!(sounds(&gi).len(), 1);
    }

    #[test]
    fn gladiator_dead_shrinks_to_the_corpse_box_and_stops_thinking() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.nextthink = 9.0;
        gladiator_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.mins, Vec3f::new(-16.0, -16.0, -24.0));
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(ent.game.nextthink, 0.0);
    }

    #[test]
    fn gladiator_holding_ground_sights_a_player_and_shoots_it_with_the_railgun() {
        let (mut world, gi, key, player) =
            monster_arena(sp_monster_gladiator, sp_monster_gladiator_finish, 400.0);
        // Holding ground, C's missile chance is 0.4 a frame instead of 0.1
        // at near range, so the gladiator shoots instead of closing in.
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        let (mut angry, mut gunned) = (false, false);
        run(&mut world, key, 100, |w| {
            angry |= w.entities.get(key).unwrap().game.enemy == Some(player);
            gunned |= is_move(w, key, &GLADIATOR_MOVE_ATTACK_GUN);
        });
        assert!(angry);
        let played = sounds(&gi.floor.inner);
        assert_eq!(played[0], (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM));
        assert!(gunned);
        assert!(played.iter().any(|s| s.1 == SOUND_GUN));
        assert!(world.entities.get(player).unwrap().game.health <= 50);
    }
}
