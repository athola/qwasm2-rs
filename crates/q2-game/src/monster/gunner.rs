//! Gunner monster: a chaingun burst or a volley of grenades, and a
//! grenade from a crouch on hard skills.
//!
//! C ref: `~/Qwasm2/src/game/monster/gunner/` (gunner.c, gunner.h).

use q2_shared::constants::*;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::monster::{fire, flash, footstep, throw_organic_gibs, MONSTER_FOOTSTEPS};
use crate::monster::{frame, set_move, sound};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: gunner.h (`FRAME_*`, model frame numbers).
// ---------------------------------------------------------------------------

const FRAME_STAND01: i32 = 0;
const FRAME_STAND30: i32 = 29;
const FRAME_STAND31: i32 = 30;
const FRAME_STAND70: i32 = 69;
const FRAME_WALK07: i32 = 76;
const FRAME_WALK19: i32 = 88;
const FRAME_RUN01: i32 = 94;
const FRAME_RUN08: i32 = 101;
const FRAME_RUNS01: i32 = 102;
const FRAME_RUNS06: i32 = 107;
const FRAME_ATTAK101: i32 = 108;
const FRAME_ATTAK121: i32 = 128;
const FRAME_ATTAK209: i32 = 137;
const FRAME_ATTAK215: i32 = 143;
const FRAME_ATTAK216: i32 = 144;
const FRAME_ATTAK223: i32 = 151;
const FRAME_ATTAK224: i32 = 152;
const FRAME_ATTAK230: i32 = 158;
const FRAME_PAIN101: i32 = 159;
const FRAME_PAIN118: i32 = 176;
const FRAME_PAIN201: i32 = 177;
const FRAME_PAIN208: i32 = 184;
const FRAME_PAIN301: i32 = 185;
const FRAME_PAIN305: i32 = 189;
const FRAME_DEATH01: i32 = 190;
const FRAME_DEATH11: i32 = 200;
const FRAME_DUCK01: i32 = 201;
const FRAME_DUCK08: i32 = 208;
const FRAME_ATTAK105: i32 = 112;
const FRAME_ATTAK108: i32 = 115;
const FRAME_ATTAK111: i32 = 118;

/// C model scale, multiplied into every frame's distance.
const MODEL_SCALE: f32 = 1.15;

// C SP_monster_gunner values.
const HEALTH: i32 = 175;
const MINS: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const MAXS: Vec3f = Vec3f::new(16.0, 16.0, 32.0);
const MASS: i32 = 200;

const SOUND_DEATH: &str = "gunner/death1.wav";
const SOUND_PAIN: &str = "gunner/gunpain2.wav";
const SOUND_PAIN2: &str = "gunner/gunpain1.wav";
const SOUND_IDLE: &str = "gunner/gunidle1.wav";
const SOUND_OPEN: &str = "gunner/gunatck1.wav";
const SOUND_SEARCH: &str = "gunner/gunsrch1.wav";
const SOUND_SIGHT: &str = "gunner/sight1.wav";
pub(crate) const SOUND_STEPS: [&str; 2] = ["gunner/step1.wav", "gunner/step2.wav"];

// ---------------------------------------------------------------------------
// Moves. C ref: gunner.c `gunner_frames_*` / `gunner_move_*`, one entry
// per model frame in C order.
// ---------------------------------------------------------------------------

/// C `gunner_frames_fidget` lists 49 frames; the move plays the first 40.
static GUNNER_FRAMES_FIDGET: [MonsterFrame; 40] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(gunner_idlesound)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
];
static GUNNER_MOVE_FIDGET: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND31,
    lastframe: FRAME_STAND70,
    frames: &GUNNER_FRAMES_FIDGET,
    endfunc: Some(gunner_stand),
};

static GUNNER_FRAMES_STAND: [MonsterFrame; 30] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(gunner_fidget)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(gunner_fidget)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(gunner_fidget)),
];
static GUNNER_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND30,
    frames: &GUNNER_FRAMES_STAND,
    endfunc: None,
};

static GUNNER_FRAMES_WALK: [MonsterFrame; 13] = [
    frame(ai::ai_walk, 0.0, Some(gunner_footstep)),
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 2.0, Some(gunner_footstep)),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 4.0, Some(gunner_footstep)),
];
static GUNNER_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK07,
    lastframe: FRAME_WALK19,
    frames: &GUNNER_FRAMES_WALK,
    endfunc: None,
};

static GUNNER_FRAMES_RUN: [MonsterFrame; 8] = [
    frame(ai::ai_run, 26.0, None),
    frame(ai::ai_run, 9.0, Some(gunner_footstep)),
    frame(ai::ai_run, 9.0, None),
    frame(ai::ai_run, 9.0, None),
    frame(ai::ai_run, 15.0, None),
    frame(ai::ai_run, 10.0, Some(gunner_footstep)),
    frame(ai::ai_run, 13.0, None),
    frame(ai::ai_run, 6.0, None),
];
static GUNNER_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN01,
    lastframe: FRAME_RUN08,
    frames: &GUNNER_FRAMES_RUN,
    endfunc: None,
};

