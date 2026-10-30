//! Insane marine (`misc_insane`): screams, crawls or hangs crucified;
//! never attacks.
//!
//! C ref: `~/Qwasm2/src/game/monster/insane/insane.c` (frames: insane.h).

use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: insane.h.
// ---------------------------------------------------------------------------

const FRAME_STAND1: i32 = 0;
const FRAME_STAND40: i32 = 39;
const FRAME_STAND41: i32 = 40;
const FRAME_STAND59: i32 = 58;
const FRAME_STAND60: i32 = 59;
const FRAME_STAND65: i32 = 64;
const FRAME_STAND94: i32 = 93;
const FRAME_STAND96: i32 = 95;
const FRAME_STAND99: i32 = 98;
const FRAME_STAND100: i32 = 99;
const FRAME_STAND160: i32 = 159;
const FRAME_WALK27: i32 = 160;
const FRAME_WALK39: i32 = 172;
const FRAME_WALK1: i32 = 173;
const FRAME_WALK26: i32 = 198;
const FRAME_ST_PAIN2: i32 = 199;
const FRAME_ST_PAIN12: i32 = 209;
const FRAME_ST_DEATH2: i32 = 210;
const FRAME_ST_DEATH18: i32 = 226;
const FRAME_CRAWL1: i32 = 227;
const FRAME_CRAWL9: i32 = 235;
const FRAME_CR_PAIN2: i32 = 236;
const FRAME_CR_PAIN10: i32 = 244;
const FRAME_CR_DEATH10: i32 = 245;
const FRAME_CR_DEATH16: i32 = 251;
const FRAME_CROSS1: i32 = 252;
const FRAME_CROSS15: i32 = 266;
const FRAME_CROSS16: i32 = 267;
const FRAME_CROSS30: i32 = 281;

/// C model scale, multiplied into every frame's distance.
const MODEL_SCALE: f32 = 1.0;

// C SP_misc_insane values.
const HEALTH: i32 = 100;
const MASS: i32 = 300;

// Spawnflags (C uses the bare numbers; 8 is `SPAWNFLAG_CRUSIFIED`).
const SPAWNFLAG_CRAWL: i32 = 4;
const SPAWNFLAG_CRUSIFIED: i32 = 8;
const SPAWNFLAG_STAND_GROUND: i32 = 16;
const SPAWNFLAG_ALWAYS_STAND: i32 = 32;

const SOUND_FIST: &str = "insane/insane11.wav";
const SOUND_SHAKE: &str = "insane/insane5.wav";
const SOUND_MOAN: &str = "insane/insane7.wav";
const SOUND_SCREAM: [&str; 8] = [
    "insane/insane1.wav",
    "insane/insane2.wav",
    "insane/insane3.wav",
    "insane/insane4.wav",
    "insane/insane6.wav",
    "insane/insane8.wav",
    "insane/insane9.wav",
    "insane/insane10.wav",
];

// ---------------------------------------------------------------------------
// Moves. C ref: insane.c `insane_frames_*` / `insane_move_*`.
// ---------------------------------------------------------------------------

const fn frame(aifunc: AnimFrameFn, dist: f32, thinkfunc: Option<ThinkFn>) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

static INSANE_FRAMES_STAND_NORMAL: [MonsterFrame; 6] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(insane_checkdown)),
];

static INSANE_FRAMES_STAND_INSANE: [MonsterFrame; 30] = [
    frame(ai::ai_stand, 0.0, Some(insane_shake)),
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
    frame(ai::ai_stand, 0.0, Some(insane_checkdown)),
];

