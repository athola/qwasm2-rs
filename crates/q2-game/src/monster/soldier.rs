//! Soldier monster — 3 variants (light/normal/ss) on one model; the skin
//! picks the weapon (blaster, shotgun, machinegun).
//!
//! C ref: `~/Qwasm2/src/game/monster/soldier/` (soldier.c, soldier.h).

use q2_shared::constants::*;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::{fire, flash};
use crate::monster::{frame, set_move, sound};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: soldier.h (`FRAME_*`, model frame numbers).
// ---------------------------------------------------------------------------

const FRAME_ATTAK101: i32 = 0;
const FRAME_ATTAK112: i32 = 11;
const FRAME_ATTAK201: i32 = 12;
const FRAME_ATTAK218: i32 = 29;
const FRAME_ATTAK301: i32 = 30;
const FRAME_ATTAK309: i32 = 38;
const FRAME_ATTAK401: i32 = 39;
const FRAME_ATTAK406: i32 = 44;
const FRAME_DUCK01: i32 = 45;
const FRAME_DUCK05: i32 = 49;
const FRAME_PAIN101: i32 = 50;
const FRAME_PAIN105: i32 = 54;
const FRAME_PAIN201: i32 = 55;
const FRAME_PAIN207: i32 = 61;
const FRAME_PAIN301: i32 = 62;
const FRAME_PAIN318: i32 = 79;
const FRAME_PAIN401: i32 = 80;
const FRAME_PAIN417: i32 = 96;
const FRAME_RUN01: i32 = 97;
const FRAME_RUN02: i32 = 98;
const FRAME_RUN03: i32 = 99;
const FRAME_RUN08: i32 = 104;
const FRAME_RUNS01: i32 = 109;
const FRAME_RUNS14: i32 = 122;
const FRAME_STAND101: i32 = 146;
const FRAME_STAND130: i32 = 175;
const FRAME_STAND301: i32 = 176;
const FRAME_STAND339: i32 = 214;
const FRAME_WALK101: i32 = 215;
const FRAME_WALK133: i32 = 247;
const FRAME_WALK209: i32 = 256;
const FRAME_WALK218: i32 = 265;
const FRAME_DEATH101: i32 = 272;
const FRAME_DEATH136: i32 = 307;
const FRAME_DEATH201: i32 = 308;
const FRAME_DEATH235: i32 = 342;
const FRAME_DEATH301: i32 = 343;
const FRAME_DEATH345: i32 = 387;
const FRAME_DEATH401: i32 = 388;
const FRAME_DEATH453: i32 = 440;
const FRAME_DEATH501: i32 = 441;
const FRAME_DEATH524: i32 = 464;
const FRAME_DEATH601: i32 = 465;
const FRAME_DEATH610: i32 = 474;
const FRAME_ATTAK102: i32 = 1;
const FRAME_ATTAK110: i32 = 9;
const FRAME_ATTAK204: i32 = 15;
const FRAME_ATTAK216: i32 = 27;
const FRAME_ATTAK303: i32 = 32;
const FRAME_RUNS03: i32 = 111;
const FRAME_STAND322: i32 = 197;

pub const FRAME_ATTACK_FIRST: i32 = 105;
pub const FRAME_ATTACK_LAST: i32 = 112;

/// C model scale, multiplied into every frame's distance.
const MODEL_SCALE: f32 = 1.2;

/// C `SP_monster_soldier_*` values.
const SOLDIER_LIGHT_HEALTH: i32 = 20;
const SOLDIER_HEALTH: i32 = 30;
const SOLDIER_SS_HEALTH: i32 = 40;
const SOLDIER_MASS: i32 = 100;

/// C cvar `g_monsterfootsteps`, which defaults to "0"; the game has no
/// cvar access, so footsteps stay off as in a default C game.
const G_MONSTERFOOTSTEPS: bool = false;

const SOUND_IDLE: &str = "soldier/solidle1.wav";
const SOUND_SIGHT1: &str = "soldier/solsght1.wav";
const SOUND_SIGHT2: &str = "soldier/solsrch1.wav";
const SOUND_PAIN_LIGHT: &str = "soldier/solpain2.wav";
const SOUND_PAIN: &str = "soldier/solpain1.wav";
const SOUND_PAIN_SS: &str = "soldier/solpain3.wav";
const SOUND_DEATH_LIGHT: &str = "soldier/soldeth2.wav";
const SOUND_DEATH: &str = "soldier/soldeth1.wav";
const SOUND_DEATH_SS: &str = "soldier/soldeth3.wav";
const SOUND_COCK: &str = "infantry/infatck3.wav";
const SOUND_STEPS: [&str; 4] = [
    "player/step1.wav",
    "player/step2.wav",
    "player/step3.wav",
    "player/step4.wav",
];

// ---------------------------------------------------------------------------
// Moves. C ref: soldier.c `soldier_frames_*` / `soldier_move_*`, one entry
// per model frame in C order.
// ---------------------------------------------------------------------------

static SOLDIER_FRAMES_STAND1: [MonsterFrame; 30] = [
    frame(ai::ai_stand, 0.0, Some(soldier_idle)),
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
static SOLDIER_MOVE_STAND1: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND101,
    lastframe: FRAME_STAND130,
    frames: &SOLDIER_FRAMES_STAND1,
    endfunc: Some(soldier_stand),
};

static SOLDIER_FRAMES_STAND3: [MonsterFrame; 39] = [
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
    frame(ai::ai_stand, 0.0, Some(soldier_cock)),
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
static SOLDIER_MOVE_STAND3: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND301,
    lastframe: FRAME_STAND339,
    frames: &SOLDIER_FRAMES_STAND3,
    endfunc: Some(soldier_stand),
};

static SOLDIER_FRAMES_WALK1: [MonsterFrame; 33] = [
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 2.0, Some(soldier_footstep)),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 1.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 3.0, Some(soldier_footstep)),
    frame(ai::ai_walk, -1.0, Some(soldier_walk1_random)),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
];
static SOLDIER_MOVE_WALK1: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK101,
    lastframe: FRAME_WALK133,
    frames: &SOLDIER_FRAMES_WALK1,
    endfunc: None,
};

static SOLDIER_FRAMES_WALK2: [MonsterFrame; 10] = [
    frame(ai::ai_walk, 4.0, Some(soldier_footstep)),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, 8.0, None),
    frame(ai::ai_walk, 5.0, Some(soldier_footstep)),
    frame(ai::ai_walk, 1.0, None),
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 7.0, None),
];
static SOLDIER_MOVE_WALK2: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK209,
    lastframe: FRAME_WALK218,
    frames: &SOLDIER_FRAMES_WALK2,
    endfunc: None,
};

static SOLDIER_FRAMES_START_RUN: [MonsterFrame; 2] =
    [frame(ai::ai_run, 7.0, None), frame(ai::ai_run, 5.0, None)];
static SOLDIER_MOVE_START_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN01,
    lastframe: FRAME_RUN02,
    frames: &SOLDIER_FRAMES_START_RUN,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_RUN: [MonsterFrame; 6] = [
    frame(ai::ai_run, 10.0, None),
    frame(ai::ai_run, 11.0, Some(soldier_footstep)),
    frame(ai::ai_run, 11.0, None),
    frame(ai::ai_run, 16.0, None),
    frame(ai::ai_run, 10.0, Some(soldier_footstep)),
    frame(ai::ai_run, 15.0, None),
];
static SOLDIER_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN03,
    lastframe: FRAME_RUN08,
    frames: &SOLDIER_FRAMES_RUN,
    endfunc: None,
};

static SOLDIER_FRAMES_PAIN1: [MonsterFrame; 5] = [
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
];
static SOLDIER_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN105,
    frames: &SOLDIER_FRAMES_PAIN1,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_PAIN2: [MonsterFrame; 7] = [
    frame(ai::ai_move, -13.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
];
static SOLDIER_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN207,
    frames: &SOLDIER_FRAMES_PAIN2,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_PAIN3: [MonsterFrame; 18] = [
    frame(ai::ai_move, -8.0, None),
    frame(ai::ai_move, 10.0, None),
    frame(ai::ai_move, -4.0, Some(soldier_footstep)),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, Some(soldier_footstep)),
];
static SOLDIER_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN318,
    frames: &SOLDIER_FRAMES_PAIN3,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_PAIN4: [MonsterFrame; 17] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -10.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, 8.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
];
static SOLDIER_MOVE_PAIN4: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN401,
    lastframe: FRAME_PAIN417,
    frames: &SOLDIER_FRAMES_PAIN4,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_ATTACK1: [MonsterFrame; 12] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_fire1)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_attack1_refire1)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_cock)),
    frame(ai::ai_charge, 0.0, Some(soldier_attack1_refire2)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];
static SOLDIER_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK112,
    frames: &SOLDIER_FRAMES_ATTACK1,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_ATTACK2: [MonsterFrame; 18] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_fire2)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_attack2_refire1)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_cock)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_attack2_refire2)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];
static SOLDIER_MOVE_ATTACK2: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK218,
    frames: &SOLDIER_FRAMES_ATTACK2,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_ATTACK3: [MonsterFrame; 9] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_fire3)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_attack3_refire)),
    frame(ai::ai_charge, 0.0, Some(soldier_duck_up)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];