static GUNNER_FRAMES_RUNANDSHOOT: [MonsterFrame; 6] = [
    frame(ai::ai_run, 32.0, None),
    frame(ai::ai_run, 15.0, Some(gunner_footstep)),
    frame(ai::ai_run, 10.0, None),
    frame(ai::ai_run, 18.0, None),
    frame(ai::ai_run, 8.0, Some(gunner_footstep)),
    frame(ai::ai_run, 20.0, None),
];
static GUNNER_MOVE_RUNANDSHOOT: MonsterMove = MonsterMove {
    firstframe: FRAME_RUNS01,
    lastframe: FRAME_RUNS06,
    frames: &GUNNER_FRAMES_RUNANDSHOOT,
    endfunc: None,
};

static GUNNER_FRAMES_PAIN3: [MonsterFrame; 5] = [
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 1.0, None),
];
static GUNNER_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN305,
    frames: &GUNNER_FRAMES_PAIN3,
    endfunc: Some(gunner_run),
};

static GUNNER_FRAMES_PAIN2: [MonsterFrame; 8] = [
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 11.0, None),
    frame(ai::ai_move, 6.0, Some(gunner_footstep)),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -7.0, Some(gunner_footstep)),
];
static GUNNER_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN208,
    frames: &GUNNER_FRAMES_PAIN2,
    endfunc: Some(gunner_run),
};

static GUNNER_FRAMES_PAIN1: [MonsterFrame; 18] = [
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -5.0, Some(gunner_footstep)),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, Some(gunner_footstep)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 0.0, Some(gunner_footstep)),
    frame(ai::ai_move, 0.0, None),
];
static GUNNER_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN118,
    frames: &GUNNER_FRAMES_PAIN1,
    endfunc: Some(gunner_run),
};

static GUNNER_FRAMES_DEATH: [MonsterFrame; 11] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, 8.0, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];
static GUNNER_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH01,
    lastframe: FRAME_DEATH11,
    frames: &GUNNER_FRAMES_DEATH,
    endfunc: Some(gunner_dead),
};

static GUNNER_FRAMES_DUCK: [MonsterFrame; 8] = [
    frame(ai::ai_move, 1.0, Some(gunner_duck_down)),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, Some(gunner_duck_hold)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 0.0, Some(gunner_duck_up)),
    frame(ai::ai_move, -1.0, None),
];
static GUNNER_MOVE_DUCK: MonsterMove = MonsterMove {
    firstframe: FRAME_DUCK01,
    lastframe: FRAME_DUCK08,
    frames: &GUNNER_FRAMES_DUCK,
    endfunc: Some(gunner_run),
};

static GUNNER_FRAMES_ATTACK_CHAIN: [MonsterFrame; 7] = [
    frame(ai::ai_charge, 0.0, Some(gunner_opengun)),
    frame(ai::ai_charge, 0.0, Some(gunner_footstep)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];
static GUNNER_MOVE_ATTACK_CHAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK209,
    lastframe: FRAME_ATTAK215,
    frames: &GUNNER_FRAMES_ATTACK_CHAIN,
    endfunc: Some(gunner_fire_chain),
};

static GUNNER_FRAMES_FIRE_CHAIN: [MonsterFrame; 8] = [
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
    frame(ai::ai_charge, 0.0, Some(gunner_fire)),
];
static GUNNER_MOVE_FIRE_CHAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK216,
    lastframe: FRAME_ATTAK223,
    frames: &GUNNER_FRAMES_FIRE_CHAIN,
    endfunc: Some(gunner_refire_chain),
};

static GUNNER_FRAMES_ENDFIRE_CHAIN: [MonsterFrame; 7] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gunner_footstep)),
];
static GUNNER_MOVE_ENDFIRE_CHAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK224,
    lastframe: FRAME_ATTAK230,
    frames: &GUNNER_FRAMES_ENDFIRE_CHAIN,
    endfunc: Some(gunner_run),
};

static GUNNER_FRAMES_ATTACK_GRENADE: [MonsterFrame; 21] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gunner_grenade)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gunner_grenade)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gunner_grenade)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(gunner_grenade)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];
static GUNNER_MOVE_ATTACK_GRENADE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK121,
    frames: &GUNNER_FRAMES_ATTACK_GRENADE,
    endfunc: Some(gunner_run),
};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn monsterinfo(world: &GameWorld, key: EntityKey) -> Option<&MonsterInfo> {
    world.entities.get(key)?.monsterinfo.as_deref()
}

fn monsterinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MonsterInfo> {
    world.entities.get_mut(key)?.monsterinfo.as_deref_mut()
}

// ---------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------

/// C ref: `gunner_footstep`; silent unless `g_monsterfootsteps` is set.
pub fn gunner_footstep(world: &mut GameWorld, key: EntityKey) {
    footstep(world, key, MONSTER_FOOTSTEPS, &SOUND_STEPS, 1.0);
}

/// C ref: `gunner_idlesound`.
pub fn gunner_idlesound(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
}

/// C ref: `gunner_sight`.
pub fn gunner_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `gunner_search`.
pub fn gunner_search(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SEARCH, ATTN_NORM);
}

// ---------------------------------------------------------------------------
// Stand, walk, run
// ---------------------------------------------------------------------------

