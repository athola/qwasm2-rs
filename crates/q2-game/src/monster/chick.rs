//! Iron Maiden (the "chick"): rockets at range, a slash in melee, and a
//! duck to dodge. C ref: `~/Qwasm2/src/game/monster/chick/chick.c`.

use q2_shared::constants::MZ2_CHICK_ROCKET_1;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::fire::monster_fire_rocket;
use crate::monster::flash::monster_flash_source;
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: chick.h.
// ---------------------------------------------------------------------------

const FRAME_ATTAK101: i32 = 0;
const FRAME_ATTAK113: i32 = 12;
const FRAME_ATTAK114: i32 = 13;
const FRAME_ATTAK127: i32 = 26;
const FRAME_ATTAK128: i32 = 27;
const FRAME_ATTAK132: i32 = 31;
const FRAME_ATTAK201: i32 = 32;
const FRAME_ATTAK203: i32 = 34;
const FRAME_ATTAK204: i32 = 35;
const FRAME_ATTAK212: i32 = 43;
const FRAME_ATTAK213: i32 = 44;
const FRAME_ATTAK216: i32 = 47;
const FRAME_DEATH101: i32 = 48;
const FRAME_DEATH112: i32 = 59;
const FRAME_DEATH201: i32 = 60;
const FRAME_DEATH223: i32 = 82;
const FRAME_DUCK01: i32 = 83;
const FRAME_DUCK07: i32 = 89;
const FRAME_PAIN101: i32 = 90;
const FRAME_PAIN105: i32 = 94;
const FRAME_PAIN201: i32 = 95;
const FRAME_PAIN205: i32 = 99;
const FRAME_PAIN301: i32 = 100;
const FRAME_PAIN321: i32 = 120;
const FRAME_STAND101: i32 = 121;
const FRAME_STAND130: i32 = 150;
const FRAME_STAND201: i32 = 151;
const FRAME_STAND230: i32 = 180;
const FRAME_WALK01: i32 = 181;
const FRAME_WALK10: i32 = 190;
const FRAME_WALK11: i32 = 191;
const FRAME_WALK20: i32 = 200;

/// C `MODEL_SCALE` (chick.h).
const MODEL_SCALE: f32 = 1.0;

// C SP_monster_chick values.
const HEALTH: i32 = 175;
const MINS: Vec3f = Vec3f::new(-16.0, -16.0, 0.0);
const MAXS: Vec3f = Vec3f::new(16.0, 16.0, 56.0);
const MASS: i32 = 200;

const SOUND_MISSILE_PRELAUNCH: &str = "chick/chkatck1.wav";
const SOUND_MISSILE_LAUNCH: &str = "chick/chkatck2.wav";
const SOUND_MELEE_SWING: &str = "chick/chkatck3.wav";
const SOUND_MELEE_HIT: &str = "chick/chkatck4.wav";
const SOUND_MISSILE_RELOAD: &str = "chick/chkatck5.wav";
const SOUND_DEATH1: &str = "chick/chkdeth1.wav";
const SOUND_DEATH2: &str = "chick/chkdeth2.wav";
const SOUND_FALL_DOWN: &str = "chick/chkfall1.wav";
const SOUND_IDLE1: &str = "chick/chkidle1.wav";
const SOUND_IDLE2: &str = "chick/chkidle2.wav";
const SOUND_PAIN1: &str = "chick/chkpain1.wav";
const SOUND_PAIN2: &str = "chick/chkpain2.wav";
const SOUND_PAIN3: &str = "chick/chkpain3.wav";
const SOUND_SIGHT: &str = "chick/chksght1.wav";
const SOUND_SEARCH: &str = "chick/chksrch1.wav";

/// The sounds C `SP_monster_chick` precaches, in its order. C never plays
/// the launch, melee hit, fall and search sounds.
const PRECACHE: [&str; 15] = [
    SOUND_MISSILE_PRELAUNCH,
    SOUND_MISSILE_LAUNCH,
    SOUND_MELEE_SWING,
    SOUND_MELEE_HIT,
    SOUND_MISSILE_RELOAD,
    SOUND_DEATH1,
    SOUND_DEATH2,
    SOUND_FALL_DOWN,
    SOUND_IDLE1,
    SOUND_IDLE2,
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_PAIN3,
    SOUND_SIGHT,
    SOUND_SEARCH,
];

// ---------------------------------------------------------------------------
// Moves. C ref: chick.c `chick_frames_*` / `chick_move_*`.
// ---------------------------------------------------------------------------

const fn frame(aifunc: AnimFrameFn, dist: f32, thinkfunc: Option<ThinkFn>) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

static CHICK_FRAMES_FIDGET: [MonsterFrame; 30] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(chick_moan)),
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

static CHICK_MOVE_FIDGET: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND201,
    lastframe: FRAME_STAND230,
    frames: &CHICK_FRAMES_FIDGET,
    endfunc: Some(chick_stand),
};

static CHICK_FRAMES_STAND: [MonsterFrame; 30] = [
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
    frame(ai::ai_stand, 0.0, Some(chick_fidget)),
];

static CHICK_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND101,
    lastframe: FRAME_STAND130,
    frames: &CHICK_FRAMES_STAND,
    endfunc: None,
};