static INSANE_FRAMES_UPTODOWN: [MonsterFrame; 40] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(insane_moan)),
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
    frame(ai::ai_move, 2.7, None),
    frame(ai::ai_move, 4.1, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 7.6, None),
    frame(ai::ai_move, 3.6, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(insane_fist)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(insane_fist)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static INSANE_FRAMES_DOWNTOUP: [MonsterFrame; 19] = [
    frame(ai::ai_move, -0.7, None),
    frame(ai::ai_move, -1.2, None),
    frame(ai::ai_move, -1.5, None),
    frame(ai::ai_move, -4.5, None),
    frame(ai::ai_move, -3.5, None),
    frame(ai::ai_move, -0.2, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.3, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -3.3, None),
    frame(ai::ai_move, -1.6, None),
    frame(ai::ai_move, -0.3, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static INSANE_FRAMES_JUMPDOWN: [MonsterFrame; 5] = [
    frame(ai::ai_move, 0.2, None),
    frame(ai::ai_move, 11.5, None),
    frame(ai::ai_move, 5.1, None),
    frame(ai::ai_move, 7.1, None),
    frame(ai::ai_move, 0.0, None),
];

static INSANE_FRAMES_DOWN: [MonsterFrame; 61] = [
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
    frame(ai::ai_move, -1.7, None),
    frame(ai::ai_move, -1.6, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(insane_fist)),
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
    frame(ai::ai_move, 0.0, Some(insane_moan)),
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
    frame(ai::ai_move, 0.5, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -0.2, Some(insane_scream)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.2, None),
    frame(ai::ai_move, 0.4, None),
    frame(ai::ai_move, 0.6, None),
    frame(ai::ai_move, 0.8, None),
    frame(ai::ai_move, 0.7, None),
    frame(ai::ai_move, 0.0, Some(insane_checkup)),
];

static INSANE_FRAMES_WALK_NORMAL: [MonsterFrame; 13] = [
    frame(ai::ai_walk, 0.0, Some(insane_scream)),
    frame(ai::ai_walk, 2.5, None),
    frame(ai::ai_walk, 3.5, None),
    frame(ai::ai_walk, 1.7, None),
    frame(ai::ai_walk, 2.3, None),
    frame(ai::ai_walk, 2.4, None),
    frame(ai::ai_walk, 2.2, Some(insane_footstep)),
    frame(ai::ai_walk, 4.2, None),
    frame(ai::ai_walk, 5.6, None),
    frame(ai::ai_walk, 3.3, None),
    frame(ai::ai_walk, 2.4, None),
    frame(ai::ai_walk, 0.9, None),
    frame(ai::ai_walk, 0.0, Some(insane_footstep)),
];

static INSANE_FRAMES_WALK_INSANE: [MonsterFrame; 26] = [
    frame(ai::ai_walk, 0.0, Some(insane_scream)),
    frame(ai::ai_walk, 3.4, None),
    frame(ai::ai_walk, 3.6, None),
    frame(ai::ai_walk, 2.9, None),
    frame(ai::ai_walk, 2.2, None),
    frame(ai::ai_walk, 2.6, None),
    frame(ai::ai_walk, 0.0, Some(insane_footstep)),
    frame(ai::ai_walk, 0.7, None),
    frame(ai::ai_walk, 4.8, None),
    frame(ai::ai_walk, 5.3, None),
    frame(ai::ai_walk, 1.1, None),
    frame(ai::ai_walk, 2.0, Some(insane_footstep)),
    frame(ai::ai_walk, 0.5, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 4.9, None),
    frame(ai::ai_walk, 6.7, None),
    frame(ai::ai_walk, 3.8, None),
    frame(ai::ai_walk, 2.0, Some(insane_footstep)),
    frame(ai::ai_walk, 0.2, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 3.4, None),
    frame(ai::ai_walk, 6.4, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 1.8, Some(insane_footstep)),
    frame(ai::ai_walk, 0.0, None),
];

static INSANE_FRAMES_STAND_PAIN: [MonsterFrame; 11] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(insane_footstep)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static INSANE_FRAMES_STAND_DEATH: [MonsterFrame; 17] = [
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

static INSANE_FRAMES_CRAWL: [MonsterFrame; 9] = [
    frame(ai::ai_walk, 0.0, Some(insane_scream)),
    frame(ai::ai_walk, 1.5, None),
    frame(ai::ai_walk, 2.1, None),
    frame(ai::ai_walk, 3.6, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 0.9, None),
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 3.4, None),
    frame(ai::ai_walk, 2.4, None),
];

static INSANE_FRAMES_CRAWL_PAIN: [MonsterFrame; 9] = [
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

static INSANE_FRAMES_CRAWL_DEATH: [MonsterFrame; 7] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static INSANE_FRAMES_CROSS: [MonsterFrame; 15] = [
    frame(ai::ai_move, 0.0, Some(insane_moan)),
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

static INSANE_FRAMES_STRUGGLE_CROSS: [MonsterFrame; 15] = [
    frame(ai::ai_move, 0.0, Some(insane_scream)),
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

static INSANE_MOVE_STAND_NORMAL: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND60,
    lastframe: FRAME_STAND65,
    frames: &INSANE_FRAMES_STAND_NORMAL,
    endfunc: Some(insane_stand),
};

static INSANE_MOVE_STAND_INSANE: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND65,
    lastframe: FRAME_STAND94,
    frames: &INSANE_FRAMES_STAND_INSANE,
    endfunc: Some(insane_stand),
};

static INSANE_MOVE_UPTODOWN: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND1,
    lastframe: FRAME_STAND40,
    frames: &INSANE_FRAMES_UPTODOWN,
    endfunc: Some(insane_onground),
};

static INSANE_MOVE_DOWNTOUP: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND41,
    lastframe: FRAME_STAND59,
    frames: &INSANE_FRAMES_DOWNTOUP,
    endfunc: Some(insane_stand),
};

static INSANE_MOVE_JUMPDOWN: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND96,
    lastframe: FRAME_STAND100,
    frames: &INSANE_FRAMES_JUMPDOWN,
    endfunc: Some(insane_onground),
};

static INSANE_MOVE_DOWN: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND100,
    lastframe: FRAME_STAND160,
    frames: &INSANE_FRAMES_DOWN,
    endfunc: Some(insane_onground),
};

static INSANE_MOVE_WALK_NORMAL: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK27,
    lastframe: FRAME_WALK39,
    frames: &INSANE_FRAMES_WALK_NORMAL,
    endfunc: Some(insane_walk),
};

static INSANE_MOVE_RUN_NORMAL: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK27,
    lastframe: FRAME_WALK39,
    frames: &INSANE_FRAMES_WALK_NORMAL,
    endfunc: Some(insane_run),
};

static INSANE_MOVE_WALK_INSANE: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK1,
    lastframe: FRAME_WALK26,
    frames: &INSANE_FRAMES_WALK_INSANE,
    endfunc: Some(insane_walk),
};

static INSANE_MOVE_RUN_INSANE: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK1,
    lastframe: FRAME_WALK26,
    frames: &INSANE_FRAMES_WALK_INSANE,
    endfunc: Some(insane_run),
};

static INSANE_MOVE_STAND_PAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_ST_PAIN2,
    lastframe: FRAME_ST_PAIN12,
    frames: &INSANE_FRAMES_STAND_PAIN,
    endfunc: Some(insane_run),
};

static INSANE_MOVE_STAND_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_ST_DEATH2,
    lastframe: FRAME_ST_DEATH18,
    frames: &INSANE_FRAMES_STAND_DEATH,
    endfunc: Some(insane_dead),
};

static INSANE_MOVE_CRAWL: MonsterMove = MonsterMove {
    firstframe: FRAME_CRAWL1,
    lastframe: FRAME_CRAWL9,
    frames: &INSANE_FRAMES_CRAWL,
    endfunc: None,
};

static INSANE_MOVE_RUNCRAWL: MonsterMove = MonsterMove {
    firstframe: FRAME_CRAWL1,
    lastframe: FRAME_CRAWL9,
    frames: &INSANE_FRAMES_CRAWL,
    endfunc: None,
};

static INSANE_MOVE_CRAWL_PAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_CR_PAIN2,
    lastframe: FRAME_CR_PAIN10,
    frames: &INSANE_FRAMES_CRAWL_PAIN,
    endfunc: Some(insane_run),
};

static INSANE_MOVE_CRAWL_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_CR_DEATH10,
    lastframe: FRAME_CR_DEATH16,
    frames: &INSANE_FRAMES_CRAWL_DEATH,
    endfunc: Some(insane_dead),
};

static INSANE_MOVE_CROSS: MonsterMove = MonsterMove {
    firstframe: FRAME_CROSS1,
    lastframe: FRAME_CROSS15,
    frames: &INSANE_FRAMES_CROSS,
    endfunc: Some(insane_cross),
};

static INSANE_MOVE_STRUGGLE_CROSS: MonsterMove = MonsterMove {
    firstframe: FRAME_CROSS16,
    lastframe: FRAME_CROSS30,
    frames: &INSANE_FRAMES_STRUGGLE_CROSS,
    endfunc: Some(insane_cross),
};

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