static SOLDIER_MOVE_ATTACK3: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK301,
    lastframe: FRAME_ATTAK309,
    frames: &SOLDIER_FRAMES_ATTACK3,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_ATTACK4: [MonsterFrame; 6] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_footstep)),
    frame(ai::ai_charge, 0.0, Some(soldier_fire4)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(soldier_footstep)),
];
static SOLDIER_MOVE_ATTACK4: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK401,
    lastframe: FRAME_ATTAK406,
    frames: &SOLDIER_FRAMES_ATTACK4,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_ATTACK6: [MonsterFrame; 14] = [
    frame(ai::ai_charge, 10.0, None),
    frame(ai::ai_charge, 4.0, None),
    frame(ai::ai_charge, 12.0, Some(soldier_footstep)),
    frame(ai::ai_charge, 11.0, Some(soldier_fire8)),
    frame(ai::ai_charge, 13.0, None),
    frame(ai::ai_charge, 18.0, None),
    frame(ai::ai_charge, 15.0, Some(soldier_footstep)),
    frame(ai::ai_charge, 14.0, None),
    frame(ai::ai_charge, 11.0, None),
    frame(ai::ai_charge, 8.0, Some(soldier_footstep)),
    frame(ai::ai_charge, 11.0, None),
    frame(ai::ai_charge, 12.0, None),
    frame(ai::ai_charge, 12.0, Some(soldier_footstep)),
    frame(ai::ai_charge, 17.0, Some(soldier_attack6_refire)),
];
static SOLDIER_MOVE_ATTACK6: MonsterMove = MonsterMove {
    firstframe: FRAME_RUNS01,
    lastframe: FRAME_RUNS14,
    frames: &SOLDIER_FRAMES_ATTACK6,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_DUCK: [MonsterFrame; 5] = [
    frame(ai::ai_move, 5.0, Some(soldier_duck_down)),
    frame(ai::ai_move, -1.0, Some(soldier_duck_hold)),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, Some(soldier_duck_up)),
    frame(ai::ai_move, 5.0, None),
];
static SOLDIER_MOVE_DUCK: MonsterMove = MonsterMove {
    firstframe: FRAME_DUCK01,
    lastframe: FRAME_DUCK05,
    frames: &SOLDIER_FRAMES_DUCK,
    endfunc: Some(soldier_run),
};

static SOLDIER_FRAMES_DEATH1: [MonsterFrame; 36] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -10.0, None),
    frame(ai::ai_move, -10.0, None),
    frame(ai::ai_move, -10.0, None),
    frame(ai::ai_move, -5.0, None),
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
    frame(ai::ai_move, 0.0, Some(soldier_fire6)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(soldier_fire7)),
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
static SOLDIER_MOVE_DEATH1: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH136,
    frames: &SOLDIER_FRAMES_DEATH1,
    endfunc: Some(soldier_dead),
};

static SOLDIER_FRAMES_DEATH2: [MonsterFrame; 35] = [
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -5.0, None),
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
static SOLDIER_MOVE_DEATH2: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH201,
    lastframe: FRAME_DEATH235,
    frames: &SOLDIER_FRAMES_DEATH2,
    endfunc: Some(soldier_dead),
};

static SOLDIER_FRAMES_DEATH3: [MonsterFrame; 45] = [
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -5.0, None),
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
static SOLDIER_MOVE_DEATH3: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH301,
    lastframe: FRAME_DEATH345,
    frames: &SOLDIER_FRAMES_DEATH3,
    endfunc: Some(soldier_dead),
};

static SOLDIER_FRAMES_DEATH4: [MonsterFrame; 53] = [frame(ai::ai_move, 0.0, None); 53];
static SOLDIER_MOVE_DEATH4: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH401,
    lastframe: FRAME_DEATH453,
    frames: &SOLDIER_FRAMES_DEATH4,
    endfunc: Some(soldier_dead),
};

static SOLDIER_FRAMES_DEATH5: [MonsterFrame; 24] = [
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -5.0, None),
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
static SOLDIER_MOVE_DEATH5: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH501,
    lastframe: FRAME_DEATH524,
    frames: &SOLDIER_FRAMES_DEATH5,
    endfunc: Some(soldier_dead),
};

static SOLDIER_FRAMES_DEATH6: [MonsterFrame; 10] = [frame(ai::ai_move, 0.0, None); 10];
static SOLDIER_MOVE_DEATH6: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH601,
    lastframe: FRAME_DEATH610,
    frames: &SOLDIER_FRAMES_DEATH6,
    endfunc: Some(soldier_dead),
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

/// C compares `currentmove` pointers; the tables are statics, so their
/// addresses identify a move.
fn is_current(mi: &MonsterInfo, mv: &MonsterMove) -> bool {
    mi.currentmove
        .as_ref()
        .is_some_and(|cm| std::ptr::eq(cm.frames, mv.frames))
}

fn set_nextframe(world: &mut GameWorld, key: EntityKey, frame: i32) {
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.nextframe = frame;
    }
}

fn skinnum(world: &GameWorld, key: EntityKey) -> i32 {
    world.entities.get(key).map_or(0, |e| e.state.skinnum)
}

/// The live enemy's key, or `None` when it is gone or dead: every C
/// refire check returns on `self->enemy->health <= 0`.
fn living_enemy(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
    let enemy = world.entities.get(key)?.game.enemy?;
    (world.entities.get(enemy)?.game.health > 0).then_some(enemy)
}

/// C `AI_HOLD_FRAME` until `pausetime`, then let the move go on.
fn hold_until_pausetime(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    if let Some(mi) = monsterinfo_mut(world, key) {
        if time >= mi.pausetime {
            mi.aiflags.remove(AiFlags::HOLD_FRAME);
        } else {
            mi.aiflags.insert(AiFlags::HOLD_FRAME);
        }
    }
}

// ---------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------

/// C ref: `soldier_footstep`; silent unless `g_monsterfootsteps` is set.
pub fn soldier_footstep(world: &mut GameWorld, key: EntityKey) {
    if !G_MONSTERFOOTSTEPS {
        return;
    }
    let step = SOUND_STEPS[(world.random_u32() % 4) as usize];
    sound(world, key, CHAN_BODY, step, ATTN_NORM);
}

/// C ref: `soldier_idle`.
pub fn soldier_idle(world: &mut GameWorld, key: EntityKey) {
    if world.random() > 0.8 {
        sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
    }
}

/// Quieter while idly standing (stand3) than in an attack.
/// C ref: `soldier_cock`.
pub fn soldier_cock(world: &mut GameWorld, key: EntityKey) {
    let standing = world
        .entities
        .get(key)
        .is_some_and(|e| e.state.frame == FRAME_STAND322);
    let attenuation = if standing { ATTN_IDLE } else { ATTN_NORM };
    sound(world, key, CHAN_WEAPON, SOUND_COCK, attenuation);
}

// ---------------------------------------------------------------------------
// Stand, walk, run
// ---------------------------------------------------------------------------

/// Stand3 (the gun cock) one time in five, never twice running.
/// C ref: `soldier_stand`.
pub fn soldier_stand(world: &mut GameWorld, key: EntityKey) {
    let from_stand3 =
        monsterinfo(world, key).is_some_and(|mi| is_current(mi, &SOLDIER_MOVE_STAND3));
    if from_stand3 || world.random() < 0.8 {
        set_move(world, key, &SOLDIER_MOVE_STAND1);
    } else {
        set_move(world, key, &SOLDIER_MOVE_STAND3);
    }
}

/// Loop the walking part of walk1 nine times in ten.
/// C ref: `soldier_walk1_random`.
pub fn soldier_walk1_random(world: &mut GameWorld, key: EntityKey) {
    if world.random() > 0.1 {
        set_nextframe(world, key, FRAME_WALK101);
    }
}

/// C ref: `soldier_walk`.
pub fn soldier_walk(world: &mut GameWorld, key: EntityKey) {
    if world.random() < 0.5 {
        set_move(world, key, &SOLDIER_MOVE_WALK1);
    } else {
        set_move(world, key, &SOLDIER_MOVE_WALK2);
    }
}

/// Start running, or keep running once started or coming from a walk.
/// C ref: `soldier_run`.
pub fn soldier_run(world: &mut GameWorld, key: EntityKey) {
    let Some(mi) = monsterinfo(world, key) else {
        return;
    };
    let next = if mi.aiflags.contains(AiFlags::STAND_GROUND) {
        &SOLDIER_MOVE_STAND1
    } else if [
        &SOLDIER_MOVE_WALK1,
        &SOLDIER_MOVE_WALK2,
        &SOLDIER_MOVE_START_RUN,
    ]
    .iter()
    .any(|mv| is_current(mi, mv))
    {
        &SOLDIER_MOVE_RUN
    } else {
        &SOLDIER_MOVE_START_RUN
    };
    set_move(world, key, next);
}

// ---------------------------------------------------------------------------
// Pain
// ---------------------------------------------------------------------------

/// Bloody skin below half health; a hit while flying up knocks the
/// soldier over (pain4), and during the debounce does so only while it
/// is already in pain1-3. C ref: `soldier_pain`.
pub fn soldier_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    _damage: i32,
) {
    let time = world.level.time;
    let skill = world.game.skill;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.health < ent.game.max_health / 2 {
        ent.state.skinnum |= 1;
    }
    let flung = ent.velocity.z > 100.0;

    if time < ent.game.pain_debounce_time {
        let in_light_pain = ent.monsterinfo.as_deref().is_some_and(|mi| {
            [
                &SOLDIER_MOVE_PAIN1,
                &SOLDIER_MOVE_PAIN2,
                &SOLDIER_MOVE_PAIN3,
            ]
            .iter()
            .any(|mv| is_current(mi, mv))
        });
        if flung && in_light_pain {
            set_move(world, key, &SOLDIER_MOVE_PAIN4);
        }
        return;
    }
    ent.game.pain_debounce_time = time + 3.0;

    let pain = match ent.state.skinnum | 1 {
        1 => SOUND_PAIN_LIGHT,
        3 => SOUND_PAIN,
        _ => SOUND_PAIN_SS,
    };
    sound(world, key, CHAN_VOICE, pain, ATTN_NORM);

    if flung {
        set_move(world, key, &SOLDIER_MOVE_PAIN4);
        return;
    }
    if skill == SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    let r = world.random();
    let mv = if r < 0.33 {
        &SOLDIER_MOVE_PAIN1
    } else if r < 0.66 {
        &SOLDIER_MOVE_PAIN2
    } else {
        &SOLDIER_MOVE_PAIN3
    };
    set_move(world, key, mv);
}

// ---------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------