static CHICK_FRAMES_START_RUN: [MonsterFrame; 10] = [
    frame(ai::ai_run, 1.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, -1.0, None),
    frame(ai::ai_run, -1.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 1.0, None),
    frame(ai::ai_run, 3.0, None),
    frame(ai::ai_run, 6.0, None),
    frame(ai::ai_run, 3.0, None),
];

static CHICK_MOVE_START_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK01,
    lastframe: FRAME_WALK10,
    frames: &CHICK_FRAMES_START_RUN,
    endfunc: Some(chick_run),
};

static CHICK_FRAMES_RUN: [MonsterFrame; 10] = [
    frame(ai::ai_run, 6.0, None),
    frame(ai::ai_run, 8.0, Some(chick_footstep)),
    frame(ai::ai_run, 13.0, None),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 7.0, None),
    frame(ai::ai_run, 4.0, None),
    frame(ai::ai_run, 11.0, Some(chick_footstep)),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 9.0, None),
    frame(ai::ai_run, 7.0, None),
];

static CHICK_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK11,
    lastframe: FRAME_WALK20,
    frames: &CHICK_FRAMES_RUN,
    endfunc: None,
};

static CHICK_FRAMES_WALK: [MonsterFrame; 10] = [
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 8.0, Some(chick_footstep)),
    frame(ai::ai_walk, 13.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 11.0, Some(chick_footstep)),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, 7.0, None),
];

static CHICK_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK11,
    lastframe: FRAME_WALK20,
    frames: &CHICK_FRAMES_WALK,
    endfunc: None,
};

static CHICK_FRAMES_PAIN1: [MonsterFrame; 5] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static CHICK_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN105,
    frames: &CHICK_FRAMES_PAIN1,
    endfunc: Some(chick_run),
};

static CHICK_FRAMES_PAIN2: [MonsterFrame; 5] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static CHICK_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN205,
    frames: &CHICK_FRAMES_PAIN2,
    endfunc: Some(chick_run),
};

static CHICK_FRAMES_PAIN3: [MonsterFrame; 21] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 11.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, 7.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -8.0, None),
    frame(ai::ai_move, 2.0, None),
];

static CHICK_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN321,
    frames: &CHICK_FRAMES_PAIN3,
    endfunc: Some(chick_run),
};

static CHICK_FRAMES_DEATH2: [MonsterFrame; 23] = [
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -5.0, Some(chick_footstep)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 10.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 3.0, Some(chick_footstep)),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 1.0, Some(chick_footstep)),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 15.0, None),
    frame(ai::ai_move, 14.0, None),
    frame(ai::ai_move, 1.0, None),
];

static CHICK_MOVE_DEATH2: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH201,
    lastframe: FRAME_DEATH223,
    frames: &CHICK_FRAMES_DEATH2,
    endfunc: Some(chick_dead),
};

static CHICK_FRAMES_DEATH1: [MonsterFrame; 12] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 11.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static CHICK_MOVE_DEATH1: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH112,
    frames: &CHICK_FRAMES_DEATH1,
    endfunc: Some(chick_dead),
};

static CHICK_FRAMES_DUCK: [MonsterFrame; 7] = [
    frame(ai::ai_move, 0.0, Some(chick_duck_down)),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 4.0, Some(chick_duck_hold)),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -5.0, Some(chick_duck_up)),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 1.0, None),
];

static CHICK_MOVE_DUCK: MonsterMove = MonsterMove {
    firstframe: FRAME_DUCK01,
    lastframe: FRAME_DUCK07,
    frames: &CHICK_FRAMES_DUCK,
    endfunc: Some(chick_run),
};

static CHICK_FRAMES_START_ATTACK1: [MonsterFrame; 13] = [
    frame(ai::ai_charge, 0.0, Some(chick_preattack1)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 4.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, 7.0, Some(chick_footstep)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(chick_attack1)),
];

static CHICK_MOVE_START_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK113,
    frames: &CHICK_FRAMES_START_ATTACK1,
    endfunc: None,
};

static CHICK_FRAMES_ATTACK1: [MonsterFrame; 14] = [
    frame(ai::ai_charge, 19.0, Some(chick_rocket)),
    frame(ai::ai_charge, -6.0, None),
    frame(ai::ai_charge, -5.0, Some(chick_footstep)),
    frame(ai::ai_charge, -2.0, None),
    frame(ai::ai_charge, -7.0, Some(chick_footstep)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 10.0, Some(chick_reload)),
    frame(ai::ai_charge, 4.0, None),
    frame(ai::ai_charge, 5.0, Some(chick_footstep)),
    frame(ai::ai_charge, 6.0, None),
    frame(ai::ai_charge, 6.0, None),
    frame(ai::ai_charge, 4.0, Some(chick_footstep)),
    frame(ai::ai_charge, 3.0, Some(chick_rerocket)),
];

static CHICK_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK114,
    lastframe: FRAME_ATTAK127,
    frames: &CHICK_FRAMES_ATTACK1,
    endfunc: None,
};

static CHICK_FRAMES_END_ATTACK1: [MonsterFrame; 5] = [
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -6.0, None),
    frame(ai::ai_charge, -4.0, None),
    frame(ai::ai_charge, -2.0, Some(chick_footstep)),
];