/// Fidget one stand-check in twenty, unless busy with an enemy or holding
/// ground. C ref: `gunner_fidget`.
pub fn gunner_fidget(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.enemy.is_some() {
        return;
    }
    if ent
        .monsterinfo
        .as_deref()
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND))
    {
        return;
    }
    if world.random() <= 0.05 {
        set_move(world, key, &GUNNER_MOVE_FIDGET);
    }
}

/// C ref: `gunner_stand`.
pub fn gunner_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &GUNNER_MOVE_STAND);
}

/// C ref: `gunner_walk`.
pub fn gunner_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &GUNNER_MOVE_WALK);
}

/// C ref: `gunner_run`.
pub fn gunner_run(world: &mut GameWorld, key: EntityKey) {
    let stand_ground =
        monsterinfo(world, key).is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    if stand_ground {
        set_move(world, key, &GUNNER_MOVE_STAND);
    } else {
        set_move(world, key, &GUNNER_MOVE_RUN);
    }
}

/// C ref: `gunner_runandshoot`, which nothing in C calls.
pub fn gunner_runandshoot(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &GUNNER_MOVE_RUNANDSHOOT);
}

// ---------------------------------------------------------------------------
// Pain and death
// ---------------------------------------------------------------------------

/// Bloody skin below half health; then, at most every 3 s, a cry and a
/// flinch that grows with the damage (no flinch on nightmare).
/// C ref: `gunner_pain`.
pub fn gunner_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    damage: i32,
) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.health < ent.game.max_health / 2 {
        ent.state.skinnum = 1;
    }
    if time < ent.game.pain_debounce_time {
        return;
    }
    ent.game.pain_debounce_time = time + 3.0;

    let cry = if world.random_u32() & 1 != 0 {
        SOUND_PAIN
    } else {
        SOUND_PAIN2
    };
    sound(world, key, CHAN_VOICE, cry, ATTN_NORM);

    if world.game.skill == crate::constants::SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    let mv = if damage <= 10 {
        &GUNNER_MOVE_PAIN3
    } else if damage <= 25 {
        &GUNNER_MOVE_PAIN2
    } else {
        &GUNNER_MOVE_PAIN1
    };
    set_move(world, key, mv);
}

/// The corpse box, then stop. C ref: `gunner_dead`.
pub fn gunner_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// Bloody skin, then gib when hit hard enough, else fall. The corpse
/// stays damageable so it can still be gibbed. C ref: `gunner_die`.
pub fn gunner_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.skinnum = 1;
    let (health, deadflag) = (ent.game.health, ent.game.deadflag);
    if health <= ent.game.gib_health {
        sound(world, key, CHAN_VOICE, "misc/udeath.wav", ATTN_NORM);
        throw_organic_gibs(world, key, damage);
        return;
    }
    if deadflag == DeadFlag::Dead {
        return;
    }

    sound(world, key, CHAN_VOICE, SOUND_DEATH, ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    set_move(world, key, &GUNNER_MOVE_DEATH);
}

// ---------------------------------------------------------------------------
// Dodging
// ---------------------------------------------------------------------------

/// Crouch: 32 units shorter and hittable but no longer aimed at, for a
/// second; on hard skills lob a grenade half the time.
/// C ref: `gunner_duck_down`.
pub fn gunner_duck_down(world: &mut GameWorld, key: EntityKey) {
    let Some(mi) = monsterinfo_mut(world, key) else {
        return;
    };
    if mi.aiflags.contains(AiFlags::DUCKED) {
        return;
    }
    mi.aiflags.insert(AiFlags::DUCKED);

    if world.game.skill >= crate::constants::SKILL_HARD && world.random() > 0.5 {
        gunner_grenade(world, key);
    }

    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.maxs.z -= 32.0;
    ent.game.takedamage = TakeDamage::Yes;
    if let Some(mi) = ent.monsterinfo.as_deref_mut() {
        mi.pausetime = time + 1.0;
    }
    world.link_entity(key);
}

/// C ref: `gunner_duck_hold`.
pub fn gunner_duck_hold(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    if let Some(mi) = monsterinfo_mut(world, key) {
        if time >= mi.pausetime {
            mi.aiflags.remove(AiFlags::HOLD_FRAME);
        } else {
            mi.aiflags.insert(AiFlags::HOLD_FRAME);
        }
    }
}

/// C ref: `gunner_duck_up`.
pub fn gunner_duck_up(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if let Some(mi) = ent.monsterinfo.as_deref_mut() {
        mi.aiflags.remove(AiFlags::DUCKED);
    }
    ent.maxs.z += 32.0;
    ent.game.takedamage = TakeDamage::Aim;
    world.link_entity(key);
}

/// Duck one shot in four, turning on an attacker it had not noticed.
/// C ref: `gunner_dodge`.
pub fn gunner_dodge(world: &mut GameWorld, key: EntityKey, attacker: EntityKey, _eta: f32) {
    if world.random() > 0.25 {
        return;
    }
    if world
        .entities
        .get(key)
        .is_some_and(|e| e.game.enemy.is_none())
    {
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.enemy = Some(attacker);
        }
        ai::found_target(world, key);
    }
    set_move(world, key, &GUNNER_MOVE_DUCK);
}