/// C `blaster_flash` / `shotgun_flash` / `machinegun_flash`, indexed by
/// `soldier_fire`'s flash number.
const BLASTER_FLASH: [i32; 8] = [
    MZ2_SOLDIER_BLASTER_1,
    MZ2_SOLDIER_BLASTER_2,
    MZ2_SOLDIER_BLASTER_3,
    MZ2_SOLDIER_BLASTER_4,
    MZ2_SOLDIER_BLASTER_5,
    MZ2_SOLDIER_BLASTER_6,
    MZ2_SOLDIER_BLASTER_7,
    MZ2_SOLDIER_BLASTER_8,
];
const SHOTGUN_FLASH: [i32; 8] = [
    MZ2_SOLDIER_SHOTGUN_1,
    MZ2_SOLDIER_SHOTGUN_2,
    MZ2_SOLDIER_SHOTGUN_3,
    MZ2_SOLDIER_SHOTGUN_4,
    MZ2_SOLDIER_SHOTGUN_5,
    MZ2_SOLDIER_SHOTGUN_6,
    MZ2_SOLDIER_SHOTGUN_7,
    MZ2_SOLDIER_SHOTGUN_8,
];
const MACHINEGUN_FLASH: [i32; 8] = [
    MZ2_SOLDIER_MACHINEGUN_1,
    MZ2_SOLDIER_MACHINEGUN_2,
    MZ2_SOLDIER_MACHINEGUN_3,
    MZ2_SOLDIER_MACHINEGUN_4,
    MZ2_SOLDIER_MACHINEGUN_5,
    MZ2_SOLDIER_MACHINEGUN_6,
    MZ2_SOLDIER_MACHINEGUN_7,
    MZ2_SOLDIER_MACHINEGUN_8,
];

/// Fire the skin's weapon from muzzle `flash_number`: at the enemy's eyes
/// with a wide random spread, or straight ahead for the dying shots (5
/// and 6). The machinegun holds the frame for 3-10 frames of fire.
/// C ref: `soldier_fire`. C dereferences `self->enemy` for aimed shots;
/// without one there is nothing to aim at, so nothing fires.
/// Private: only this file's `soldier_fireN` wrappers call it, with
/// flash numbers 0-7, the length of the flash tables.
fn soldier_fire(world: &mut GameWorld, key: EntityKey, flash_number: usize) {
    let skin = skinnum(world, key);
    let flash_index = if skin < 2 {
        BLASTER_FLASH[flash_number]
    } else if skin < 4 {
        SHOTGUN_FLASH[flash_number]
    } else {
        MACHINEGUN_FLASH[flash_number]
    };
    let Some((start, forward, _)) = flash::monster_flash_source(world, key, flash_index) else {
        return;
    };

    let aim = if flash_number == 5 || flash_number == 6 {
        forward
    } else {
        let Some(enemy) = world
            .entities
            .get(key)
            .and_then(|e| e.game.enemy)
            .and_then(|e| world.entities.get(e))
        else {
            return;
        };
        let mut end = enemy.state.origin;
        end.z += enemy.game.viewheight;
        let dir = crate::weapons::vectoangles(end - start);
        let (forward, right, up) = crate::physics::angle_vectors(dir);
        let r = world.crandom() * 1000.0;
        let u = world.crandom() * 500.0;
        let end = start + forward * 8192.0 + right * r + up * u;
        (end - start).normalize_or_zero()
    };

    if skin <= 1 {
        fire::monster_fire_blaster(world, key, start, aim, 5, 600, flash_index, EF_BLASTER);
    } else if skin <= 3 {
        fire::monster_fire_shotgun(
            world,
            key,
            start,
            aim,
            2,
            1,
            DEFAULT_SHOTGUN_HSPREAD,
            DEFAULT_SHOTGUN_VSPREAD,
            DEFAULT_SHOTGUN_COUNT,
            flash_index,
        );
    } else {
        let holding =
            monsterinfo(world, key).is_some_and(|mi| mi.aiflags.contains(AiFlags::HOLD_FRAME));
        if !holding {
            let frames = 3 + world.random_u32() % 8;
            let time = world.level.time;
            if let Some(mi) = monsterinfo_mut(world, key) {
                mi.pausetime = time + frames as f32 * FRAMETIME;
            }
        }
        fire::monster_fire_bullet(
            world,
            key,
            start,
            aim,
            2,
            4,
            DEFAULT_BULLET_HSPREAD,
            DEFAULT_BULLET_VSPREAD,
            flash_index,
        );
        hold_until_pausetime(world, key);
    }
}

/// C ref: `soldier_fire1` (attack1, blaster/shotgun).
pub fn soldier_fire1(world: &mut GameWorld, key: EntityKey) {
    soldier_fire(world, key, 0);
}

/// Fire again on nightmare half the time, or always at melee range.
/// The C refire tests in order, so `random()` is only drawn on nightmare.
fn refire(world: &mut GameWorld, key: EntityKey, enemy: EntityKey) -> bool {
    (world.game.skill == SKILL_HARDPLUS && world.random() < 0.5)
        || ai::range(world, key, enemy) == Range::Melee
}

/// The blaster fires again or skips to the end; the shotgun goes on to
/// cock. C ref: `soldier_attack1_refire1`.
pub fn soldier_attack1_refire1(world: &mut GameWorld, key: EntityKey) {
    if skinnum(world, key) > 1 {
        return;
    }
    let Some(enemy) = living_enemy(world, key) else {
        return;
    };
    let next = if refire(world, key, enemy) {
        FRAME_ATTAK102
    } else {
        FRAME_ATTAK110
    };
    set_nextframe(world, key, next);
}

/// The shotgun, cocked, may fire again. C ref: `soldier_attack1_refire2`.
pub fn soldier_attack1_refire2(world: &mut GameWorld, key: EntityKey) {
    if skinnum(world, key) < 2 {
        return;
    }
    let Some(enemy) = living_enemy(world, key) else {
        return;
    };
    if refire(world, key, enemy) {
        set_nextframe(world, key, FRAME_ATTAK102);
    }
}

/// C ref: `soldier_fire2` (attack2, blaster/shotgun).
pub fn soldier_fire2(world: &mut GameWorld, key: EntityKey) {
    soldier_fire(world, key, 1);
}

/// C ref: `soldier_attack2_refire1`.
pub fn soldier_attack2_refire1(world: &mut GameWorld, key: EntityKey) {
    if skinnum(world, key) > 1 {
        return;
    }
    let Some(enemy) = living_enemy(world, key) else {
        return;
    };
    let next = if refire(world, key, enemy) {
        FRAME_ATTAK204
    } else {
        FRAME_ATTAK216
    };
    set_nextframe(world, key, next);
}

/// C ref: `soldier_attack2_refire2`.
pub fn soldier_attack2_refire2(world: &mut GameWorld, key: EntityKey) {
    if skinnum(world, key) < 2 {
        return;
    }
    let Some(enemy) = living_enemy(world, key) else {
        return;
    };
    if refire(world, key, enemy) {
        set_nextframe(world, key, FRAME_ATTAK204);
    }
}

/// Crouch: 32 units shorter and hittable but no longer aimed at, for a
/// second. C ref: `soldier_duck_down`.
pub fn soldier_duck_down(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let Some(mi) = ent.monsterinfo.as_deref_mut() else {
        return;
    };
    if mi.aiflags.contains(AiFlags::DUCKED) {
        return;
    }
    mi.aiflags.insert(AiFlags::DUCKED);
    mi.pausetime = time + 1.0;
    ent.maxs.z -= 32.0;
    ent.game.takedamage = TakeDamage::Yes;
    world.link_entity(key);
}

/// C ref: `soldier_duck_up`.
pub fn soldier_duck_up(world: &mut GameWorld, key: EntityKey) {
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

/// C ref: `soldier_fire3` (attack3, duck and shoot).
pub fn soldier_fire3(world: &mut GameWorld, key: EntityKey) {
    soldier_duck_down(world, key);
    soldier_fire(world, key, 2);
}

/// Keep shooting from the crouch while the duck lasts.
/// C ref: `soldier_attack3_refire`.
pub fn soldier_attack3_refire(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    if monsterinfo(world, key).is_some_and(|mi| time + 0.4 < mi.pausetime) {
        set_nextframe(world, key, FRAME_ATTAK303);
    }
}

/// C ref: `soldier_fire4` (attack4, machinegun).
pub fn soldier_fire4(world: &mut GameWorld, key: EntityKey) {
    soldier_fire(world, key, 3);
}

/// C ref: `soldier_fire8` (attack6, run and shoot).
pub fn soldier_fire8(world: &mut GameWorld, key: EntityKey) {
    soldier_fire(world, key, 7);
}

/// On nightmare, run and shoot again while the enemy is mid range or
/// further. C ref: `soldier_attack6_refire`.
pub fn soldier_attack6_refire(world: &mut GameWorld, key: EntityKey) {
    let Some(enemy) = living_enemy(world, key) else {
        return;
    };
    if (ai::range(world, key, enemy) as i32) < Range::Mid as i32 {
        return;
    }
    if world.game.skill == SKILL_HARDPLUS {
        set_nextframe(world, key, FRAME_RUNS03);
    }
}

/// Blaster and shotgun soldiers pick attack1 or attack2; the machinegun
/// one always attack4. C ref: `soldier_attack`.
pub fn soldier_attack(world: &mut GameWorld, key: EntityKey) {
    if skinnum(world, key) < 4 {
        if world.random() < 0.5 {
            set_move(world, key, &SOLDIER_MOVE_ATTACK1);
        } else {
            set_move(world, key, &SOLDIER_MOVE_ATTACK2);
        }
    } else {
        set_move(world, key, &SOLDIER_MOVE_ATTACK4);
    }
}

/// Shout, and above easy sometimes open fire on the run at a distant
/// enemy. C ref: `soldier_sight`.
pub fn soldier_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    let shout = if world.random() < 0.5 {
        SOUND_SIGHT1
    } else {
        SOUND_SIGHT2
    };
    sound(world, key, CHAN_VOICE, shout, ATTN_NORM);

    // C calls range(self, self->enemy) unchecked; with no enemy there
    // is nothing to range, so the port only shouts.
    let Some(enemy) = world.entities.get(key).and_then(|e| e.game.enemy) else {
        return;
    };
    if world.game.skill > SKILL_EASY
        && ai::range(world, key, enemy) as i32 >= Range::Mid as i32
        && world.random() > 0.5
    {
        set_move(world, key, &SOLDIER_MOVE_ATTACK6);
    }
}