static CHICK_MOVE_END_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK128,
    lastframe: FRAME_ATTAK132,
    frames: &CHICK_FRAMES_END_ATTACK1,
    endfunc: Some(chick_run),
};

static CHICK_FRAMES_SLASH: [MonsterFrame; 9] = [
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 7.0, Some(chick_slash_hit)),
    frame(ai::ai_charge, -7.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, -2.0, Some(chick_reslash)),
];

static CHICK_MOVE_SLASH: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK204,
    lastframe: FRAME_ATTAK212,
    frames: &CHICK_FRAMES_SLASH,
    endfunc: None,
};

static CHICK_FRAMES_END_SLASH: [MonsterFrame; 4] = [
    frame(ai::ai_charge, -6.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -6.0, None),
    frame(ai::ai_charge, 0.0, Some(chick_footstep)),
];

static CHICK_MOVE_END_SLASH: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK213,
    lastframe: FRAME_ATTAK216,
    frames: &CHICK_FRAMES_END_SLASH,
    endfunc: Some(chick_run),
};

static CHICK_FRAMES_START_SLASH: [MonsterFrame; 3] = [
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 8.0, None),
    frame(ai::ai_charge, 3.0, Some(chick_footstep)),
];

static CHICK_MOVE_START_SLASH: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK203,
    frames: &CHICK_FRAMES_START_SLASH,
    endfunc: Some(chick_slash),
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// C `gi.sound(self, chan, sound_x, 1, attn, 0)`. C keeps the index in a
/// file static; the server's `SV_FindIndex` returns the precached one.
fn sound(world: &mut GameWorld, key: EntityKey, chan: i32, name: &str, attn: f32) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), chan, index, 1.0, attn, 0.0);
}

/// C `self->monsterinfo.currentmove = &move`.
fn set_move(world: &mut GameWorld, key: EntityKey, mv: &MonsterMove) {
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.currentmove = Some(mv.clone());
    }
}

fn monsterinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MonsterInfo> {
    world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_deref_mut())
}

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

/// `self->enemy` while it has health left (C dereferences it unchecked).
fn live_enemy(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
    world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .filter(|&e| world.entities.get(e).is_some_and(|e| e.game.health > 0))
}

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

/// C `g_monsterfootsteps` defaults to 0 and is not ported, so the step
/// sounds (`bitch/step1.wav`, `step2.wav`) never play.
/// C ref: `chick_footstep`.
pub fn chick_footstep(_world: &mut GameWorld, _key: EntityKey) {}

/// C ref: `ChickMoan`.
pub fn chick_moan(world: &mut GameWorld, key: EntityKey) {
    let name = if world.random() < 0.5 {
        SOUND_IDLE1
    } else {
        SOUND_IDLE2
    };
    sound(world, key, CHAN_VOICE, name, ATTN_IDLE);
}

/// Fidget three stands in ten, unless holding ground. C ref: `chick_fidget`.
pub fn chick_fidget(world: &mut GameWorld, key: EntityKey) {
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        return;
    }
    if world.random() <= 0.3 {
        set_move(world, key, &CHICK_MOVE_FIDGET);
    }
}

/// C ref: `chick_stand`.
pub fn chick_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &CHICK_MOVE_STAND);
}

/// C ref: `chick_walk`.
pub fn chick_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &CHICK_MOVE_WALK);
}

/// Start running, or keep running once started or coming from a walk.
/// C ref: `chick_run`.
pub fn chick_run(world: &mut GameWorld, key: EntityKey) {
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        set_move(world, key, &CHICK_MOVE_STAND);
        return;
    }
    if is_current(world, key, &CHICK_MOVE_WALK) || is_current(world, key, &CHICK_MOVE_START_RUN) {
        set_move(world, key, &CHICK_MOVE_RUN);
    } else {
        set_move(world, key, &CHICK_MOVE_START_RUN);
    }
}

/// C ref: `chick_pain`.
pub fn chick_pain(
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
    let r = world.random();
    let name = if r < 0.33 {
        SOUND_PAIN1
    } else if r < 0.66 {
        SOUND_PAIN2
    } else {
        SOUND_PAIN3
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
    if world.game.skill == SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    if damage <= 10 {
        set_move(world, key, &CHICK_MOVE_PAIN1);
    } else if damage <= 25 {
        set_move(world, key, &CHICK_MOVE_PAIN2);
    } else {
        set_move(world, key, &CHICK_MOVE_PAIN3);
    }
}

/// The corpse box, then stop. C ref: `chick_dead`.
pub fn chick_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, 0.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 16.0);
    }
    ai::monster_dead(world, key);
}

/// Gib at `gib_health`, else fall one of two ways. C ref: `chick_die`.
pub fn chick_die(
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
        for _ in 0..2 {
            throw_gib(
                world,
                key,
                "models/objects/gibs/bone/tris.md2",
                damage,
                GibType::Organic,
            );
        }
        for _ in 0..4 {
            throw_gib(
                world,
                key,
                "models/objects/gibs/sm_meat/tris.md2",
                damage,
                GibType::Organic,
            );
        }
        throw_head(
            world,
            key,
            "models/objects/gibs/head2/tris.md2",
            damage,
            GibType::Organic,
        );
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.deadflag = DeadFlag::Dead;
        }
        return;
    }
    if ent.game.deadflag == DeadFlag::Dead {
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    if world.random_u32().is_multiple_of(2) {
        set_move(world, key, &CHICK_MOVE_DEATH1);
        sound(world, key, CHAN_VOICE, SOUND_DEATH1, ATTN_NORM);
    } else {
        set_move(world, key, &CHICK_MOVE_DEATH2);
        sound(world, key, CHAN_VOICE, SOUND_DEATH2, ATTN_NORM);
    }
}