fn set_move(world: &mut GameWorld, key: EntityKey, mv: &MonsterMove) {
    if let Some(mi) = world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_mut())
    {
        mi.currentmove = Some(mv.clone());
    }
}

fn sound(world: &mut GameWorld, key: EntityKey, name: &str) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), CHAN_VOICE, index, 1.0, ATTN_IDLE, 0.0);
}

fn spawnflags(world: &GameWorld, key: EntityKey) -> i32 {
    world.entities.get(key).map_or(0, |e| e.game.spawnflags)
}

fn frame_of(world: &GameWorld, key: EntityKey) -> i32 {
    world.entities.get(key).map_or(0, |e| e.state.frame)
}

/// Screams and moans wait while a pain sound plays.
fn screams_suppressed(world: &GameWorld, key: EntityKey) -> bool {
    world
        .entities
        .get(key)
        .is_some_and(|e| e.game.fly_sound_debounce_time > world.level.time)
}

/// Crawling or lying down: pain and death use the crawl moves.
fn is_down(frame: i32) -> bool {
    (FRAME_CRAWL1..=FRAME_CRAWL9).contains(&frame)
        || (FRAME_STAND99..=FRAME_STAND160).contains(&frame)
}

/// C plays a footstep only with the `g_monsterfootsteps` cvar, which
/// defaults to 0 and is not ported, so this is silent.
/// C ref: `insane_footstep`.
pub fn insane_footstep(_world: &mut GameWorld, _key: EntityKey) {}

/// C ref: `insane_fist`.
pub fn insane_fist(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, SOUND_FIST);
}

/// C ref: `insane_shake`.
pub fn insane_shake(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, SOUND_SHAKE);
}

/// C ref: `insane_moan`.
pub fn insane_moan(world: &mut GameWorld, key: EntityKey) {
    if screams_suppressed(world, key) {
        return;
    }
    sound(world, key, SOUND_MOAN);
}

/// C ref: `insane_scream`.
pub fn insane_scream(world: &mut GameWorld, key: EntityKey) {
    if screams_suppressed(world, key) {
        return;
    }
    let which = world.random_u32() as usize % SOUND_SCREAM.len();
    sound(world, key, SOUND_SCREAM[which]);
}

/// Hang on the cross, struggling one time in five. C ref: `insane_cross`.
pub fn insane_cross(world: &mut GameWorld, key: EntityKey) {
    let mv = if world.random() < 0.8 {
        &INSANE_MOVE_CROSS
    } else {
        &INSANE_MOVE_STRUGGLE_CROSS
    };
    set_move(world, key, mv);
}

/// One that stands its ground lies down after crawl pain; others crawl,
/// or walk one of two ways. C ref: `insane_walk`.
pub fn insane_walk(world: &mut GameWorld, key: EntityKey) {
    let flags = spawnflags(world, key);
    if flags & SPAWNFLAG_STAND_GROUND != 0 && frame_of(world, key) == FRAME_CR_PAIN10 {
        set_move(world, key, &INSANE_MOVE_DOWN);
        return;
    }
    let mv = if flags & SPAWNFLAG_CRAWL != 0 {
        &INSANE_MOVE_CRAWL
    } else if world.random() <= 0.5 {
        &INSANE_MOVE_WALK_NORMAL
    } else {
        &INSANE_MOVE_WALK_INSANE
    };
    set_move(world, key, mv);
}

/// C ref: `insane_run` (C draws `frandk()`; ours is `random()`, the same
/// distribution).
pub fn insane_run(world: &mut GameWorld, key: EntityKey) {
    let flags = spawnflags(world, key);
    if flags & SPAWNFLAG_STAND_GROUND != 0 && frame_of(world, key) == FRAME_CR_PAIN10 {
        set_move(world, key, &INSANE_MOVE_DOWN);
        return;
    }
    let mv = if flags & SPAWNFLAG_CRAWL != 0 {
        &INSANE_MOVE_RUNCRAWL
    } else if world.random() <= 0.5 {
        &INSANE_MOVE_RUN_NORMAL
    } else {
        &INSANE_MOVE_RUN_INSANE
    };
    set_move(world, key, mv);
}

/// A player pain sound by remaining health, then the pain move (the
/// crucified struggle on the cross). C ref: `insane_pain`.
pub fn insane_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    _damage: i32,
) {
    let time = world.level.time;
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if time < ent.game.pain_debounce_time {
        return;
    }
    let health = ent.game.health;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.pain_debounce_time = time + 3.0;
    }
    let r = 1 + (world.random_u32() & 1);
    let l = if health < 25 {
        25
    } else if health < 50 {
        50
    } else if health < 75 {
        75
    } else {
        100
    };
    sound(world, key, &format!("player/male/pain{l}_{r}.wav"));
    // Suppress screaming and moaning for 1 second so the pain sound plays.
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.fly_sound_debounce_time = time + 1.0;
    }
    if world.game.skill == crate::constants::SKILL_HARDPLUS {
        return; // No pain anims in nightmare.
    }
    if spawnflags(world, key) & SPAWNFLAG_CRUSIFIED != 0 {
        set_move(world, key, &INSANE_MOVE_STRUGGLE_CROSS);
        return;
    }
    let mv = if is_down(frame_of(world, key)) {
        &INSANE_MOVE_CRAWL_PAIN
    } else {
        &INSANE_MOVE_STAND_PAIN
    };
    set_move(world, key, mv);
}

/// C ref: `insane_onground`.
pub fn insane_onground(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &INSANE_MOVE_DOWN);
}

/// Three times in ten, drop to the floor one of two ways (never with
/// ALWAYS_STAND). C ref: `insane_checkdown`.
pub fn insane_checkdown(world: &mut GameWorld, key: EntityKey) {
    if spawnflags(world, key) & SPAWNFLAG_ALWAYS_STAND != 0 {
        return;
    }
    if world.random() < 0.3 {
        let mv = if world.random() < 0.5 {
            &INSANE_MOVE_UPTODOWN
        } else {
            &INSANE_MOVE_JUMPDOWN
        };
        set_move(world, key, mv);
    }
}