// ---------------------------------------------------------------------------
// Dodging
// ---------------------------------------------------------------------------

/// C ref: `soldier_duck_hold`.
pub fn soldier_duck_hold(world: &mut GameWorld, key: EntityKey) {
    hold_until_pausetime(world, key);
}

/// One shot in four: duck, or duck and shoot back, more often shooting
/// on harder skills. C ref: `soldier_dodge`.
pub fn soldier_dodge(world: &mut GameWorld, key: EntityKey, attacker: EntityKey, eta: f32) {
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

    let skill = world.game.skill;
    if skill == SKILL_EASY {
        set_move(world, key, &SOLDIER_MOVE_DUCK);
        return;
    }

    let time = world.level.time;
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.pausetime = time + eta + 0.3;
    }
    let r = world.random();
    let duck_above = if skill == SKILL_MEDIUM {
        Some(0.33)
    } else if skill >= SKILL_HARD {
        Some(0.66)
    } else {
        None
    };
    match duck_above {
        Some(threshold) if r > threshold => set_move(world, key, &SOLDIER_MOVE_DUCK),
        _ => set_move(world, key, &SOLDIER_MOVE_ATTACK3),
    }
}

// ---------------------------------------------------------------------------
// Death
// ---------------------------------------------------------------------------

/// C ref: `soldier_fire6` (death1, a dying shot straight ahead).
pub fn soldier_fire6(world: &mut GameWorld, key: EntityKey) {
    soldier_fire(world, key, 5);
}

/// C ref: `soldier_fire7` (death1, the second dying shot).
pub fn soldier_fire7(world: &mut GameWorld, key: EntityKey) {
    soldier_fire(world, key, 6);
}