/// Crouch 32 units for a second. C ref: `chick_duck_down`.
pub fn chick_duck_down(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.monsterinfo.as_mut() else {
        return;
    };
    if mi.aiflags.contains(AiFlags::DUCKED) {
        return;
    }
    mi.aiflags |= AiFlags::DUCKED;
    mi.pausetime = time + 1.0;
    ent.maxs.z -= 32.0;
    ent.game.takedamage = TakeDamage::Yes;
    world.link_entity(key);
}

/// Hold the crouch frame until `pausetime`. C ref: `chick_duck_hold`.
pub fn chick_duck_hold(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(mi) = monsterinfo_mut(world, key) else {
        return;
    };
    if time >= mi.pausetime {
        mi.aiflags.remove(AiFlags::HOLD_FRAME);
    } else {
        mi.aiflags |= AiFlags::HOLD_FRAME;
    }
}

/// C ref: `chick_duck_up`.
pub fn chick_duck_up(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.aiflags.remove(AiFlags::DUCKED);
    }
    ent.maxs.z += 32.0;
    ent.game.takedamage = TakeDamage::Aim;
    world.link_entity(key);
}

/// Duck one shot in four, turning on an attacker if it had no enemy.
/// C ref: `chick_dodge`.
pub fn chick_dodge(world: &mut GameWorld, key: EntityKey, attacker: EntityKey, _eta: f32) {
    if world.random() > 0.25 {
        return;
    }
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.enemy.is_none() {
        ent.game.enemy = Some(attacker);
        ai::found_target(world, key);
    }
    set_move(world, key, &CHICK_MOVE_DUCK);
}

/// A slash for 10 to 15. C ref: `ChickSlash`.
pub fn chick_slash_hit(world: &mut GameWorld, key: EntityKey) {
    let Some(mins_x) = world.entities.get(key).map(|e| e.mins.x) else {
        return;
    };
    let aim = Vec3f::new(ai::MELEE_DISTANCE, mins_x, 10.0);
    sound(world, key, CHAN_WEAPON, SOUND_MELEE_SWING, ATTN_NORM);
    let damage = 10 + (world.random_u32() % 6) as i32;
    world.fire_hit(key, aim, damage, 100);
}

/// A 50 damage rocket at 500 units/s, at the enemy's eyes.
/// C ref: `ChickRocket`.
pub fn chick_rocket(world: &mut GameWorld, key: EntityKey) {
    let Some((start, _, _)) = monster_flash_source(world, key, MZ2_CHICK_ROCKET_1) else {
        return;
    };
    let Some(enemy) = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .and_then(|e| world.entities.get(e))
    else {
        return;
    };
    let target = enemy.state.origin + Vec3f::new(0.0, 0.0, enemy.game.viewheight);
    let dir = (target - start).normalize_or_zero();
    monster_fire_rocket(world, key, start, dir, 50, 500, MZ2_CHICK_ROCKET_1);
}

/// C ref: `Chick_PreAttack1`.
pub fn chick_preattack1(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_MISSILE_PRELAUNCH, ATTN_NORM);
}

/// C ref: `ChickReload`.
pub fn chick_reload(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_MISSILE_RELOAD, ATTN_NORM);
}

/// Fire again six times in ten at a live, visible enemy out of reach.
/// C ref: `chick_rerocket`.
pub fn chick_rerocket(world: &mut GameWorld, key: EntityKey) {
    if let Some(enemy) = live_enemy(world, key) {
        if ai::range(world, key, enemy) != Range::Melee
            && ai::visible(world, key, enemy)
            && world.random() <= 0.6
        {
            set_move(world, key, &CHICK_MOVE_ATTACK1);
            return;
        }
    }
    set_move(world, key, &CHICK_MOVE_END_ATTACK1);
}

/// C ref: `chick_attack1`.
pub fn chick_attack1(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &CHICK_MOVE_ATTACK1);
}

/// Slash again nine times in ten while a live enemy is in reach.
/// C ref: `chick_reslash`.
pub fn chick_reslash(world: &mut GameWorld, key: EntityKey) {
    if let Some(enemy) = live_enemy(world, key) {
        if ai::range(world, key, enemy) == Range::Melee {
            if world.random() <= 0.9 {
                set_move(world, key, &CHICK_MOVE_SLASH);
            } else {
                set_move(world, key, &CHICK_MOVE_END_SLASH);
            }
            return;
        }
    }
    set_move(world, key, &CHICK_MOVE_END_SLASH);
}

/// C ref: `chick_slash`.
pub fn chick_slash(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &CHICK_MOVE_SLASH);
}

/// C ref: `chick_melee`.
pub fn chick_melee(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &CHICK_MOVE_START_SLASH);
}

/// C ref: `chick_attack`.
pub fn chick_attack(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &CHICK_MOVE_START_ATTACK1);
}