/// Get up half the time, unless crawling and standing ground.
/// C ref: `insane_checkup`.
pub fn insane_checkup(world: &mut GameWorld, key: EntityKey) {
    let flags = spawnflags(world, key);
    if flags & SPAWNFLAG_CRAWL != 0 && flags & SPAWNFLAG_STAND_GROUND != 0 {
        return;
    }
    if world.random() < 0.5 {
        set_move(world, key, &INSANE_MOVE_DOWNTOUP);
    }
}

/// C ref: `insane_stand`.
pub fn insane_stand(world: &mut GameWorld, key: EntityKey) {
    let flags = spawnflags(world, key);
    if flags & SPAWNFLAG_CRUSIFIED != 0 {
        set_move(world, key, &INSANE_MOVE_CROSS);
        if let Some(mi) = world
            .entities
            .get_mut(key)
            .and_then(|e| e.monsterinfo.as_mut())
        {
            mi.aiflags |= AiFlags::STAND_GROUND;
        }
    } else if flags & SPAWNFLAG_CRAWL != 0 && flags & SPAWNFLAG_STAND_GROUND != 0 {
        set_move(world, key, &INSANE_MOVE_DOWN);
    } else if world.random() < 0.5 {
        set_move(world, key, &INSANE_MOVE_STAND_NORMAL);
    } else {
        set_move(world, key, &INSANE_MOVE_STAND_INSANE);
    }
}

/// The corpse: a crucified one stays up on the cross (flies), others get
/// the lying box and fall. C ref: `insane_dead`.
pub fn insane_dead(world: &mut GameWorld, key: EntityKey) {
    let crucified = spawnflags(world, key) & SPAWNFLAG_CRUSIFIED != 0;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if crucified {
        ent.game.flags |= EntityFlags::FLY;
    } else {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
        ent.game.movetype = MoveType::Toss;
    }
    ent.svflags |= SvFlags::DEADMONSTER.bits();
    ent.game.nextthink = 0.0;
    world.link_entity(key);
}