/// The corpse box, then stop. C ref: `soldier_dead`.
pub fn soldier_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// Gib when hit hard enough; otherwise a headshot falls with death3 and
/// any other hit with one of five deaths. The corpse stays damageable so
/// it can still be gibbed. C ref: `soldier_die`.
pub fn soldier_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    point: Vec3f,
) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.health <= ent.game.gib_health {
        sound(world, key, CHAN_VOICE, "misc/udeath.wav", ATTN_NORM);
        for _ in 0..3 {
            throw_gib(
                world,
                key,
                "models/objects/gibs/sm_meat/tris.md2",
                damage,
                GibType::Organic,
            );
        }
        throw_gib(
            world,
            key,
            "models/objects/gibs/chest/tris.md2",
            damage,
            GibType::Organic,
        );
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

    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.deadflag = DeadFlag::Dead;
    ent.game.takedamage = TakeDamage::Yes;
    ent.state.skinnum |= 1;
    let death = match ent.state.skinnum {
        1 => SOUND_DEATH_LIGHT,
        3 => SOUND_DEATH,
        _ => SOUND_DEATH_SS,
    };
    let head_shot = ((ent.state.origin.z + ent.game.viewheight) - point.z).abs() <= 4.0;
    sound(world, key, CHAN_VOICE, death, ATTN_NORM);

    if head_shot {
        set_move(world, key, &SOLDIER_MOVE_DEATH3);
        return;
    }
    let mv = match world.random_u32() % 5 {
        0 => &SOLDIER_MOVE_DEATH1,
        1 => &SOLDIER_MOVE_DEATH2,
        2 => &SOLDIER_MOVE_DEATH4,
        3 => &SOLDIER_MOVE_DEATH5,
        _ => &SOLDIER_MOVE_DEATH6,
    };
    set_move(world, key, mv);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_soldier_x` plus the
/// variant's health and skin. C ref: `SP_monster_soldier_x`.
fn spawn_soldier(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
    health: i32,
    skin: i32,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "monster_soldier".to_string());
        ent.game.health = health;
        ent.game.gib_health = -30;
        ent.game.max_health = health;
        ent.state.skinnum = skin;

        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
        ent.game.movetype = MoveType::Step;
        ent.solid = Solid::Bbox;
        ent.game.mass = SOLDIER_MASS;

        ent.monsterinfo = Some(Box::new(MonsterInfo {
            stand: Some(soldier_stand),
            walk: Some(soldier_walk),
            run: Some(soldier_run),
            dodge: Some(soldier_dodge),
            attack: Some(soldier_attack),
            melee: None,
            sight: Some(soldier_sight),
            // `finish_*` runs C's `monsterinfo.stand(self)`, which may
            // pick stand3 instead.
            currentmove: Some(SOLDIER_MOVE_STAND1.clone()),
            scale: MODEL_SCALE,
            ..Default::default()
        }));

        ent.pain = Some(soldier_pain);
        ent.die = Some(soldier_die);
        ai::walkmonster_start(ent);
    }
}

pub fn sp_monster_soldier_light(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_soldier(storage, key, fields, SOLDIER_LIGHT_HEALTH, 0);
}

pub fn sp_monster_soldier(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_soldier(storage, key, fields, SOLDIER_HEALTH, 2);
}

pub fn sp_monster_soldier_ss(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_soldier(storage, key, fields, SOLDIER_SS_HEALTH, 4);
}

/// The `GameWorld` half of C `SP_monster_soldier_x`: precache the shared
/// sounds and run `monsterinfo.stand`.
fn finish_soldier_x(world: &mut GameWorld, key: EntityKey, sounds: &[&str]) {
    for name in [SOUND_IDLE, SOUND_SIGHT1, SOUND_SIGHT2, SOUND_COCK] {
        world.gi.sound_index(name);
    }
    soldier_stand(world, key);
    for name in sounds.iter().chain(&SOUND_STEPS) {
        world.gi.sound_index(name);
    }
}

/// C ref: `SP_monster_soldier_light` (after `SP_monster_soldier_x`): its
/// pain and death sounds and the blaster bolt.
pub fn finish_soldier_light(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    finish_soldier_x(world, key, &[SOUND_PAIN_LIGHT, SOUND_DEATH_LIGHT]);
    world.gi.model_index("models/objects/laser/tris.md2");
    world.gi.sound_index("misc/lasfly.wav");
    world.gi.sound_index("soldier/solatck2.wav");
}

/// C ref: `SP_monster_soldier` (after `SP_monster_soldier_x`).
pub fn finish_soldier(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    finish_soldier_x(world, key, &[SOUND_PAIN, SOUND_DEATH]);
    world.gi.sound_index("soldier/solatck1.wav");
}

/// C ref: `SP_monster_soldier_ss` (after `SP_monster_soldier_x`).
pub fn finish_soldier_ss(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    finish_soldier_x(world, key, &[SOUND_PAIN_SS, SOUND_DEATH_SS]);
    world.gi.sound_index("soldier/solatck3.wav");
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    const NAMES: &[(ThinkFn, &str)] = &[
        (soldier_idle, "soldier_idle"),
        (soldier_stand, "soldier_stand"),
        (soldier_cock, "soldier_cock"),
        (soldier_footstep, "soldier_footstep"),
        (soldier_walk1_random, "soldier_walk1_random"),
        (soldier_run, "soldier_run"),
        (soldier_fire1, "soldier_fire1"),
        (soldier_attack1_refire1, "soldier_attack1_refire1"),
        (soldier_attack1_refire2, "soldier_attack1_refire2"),
        (soldier_fire2, "soldier_fire2"),
        (soldier_attack2_refire1, "soldier_attack2_refire1"),
        (soldier_attack2_refire2, "soldier_attack2_refire2"),
        (soldier_fire3, "soldier_fire3"),
        (soldier_attack3_refire, "soldier_attack3_refire"),
        (soldier_duck_up, "soldier_duck_up"),
        (soldier_fire4, "soldier_fire4"),
        (soldier_fire8, "soldier_fire8"),
        (soldier_attack6_refire, "soldier_attack6_refire"),
        (soldier_duck_down, "soldier_duck_down"),
        (soldier_duck_hold, "soldier_duck_hold"),
        (soldier_fire6, "soldier_fire6"),
        (soldier_fire7, "soldier_fire7"),
        (soldier_dead, "soldier_dead"),
    ];

    use crate::entity::ThinkFn;
    use crate::monster::test_support::{
        assert_moves_match_c, player_arena, seed, sounds, think_for, CMoveRow,
    };
    use crate::world::{test_world, test_world_recording, MockGameImport};
    use std::rc::Rc;

    #[test]
    fn soldier_spawns_with_correct_health() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let fields = HashMap::new();

        sp_monster_soldier(&mut world.entities, key, &fields);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.health, SOLDIER_HEALTH);
        assert!(ent.monsterinfo.is_some());
        assert!(ent.pain.is_some());
        assert!(ent.die.is_some());
    }

    #[test]
    fn soldier_variants_have_different_health() {
        let mut world = test_world();
        let empty = HashMap::new();

        let light = world.spawn().unwrap();
        sp_monster_soldier_light(&mut world.entities, light, &empty);

        let normal = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, normal, &empty);

        let ss = world.spawn().unwrap();
        sp_monster_soldier_ss(&mut world.entities, ss, &empty);

        assert_eq!(world.entities.get(light).unwrap().game.health, 20);
        assert_eq!(world.entities.get(normal).unwrap().game.health, 30);
        assert_eq!(world.entities.get(ss).unwrap().game.health, 40);
    }

    #[test]
    fn soldier_stand_sets_animation() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());
        seed(&mut world, |w| w.random() < 0.8);

        soldier_stand(&mut world, key);

        let cm = world
            .entities
            .get(key)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap()
            .currentmove
            .as_ref()
            .unwrap();
        assert_eq!(cm.firstframe, FRAME_STAND101);
        assert_eq!(cm.lastframe, FRAME_STAND130);
    }

    fn current_move(world: &GameWorld, key: EntityKey) -> MonsterMove {
        let ent = world.entities.get(key).unwrap();
        ent.monsterinfo
            .as_ref()
            .unwrap()
            .currentmove
            .clone()
            .unwrap()
    }

    #[test]
    fn every_soldier_move_matches_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&SOLDIER_MOVE_STAND1, 146, 175, "soldier_stand", "stand:0:soldier_idle stand:0:-*29"),
            (&SOLDIER_MOVE_STAND3, 176, 214, "soldier_stand", "stand:0:-*21 stand:0:soldier_cock stand:0:-*17"),
            (&SOLDIER_MOVE_WALK1, 215, 247, "-", "walk:3:- walk:6:- walk:2:- walk:2:soldier_footstep walk:2:- walk:1:- walk:6:- walk:5:- walk:3:soldier_footstep walk:-1:soldier_walk1_random walk:0:-*23"),
            (&SOLDIER_MOVE_WALK2, 256, 265, "-", "walk:4:soldier_footstep walk:4:- walk:9:- walk:8:- walk:5:soldier_footstep walk:1:- walk:3:- walk:7:- walk:6:- walk:7:-"),
            (&SOLDIER_MOVE_START_RUN, 97, 98, "soldier_run", "run:7:- run:5:-"),
            (&SOLDIER_MOVE_RUN, 99, 104, "-", "run:10:- run:11:soldier_footstep run:11:- run:16:- run:10:soldier_footstep run:15:-"),
            (&SOLDIER_MOVE_PAIN1, 50, 54, "soldier_run", "move:-3:- move:4:- move:1:-*2 move:0:-"),
            (&SOLDIER_MOVE_PAIN2, 55, 61, "soldier_run", "move:-13:- move:-1:- move:2:- move:4:- move:2:- move:3:- move:2:-"),
            (&SOLDIER_MOVE_PAIN3, 62, 79, "soldier_run", "move:-8:- move:10:- move:-4:soldier_footstep move:-1:- move:-3:- move:0:- move:3:- move:0:-*4 move:1:- move:0:- move:1:- move:2:- move:4:- move:3:- move:2:soldier_footstep"),
            (&SOLDIER_MOVE_PAIN4, 80, 96, "soldier_run", "move:0:-*3 move:-10:- move:-6:- move:8:- move:4:- move:1:- move:0:- move:2:- move:5:- move:2:- move:-1:-*2 move:3:- move:2:- move:0:-"),
            (&SOLDIER_MOVE_ATTACK1, 0, 11, "soldier_run", "charge:0:-*2 charge:0:soldier_fire1 charge:0:-*2 charge:0:soldier_attack1_refire1 charge:0:- charge:0:soldier_cock charge:0:soldier_attack1_refire2 charge:0:-*3"),
            (&SOLDIER_MOVE_ATTACK2, 12, 29, "soldier_run", "charge:0:-*4 charge:0:soldier_fire2 charge:0:-*2 charge:0:soldier_attack2_refire1 charge:0:-*4 charge:0:soldier_cock charge:0:- charge:0:soldier_attack2_refire2 charge:0:-*3"),
            (&SOLDIER_MOVE_ATTACK3, 30, 38, "soldier_run", "charge:0:-*2 charge:0:soldier_fire3 charge:0:-*2 charge:0:soldier_attack3_refire charge:0:soldier_duck_up charge:0:-*2"),
            (&SOLDIER_MOVE_ATTACK4, 39, 44, "soldier_run", "charge:0:- charge:0:soldier_footstep charge:0:soldier_fire4 charge:0:-*2 charge:0:soldier_footstep"),
            (&SOLDIER_MOVE_ATTACK6, 109, 122, "soldier_run", "charge:10:- charge:4:- charge:12:soldier_footstep charge:11:soldier_fire8 charge:13:- charge:18:- charge:15:soldier_footstep charge:14:- charge:11:- charge:8:soldier_footstep charge:11:- charge:12:- charge:12:soldier_footstep charge:17:soldier_attack6_refire"),
            (&SOLDIER_MOVE_DUCK, 45, 49, "soldier_run", "move:5:soldier_duck_down move:-1:soldier_duck_hold move:1:- move:0:soldier_duck_up move:5:-"),
            (&SOLDIER_MOVE_DEATH1, 272, 307, "soldier_dead", "move:0:- move:-10:-*3 move:-5:- move:0:-*16 move:0:soldier_fire6 move:0:-*2 move:0:soldier_fire7 move:0:-*11"),
            (&SOLDIER_MOVE_DEATH2, 308, 342, "soldier_dead", "move:-5:-*3 move:0:-*32"),
            (&SOLDIER_MOVE_DEATH3, 343, 387, "soldier_dead", "move:-5:-*3 move:0:-*42"),
            (&SOLDIER_MOVE_DEATH4, 388, 440, "soldier_dead", "move:0:-*53"),
            (&SOLDIER_MOVE_DEATH5, 441, 464, "soldier_dead", "move:-5:-*3 move:0:-*21"),
            (&SOLDIER_MOVE_DEATH6, 465, 474, "soldier_dead", "move:0:-*10"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    #[test]
    fn soldier_spawns_with_the_c_model_scale() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());
        let mi = world
            .entities
            .get(key)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap();
        assert_eq!(mi.scale, 1.2);
    }

    #[test]
    fn soldier_run_from_stand_starts_with_start_run() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        soldier_run(&mut world, key);

        assert_eq!(current_move(&world, key).firstframe, FRAME_RUN01);
    }

    #[test]
    fn soldier_run_after_walk_or_start_run_goes_straight_to_run() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        for walk in [
            &SOLDIER_MOVE_WALK1,
            &SOLDIER_MOVE_WALK2,
            &SOLDIER_MOVE_START_RUN,
        ] {
            set_move(&mut world, key, walk);
            soldier_run(&mut world, key);
            assert_eq!(current_move(&world, key).firstframe, FRAME_RUN03);
        }
    }

    #[test]
    fn soldier_run_with_stand_ground_stands() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());
        soldier_walk(&mut world, key);
        let ent = world.entities.get_mut(key).unwrap();
        ent.monsterinfo.as_mut().unwrap().aiflags = AiFlags::STAND_GROUND;

        soldier_run(&mut world, key);

        assert_eq!(current_move(&world, key).firstframe, FRAME_STAND101);
    }

    #[test]
    fn soldier_walk1_random_loops_the_walk_nine_times_in_ten() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        let mut looped = 0;
        for _ in 0..1000 {
            let mi = world.entities.get_mut(key).unwrap().monsterinfo.as_mut();
            mi.unwrap().nextframe = 0;
            soldier_walk1_random(&mut world, key);
            match world
                .entities
                .get(key)
                .unwrap()
                .monsterinfo
                .as_ref()
                .unwrap()
                .nextframe
            {
                0 => {}
                FRAME_WALK101 => looped += 1,
                other => panic!("nextframe {other}"),
            }
        }
        assert!((850..950).contains(&looped), "{looped}");
    }

    /// C `soldier_die` keeps the corpse solid and damageable (so it can
    /// be gibbed); `soldier_dead` marks it a dead monster when the death
    /// move ends.
    #[test]
    fn soldier_die_sets_dead_state() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        let attacker = world.spawn().unwrap();
        soldier_die(&mut world, key, attacker, attacker, 100, Vec3f::ZERO);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert_eq!(ent.solid, Solid::Bbox);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() == 0);

        soldier_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
    }

    #[test]
    fn soldier_full_lifecycle() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        // Initialize.
        ai::monster_start(&mut world, key);
        assert!(world.entities.get(key).unwrap().think.is_some());

        // Run one AI frame (think → monster_move_frame → advance animation).
        // The spawn frame 0 is outside stand1, so the first tick snaps to it.
        ai::monster_think(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().state.frame, FRAME_STAND101);

        // Run several more frames.
        for _ in 0..5 {
            ai::monster_think(&mut world, key);
        }
        assert_eq!(
            world.entities.get(key).unwrap().state.frame,
            FRAME_STAND101 + 5
        );
    }

    // -- callbacks ----------------------------------------------------------

    /// A soldier with `skin` at the origin facing +X, its live enemy at
    /// (x, 0, 0), at level time 5.
    fn duel(skin: i32, x: f32) -> (GameWorld, Rc<MockGameImport>, EntityKey, EntityKey) {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());
        let enemy = world.spawn().unwrap();
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = Vec3f::new(x, 0.0, 0.0);
        e.game.health = 100;
        e.game.viewheight = 22.0;
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.skinnum = skin;
        ent.game.enemy = Some(enemy);
        ent.game.viewheight = 25.0;
        world.level.time = 5.0;
        (world, gi, key, enemy)
    }

    fn mi(world: &GameWorld, key: EntityKey) -> &MonsterInfo {
        monsterinfo(world, key).unwrap()
    }

    fn mi_mut(world: &mut GameWorld, key: EntityKey) -> &mut MonsterInfo {
        monsterinfo_mut(world, key).unwrap()
    }

    fn is(world: &GameWorld, key: EntityKey, mv: &MonsterMove) -> bool {
        is_current(mi(world, key), mv)
    }

    fn voice(name: &str) -> (i32, String, f32) {
        (CHAN_VOICE, name.to_string(), ATTN_NORM)
    }

    #[test]
    fn soldier_stand_from_stand3_always_returns_to_stand1() {
        let (mut world, _, key, _) = duel(2, 300.0);
        set_move(&mut world, key, &SOLDIER_MOVE_STAND3);
        seed(&mut world, |w| w.random() >= 0.8);
        soldier_stand(&mut world, key);
        assert!(is(&world, key, &SOLDIER_MOVE_STAND1));
    }

    #[test]
    fn soldier_stand_picks_stand3_one_time_in_five() {
        let (mut world, _, key, _) = duel(2, 300.0);
        seed(&mut world, |w| w.random() >= 0.8);
        soldier_stand(&mut world, key);
        assert!(is(&world, key, &SOLDIER_MOVE_STAND3));
    }

    #[test]
    fn soldier_walk_picks_walk1_or_walk2_evenly() {
        let (mut world, _, key, _) = duel(2, 300.0);
        seed(&mut world, |w| w.random() < 0.5);
        soldier_walk(&mut world, key);
        assert!(is(&world, key, &SOLDIER_MOVE_WALK1));
        seed(&mut world, |w| w.random() >= 0.5);
        soldier_walk(&mut world, key);
        assert!(is(&world, key, &SOLDIER_MOVE_WALK2));
    }

    #[test]
    fn soldier_idle_mutters_one_time_in_five() {
        let (mut world, gi, key, _) = duel(2, 300.0);
        seed(&mut world, |w| w.random() <= 0.8);
        soldier_idle(&mut world, key);
        assert!(sounds(&gi).is_empty());
        seed(&mut world, |w| w.random() > 0.8);
        soldier_idle(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [(CHAN_VOICE, SOUND_IDLE.to_string(), ATTN_IDLE)]
        );
    }

    #[test]
    fn soldier_cock_is_idle_quiet_only_on_stand322() {
        let (mut world, gi, key, _) = duel(2, 300.0);
        world.entities.get_mut(key).unwrap().state.frame = FRAME_STAND322;
        soldier_cock(&mut world, key);
        world.entities.get_mut(key).unwrap().state.frame = 7;
        soldier_cock(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [
                (CHAN_WEAPON, SOUND_COCK.to_string(), ATTN_IDLE),
                (CHAN_WEAPON, SOUND_COCK.to_string(), ATTN_NORM)
            ]
        );
    }

    #[test]
    fn soldier_footstep_is_silent_with_the_default_footsteps_cvar() {
        let (mut world, gi, key, _) = duel(2, 300.0);
        let rand = world.level.rand_state;
        soldier_footstep(&mut world, key);
        assert!(sounds(&gi).is_empty());
        assert_eq!(world.level.rand_state, rand, "no randk() drawn");
    }

    #[test]
    fn soldier_pain_bloodies_the_skin_below_half_health() {
        let (mut world, _, key, enemy) = duel(2, 300.0);
        world.entities.get_mut(key).unwrap().game.health = 15;
        soldier_pain(&mut world, key, enemy, 0.0, 15);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 2);
        world.entities.get_mut(key).unwrap().game.health = 14;
        soldier_pain(&mut world, key, enemy, 0.0, 1);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 3);
    }

    #[test]
    fn soldier_pain_plays_the_variant_sound_and_starts_a_three_second_debounce() {
        for (skin, name) in [(0, SOUND_PAIN_LIGHT), (2, SOUND_PAIN), (4, SOUND_PAIN_SS)] {
            let (mut world, gi, key, enemy) = duel(skin, 300.0);
            soldier_pain(&mut world, key, enemy, 0.0, 5);
            assert_eq!(sounds(&gi), [voice(name)], "skin {skin}");
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                8.0
            );
        }
    }

    #[test]
    fn soldier_pain_during_the_debounce_is_silent_and_keeps_the_move() {
        let (mut world, gi, key, enemy) = duel(2, 300.0);
        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 6.0;
        set_move(&mut world, key, &SOLDIER_MOVE_PAIN1);
        soldier_pain(&mut world, key, enemy, 0.0, 5);
        assert!(sounds(&gi).is_empty());
        assert!(is(&world, key, &SOLDIER_MOVE_PAIN1));
    }

    #[test]
    fn soldier_pain_during_the_debounce_knocks_a_flung_soldier_out_of_light_pain() {
        for (mv, falls) in [
            (&SOLDIER_MOVE_PAIN1, true),
            (&SOLDIER_MOVE_PAIN2, true),
            (&SOLDIER_MOVE_PAIN3, true),
            (&SOLDIER_MOVE_RUN, false),
        ] {
            let (mut world, gi, key, enemy) = duel(2, 300.0);
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.pain_debounce_time = 6.0;
            ent.velocity.z = 150.0;
            set_move(&mut world, key, mv);
            soldier_pain(&mut world, key, enemy, 0.0, 5);
            assert_eq!(
                is(&world, key, &SOLDIER_MOVE_PAIN4),
                falls,
                "{}",
                mv.firstframe
            );
            assert!(sounds(&gi).is_empty());
        }
    }

    #[test]
    fn soldier_pain_flung_upward_falls_over_even_on_nightmare() {
        let (mut world, _, key, enemy) = duel(2, 300.0);
        world.game.skill = SKILL_HARDPLUS;
        world.entities.get_mut(key).unwrap().velocity.z = 101.0;
        soldier_pain(&mut world, key, enemy, 0.0, 5);
        assert!(is(&world, key, &SOLDIER_MOVE_PAIN4));
    }

    #[test]
    fn soldier_pain_on_nightmare_only_cries_out() {
        let (mut world, gi, key, enemy) = duel(2, 300.0);
        world.game.skill = SKILL_HARDPLUS;
        soldier_pain(&mut world, key, enemy, 0.0, 5);
        assert!(is(&world, key, &SOLDIER_MOVE_STAND1));
        assert_eq!(sounds(&gi), [voice(SOUND_PAIN)]);
    }

    #[test]
    fn soldier_pain_picks_pain1_2_or_3_by_thirds() {
        for (lo, hi, mv) in [
            (0.0, 0.33, &SOLDIER_MOVE_PAIN1),
            (0.33, 0.66, &SOLDIER_MOVE_PAIN2),
            (0.66, 1.01, &SOLDIER_MOVE_PAIN3),
        ] {
            let (mut world, _, key, enemy) = duel(2, 300.0);
            seed(&mut world, |w| (lo..hi).contains(&w.random()));
            soldier_pain(&mut world, key, enemy, 0.0, 5);
            assert!(is(&world, key, mv), "{lo}");
        }
    }

    fn bolt(world: &GameWorld) -> Option<crate::entity::Entity> {
        let key = world.find_by_classname(None, "bolt")?;
        world.entities.get(key).cloned()
    }

    #[test]
    fn soldier_fire_blaster_shoots_a_bolt_toward_the_enemy() {
        for skin in [0, 1] {
            let (mut world, _, key, _) = duel(skin, 300.0);
            soldier_fire1(&mut world, key);
            let bolt = bolt(&world).expect("a blaster bolt");
            assert_eq!(bolt.game.dmg, 5);
            assert!((bolt.velocity.length() - 600.0).abs() < 0.5);
            assert!(bolt.velocity.x > 0.0);
            assert_eq!(bolt.owner, Some(key));
            assert_ne!(bolt.state.effects & EF_BLASTER, 0);
        }
    }

    #[test]
    fn soldier_fire_shotgun_fires_twelve_pellets() {
        for skin in [2, 3] {
            let (mut world, gi, key, _) = duel(skin, 300.0);
            soldier_fire2(&mut world, key);
            // Each pellet: the muzzle check, then the shot.
            assert_eq!(gi.trace_masks.borrow().len(), 2 * 12);
            assert!(bolt(&world).is_none());
        }
    }

    #[test]
    fn soldier_fire_machinegun_holds_the_frame_for_3_to_10_frames() {
        let (mut world, gi, key, _) = duel(5, 300.0);
        soldier_fire4(&mut world, key);
        let pause = mi(&world, key).pausetime;
        assert!((5.3 - 0.001..=6.0 + 0.001).contains(&pause), "{pause}");
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        assert_eq!(gi.trace_masks.borrow().len(), 2, "one bullet");

        // Held: no new burst length; past it, the move goes on.
        world.level.time = pause;
        soldier_fire4(&mut world, key);
        assert_eq!(mi(&world, key).pausetime, pause);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
    }

    #[test]
    fn soldier_dying_shots_fire_straight_ahead_without_an_enemy() {
        for shot in [soldier_fire6 as ThinkFn, soldier_fire7] {
            let (mut world, _, key, _) = duel(0, 300.0);
            world.entities.get_mut(key).unwrap().game.enemy = None;
            shot(&mut world, key);
            let v = bolt(&world).expect("a bolt").velocity;
            assert!((v - Vec3f::new(600.0, 0.0, 0.0)).length() < 0.01, "{v}");
        }
    }

    #[test]
    fn soldier_fire_picks_the_skins_muzzle_flash_by_flash_number() {
        // C soldier_fire: blaster_flash / shotgun_flash / machinegun_flash
        // by skin, indexed by the caller's flash number.
        for (skin, shot, flash) in [
            (0, soldier_fire1 as ThinkFn, MZ2_SOLDIER_BLASTER_1),
            (1, soldier_fire6, MZ2_SOLDIER_BLASTER_6),
            (2, soldier_fire3, MZ2_SOLDIER_SHOTGUN_3),
            (3, soldier_fire2, MZ2_SOLDIER_SHOTGUN_2),
            (4, soldier_fire8, MZ2_SOLDIER_MACHINEGUN_8),
            (5, soldier_fire7, MZ2_SOLDIER_MACHINEGUN_7),
        ] {
            let (mut world, gi) = crate::misc::wire::wire_world();
            let key = world.spawn().unwrap();
            sp_monster_soldier(&mut world.entities, key, &HashMap::new());
            let enemy = world.spawn().unwrap();
            world.entities.get_mut(enemy).unwrap().state.origin = Vec3f::new(300.0, 0.0, 0.0);
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.skinnum = skin;
            ent.game.enemy = Some(enemy);
            shot(&mut world, key);
            let wire = gi.wire.borrow();
            assert_eq!(
                wire[wire.len() - 2],
                crate::misc::wire::Wire::Byte(flash),
                "skin {skin}"
            );
        }
    }

    #[test]
    fn soldier_aimed_shot_without_an_enemy_fires_nothing() {
        let (mut world, _, key, _) = duel(0, 300.0);
        world.entities.get_mut(key).unwrap().game.enemy = None;
        soldier_fire1(&mut world, key);
        assert!(bolt(&world).is_none());
    }

    fn nextframe(world: &GameWorld, key: EntityKey) -> i32 {
        mi(world, key).nextframe
    }

    #[test]
    fn soldier_blaster_refire1_fires_again_in_melee_range_else_skips_to_the_end() {
        for (refire1, again, end) in [
            (
                soldier_attack1_refire1 as ThinkFn,
                FRAME_ATTAK102,
                FRAME_ATTAK110,
            ),
            (soldier_attack2_refire1, FRAME_ATTAK204, FRAME_ATTAK216),
        ] {
            let (mut world, _, key, _) = duel(1, 50.0);
            refire1(&mut world, key);
            assert_eq!(nextframe(&world, key), again);

            let (mut world, _, key, _) = duel(0, 300.0);
            let rand = world.level.rand_state;
            refire1(&mut world, key);
            assert_eq!(nextframe(&world, key), end);
            assert_eq!(world.level.rand_state, rand, "random() only on nightmare");
        }
    }

    #[test]
    fn soldier_blaster_refire1_on_nightmare_fires_again_half_the_time() {
        for (refire1, again, end) in [
            (
                soldier_attack1_refire1 as ThinkFn,
                FRAME_ATTAK102,
                FRAME_ATTAK110,
            ),
            (soldier_attack2_refire1, FRAME_ATTAK204, FRAME_ATTAK216),
        ] {
            for (lo, hi, want) in [(0.0, 0.5, again), (0.5, 1.01, end)] {
                let (mut world, _, key, _) = duel(0, 300.0);
                world.game.skill = SKILL_HARDPLUS;
                seed(&mut world, |w| (lo..hi).contains(&w.random()));
                refire1(&mut world, key);
                assert_eq!(nextframe(&world, key), want);
            }
        }
    }

    #[test]
    fn soldier_refire1_skips_shotgun_soldiers_and_dead_enemies() {
        for refire1 in [soldier_attack1_refire1 as ThinkFn, soldier_attack2_refire1] {
            let (mut world, _, key, _) = duel(2, 50.0);
            refire1(&mut world, key);
            assert_eq!(nextframe(&world, key), 0);

            let (mut world, _, key, enemy) = duel(0, 50.0);
            world.entities.get_mut(enemy).unwrap().game.health = 0;
            refire1(&mut world, key);
            assert_eq!(nextframe(&world, key), 0);
        }
    }

    #[test]
    fn soldier_shotgun_refire2_fires_again_only_in_melee_range() {
        for (refire2, again) in [
            (soldier_attack1_refire2 as ThinkFn, FRAME_ATTAK102),
            (soldier_attack2_refire2, FRAME_ATTAK204),
        ] {
            let (mut world, _, key, _) = duel(3, 50.0);
            refire2(&mut world, key);
            assert_eq!(nextframe(&world, key), again);

            let (mut world, _, key, _) = duel(2, 300.0);
            refire2(&mut world, key);
            assert_eq!(nextframe(&world, key), 0);

            let (mut world, _, key, _) = duel(2, 300.0);
            world.game.skill = SKILL_HARDPLUS;
            seed(&mut world, |w| w.random() < 0.5);
            refire2(&mut world, key);
            assert_eq!(nextframe(&world, key), again, "nightmare");

            let (mut world, _, key, _) = duel(1, 50.0);
            refire2(&mut world, key);
            assert_eq!(nextframe(&world, key), 0, "blaster");

            let (mut world, _, key, enemy) = duel(2, 50.0);
            world.entities.get_mut(enemy).unwrap().game.health = -1;
            refire2(&mut world, key);
            assert_eq!(nextframe(&world, key), 0, "dead enemy");
        }
    }

    #[test]
    fn soldier_duck_down_crouches_once_for_a_second() {
        let (mut world, _, key, _) = duel(0, 300.0);
        world.entities.get_mut(key).unwrap().game.takedamage = TakeDamage::Aim;
        soldier_duck_down(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 0.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert!(mi(&world, key).aiflags.contains(AiFlags::DUCKED));
        assert_eq!(mi(&world, key).pausetime, 6.0);

        world.level.time = 5.5;
        soldier_duck_down(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().maxs.z, 0.0);
        assert_eq!(mi(&world, key).pausetime, 6.0);
    }

    #[test]
    fn soldier_duck_up_stands_back_up() {
        let (mut world, _, key, _) = duel(0, 300.0);
        soldier_duck_down(&mut world, key);
        soldier_duck_up(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 32.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Aim);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::DUCKED));
    }

    #[test]
    fn soldier_fire3_ducks_and_shoots() {
        let (mut world, _, key, _) = duel(0, 300.0);
        soldier_fire3(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::DUCKED));
        assert!(bolt(&world).is_some());
    }

    #[test]
    fn soldier_attack3_refire_shoots_again_while_the_duck_lasts() {
        let (mut world, _, key, _) = duel(0, 300.0);
        mi_mut(&mut world, key).pausetime = 5.41;
        soldier_attack3_refire(&mut world, key);
        assert_eq!(nextframe(&world, key), FRAME_ATTAK303);

        mi_mut(&mut world, key).nextframe = 0;
        mi_mut(&mut world, key).pausetime = 5.4;
        soldier_attack3_refire(&mut world, key);
        assert_eq!(nextframe(&world, key), 0);
    }

    #[test]
    fn soldier_duck_hold_holds_the_frame_until_pausetime() {
        let (mut world, _, key, _) = duel(0, 300.0);
        mi_mut(&mut world, key).pausetime = 5.1;
        soldier_duck_hold(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        world.level.time = 5.1;
        soldier_duck_hold(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
    }

    #[test]
    fn soldier_attack6_refire_on_nightmare_runs_and_shoots_again_at_mid_range() {
        let (mut world, _, key, _) = duel(0, 600.0);
        world.game.skill = SKILL_HARDPLUS;
        soldier_attack6_refire(&mut world, key);
        assert_eq!(nextframe(&world, key), FRAME_RUNS03);

        let (mut world, _, key, _) = duel(0, 600.0);
        world.game.skill = SKILL_HARD;
        soldier_attack6_refire(&mut world, key);
        assert_eq!(nextframe(&world, key), 0, "hard");

        let (mut world, _, key, _) = duel(0, 300.0);
        world.game.skill = SKILL_HARDPLUS;
        soldier_attack6_refire(&mut world, key);
        assert_eq!(nextframe(&world, key), 0, "near");

        let (mut world, _, key, enemy) = duel(0, 600.0);
        world.game.skill = SKILL_HARDPLUS;
        world.entities.get_mut(enemy).unwrap().game.health = 0;
        soldier_attack6_refire(&mut world, key);
        assert_eq!(nextframe(&world, key), 0, "dead enemy");
    }

    #[test]
    fn soldier_attack_picks_attack1_or_2_unless_it_has_the_machinegun() {
        for skin in [0, 1, 2, 3] {
            let (mut world, _, key, _) = duel(skin, 300.0);
            seed(&mut world, |w| w.random() < 0.5);
            soldier_attack(&mut world, key);
            assert!(is(&world, key, &SOLDIER_MOVE_ATTACK1), "skin {skin}");
            seed(&mut world, |w| w.random() >= 0.5);
            soldier_attack(&mut world, key);
            assert!(is(&world, key, &SOLDIER_MOVE_ATTACK2), "skin {skin}");
        }
        for skin in [4, 5] {
            let (mut world, _, key, _) = duel(skin, 300.0);
            soldier_attack(&mut world, key);
            assert!(is(&world, key, &SOLDIER_MOVE_ATTACK4), "skin {skin}");
        }
    }

    #[test]
    fn soldier_sight_shouts_one_of_two_lines() {
        for (lo, hi, name) in [(0.0, 0.5, SOUND_SIGHT1), (0.5, 1.01, SOUND_SIGHT2)] {
            let (mut world, gi, key, enemy) = duel(0, 600.0);
            world.game.skill = SKILL_EASY;
            seed(&mut world, |w| (lo..hi).contains(&w.random()));
            soldier_sight(&mut world, key, enemy);
            assert_eq!(sounds(&gi), [voice(name)]);
            assert!(is(&world, key, &SOLDIER_MOVE_STAND1), "easy: no attack6");
        }
    }

    #[test]
    fn soldier_sight_above_easy_runs_and_shoots_half_the_time_at_mid_range() {
        let (mut world, _, key, enemy) = duel(0, 600.0);
        world.game.skill = 1;
        seed(&mut world, |w| {
            w.random();
            w.random() > 0.5
        });
        soldier_sight(&mut world, key, enemy);
        assert!(is(&world, key, &SOLDIER_MOVE_ATTACK6));

        let (mut world, _, key, enemy) = duel(0, 600.0);
        world.game.skill = 1;
        seed(&mut world, |w| {
            w.random();
            w.random() <= 0.5
        });
        soldier_sight(&mut world, key, enemy);
        assert!(is(&world, key, &SOLDIER_MOVE_STAND1));

        let (mut world, _, key, enemy) = duel(0, 300.0);
        world.game.skill = SKILL_HARDPLUS;
        seed(&mut world, |w| {
            w.random();
            w.random() > 0.5
        });
        soldier_sight(&mut world, key, enemy);
        assert!(is(&world, key, &SOLDIER_MOVE_STAND1), "near");
    }

    #[test]
    fn soldier_dodge_ignores_three_shots_in_four() {
        let (mut world, _, key, _) = duel(0, 300.0);
        let attacker = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.enemy = None;
        seed(&mut world, |w| w.random() > 0.25);
        soldier_dodge(&mut world, key, attacker, 0.5);
        assert!(is(&world, key, &SOLDIER_MOVE_STAND1));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, None);
    }

    #[test]
    fn soldier_dodge_on_easy_ducks_and_turns_on_an_unseen_attacker() {
        let (mut world, _, key, enemy) = duel(0, 300.0);
        world.game.skill = SKILL_EASY;
        world.entities.get_mut(key).unwrap().game.enemy = None;
        seed(&mut world, |w| w.random() <= 0.25);
        soldier_dodge(&mut world, key, enemy, 0.5);
        assert!(is(&world, key, &SOLDIER_MOVE_DUCK));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(enemy));
        assert_eq!(mi(&world, key).pausetime, 0.0, "easy sets no pausetime");
    }

    #[test]
    fn soldier_dodge_ducks_or_shoots_from_a_crouch_by_skill() {
        for (skill, duck_above) in [(1, 0.33), (2, 0.66), (3, 0.66)] {
            for (lo, hi, mv) in [
                (duck_above + 0.001, 1.01, &SOLDIER_MOVE_DUCK),
                (0.0, duck_above, &SOLDIER_MOVE_ATTACK3),
            ] {
                let (mut world, _, key, enemy) = duel(0, 300.0);
                world.game.skill = skill;
                seed(&mut world, |w| {
                    w.random() <= 0.25 && (lo..hi).contains(&w.random())
                });
                soldier_dodge(&mut world, key, enemy, 0.5);
                assert!(is(&world, key, mv), "skill {skill} {lo}");
                assert!((mi(&world, key).pausetime - 5.8).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn soldier_die_gibs_at_minus_30_health() {
        let (mut world, gi, key, enemy) = duel(2, 300.0);
        world.entities.get_mut(key).unwrap().game.health = -30;
        let before = world.entities.count();
        soldier_die(&mut world, key, enemy, enemy, 50, Vec3f::ZERO);
        assert_eq!(sounds(&gi), [voice("misc/udeath.wav")]);
        assert_eq!(world.entities.count(), before + 4, "3 meat + 1 chest");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.maxs, Vec3f::ZERO, "now the head");
    }

    #[test]
    fn soldier_die_twice_does_nothing_more_short_of_a_gib() {
        let (mut world, gi, key, enemy) = duel(2, 300.0);
        world.entities.get_mut(key).unwrap().game.deadflag = DeadFlag::Dead;
        world.entities.get_mut(key).unwrap().game.health = -29;
        soldier_die(&mut world, key, enemy, enemy, 5, Vec3f::ZERO);
        assert!(sounds(&gi).is_empty());
        assert!(is(&world, key, &SOLDIER_MOVE_STAND1));
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 2);
    }

    #[test]
    fn soldier_die_plays_the_variant_death_and_bloodies_the_skin() {
        for (skin, bloody, name) in [
            (0, 1, SOUND_DEATH_LIGHT),
            (2, 3, SOUND_DEATH),
            (4, 5, SOUND_DEATH_SS),
        ] {
            let (mut world, gi, key, enemy) = duel(skin, 300.0);
            soldier_die(&mut world, key, enemy, enemy, 5, Vec3f::ZERO);
            assert_eq!(sounds(&gi), [voice(name)]);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.state.skinnum, bloody);
            assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        }
    }

    #[test]
    fn soldier_die_from_a_head_shot_plays_death3() {
        let (mut world, _, key, enemy) = duel(2, 300.0);
        let rand = world.level.rand_state;
        soldier_die(&mut world, key, enemy, enemy, 5, Vec3f::new(0.0, 0.0, 29.0));
        assert!(is(&world, key, &SOLDIER_MOVE_DEATH3));
        assert_eq!(world.level.rand_state, rand);

        let (mut world, _, key, enemy) = duel(2, 300.0);
        seed(&mut world, |w| w.random_u32() % 5 == 0);
        soldier_die(&mut world, key, enemy, enemy, 5, Vec3f::new(0.0, 0.0, 29.5));
        assert!(is(&world, key, &SOLDIER_MOVE_DEATH1), "4.5 above the eyes");
    }

    #[test]
    fn soldier_die_picks_one_of_five_other_deaths() {
        let deaths = [
            &SOLDIER_MOVE_DEATH1,
            &SOLDIER_MOVE_DEATH2,
            &SOLDIER_MOVE_DEATH4,
            &SOLDIER_MOVE_DEATH5,
            &SOLDIER_MOVE_DEATH6,
        ];
        for (n, mv) in deaths.into_iter().enumerate() {
            let (mut world, _, key, enemy) = duel(2, 300.0);
            seed(&mut world, |w| w.random_u32() % 5 == n as u32);
            soldier_die(&mut world, key, enemy, enemy, 5, Vec3f::ZERO);
            assert!(is(&world, key, mv), "{n}");
        }
    }

    #[test]
    fn soldier_dead_shrinks_to_a_corpse_and_stops_thinking() {
        let (mut world, _, key, _) = duel(2, 300.0);
        world.entities.get_mut(key).unwrap().game.nextthink = 6.0;
        soldier_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.mins, Vec3f::new(-16.0, -16.0, -24.0));
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_ne!(ent.svflags & SvFlags::DEADMONSTER.bits(), 0);
        assert_eq!(ent.game.nextthink, 0.0);
    }

    // -- spawning -----------------------------------------------------------

    #[test]
    fn soldier_spawn_sets_the_c_callbacks_skin_and_walker_start() {
        let spawns: [(SpawnFnPtr, i32); 3] = [
            (sp_monster_soldier_light, 0),
            (sp_monster_soldier, 2),
            (sp_monster_soldier_ss, 4),
        ];
        for (spawn, skin) in spawns {
            let mut world = test_world();
            let key = world.spawn().unwrap();
            spawn(&mut world.entities, key, &HashMap::new());
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.state.skinnum, skin);
            assert_eq!(ent.game.movetype, MoveType::Step);
            assert_eq!(ent.solid, Solid::Bbox);
            assert!(ent
                .think
                .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::walkmonster_start_go as ThinkFn)));
            let mi = ent.monsterinfo.as_deref().unwrap();
            assert!(mi.dodge.is_some_and(|f| std::ptr::fn_addr_eq(
                f,
                soldier_dodge as crate::entity::MonsterDodgeFn
            )));
            assert!(mi
                .attack
                .is_some_and(|f| std::ptr::fn_addr_eq(f, soldier_attack as ThinkFn)));
            assert!(mi.sight.is_some());
            assert!(mi.melee.is_none() && mi.idle.is_none() && mi.search.is_none());
        }
    }

    type SpawnFnPtr = fn(&mut EntityStorage, EntityKey, &HashMap<String, String>);

    #[test]
    fn soldier_finish_precaches_the_variant_sounds_and_may_stand3() {
        let finishes: [(crate::spawn::FinishFn, [&str; 3]); 3] = [
            (
                finish_soldier_light,
                [SOUND_PAIN_LIGHT, SOUND_DEATH_LIGHT, "soldier/solatck2.wav"],
            ),
            (
                finish_soldier,
                [SOUND_PAIN, SOUND_DEATH, "soldier/solatck1.wav"],
            ),
            (
                finish_soldier_ss,
                [SOUND_PAIN_SS, SOUND_DEATH_SS, "soldier/solatck3.wav"],
            ),
        ];
        for (finish, own) in finishes {
            let (mut world, gi) = test_world_recording();
            let key = world.spawn().unwrap();
            sp_monster_soldier(&mut world.entities, key, &HashMap::new());
            seed(&mut world, |w| w.random() >= 0.8);
            finish(&mut world, key, &HashMap::new());
            let sounds = gi.sounds.borrow();
            for name in [SOUND_IDLE, SOUND_SIGHT1, SOUND_SIGHT2, SOUND_COCK]
                .iter()
                .chain(&own)
                .chain(&SOUND_STEPS)
            {
                assert!(sounds.iter().any(|s| s == name), "{name}");
            }
            assert!(is(&world, key, &SOLDIER_MOVE_STAND3));
        }
    }

    #[test]
    fn soldier_spawn_rows_run_the_finish_functions() {
        for (class, finish) in [
            (
                "monster_soldier_light",
                finish_soldier_light as crate::spawn::FinishFn,
            ),
            ("monster_soldier", finish_soldier),
            ("monster_soldier_ss", finish_soldier_ss),
        ] {
            let row = crate::spawn::find_spawn(class).unwrap();
            assert!(
                row.3.is_some_and(|f| std::ptr::fn_addr_eq(f, finish)),
                "{class}"
            );
        }
    }

    // -- in play ------------------------------------------------------------

    /// A soldier spawned through its row at the origin facing +X, with
    /// the player `x` units ahead.
    fn soldier_facing_player(
        class: &str,
        x: f32,
    ) -> (
        GameWorld,
        Rc<crate::monster::test_support::ArenaImport>,
        EntityKey,
        EntityKey,
    ) {
        let (mut world, gi, player) = player_arena(x);
        world.game.skill = 1;
        let key = world.spawn().unwrap();
        let fields: HashMap<String, String> = [("classname".to_string(), class.to_string())].into();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        world.entities.get_mut(key).unwrap().game.classname = class.to_string();
        let &(_, spawn, _, finish) = crate::spawn::find_spawn(class).unwrap();
        spawn(&mut world.entities, key, &fields);
        finish.unwrap()(&mut world, key, &fields);
        ai::monster_start(&mut world, key);
        world.link_entity(key);
        (world, gi, key, player)
    }

    #[test]
    fn light_soldier_sights_the_player_and_fires_blaster_bolts_at_it() {
        let (mut world, gi, key, player) = soldier_facing_player("monster_soldier_light", 300.0);

        think_for(&mut world, key, 150);

        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(
            sounds(&gi.floor.inner)
                .iter()
                .any(|(_, s, _)| s == SOUND_SIGHT1 || s == SOUND_SIGHT2),
            "sight shout"
        );
        let bolt = bolt(&world).expect("the soldier fired");
        assert_eq!(bolt.owner, Some(key));
        assert!(
            bolt.velocity.x > 500.0,
            "toward the player: {}",
            bolt.velocity
        );
    }

    #[test]
    fn machinegun_soldier_shoots_the_player() {
        let (mut world, _, key, player) = soldier_facing_player("monster_soldier_ss", 300.0);

        think_for(&mut world, key, 150);

        assert!(world.entities.get(player).unwrap().game.health < 100);
    }
}