/// C ref: `chick_sight`.
pub fn chick_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_chick`; `monster_start`
/// (run by `call_spawn`) removes it in deathmatch.
pub fn sp_monster_chick(
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
        .unwrap_or_else(|| "monster_chick".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = MINS;
    ent.maxs = MAXS;
    ent.game.health = HEALTH;
    ent.game.gib_health = -70;
    ent.game.mass = MASS;
    ent.pain = Some(chick_pain);
    ent.die = Some(chick_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(chick_stand),
        walk: Some(chick_walk),
        run: Some(chick_run),
        dodge: Some(chick_dodge),
        attack: Some(chick_attack),
        melee: Some(chick_melee),
        sight: Some(chick_sight),
        currentmove: Some(CHICK_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_chick`: its sound precaches.
pub fn sp_monster_chick_finish(
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
            (&CHICK_MOVE_FIDGET, 151, 180, vec![
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, Some(chick_moan as ThinkFn)),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
            ], Some(chick_stand as ThinkFn)),
            (&CHICK_MOVE_STAND, 121, 150, vec![
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, Some(chick_fidget as ThinkFn)),
            ], None),
            (&CHICK_MOVE_START_RUN, 181, 190, vec![
                (ai::ai_run as AnimFrameFn, 1.0, None),
                (ai::ai_run as AnimFrameFn, 0.0, None),
                (ai::ai_run as AnimFrameFn, 0.0, None),
                (ai::ai_run as AnimFrameFn, -1.0, None),
                (ai::ai_run as AnimFrameFn, -1.0, None),
                (ai::ai_run as AnimFrameFn, 0.0, None),
                (ai::ai_run as AnimFrameFn, 1.0, None),
                (ai::ai_run as AnimFrameFn, 3.0, None),
                (ai::ai_run as AnimFrameFn, 6.0, None),
                (ai::ai_run as AnimFrameFn, 3.0, None),
            ], Some(chick_run as ThinkFn)),
            (&CHICK_MOVE_RUN, 191, 200, vec![
                (ai::ai_run as AnimFrameFn, 6.0, None),
                (ai::ai_run as AnimFrameFn, 8.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 13.0, None),
                (ai::ai_run as AnimFrameFn, 5.0, None),
                (ai::ai_run as AnimFrameFn, 7.0, None),
                (ai::ai_run as AnimFrameFn, 4.0, None),
                (ai::ai_run as AnimFrameFn, 11.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 5.0, None),
                (ai::ai_run as AnimFrameFn, 9.0, None),
                (ai::ai_run as AnimFrameFn, 7.0, None),
            ], None),
            (&CHICK_MOVE_WALK, 191, 200, vec![
                (ai::ai_walk as AnimFrameFn, 6.0, None),
                (ai::ai_walk as AnimFrameFn, 8.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 13.0, None),
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, 7.0, None),
                (ai::ai_walk as AnimFrameFn, 4.0, None),
                (ai::ai_walk as AnimFrameFn, 11.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, 9.0, None),
                (ai::ai_walk as AnimFrameFn, 7.0, None),
            ], None),
            (&CHICK_MOVE_PAIN1, 90, 94, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(chick_run as ThinkFn)),
            (&CHICK_MOVE_PAIN2, 95, 99, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(chick_run as ThinkFn)),
            (&CHICK_MOVE_PAIN3, 100, 120, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -6.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 11.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 4.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -3.0, None),
                (ai::ai_move as AnimFrameFn, -4.0, None),
                (ai::ai_move as AnimFrameFn, 5.0, None),
                (ai::ai_move as AnimFrameFn, 7.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, -5.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, -8.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
            ], Some(chick_run as ThinkFn)),
            (&CHICK_MOVE_DEATH2, 60, 82, vec![
                (ai::ai_move as AnimFrameFn, -6.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -1.0, None),
                (ai::ai_move as AnimFrameFn, -5.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -1.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 10.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_move as AnimFrameFn, -3.0, None),
                (ai::ai_move as AnimFrameFn, -5.0, None),
                (ai::ai_move as AnimFrameFn, 4.0, None),
                (ai::ai_move as AnimFrameFn, 15.0, None),
                (ai::ai_move as AnimFrameFn, 14.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
            ], Some(chick_dead as ThinkFn)),
            (&CHICK_MOVE_DEATH1, 48, 59, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -7.0, None),
                (ai::ai_move as AnimFrameFn, 4.0, None),
                (ai::ai_move as AnimFrameFn, 11.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(chick_dead as ThinkFn)),
            (&CHICK_MOVE_DUCK, 83, 89, vec![
                (ai::ai_move as AnimFrameFn, 0.0, Some(chick_duck_down as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 4.0, Some(chick_duck_hold as ThinkFn)),
                (ai::ai_move as AnimFrameFn, -4.0, None),
                (ai::ai_move as AnimFrameFn, -5.0, Some(chick_duck_up as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
            ], Some(chick_run as ThinkFn)),
            (&CHICK_MOVE_START_ATTACK1, 0, 12, vec![
                (ai::ai_charge as AnimFrameFn, 0.0, Some(chick_preattack1 as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 4.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, -3.0, None),
                (ai::ai_charge as AnimFrameFn, 3.0, None),
                (ai::ai_charge as AnimFrameFn, 5.0, None),
                (ai::ai_charge as AnimFrameFn, 7.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(chick_attack1 as ThinkFn)),
            ], None),
            (&CHICK_MOVE_ATTACK1, 13, 26, vec![
                (ai::ai_charge as AnimFrameFn, 19.0, Some(chick_rocket as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, -6.0, None),
                (ai::ai_charge as AnimFrameFn, -5.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, -2.0, None),
                (ai::ai_charge as AnimFrameFn, -7.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 1.0, None),
                (ai::ai_charge as AnimFrameFn, 10.0, Some(chick_reload as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 4.0, None),
                (ai::ai_charge as AnimFrameFn, 5.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 6.0, None),
                (ai::ai_charge as AnimFrameFn, 6.0, None),
                (ai::ai_charge as AnimFrameFn, 4.0, Some(chick_footstep as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 3.0, Some(chick_rerocket as ThinkFn)),
            ], None),
            (&CHICK_MOVE_END_ATTACK1, 27, 31, vec![
                (ai::ai_charge as AnimFrameFn, -3.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, -6.0, None),
                (ai::ai_charge as AnimFrameFn, -4.0, None),
                (ai::ai_charge as AnimFrameFn, -2.0, Some(chick_footstep as ThinkFn)),
            ], Some(chick_run as ThinkFn)),
            (&CHICK_MOVE_SLASH, 35, 43, vec![
                (ai::ai_charge as AnimFrameFn, 1.0, None),
                (ai::ai_charge as AnimFrameFn, 7.0, Some(chick_slash_hit as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, -7.0, None),
                (ai::ai_charge as AnimFrameFn, 1.0, None),
                (ai::ai_charge as AnimFrameFn, -1.0, None),
                (ai::ai_charge as AnimFrameFn, 1.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 1.0, None),
                (ai::ai_charge as AnimFrameFn, -2.0, Some(chick_reslash as ThinkFn)),
            ], None),
            (&CHICK_MOVE_END_SLASH, 44, 47, vec![
                (ai::ai_charge as AnimFrameFn, -6.0, None),
                (ai::ai_charge as AnimFrameFn, -1.0, None),
                (ai::ai_charge as AnimFrameFn, -6.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(chick_footstep as ThinkFn)),
            ], Some(chick_run as ThinkFn)),
            (&CHICK_MOVE_START_SLASH, 32, 34, vec![
                (ai::ai_charge as AnimFrameFn, 1.0, None),
                (ai::ai_charge as AnimFrameFn, 8.0, None),
                (ai::ai_charge as AnimFrameFn, 3.0, Some(chick_footstep as ThinkFn)),
            ], Some(chick_slash as ThinkFn)),
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
    fn chick_moves_match_c_frame_by_frame() {
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
        sp_monster_chick(&mut world.entities, key, &HashMap::new());
        key
    }

    fn mi(world: &GameWorld, key: EntityKey) -> &MonsterInfo {
        world
            .entities
            .get(key)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap()
    }

    fn mi_mut(world: &mut GameWorld, key: EntityKey) -> &mut MonsterInfo {
        monsterinfo_mut(world, key).unwrap()
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

    /// A monster enemy `x` units in front, eyes 22 up.
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
        world.link_entity(key);
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        enemy
    }

    #[test]
    fn chick_spawns_with_its_c_pain_walk_run_and_melee_callbacks() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(ent.pain.is_some());
        assert!(mi.walk.is_some() && mi.run.is_some() && mi.melee.is_some());
    }

    #[test]
    fn chick_spawns_with_the_c_values_and_callbacks() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "monster_chick");
        assert_eq!((ent.game.health, ent.game.mass), (175, 200));
        assert_eq!((ent.mins, ent.maxs), (MINS, MAXS));
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert_eq!(mi.scale, 1.0);
        assert!(mi.idle.is_none() && mi.search.is_none());
        assert!(std::ptr::fn_addr_eq(
            mi.attack.unwrap(),
            chick_attack as ThinkFn
        ));
        assert!(std::ptr::fn_addr_eq(
            mi.dodge.unwrap(),
            chick_dodge as crate::entity::MonsterDodgeFn
        ));
        assert!(mi.sight.is_some());
        assert!(is_move(&world, key, &CHICK_MOVE_STAND));
    }

    #[test]
    fn chick_finish_precaches_the_c_sounds_and_the_spawn_row_uses_it() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        let row = crate::spawn::find_spawn("monster_chick").unwrap();
        assert_eq!(row.2, Some("models/monsters/bitch/tris.md2"));
        row.3.unwrap()(&mut world, key, &HashMap::new());
        assert_eq!(*gi.sounds.borrow(), PRECACHE);
    }

    #[test]
    fn chick_moans_one_of_two_idle_sounds_and_sights_with_its_own() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        seed_random(&mut world, |r| r < 0.5);
        chick_moan(&mut world, key);
        seed_random(&mut world, |r| r >= 0.5);
        chick_moan(&mut world, key);
        chick_sight(&mut world, key, key);
        chick_preattack1(&mut world, key);
        chick_reload(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [
                (CHAN_VOICE, SOUND_IDLE1.to_string(), ATTN_IDLE),
                (CHAN_VOICE, SOUND_IDLE2.to_string(), ATTN_IDLE),
                (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_MISSILE_PRELAUNCH.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_MISSILE_RELOAD.to_string(), ATTN_NORM),
            ]
        );
    }

    #[test]
    fn chick_fidgets_three_times_in_ten_unless_holding_ground() {
        let mut world = test_world();
        let key = spawned(&mut world);
        seed_random(&mut world, |r| r > 0.3);
        chick_fidget(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_STAND));
        seed_random(&mut world, |r| r <= 0.3);
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        chick_fidget(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_STAND));
        mi_mut(&mut world, key).aiflags = AiFlags::empty();
        chick_fidget(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_FIDGET));
    }

    #[test]
    fn chick_run_starts_then_runs_and_holding_ground_stands() {
        let mut world = test_world();
        let key = spawned(&mut world);
        chick_run(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_START_RUN));
        chick_run(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_RUN));
        chick_walk(&mut world, key);
        chick_run(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_RUN));
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        chick_run(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_STAND));
    }

    #[test]
    fn chick_attack_and_melee_start_their_c_moves() {
        let mut world = test_world();
        let key = spawned(&mut world);
        chick_attack(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_START_ATTACK1));
        chick_attack1(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_ATTACK1));
        chick_melee(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_START_SLASH));
        chick_slash(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_SLASH));
    }

    #[test]
    fn chick_pain_sound_by_roll_and_move_by_damage() {
        for (damage, mv) in [
            (10, &CHICK_MOVE_PAIN1),
            (11, &CHICK_MOVE_PAIN2),
            (25, &CHICK_MOVE_PAIN2),
            (26, &CHICK_MOVE_PAIN3),
        ] {
            let mut world = test_world();
            let key = spawned(&mut world);
            chick_pain(&mut world, key, key, 0.0, damage);
            assert!(is_move(&world, key, mv), "{damage}");
        }
        for (rolls, name) in [
            (0.0_f32..0.33, SOUND_PAIN1),
            (0.33..0.66, SOUND_PAIN2),
            (0.66..1.01, SOUND_PAIN3),
        ] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            world.level.time = 1.0;
            seed_random(&mut world, |r| rolls.contains(&r));
            chick_pain(&mut world, key, key, 0.0, 5);
            assert_eq!(sounds(&gi), [(CHAN_VOICE, name.to_string(), ATTN_NORM)]);
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                4.0
            );
            chick_pain(&mut world, key, key, 0.0, 5);
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn chick_pain_below_half_health_turns_the_skin_and_nightmare_skips_the_move() {
        let (mut world, gi) = test_world_recording();
        world.game.skill = 3;
        let key = spawned(&mut world);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.max_health = 175;
        // C integer division: below 175 / 2 = 87.
        ent.game.health = 86;
        chick_pain(&mut world, key, key, 0.0, 50);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
        assert_eq!(sounds(&gi).len(), 1);
        assert!(is_move(&world, key, &CHICK_MOVE_STAND));
    }

    #[test]
    fn chick_at_its_c_gib_health_throws_six_gibs() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -70;
        let before = world.entities.count();
        let die = world.entities.get(key).unwrap().die.unwrap();
        die(&mut world, key, key, key, 100, Vec3f::ZERO);
        assert_eq!(world.entities.count(), before + 6);
        assert_eq!(
            sounds(&gi),
            [(CHAN_VOICE, "misc/udeath.wav".to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn chick_above_gib_health_falls_by_randk_parity_once() {
        for (parity, mv, name) in [
            (0, &CHICK_MOVE_DEATH1, SOUND_DEATH1),
            (1, &CHICK_MOVE_DEATH2, SOUND_DEATH2),
        ] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            world.entities.get_mut(key).unwrap().game.health = -69;
            seed_randk(&mut world, |r| r % 2 == parity);
            chick_die(&mut world, key, key, key, 100, Vec3f::ZERO);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.deadflag, DeadFlag::Dead);
            assert_eq!(ent.game.takedamage, TakeDamage::Yes);
            assert!(is_move(&world, key, mv));
            assert_eq!(sounds(&gi), [(CHAN_VOICE, name.to_string(), ATTN_NORM)]);
            chick_die(&mut world, key, key, key, 10, Vec3f::ZERO);
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn chick_dead_shrinks_to_its_corpse_box_and_stops_thinking() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.nextthink = 9.0;
        chick_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.mins, Vec3f::new(-16.0, -16.0, 0.0));
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, 16.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(ent.game.nextthink, 0.0);
    }

    #[test]
    fn chick_ducks_32_units_for_a_second_once_then_stands_back_up() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.level.time = 2.0;
        chick_duck_down(&mut world, key);
        chick_duck_down(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 24.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert!(mi(&world, key).aiflags.contains(AiFlags::DUCKED));
        assert_eq!(mi(&world, key).pausetime, 3.0);

        chick_duck_hold(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        world.level.time = 3.0;
        chick_duck_hold(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));

        chick_duck_up(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 56.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Aim);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::DUCKED));
    }

    #[test]
    fn chick_dodges_one_shot_in_four_and_turns_on_an_attacker() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let attacker = world.spawn().unwrap();
        seed_random(&mut world, |r| r > 0.25);
        chick_dodge(&mut world, key, attacker, 0.5);
        assert!(is_move(&world, key, &CHICK_MOVE_STAND));
        assert!(world.entities.get(key).unwrap().game.enemy.is_none());

        seed_random(&mut world, |r| r <= 0.25);
        chick_dodge(&mut world, key, attacker, 0.5);
        assert!(is_move(&world, key, &CHICK_MOVE_DUCK));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(attacker));
    }

    #[test]
    fn chick_dodge_keeps_its_current_enemy() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let enemy = enemy_at(&mut world, key, 200.0);
        let attacker = world.spawn().unwrap();
        seed_random(&mut world, |r| r <= 0.25);
        chick_dodge(&mut world, key, attacker, 0.5);
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(enemy));
        assert!(is_move(&world, key, &CHICK_MOVE_DUCK));
    }

    #[test]
    fn chick_slash_swings_with_its_sound_and_hits_for_10_to_15() {
        for k in 0..6 {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            let enemy = enemy_at(&mut world, key, 40.0);
            *gi.next_trace.borrow_mut() = Some(Trace {
                fraction: 0.5,
                ent_index: world.number_of(enemy),
                ..Trace::default()
            });
            seed_randk(&mut world, |r| r % 6 == k);
            chick_slash_hit(&mut world, key);
            let health = world.entities.get(enemy).unwrap().game.health;
            assert_eq!(health, 100 - (10 + k as i32));
            assert_eq!(
                sounds(&gi),
                [(CHAN_WEAPON, SOUND_MELEE_SWING.to_string(), ATTN_NORM)]
            );
        }
    }

    #[test]
    fn chick_rocket_flies_at_500_toward_the_enemy_eyes_with_its_flash() {
        let (mut world, gi) = wire_world();
        let key = spawned(&mut world);
        enemy_at(&mut world, key, 300.0);
        chick_rocket(&mut world, key);
        let start = monster_flash_source(&world, key, MZ2_CHICK_ROCKET_1)
            .unwrap()
            .0;
        let rocket = world.find_by_classname(None, "rocket").unwrap();
        let r = world.entities.get(rocket).unwrap();
        let expected = (Vec3f::new(300.0, 0.0, 22.0) - start).normalize() * 500.0;
        assert!((r.velocity - expected).length() < 0.01, "{}", r.velocity);
        assert_eq!(r.owner, Some(key));
        let number = world.number_of(key).unwrap() as i32;
        let wire = gi.wire.borrow();
        assert_eq!(
            wire[wire.len() - 4..],
            [
                Wire::Byte(SvcOp::MuzzleFlash2 as i32),
                Wire::Short(number),
                Wire::Byte(MZ2_CHICK_ROCKET_1),
                Wire::Multicast(start, Multicast::PVS),
            ]
        );
    }

    #[test]
    fn chick_rerockets_six_times_in_ten_at_a_live_visible_enemy_out_of_reach() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let enemy = enemy_at(&mut world, key, 300.0);
        seed_random(&mut world, |r| r <= 0.6);
        chick_rerocket(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_ATTACK1));
        seed_random(&mut world, |r| r > 0.6);
        chick_rerocket(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_END_ATTACK1));

        // In reach, or dead: stop.
        world.entities.get_mut(enemy).unwrap().state.origin.x = 40.0;
        world.link_entity(enemy);
        seed_random(&mut world, |r| r <= 0.6);
        chick_rerocket(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_END_ATTACK1));
        set_move(&mut world, key, &CHICK_MOVE_ATTACK1);
        world.entities.get_mut(enemy).unwrap().state.origin.x = 300.0;
        world.link_entity(enemy);
        world.entities.get_mut(enemy).unwrap().game.health = 0;
        chick_rerocket(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_END_ATTACK1));
    }

    #[test]
    fn chick_does_not_rerocket_at_an_enemy_out_of_sight() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        enemy_at(&mut world, key, 300.0);
        // The visibility trace stops at a wall halfway.
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        seed_random(&mut world, |r| r <= 0.6);
        chick_rerocket(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_END_ATTACK1));
    }

    #[test]
    fn chick_reslashes_nine_times_in_ten_at_a_live_enemy_in_reach() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let enemy = enemy_at(&mut world, key, 40.0);
        seed_random(&mut world, |r| r <= 0.9);
        chick_reslash(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_SLASH));
        seed_random(&mut world, |r| r > 0.9);
        chick_reslash(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_END_SLASH));

        set_move(&mut world, key, &CHICK_MOVE_SLASH);
        world.entities.get_mut(enemy).unwrap().state.origin.x = 300.0;
        world.link_entity(enemy);
        seed_random(&mut world, |r| r <= 0.9);
        chick_reslash(&mut world, key);
        assert!(is_move(&world, key, &CHICK_MOVE_END_SLASH));
    }

    #[test]
    fn chick_sights_a_player_and_fires_rockets_at_it() {
        let (mut world, gi, key, player) =
            monster_arena(sp_monster_chick, sp_monster_chick_finish, 400.0);
        let mut angry = false;
        run(&mut world, key, 150, |w| {
            angry |= w.entities.get(key).unwrap().game.enemy == Some(player);
        });
        assert!(angry);
        let played = sounds(&gi.floor.inner);
        assert_eq!(played[0], (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM));
        assert!(played.iter().any(|s| s.1 == SOUND_MISSILE_PRELAUNCH));
        let rocket = world.find_by_classname(None, "rocket").unwrap();
        assert_eq!(world.entities.get(rocket).unwrap().owner, Some(key));
    }
}