/// Gib below `gib_health`, else a player death sound and the death move
/// (crucified: dead on the spot). C ref: `insane_die`.
pub fn insane_die(
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
        sound(world, key, "misc/udeath.wav");
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
    let n = world.random_u32() % 4 + 1;
    sound(world, key, &format!("player/male/death{n}.wav"));
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    if spawnflags(world, key) & SPAWNFLAG_CRUSIFIED != 0 {
        insane_dead(world, key);
    } else if is_down(frame_of(world, key)) {
        set_move(world, key, &INSANE_MOVE_CRAWL_DEATH);
    } else {
        set_move(world, key, &INSANE_MOVE_STAND_DEATH);
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_misc_insane`: a good guy that never
/// attacks; crucified ones hang in a small box and fly.
pub fn sp_monster_insane(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
    ent.game.health = HEALTH;
    ent.game.gib_health = -50;
    ent.game.mass = MASS;
    ent.pain = Some(insane_pain);
    ent.die = Some(insane_die);
    let mut aiflags = AiFlags::GOOD_GUY;
    if ent.game.spawnflags & SPAWNFLAG_STAND_GROUND != 0 {
        aiflags |= AiFlags::STAND_GROUND;
    }
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(insane_stand),
        walk: Some(insane_walk),
        run: Some(insane_run),
        aiflags,
        currentmove: Some(INSANE_MOVE_STAND_NORMAL.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    if ent.game.spawnflags & SPAWNFLAG_CRUSIFIED != 0 {
        ent.mins = Vec3f::new(-16.0, 0.0, 0.0);
        ent.maxs = Vec3f::new(16.0, 8.0, 32.0);
        ent.game.flags |= EntityFlags::NO_KNOCKBACK;
        ai::flymonster_start(ent);
    } else {
        ai::walkmonster_start(ent);
    }
}

/// The `GameWorld` half of C `SP_misc_insane`: precache the sounds, and a
/// random one of three skins for a walker.
///
/// C sets the skin after walkmonster_start because C monster_start
/// resets `s.skinnum` to 0. This finish runs before `call_spawn`'s
/// `monster_start`, so it relies on the Rust `monster_start` not porting
/// that reset; porting it would give every insane skin 0.
pub fn insane_finish(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    for name in [SOUND_FIST, SOUND_SHAKE, SOUND_MOAN]
        .iter()
        .chain(&SOUND_SCREAM)
    {
        world.gi.sound_index(name);
    }
    if spawnflags(world, key) & SPAWNFLAG_CRUSIFIED == 0 {
        let skin = (world.random_u32() % 3) as i32;
        if let Some(ent) = world.entities.get_mut(key) {
            ent.state.skinnum = skin;
        }
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::misc::wire::wire_world;
    use crate::monster::test_support::wire_sounds;
    use crate::world::test_world;

    /// One C `mmove_t` and its `mframe_t` table: ai per frame as a letter
    /// (Stand, Walk, Run, Charge, Move), dists, think slots, endfunc.
    struct CTable {
        mv: &'static MonsterMove,
        first: i32,
        last: i32,
        ai: &'static str,
        dist: &'static [f32],
        thinks: &'static [(usize, ThinkFn)],
        end: Option<ThinkFn>,
    }

    fn ai_letter(f: AnimFrameFn) -> char {
        let table: [(AnimFrameFn, char); 5] = [
            (ai::ai_stand, 'S'),
            (ai::ai_walk, 'W'),
            (ai::ai_run, 'R'),
            (ai::ai_charge, 'C'),
            (ai::ai_move, 'M'),
        ];
        table
            .iter()
            .find(|(g, _)| std::ptr::fn_addr_eq(f, *g))
            .map_or('?', |&(_, c)| c)
    }

    fn check(t: &CTable) {
        let mv = t.mv;
        let at = format!("move {}..{}", t.first, t.last);
        assert_eq!((mv.firstframe, mv.lastframe), (t.first, t.last), "{at}");
        assert_eq!(mv.frames.len() as i32, t.last - t.first + 1, "{at}");
        let ai: String = mv
            .frames
            .iter()
            .map(|f| ai_letter(f.aifunc.unwrap()))
            .collect();
        assert_eq!(ai, t.ai, "{at}");
        let dist: Vec<f32> = mv.frames.iter().map(|f| f.dist).collect();
        assert_eq!(dist, t.dist, "{at}");
        let thinks: Vec<usize> = (0..mv.frames.len())
            .filter(|&i| mv.frames[i].thinkfunc.is_some())
            .collect();
        let want: Vec<usize> = t.thinks.iter().map(|&(i, _)| i).collect();
        assert_eq!(thinks, want, "{at}");
        for &(i, f) in t.thinks {
            assert!(
                std::ptr::fn_addr_eq(mv.frames[i].thinkfunc.unwrap(), f),
                "{at} frame {i}"
            );
        }
        match (mv.endfunc, t.end) {
            (None, None) => {}
            (Some(a), Some(b)) => assert!(std::ptr::fn_addr_eq(a, b), "{at} endfunc"),
            _ => panic!("{at} endfunc"),
        }
    }

    #[test]
    #[rustfmt::skip]
    fn insane_moves_match_c_insane_c() {
        let tables = [
        CTable { mv: &INSANE_MOVE_STAND_NORMAL, first: FRAME_STAND60, last: FRAME_STAND65, ai: "SSSSSS", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[(5, insane_checkdown as ThinkFn)], end: Some(insane_stand as ThinkFn) },
        CTable { mv: &INSANE_MOVE_STAND_INSANE, first: FRAME_STAND65, last: FRAME_STAND94, ai: "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSS", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[(0, insane_shake as ThinkFn), (29, insane_checkdown as ThinkFn)], end: Some(insane_stand as ThinkFn) },
        CTable { mv: &INSANE_MOVE_UPTODOWN, first: FRAME_STAND1, last: FRAME_STAND40, ai: "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.7, 4.1, 6.0, 7.6, 3.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[(7, insane_moan as ThinkFn), (27, insane_fist as ThinkFn), (33, insane_fist as ThinkFn)], end: Some(insane_onground as ThinkFn) },
        CTable { mv: &INSANE_MOVE_DOWNTOUP, first: FRAME_STAND41, last: FRAME_STAND59, ai: "MMMMMMMMMMMMMMMMMMM", dist: &[-0.7, -1.2, -1.5, -4.5, -3.5, -0.2, 0.0, -1.3, -3.0, -2.0, 0.0, 0.0, 0.0, -3.3, -1.6, -0.3, 0.0, 0.0, 0.0], thinks: &[], end: Some(insane_stand as ThinkFn) },
        CTable { mv: &INSANE_MOVE_JUMPDOWN, first: FRAME_STAND96, last: FRAME_STAND100, ai: "MMMMM", dist: &[0.2, 11.5, 5.1, 7.1, 0.0], thinks: &[], end: Some(insane_onground as ThinkFn) },
        CTable { mv: &INSANE_MOVE_DOWN, first: FRAME_STAND100, last: FRAME_STAND160, ai: "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.7, -1.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, -0.2, 0.0, 0.2, 0.4, 0.6, 0.8, 0.7, 0.0], thinks: &[(16, insane_fist as ThinkFn), (33, insane_moan as ThinkFn), (53, insane_scream as ThinkFn), (60, insane_checkup as ThinkFn)], end: Some(insane_onground as ThinkFn) },
        CTable { mv: &INSANE_MOVE_WALK_NORMAL, first: FRAME_WALK27, last: FRAME_WALK39, ai: "WWWWWWWWWWWWW", dist: &[0.0, 2.5, 3.5, 1.7, 2.3, 2.4, 2.2, 4.2, 5.6, 3.3, 2.4, 0.9, 0.0], thinks: &[(0, insane_scream as ThinkFn), (6, insane_footstep as ThinkFn), (12, insane_footstep as ThinkFn)], end: Some(insane_walk as ThinkFn) },
        CTable { mv: &INSANE_MOVE_RUN_NORMAL, first: FRAME_WALK27, last: FRAME_WALK39, ai: "WWWWWWWWWWWWW", dist: &[0.0, 2.5, 3.5, 1.7, 2.3, 2.4, 2.2, 4.2, 5.6, 3.3, 2.4, 0.9, 0.0], thinks: &[(0, insane_scream as ThinkFn), (6, insane_footstep as ThinkFn), (12, insane_footstep as ThinkFn)], end: Some(insane_run as ThinkFn) },
        CTable { mv: &INSANE_MOVE_WALK_INSANE, first: FRAME_WALK1, last: FRAME_WALK26, ai: "WWWWWWWWWWWWWWWWWWWWWWWWWW", dist: &[0.0, 3.4, 3.6, 2.9, 2.2, 2.6, 0.0, 0.7, 4.8, 5.3, 1.1, 2.0, 0.5, 0.0, 0.0, 4.9, 6.7, 3.8, 2.0, 0.2, 0.0, 3.4, 6.4, 5.0, 1.8, 0.0], thinks: &[(0, insane_scream as ThinkFn), (6, insane_footstep as ThinkFn), (11, insane_footstep as ThinkFn), (18, insane_footstep as ThinkFn), (24, insane_footstep as ThinkFn)], end: Some(insane_walk as ThinkFn) },
        CTable { mv: &INSANE_MOVE_RUN_INSANE, first: FRAME_WALK1, last: FRAME_WALK26, ai: "WWWWWWWWWWWWWWWWWWWWWWWWWW", dist: &[0.0, 3.4, 3.6, 2.9, 2.2, 2.6, 0.0, 0.7, 4.8, 5.3, 1.1, 2.0, 0.5, 0.0, 0.0, 4.9, 6.7, 3.8, 2.0, 0.2, 0.0, 3.4, 6.4, 5.0, 1.8, 0.0], thinks: &[(0, insane_scream as ThinkFn), (6, insane_footstep as ThinkFn), (11, insane_footstep as ThinkFn), (18, insane_footstep as ThinkFn), (24, insane_footstep as ThinkFn)], end: Some(insane_run as ThinkFn) },
        CTable { mv: &INSANE_MOVE_STAND_PAIN, first: FRAME_ST_PAIN2, last: FRAME_ST_PAIN12, ai: "MMMMMMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[(4, insane_footstep as ThinkFn)], end: Some(insane_run as ThinkFn) },
        CTable { mv: &INSANE_MOVE_STAND_DEATH, first: FRAME_ST_DEATH2, last: FRAME_ST_DEATH18, ai: "MMMMMMMMMMMMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[], end: Some(insane_dead as ThinkFn) },
        CTable { mv: &INSANE_MOVE_CRAWL, first: FRAME_CRAWL1, last: FRAME_CRAWL9, ai: "WWWWWWWWW", dist: &[0.0, 1.5, 2.1, 3.6, 2.0, 0.9, 3.0, 3.4, 2.4], thinks: &[(0, insane_scream as ThinkFn)], end: None },
        CTable { mv: &INSANE_MOVE_RUNCRAWL, first: FRAME_CRAWL1, last: FRAME_CRAWL9, ai: "WWWWWWWWW", dist: &[0.0, 1.5, 2.1, 3.6, 2.0, 0.9, 3.0, 3.4, 2.4], thinks: &[(0, insane_scream as ThinkFn)], end: None },
        CTable { mv: &INSANE_MOVE_CRAWL_PAIN, first: FRAME_CR_PAIN2, last: FRAME_CR_PAIN10, ai: "MMMMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[], end: Some(insane_run as ThinkFn) },
        CTable { mv: &INSANE_MOVE_CRAWL_DEATH, first: FRAME_CR_DEATH10, last: FRAME_CR_DEATH16, ai: "MMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[], end: Some(insane_dead as ThinkFn) },
        CTable { mv: &INSANE_MOVE_CROSS, first: FRAME_CROSS1, last: FRAME_CROSS15, ai: "MMMMMMMMMMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[(0, insane_moan as ThinkFn)], end: Some(insane_cross as ThinkFn) },
        CTable { mv: &INSANE_MOVE_STRUGGLE_CROSS, first: FRAME_CROSS16, last: FRAME_CROSS30, ai: "MMMMMMMMMMMMMMM", dist: &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], thinks: &[(0, insane_scream as ThinkFn)], end: Some(insane_cross as ThinkFn) },
        ];
        for t in &tables {
            check(t);
        }
    }

    /// A seed whose game RNG satisfies `pred` from its first draw.
    fn seed_for(pred: impl Fn(&mut GameWorld) -> bool) -> u32 {
        (0..)
            .find(|&seed| {
                let mut probe = test_world();
                probe.level.rand_state = seed;
                pred(&mut probe)
            })
            .unwrap()
    }

    /// A seed whose successive `random()` draws fall in `rolls`.
    fn seed_rolls(rolls: &[std::ops::Range<f32>]) -> u32 {
        seed_for(|w| rolls.iter().all(|r| r.contains(&w.random())))
    }

    fn insane(world: &mut GameWorld, spawnflags: i32) -> EntityKey {
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.spawnflags = spawnflags;
        sp_monster_insane(&mut world.entities, key, &HashMap::new());
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

    /// Whether `key` plays `mv`: the same frames and end function (the
    /// walk and run moves share frames).
    fn plays(world: &GameWorld, key: EntityKey, mv: &MonsterMove) -> bool {
        mi(world, key).currentmove.as_ref().is_some_and(|cm| {
            std::ptr::eq(cm.frames, mv.frames)
                && cm.firstframe == mv.firstframe
                && cm.endfunc.map(|f| f as usize) == mv.endfunc.map(|f| f as usize)
        })
    }

    #[test]
    fn insane_spawns_as_a_walking_good_guy_with_c_values() {
        let mut world = test_world();
        let key = insane(&mut world, 0);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.health, 100);
        assert_eq!(ent.game.mass, 300);
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert_eq!(ent.solid, Solid::Bbox);
        assert_eq!(ent.mins, Vec3f::new(-16.0, -16.0, -24.0));
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, 32.0));
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        let mi = mi(&world, key);
        assert_eq!(mi.aiflags, AiFlags::GOOD_GUY);
        assert!(mi.attack.is_none() && mi.melee.is_none() && mi.sight.is_none());
        assert_eq!(mi.scale, 1.0);
        assert!(plays(&world, key, &INSANE_MOVE_STAND_NORMAL));
    }

    #[test]
    fn crucified_insane_hangs_in_a_small_box_and_flies() {
        let mut world = test_world();
        let key = insane(&mut world, SPAWNFLAG_CRUSIFIED | SPAWNFLAG_STAND_GROUND);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.mins, Vec3f::new(-16.0, 0.0, 0.0));
        assert_eq!(ent.maxs, Vec3f::new(16.0, 8.0, 32.0));
        assert!(ent
            .game
            .flags
            .contains(EntityFlags::NO_KNOCKBACK | EntityFlags::FLY));
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::flymonster_start_go as ThinkFn
        ));
        assert_eq!(
            mi(&world, key).aiflags,
            AiFlags::GOOD_GUY | AiFlags::STAND_GROUND
        );
    }

    #[test]
    fn insane_finish_precaches_sounds_and_picks_a_walker_skin() {
        let (mut world, gi) = wire_world();
        let key = insane(&mut world, 0);
        world.level.rand_state = seed_for(|w| w.random_u32() % 3 == 2);
        insane_finish(&mut world, key, &HashMap::new());
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 2);
        let names = gi.inner.sounds.borrow();
        assert!(names.contains(&"insane/insane11.wav".to_string()));
        assert!(names.contains(&"insane/insane10.wav".to_string()));
        assert_eq!(names.len(), 11);
        drop(names);

        let cross = insane(&mut world, SPAWNFLAG_CRUSIFIED);
        let state = world.level.rand_state;
        insane_finish(&mut world, cross, &HashMap::new());
        assert_eq!(world.entities.get(cross).unwrap().state.skinnum, 0);
        assert_eq!(world.level.rand_state, state, "no skin draw on the cross");
    }

    #[test]
    fn insane_stand_picks_cross_down_or_a_random_stand() {
        let mut world = test_world();
        let cross = insane(&mut world, SPAWNFLAG_CRUSIFIED);
        insane_stand(&mut world, cross);
        assert!(plays(&world, cross, &INSANE_MOVE_CROSS));
        assert!(mi(&world, cross).aiflags.contains(AiFlags::STAND_GROUND));

        let down = insane(&mut world, SPAWNFLAG_CRAWL | SPAWNFLAG_STAND_GROUND);
        insane_stand(&mut world, down);
        assert!(plays(&world, down, &INSANE_MOVE_DOWN));

        let key = insane(&mut world, 0);
        world.level.rand_state = seed_rolls(&[0.0..0.5]);
        insane_stand(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_STAND_NORMAL));
        world.level.rand_state = seed_rolls(&[0.5..1.01]);
        insane_stand(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_STAND_INSANE));
    }

    #[test]
    fn insane_walk_and_run_lie_down_crawl_or_pick_a_random_gait() {
        let mut world = test_world();
        let hold = insane(&mut world, SPAWNFLAG_STAND_GROUND);
        world.entities.get_mut(hold).unwrap().state.frame = FRAME_CR_PAIN10;
        insane_walk(&mut world, hold);
        assert!(plays(&world, hold, &INSANE_MOVE_DOWN));
        insane_set_walk(&mut world, hold);
        insane_run(&mut world, hold);
        assert!(plays(&world, hold, &INSANE_MOVE_DOWN));

        let crawl = insane(&mut world, SPAWNFLAG_CRAWL);
        insane_walk(&mut world, crawl);
        assert!(plays(&world, crawl, &INSANE_MOVE_CRAWL));
        insane_run(&mut world, crawl);
        assert!(plays(&world, crawl, &INSANE_MOVE_RUNCRAWL));

        let key = insane(&mut world, 0);
        for (roll, walk, run) in [
            (0.0..0.5, &INSANE_MOVE_WALK_NORMAL, &INSANE_MOVE_RUN_NORMAL),
            (
                0.51..1.01,
                &INSANE_MOVE_WALK_INSANE,
                &INSANE_MOVE_RUN_INSANE,
            ),
        ] {
            world.level.rand_state = seed_rolls(std::slice::from_ref(&roll));
            insane_walk(&mut world, key);
            assert!(plays(&world, key, walk));
            world.level.rand_state = seed_rolls(&[roll]);
            insane_run(&mut world, key);
            assert!(plays(&world, key, run));
        }
    }

    fn insane_set_walk(world: &mut GameWorld, key: EntityKey) {
        set_move(world, key, &INSANE_MOVE_WALK_NORMAL);
    }

    #[test]
    fn insane_pain_plays_a_pain_sound_by_health_and_debounces() {
        let (mut world, gi) = wire_world();
        world.level.time = 10.0;
        let key = insane(&mut world, 0);
        world.entities.get_mut(key).unwrap().game.health = 40;
        world.level.rand_state = seed_for(|w| w.random_u32() & 1 == 1);
        insane_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(
            wire_sounds(&gi),
            [(
                CHAN_VOICE,
                "player/male/pain50_2.wav".to_string(),
                ATTN_IDLE
            )]
        );
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.pain_debounce_time, 13.0);
        assert_eq!(ent.game.fly_sound_debounce_time, 11.0);
        assert!(plays(&world, key, &INSANE_MOVE_STAND_PAIN));

        set_move(&mut world, key, &INSANE_MOVE_STAND_NORMAL);
        world.level.time = 12.9;
        insane_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(gi.sounds.borrow().len(), 1, "debounced");
        assert!(plays(&world, key, &INSANE_MOVE_STAND_NORMAL));

        for (health, l) in [(10, 25), (60, 75), (100, 100)] {
            world.level.time += 3.0;
            world.entities.get_mut(key).unwrap().game.health = health;
            world.level.rand_state = seed_for(|w| w.random_u32() & 1 == 0);
            insane_pain(&mut world, key, key, 0.0, 10);
            let name = &wire_sounds(&gi).last().unwrap().1.clone();
            assert_eq!(name, &format!("player/male/pain{l}_1.wav"));
        }
    }

    #[test]
    fn insane_pain_crawls_struggles_or_skips_the_move_in_nightmare() {
        let mut world = test_world();
        let down = insane(&mut world, 0);
        world.entities.get_mut(down).unwrap().state.frame = FRAME_STAND99;
        insane_pain(&mut world, down, down, 0.0, 10);
        assert!(plays(&world, down, &INSANE_MOVE_CRAWL_PAIN));
        let crawling = insane(&mut world, 0);
        world.entities.get_mut(crawling).unwrap().state.frame = FRAME_CRAWL9;
        insane_pain(&mut world, crawling, crawling, 0.0, 10);
        assert!(plays(&world, crawling, &INSANE_MOVE_CRAWL_PAIN));

        let cross = insane(&mut world, SPAWNFLAG_CRUSIFIED);
        insane_pain(&mut world, cross, cross, 0.0, 10);
        assert!(plays(&world, cross, &INSANE_MOVE_STRUGGLE_CROSS));

        world.game.skill = crate::constants::SKILL_HARDPLUS;
        let nightmare = insane(&mut world, SPAWNFLAG_CRUSIFIED);
        insane_pain(&mut world, nightmare, nightmare, 0.0, 10);
        assert!(plays(&world, nightmare, &INSANE_MOVE_STAND_NORMAL));
        assert_eq!(
            world
                .entities
                .get(nightmare)
                .unwrap()
                .game
                .fly_sound_debounce_time,
            1.0
        );
    }

    #[test]
    fn insane_moans_and_screams_wait_for_the_pain_sound() {
        let (mut world, gi) = wire_world();
        world.level.time = 5.0;
        let key = insane(&mut world, 0);
        world
            .entities
            .get_mut(key)
            .unwrap()
            .game
            .fly_sound_debounce_time = 5.5;
        insane_moan(&mut world, key);
        insane_scream(&mut world, key);
        assert!(gi.sounds.borrow().is_empty());

        world.level.time = 5.5;
        insane_moan(&mut world, key);
        world.level.rand_state = seed_for(|w| w.random_u32() % 8 == 4);
        insane_scream(&mut world, key);
        insane_fist(&mut world, key);
        insane_shake(&mut world, key);
        let names: Vec<String> = wire_sounds(&gi).into_iter().map(|s| s.1).collect();
        assert_eq!(
            names,
            [SOUND_MOAN, "insane/insane6.wav", SOUND_FIST, SOUND_SHAKE]
        );
        assert!(wire_sounds(&gi)
            .iter()
            .all(|s| s.0 == CHAN_VOICE && s.2 == ATTN_IDLE));
    }

    #[test]
    fn insane_footstep_is_silent_without_g_monsterfootsteps() {
        let (mut world, gi) = wire_world();
        let key = insane(&mut world, 0);
        insane_footstep(&mut world, key);
        assert!(gi.sounds.borrow().is_empty());
    }

    #[test]
    fn insane_checkdown_drops_three_times_in_ten_unless_always_stand() {
        let mut world = test_world();
        let key = insane(&mut world, 0);
        world.level.rand_state = seed_rolls(&[0.0..0.3, 0.0..0.5]);
        insane_checkdown(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_UPTODOWN));
        world.level.rand_state = seed_rolls(&[0.0..0.3, 0.5..1.01]);
        insane_checkdown(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_JUMPDOWN));
        set_move(&mut world, key, &INSANE_MOVE_STAND_NORMAL);
        world.level.rand_state = seed_rolls(&[0.3..1.01]);
        insane_checkdown(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_STAND_NORMAL));

        let stander = insane(&mut world, SPAWNFLAG_ALWAYS_STAND);
        world.level.rand_state = seed_rolls(&[0.0..0.3, 0.0..0.5]);
        insane_checkdown(&mut world, stander);
        assert!(plays(&world, stander, &INSANE_MOVE_STAND_NORMAL));
    }

    #[test]
    fn insane_checkup_gets_up_half_the_time_unless_holding_a_crawl() {
        let mut world = test_world();
        let key = insane(&mut world, 0);
        world.level.rand_state = seed_rolls(&[0.5..1.01]);
        insane_checkup(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_STAND_NORMAL));
        world.level.rand_state = seed_rolls(&[0.0..0.5]);
        insane_checkup(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_DOWNTOUP));

        let held = insane(&mut world, SPAWNFLAG_CRAWL | SPAWNFLAG_STAND_GROUND);
        world.level.rand_state = seed_rolls(&[0.0..0.5]);
        insane_checkup(&mut world, held);
        assert!(plays(&world, held, &INSANE_MOVE_STAND_NORMAL));
    }

    #[test]
    fn insane_cross_struggles_one_time_in_five() {
        let mut world = test_world();
        let key = insane(&mut world, SPAWNFLAG_CRUSIFIED);
        world.level.rand_state = seed_rolls(&[0.0..0.8]);
        insane_cross(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_CROSS));
        world.level.rand_state = seed_rolls(&[0.8..1.01]);
        insane_cross(&mut world, key);
        assert!(plays(&world, key, &INSANE_MOVE_STRUGGLE_CROSS));
    }

    #[test]
    fn insane_dead_drops_a_walker_and_leaves_the_crucified_hanging() {
        let mut world = test_world();
        let key = insane(&mut world, 0);
        world.entities.get_mut(key).unwrap().game.nextthink = 3.0;
        insane_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
        assert_eq!(ent.game.nextthink, 0.0);

        let cross = insane(&mut world, SPAWNFLAG_CRUSIFIED);
        world.entities.get_mut(cross).unwrap().game.flags = EntityFlags::empty();
        insane_dead(&mut world, cross);
        let ent = world.entities.get(cross).unwrap();
        assert!(ent.game.flags.contains(EntityFlags::FLY));
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert_eq!(ent.maxs, Vec3f::new(16.0, 8.0, 32.0));
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
    }

    #[test]
    fn insane_die_gibs_at_c_gib_health() {
        let (mut world, gi) = wire_world();
        let key = insane(&mut world, 0);
        world.entities.get_mut(key).unwrap().game.health = -50;
        let before = world.entities.count();
        insane_die(&mut world, key, key, key, 60, Vec3f::ZERO);
        assert_eq!(world.entities.count(), before + 6, "2 bones + 4 meat");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert!(ent.state.effects & EF_GIB != 0, "became the head");
        assert_eq!(wire_sounds(&gi)[0].1, "misc/udeath.wav");
    }

    #[test]
    fn insane_die_plays_a_death_by_pose_once() {
        let (mut world, gi) = wire_world();
        let stood = insane(&mut world, 0);
        world.entities.get_mut(stood).unwrap().game.health = -49;
        world.level.rand_state = seed_for(|w| w.random_u32() % 4 == 2);
        insane_die(&mut world, stood, stood, stood, 10, Vec3f::ZERO);
        assert_eq!(wire_sounds(&gi)[0].1, "player/male/death3.wav");
        assert!(plays(&world, stood, &INSANE_MOVE_STAND_DEATH));
        let ent = world.entities.get(stood).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage),
            (DeadFlag::Dead, TakeDamage::Yes)
        );
        insane_die(&mut world, stood, stood, stood, 10, Vec3f::ZERO);
        assert_eq!(gi.sounds.borrow().len(), 1, "already dead");

        let down = insane(&mut world, 0);
        world.entities.get_mut(down).unwrap().state.frame = FRAME_STAND160;
        insane_die(&mut world, down, down, down, 10, Vec3f::ZERO);
        assert!(plays(&world, down, &INSANE_MOVE_CRAWL_DEATH));

        let cross = insane(&mut world, SPAWNFLAG_CRUSIFIED);
        insane_die(&mut world, cross, cross, cross, 10, Vec3f::ZERO);
        let ent = world.entities.get(cross).unwrap();
        assert!(
            ent.svflags & SvFlags::DEADMONSTER.bits() != 0,
            "dead on the cross"
        );
        assert!(plays(&world, cross, &INSANE_MOVE_STAND_NORMAL));
    }

    /// A player in plain view never becomes the insane marine's enemy (C
    /// `FindTarget` ignores clients for AI_GOOD_GUY) and is never hurt:
    /// he only stands and fidgets.
    #[test]
    fn insane_in_view_of_a_player_never_targets_or_hurts_it() {
        let mut world = test_world();
        world.level.time = 1.0;
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        let ent = world.entities.get_mut(player).unwrap();
        ent.state.origin = Vec3f::new(200.0, 0.0, 0.0);
        ent.game.health = 100;
        world.level.sight_client_key = Some(player);
        let key = insane(&mut world, 0);
        insane_finish(&mut world, key, &HashMap::new());
        assert!(ai::monster_start(&mut world, key));

        let mut frames = Vec::new();
        for _ in 0..60 {
            world.level.time += FRAMETIME;
            let think = world.entities.get(key).unwrap().think.unwrap();
            think(&mut world, key);
            frames.push(world.entities.get(key).unwrap().state.frame);
        }
        assert_eq!(world.entities.get(key).unwrap().game.enemy, None);
        assert_eq!(world.entities.get(player).unwrap().game.health, 100);
        assert!(
            frames
                .iter()
                .all(|f| (FRAME_STAND1..=FRAME_STAND160).contains(f)),
            "{frames:?}"
        );
        assert!(frames.windows(2).any(|w| w[0] != w[1]), "animates");
    }
}