// ---------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------

/// C ref: `gunner_opengun`.
pub fn gunner_opengun(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_OPEN, ATTN_IDLE);
}

/// One chaingun bullet at where the enemy was 0.2 s ago, from the
/// frame's barrel. C ref: `GunnerFire`. C dereferences `self->enemy`;
/// without one there is nothing to aim at, so nothing fires.
pub fn gunner_fire(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let flash_number = MZ2_GUNNER_MACHINEGUN_1 + (ent.state.frame - FRAME_ATTAK216);
    let Some(target) = ent
        .game
        .enemy
        .and_then(|e| world.entities.get(e))
        .map(|e| e.state.origin + e.velocity * -0.2 + Vec3f::new(0.0, 0.0, e.game.viewheight))
    else {
        return;
    };
    let Some((start, _, _)) = flash::monster_flash_source(world, key, flash_number) else {
        return;
    };
    let aim = (target - start).normalize_or_zero();
    fire::monster_fire_bullet(
        world,
        key,
        start,
        aim,
        3,
        4,
        DEFAULT_BULLET_HSPREAD,
        DEFAULT_BULLET_VSPREAD,
        flash_number,
    );
}

/// A 50 damage grenade straight ahead from the frame's launcher.
/// C ref: `GunnerGrenade`.
pub fn gunner_grenade(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let flash_number = match ent.state.frame {
        FRAME_ATTAK105 => MZ2_GUNNER_GRENADE_1,
        FRAME_ATTAK108 => MZ2_GUNNER_GRENADE_2,
        FRAME_ATTAK111 => MZ2_GUNNER_GRENADE_3,
        _ => MZ2_GUNNER_GRENADE_4,
    };
    let Some((start, forward, _)) = flash::monster_flash_source(world, key, flash_number) else {
        return;
    };
    fire::monster_fire_grenade(world, key, start, forward, 50, 600, flash_number);
}

/// The chaingun in melee range; further off, it or the grenades evenly.
/// C ref: `gunner_attack`. C passes a NULL `self->enemy` to `range`;
/// without one the port takes it as out of melee range and rolls.
pub fn gunner_attack(world: &mut GameWorld, key: EntityKey) {
    let melee = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .is_some_and(|enemy| ai::range(world, key, enemy) == Range::Melee);
    if melee || world.random() > 0.5 {
        set_move(world, key, &GUNNER_MOVE_ATTACK_CHAIN);
    } else {
        set_move(world, key, &GUNNER_MOVE_ATTACK_GRENADE);
    }
}

/// C ref: `gunner_fire_chain`.
pub fn gunner_fire_chain(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &GUNNER_MOVE_FIRE_CHAIN);
}

/// Keep firing half the time while the enemy lives and is in sight.
/// C ref: `gunner_refire_chain`. C reads `self->enemy->health` without
/// a NULL check; without an enemy the port ends the burst.
pub fn gunner_refire_chain(world: &mut GameWorld, key: EntityKey) {
    let enemy = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .filter(|&e| world.entities.get(e).is_some_and(|e| e.game.health > 0));
    if let Some(enemy) = enemy {
        if ai::visible(world, key, enemy) && world.random() <= 0.5 {
            set_move(world, key, &GUNNER_MOVE_FIRE_CHAIN);
            return;
        }
    }
    set_move(world, key, &GUNNER_MOVE_ENDFIRE_CHAIN);
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_gunner`.
pub fn sp_monster_gunner(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "monster_gunner".to_string());
        ent.game.movetype = MoveType::Step;
        ent.solid = Solid::Bbox;
        ent.mins = MINS;
        ent.maxs = MAXS;
        ent.game.health = HEALTH;
        ent.game.gib_health = -70;
        ent.game.max_health = HEALTH;
        ent.game.mass = MASS;

        ent.pain = Some(gunner_pain);
        ent.die = Some(gunner_die);
        ent.monsterinfo = Some(Box::new(MonsterInfo {
            stand: Some(gunner_stand),
            walk: Some(gunner_walk),
            run: Some(gunner_run),
            dodge: Some(gunner_dodge),
            attack: Some(gunner_attack),
            melee: None,
            sight: Some(gunner_sight),
            search: Some(gunner_search),
            currentmove: Some(GUNNER_MOVE_STAND.clone()),
            scale: MODEL_SCALE,
            ..Default::default()
        }));
        ai::walkmonster_start(ent);
    }
}

/// The `GameWorld` half of C `SP_monster_gunner`: its sounds.
pub fn finish_gunner(world: &mut GameWorld, _key: EntityKey, _fields: &HashMap<String, String>) {
    for name in [
        SOUND_DEATH,
        SOUND_PAIN,
        SOUND_PAIN2,
        SOUND_IDLE,
        SOUND_OPEN,
        SOUND_SEARCH,
        SOUND_SIGHT,
        "gunner/gunatck2.wav",
        "gunner/gunatck3.wav",
    ] {
        world.gi.sound_index(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NAMES: &[(ThinkFn, &str)] = &[
        (gunner_idlesound, "gunner_idlesound"),
        (gunner_stand, "gunner_stand"),
        (gunner_fidget, "gunner_fidget"),
        (gunner_footstep, "gunner_footstep"),
        (gunner_run, "gunner_run"),
        (gunner_dead, "gunner_dead"),
        (gunner_duck_down, "gunner_duck_down"),
        (gunner_duck_hold, "gunner_duck_hold"),
        (gunner_duck_up, "gunner_duck_up"),
        (gunner_opengun, "gunner_opengun"),
        (gunner_fire_chain, "gunner_fire_chain"),
        (gunner_fire, "gunner_fire"),
        (gunner_refire_chain, "gunner_refire_chain"),
        (gunner_grenade, "gunner_grenade"),
    ];

    use crate::entity::ThinkFn;
    use crate::misc::wire::{wire_world, Wire};
    use crate::monster::test_support::{
        assert_moves_match_c, keep_kick, mi, mi_mut, player_arena, playing, seed, sounds,
        think_for, ArenaImport, CMoveRow,
    };
    use crate::world::{test_world, test_world_recording, MockGameImport};
    use std::rc::Rc;

    #[test]
    fn every_gunner_move_matches_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&GUNNER_MOVE_FIDGET, 30, 69, "gunner_stand", "stand:0:-*7 stand:0:gunner_idlesound stand:0:-*32"),
            (&GUNNER_MOVE_STAND, 0, 29, "-", "stand:0:-*9 stand:0:gunner_fidget stand:0:-*9 stand:0:gunner_fidget stand:0:-*9 stand:0:gunner_fidget"),
            (&GUNNER_MOVE_WALK, 76, 88, "-", "walk:0:gunner_footstep walk:3:- walk:4:- walk:5:- walk:7:- walk:2:gunner_footstep walk:6:- walk:4:- walk:2:- walk:7:- walk:5:- walk:7:- walk:4:gunner_footstep"),
            (&GUNNER_MOVE_RUN, 94, 101, "-", "run:26:- run:9:gunner_footstep run:9:-*2 run:15:- run:10:gunner_footstep run:13:- run:6:-"),
            (&GUNNER_MOVE_RUNANDSHOOT, 102, 107, "-", "run:32:- run:15:gunner_footstep run:10:- run:18:- run:8:gunner_footstep run:20:-"),
            (&GUNNER_MOVE_PAIN3, 185, 189, "gunner_run", "move:-3:- move:1:-*2 move:0:- move:1:-"),
            (&GUNNER_MOVE_PAIN2, 177, 184, "gunner_run", "move:-2:- move:11:- move:6:gunner_footstep move:2:- move:-1:- move:-7:- move:-2:- move:-7:gunner_footstep"),
            (&GUNNER_MOVE_PAIN1, 159, 176, "gunner_run", "move:2:- move:0:- move:-5:gunner_footstep move:3:- move:-1:- move:0:-*4 move:1:-*2 move:2:- move:1:gunner_footstep move:0:- move:-2:-*2 move:0:gunner_footstep move:0:-"),
            (&GUNNER_MOVE_DEATH, 190, 200, "gunner_dead", "move:0:-*3 move:-7:- move:-3:- move:-5:- move:8:- move:6:- move:0:-*3"),
            (&GUNNER_MOVE_DUCK, 201, 208, "gunner_run", "move:1:gunner_duck_down move:1:- move:1:gunner_duck_hold move:0:- move:-1:-*2 move:0:gunner_duck_up move:-1:-"),
            (&GUNNER_MOVE_ATTACK_CHAIN, 137, 143, "gunner_fire_chain", "charge:0:gunner_opengun charge:0:gunner_footstep charge:0:-*5"),
            (&GUNNER_MOVE_FIRE_CHAIN, 144, 151, "gunner_refire_chain", "charge:0:gunner_fire*8"),
            (&GUNNER_MOVE_ENDFIRE_CHAIN, 152, 158, "gunner_run", "charge:0:-*6 charge:0:gunner_footstep"),
            (&GUNNER_MOVE_ATTACK_GRENADE, 108, 128, "gunner_run", "charge:0:-*4 charge:0:gunner_grenade charge:0:-*2 charge:0:gunner_grenade charge:0:-*2 charge:0:gunner_grenade charge:0:-*2 charge:0:gunner_grenade charge:0:-*7"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    /// A gunner at the origin facing +X, its live enemy at (x, 0, 0), at
    /// level time 5.
    fn duel(x: f32) -> (GameWorld, Rc<MockGameImport>, EntityKey, EntityKey) {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        sp_monster_gunner(&mut world.entities, key, &HashMap::new());
        let enemy = world.spawn().unwrap();
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = Vec3f::new(x, 0.0, 0.0);
        e.game.health = 100;
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        world.level.time = 5.0;
        (world, gi, key, enemy)
    }

    fn voice(name: &str, attn: f32) -> (i32, String, f32) {
        (CHAN_VOICE, name.to_string(), attn)
    }

    fn health(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().game.health
    }

    #[test]
    fn gunner_voice_lines_play_on_the_voice_channel() {
        let (mut world, gi, key, enemy) = duel(300.0);
        gunner_idlesound(&mut world, key);
        gunner_sight(&mut world, key, enemy);
        gunner_search(&mut world, key);
        gunner_opengun(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [
                voice(SOUND_IDLE, ATTN_IDLE),
                voice(SOUND_SIGHT, ATTN_NORM),
                voice(SOUND_SEARCH, ATTN_NORM),
                voice(SOUND_OPEN, ATTN_IDLE),
            ]
        );
    }

    #[test]
    fn gunner_footstep_is_silent_with_the_default_footsteps_cvar() {
        let (mut world, gi, key, _) = duel(300.0);
        let rand = world.level.rand_state;
        gunner_footstep(&mut world, key);
        assert!(sounds(&gi).is_empty());
        assert_eq!(world.level.rand_state, rand);
    }

    #[test]
    fn gunner_fidget_fidgets_one_time_in_twenty_when_idle() {
        let (mut world, _, key, _) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.enemy = None;
        seed(&mut world, |w| w.random() > 0.05);
        gunner_fidget(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
        seed(&mut world, |w| w.random() <= 0.05);
        gunner_fidget(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_FIDGET));
    }

    #[test]
    fn gunner_fidget_never_with_an_enemy_or_holding_ground() {
        let (mut world, _, key, _) = duel(300.0);
        let rand = world.level.rand_state;
        gunner_fidget(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
        world.entities.get_mut(key).unwrap().game.enemy = None;
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        gunner_fidget(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
        assert_eq!(world.level.rand_state, rand);
    }

    #[test]
    fn gunner_stand_walk_run_and_runandshoot_set_their_moves() {
        let (mut world, _, key, _) = duel(300.0);
        gunner_walk(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_WALK));
        gunner_run(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_RUN));
        gunner_runandshoot(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_RUNANDSHOOT));
        gunner_stand(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        gunner_run(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
    }

    #[test]
    fn gunner_pain_turns_bloody_below_half_health() {
        let (mut world, _, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.health = 87;
        gunner_pain(&mut world, key, enemy, 0.0, 5);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 0);
        world.entities.get_mut(key).unwrap().game.health = 86;
        gunner_pain(&mut world, key, enemy, 0.0, 5);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
    }

    #[test]
    fn gunner_pain_cries_one_of_two_ways_with_a_three_second_debounce() {
        for (odd, cry) in [(1, SOUND_PAIN), (0, SOUND_PAIN2)] {
            let (mut world, gi, key, enemy) = duel(300.0);
            seed(&mut world, |w| w.random_u32() & 1 == odd);
            gunner_pain(&mut world, key, enemy, 0.0, 5);
            assert_eq!(sounds(&gi), [voice(cry, ATTN_NORM)]);
            assert_eq!(world.entities.get(key).unwrap().state.skinnum, 0);
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                8.0
            );
            gunner_stand(&mut world, key);
            gunner_pain(&mut world, key, enemy, 0.0, 5);
            assert_eq!(sounds(&gi).len(), 1);
            assert!(playing(&world, key, &GUNNER_MOVE_STAND));
        }
    }

    #[test]
    fn gunner_pain_flinch_grows_with_the_damage_but_not_on_nightmare() {
        for (damage, mv) in [
            (10, &GUNNER_MOVE_PAIN3),
            (11, &GUNNER_MOVE_PAIN2),
            (25, &GUNNER_MOVE_PAIN2),
            (26, &GUNNER_MOVE_PAIN1),
        ] {
            let (mut world, _, key, enemy) = duel(300.0);
            gunner_pain(&mut world, key, enemy, 0.0, damage);
            assert!(playing(&world, key, mv), "{damage}");
        }
        let (mut world, gi, key, enemy) = duel(300.0);
        world.game.skill = crate::constants::SKILL_HARDPLUS;
        gunner_pain(&mut world, key, enemy, 0.0, 50);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
        assert_eq!(sounds(&gi).len(), 1, "still cries out");
    }

    #[test]
    fn gunner_dead_shrinks_to_a_corpse_and_stops_thinking() {
        let (mut world, _, key, _) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.nextthink = 6.0;
        gunner_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.mins, Vec3f::new(-16.0, -16.0, -24.0));
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_ne!(ent.svflags & SvFlags::DEADMONSTER.bits(), 0);
        assert_eq!(ent.game.nextthink, 0.0);
    }

    #[test]
    fn gunner_die_gibs_at_minus_70_health() {
        let (mut world, gi, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.health = -70;
        let before = world.entities.count();
        gunner_die(&mut world, key, enemy, enemy, 80, Vec3f::ZERO);
        assert_eq!(sounds(&gi), [voice("misc/udeath.wav", ATTN_NORM)]);
        assert_eq!(world.entities.count(), before + 6, "2 bones + 4 meat");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.maxs, Vec3f::ZERO, "now the head");
    }

    #[test]
    fn gunner_die_falls_bloody_once() {
        let (mut world, gi, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.health = -69;
        gunner_die(&mut world, key, enemy, enemy, 5, Vec3f::ZERO);
        assert!(playing(&world, key, &GUNNER_MOVE_DEATH));
        assert_eq!(sounds(&gi), [voice(SOUND_DEATH, ATTN_NORM)]);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.skinnum, 1);
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);

        gunner_stand(&mut world, key);
        gunner_die(&mut world, key, enemy, enemy, 5, Vec3f::ZERO);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND), "already dead");
        assert_eq!(sounds(&gi).len(), 1);
    }

    fn grenade(world: &GameWorld) -> Option<crate::entity::Entity> {
        let key = world.find_by_classname(None, "grenade")?;
        world.entities.get(key).cloned()
    }

    #[test]
    fn gunner_duck_down_crouches_once_and_on_hard_lobs_a_grenade_half_the_time() {
        let (mut world, _, key, _) = duel(300.0);
        world.game.skill = 1;
        gunner_duck_down(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 0.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert!(mi(&world, key).aiflags.contains(AiFlags::DUCKED));
        assert_eq!(mi(&world, key).pausetime, 6.0);
        assert!(grenade(&world).is_none(), "medium");
        gunner_duck_down(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().maxs.z, 0.0);

        for (skill, lo, hi, lobs) in [(2, 0.5001, 1.01, true), (3, 0.0, 0.5, false)] {
            let (mut world, _, key, _) = duel(300.0);
            world.game.skill = skill;
            seed(&mut world, |w| (lo..hi).contains(&w.random()));
            gunner_duck_down(&mut world, key);
            assert_eq!(grenade(&world).is_some(), lobs, "skill {skill}");
        }
    }

    #[test]
    fn gunner_duck_holds_then_stands_back_up() {
        let (mut world, _, key, _) = duel(300.0);
        world.game.skill = 1;
        gunner_duck_down(&mut world, key);
        gunner_duck_hold(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        world.level.time = 6.0;
        gunner_duck_hold(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        gunner_duck_up(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 32.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Aim);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::DUCKED));
    }

    #[test]
    fn gunner_dodge_ducks_one_shot_in_four_and_turns_on_the_attacker() {
        let (mut world, _, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.enemy = None;
        seed(&mut world, |w| w.random() > 0.25);
        gunner_dodge(&mut world, key, enemy, 0.5);
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
        seed(&mut world, |w| w.random() <= 0.25);
        gunner_dodge(&mut world, key, enemy, 0.5);
        assert!(playing(&world, key, &GUNNER_MOVE_DUCK));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(enemy));
    }

    /// The flash number of the last muzzle flash on the wire.
    fn last_flash(wire: &[Wire]) -> &Wire {
        &wire[wire.len() - 2]
    }

    #[test]
    fn gunner_fire_shoots_the_enemy_from_the_frames_barrel() {
        let (mut world, gi, player) = player_arena(200.0);
        let key = world.spawn().unwrap();
        sp_monster_gunner(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::new(0.0, 0.0, 24.0);
        ent.state.frame = FRAME_ATTAK216 + 3;
        ent.game.enemy = Some(player);
        world.entities.get_mut(player).unwrap().pain = Some(keep_kick);
        gunner_fire(&mut world, key);
        assert_eq!(health(&world, player), 97);
        assert_eq!(world.entities.get(player).unwrap().game.count, 4, "kick");
        let _ = gi;

        let (mut world, gi) = wire_world();
        let key = world.spawn().unwrap();
        let enemy = world.spawn().unwrap();
        sp_monster_gunner(&mut world.entities, key, &HashMap::new());
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK216 + 7;
        gunner_fire(&mut world, key);
        assert_eq!(
            *last_flash(&gi.wire.borrow()),
            Wire::Byte(MZ2_GUNNER_MACHINEGUN_1 + 7)
        );
    }

    #[test]
    fn gunner_fire_leads_a_moving_enemy_by_0_2_seconds_back() {
        let (mut world, gi, player) = player_arena(200.0);
        let key = world.spawn().unwrap();
        sp_monster_gunner(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::new(0.0, 0.0, 24.0);
        ent.state.frame = FRAME_ATTAK216;
        ent.game.enemy = Some(player);
        world.entities.get_mut(player).unwrap().velocity = Vec3f::new(0.0, 500.0, 0.0);
        gi.target_at(
            Vec3f::new(200.0, -100.0, 46.0),
            16.0,
            world.number_of(player).unwrap(),
        );
        gunner_fire(&mut world, key);
        assert_eq!(health(&world, player), 97);
    }

    #[test]
    fn gunner_fire_without_an_enemy_fires_nothing() {
        let (mut world, gi) = wire_world();
        let key = world.spawn().unwrap();
        sp_monster_gunner(&mut world.entities, key, &HashMap::new());
        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK216;
        gunner_fire(&mut world, key);
        assert!(gi.wire.borrow().is_empty());
    }

    #[test]
    fn gunner_grenade_launches_a_50_damage_grenade_from_the_frames_launcher() {
        for (frame, flash) in [
            (FRAME_ATTAK105, MZ2_GUNNER_GRENADE_1),
            (FRAME_ATTAK108, MZ2_GUNNER_GRENADE_2),
            (FRAME_ATTAK111, MZ2_GUNNER_GRENADE_3),
            (FRAME_DUCK01, MZ2_GUNNER_GRENADE_4),
        ] {
            let (mut world, gi) = wire_world();
            let key = world.spawn().unwrap();
            sp_monster_gunner(&mut world.entities, key, &HashMap::new());
            world.entities.get_mut(key).unwrap().state.frame = frame;
            gunner_grenade(&mut world, key);
            let g = grenade(&world).expect("a grenade");
            assert_eq!((g.game.dmg, g.game.dmg_radius), (50, 90.0));
            assert_eq!(g.owner, Some(key));
            // 600 straight ahead; C's up and right jitter is level-aim
            // perpendicular, so it leaves x alone.
            assert!((g.velocity.x - 600.0).abs() < 0.01, "ahead: {}", g.velocity);
            assert!(g.velocity.y.abs() <= 10.0, "{}", g.velocity);
            assert!((190.0..=210.0).contains(&g.velocity.z), "{}", g.velocity);
            assert_eq!(*last_flash(&gi.wire.borrow()), Wire::Byte(flash), "{frame}");
        }
    }

    #[test]
    fn gunner_attack_chainguns_in_melee_range_else_picks_evenly() {
        let (mut world, _, key, _) = duel(50.0);
        let rand = world.level.rand_state;
        gunner_attack(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_ATTACK_CHAIN));
        assert_eq!(world.level.rand_state, rand);

        let (mut world, _, key, _) = duel(300.0);
        seed(&mut world, |w| w.random() <= 0.5);
        gunner_attack(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_ATTACK_GRENADE));
        seed(&mut world, |w| w.random() > 0.5);
        gunner_attack(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_ATTACK_CHAIN));
    }

    #[test]
    fn gunner_fire_chain_starts_the_burst() {
        let (mut world, _, key, _) = duel(300.0);
        gunner_fire_chain(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_FIRE_CHAIN));
    }

    #[test]
    fn gunner_refire_chain_keeps_firing_half_the_time_at_a_visible_live_enemy() {
        let (mut world, _, key, _) = duel(300.0);
        seed(&mut world, |w| w.random() <= 0.5);
        gunner_refire_chain(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_FIRE_CHAIN));

        let (mut world, _, key, _) = duel(300.0);
        seed(&mut world, |w| w.random() > 0.5);
        gunner_refire_chain(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_ENDFIRE_CHAIN));

        let (mut world, _, key, enemy) = duel(300.0);
        world.entities.get_mut(enemy).unwrap().game.health = 0;
        seed(&mut world, |w| w.random() <= 0.5);
        gunner_refire_chain(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_ENDFIRE_CHAIN), "dead");

        let (mut world, gi, key, _) = duel(300.0);
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        seed(&mut world, |w| w.random() <= 0.5);
        gunner_refire_chain(&mut world, key);
        assert!(playing(&world, key, &GUNNER_MOVE_ENDFIRE_CHAIN), "hidden");
    }

    #[test]
    fn gunner_spawns_with_the_c_callbacks_and_starts_as_a_walker() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_gunner(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.health, ent.game.mass), (175, 200));
        assert_eq!((ent.mins, ent.maxs), (MINS, MAXS));
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert!(ent.pain.is_some() && ent.die.is_some());
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::walkmonster_start_go as ThinkFn)));
        let mi = ent.monsterinfo.as_deref().unwrap();
        assert_eq!(mi.scale, 1.15);
        assert!(mi.stand.is_some() && mi.walk.is_some() && mi.run.is_some());
        assert!(mi.dodge.is_some() && mi.attack.is_some() && mi.sight.is_some());
        assert!(mi
            .search
            .is_some_and(|f| std::ptr::fn_addr_eq(f, gunner_search as ThinkFn)));
        assert!(mi.melee.is_none() && mi.idle.is_none());
        assert!(playing(&world, key, &GUNNER_MOVE_STAND));
    }

    #[test]
    fn gunner_finish_precaches_its_sounds_and_runs_from_its_row() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        finish_gunner(&mut world, key, &HashMap::new());
        assert_eq!(gi.sounds.borrow().len(), 9);
        let row = crate::spawn::find_spawn("monster_gunner").unwrap();
        assert!(row
            .3
            .is_some_and(|f| std::ptr::fn_addr_eq(f, finish_gunner as crate::spawn::FinishFn)));
    }

    /// A gunner spawned through its row at the origin facing +X, the
    /// player `x` ahead.
    fn gunner_facing_player(x: f32) -> (GameWorld, Rc<ArenaImport>, EntityKey, EntityKey) {
        let (mut world, gi, player) = player_arena(x);
        world.game.skill = 1;
        let key = world.spawn().unwrap();
        let fields: HashMap<String, String> =
            [("classname".to_string(), "monster_gunner".to_string())].into();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        let &(_, spawn, _, finish) = crate::spawn::find_spawn("monster_gunner").unwrap();
        spawn(&mut world.entities, key, &fields);
        finish.unwrap()(&mut world, key, &fields);
        ai::monster_start(&mut world, key);
        world.link_entity(key);
        (world, gi, key, player)
    }

    #[test]
    fn gunner_sights_the_player_and_attacks_with_grenades_or_chaingun() {
        let (mut world, gi, key, player) = gunner_facing_player(300.0);

        think_for(&mut world, key, 150);

        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(sounds(&gi.floor.inner)
            .iter()
            .any(|(_, s, _)| s == SOUND_SIGHT));
        assert!(
            grenade(&world).is_some() || health(&world, player) < 100,
            "the gunner attacked"
        );
    }
}
