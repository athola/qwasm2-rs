//! Boss3: the Jorg walker, the Makron it throws out when it dies, and the
//! Makron rider prop that teleports away when used.
//!
//! C ref: `~/Qwasm2/src/game/monster/boss3/` — `boss3.c` (the prop),
//! `boss31.c` (Jorg), `boss32.c` (Makron); frames from `boss32.h`, which
//! numbers every `boss31.h` frame the same.

use q2_shared::constants::*;
use q2_shared::protocol::SvcOp;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::fire::{
    monster_fire_bfg, monster_fire_blaster, monster_fire_bullet, monster_fire_railgun,
};
use crate::monster::flash::monster_flash_source;
use crate::monster::movement::vectoyaw;
use crate::monster::supertank::boss_explode;
use crate::monster::{frame, set_move, sound};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: boss32.h.
// ---------------------------------------------------------------------------

const FRAME_ATTAK101: i32 = 0;
const FRAME_ATTAK108: i32 = 7;
const FRAME_ATTAK109: i32 = 8;
const FRAME_ATTAK114: i32 = 13;
const FRAME_ATTAK115: i32 = 14;
const FRAME_ATTAK118: i32 = 17;
const FRAME_ATTAK201: i32 = 18;
const FRAME_ATTAK208: i32 = 25;
const FRAME_ATTAK213: i32 = 30;
const FRAME_DEATH01: i32 = 31;
const FRAME_DEATH50: i32 = 80;
const FRAME_PAIN101: i32 = 81;
const FRAME_PAIN103: i32 = 83;
const FRAME_PAIN201: i32 = 84;
const FRAME_PAIN203: i32 = 86;
const FRAME_PAIN301: i32 = 87;
const FRAME_PAIN325: i32 = 111;
const FRAME_STAND01: i32 = 112;
const FRAME_STAND51: i32 = 162;
const FRAME_WALK06: i32 = 168;
const FRAME_WALK19: i32 = 181;
const FRAME_ACTIVE01: i32 = 188;
const FRAME_ACTIVE13: i32 = 200;
const FRAME_ATTAK301: i32 = 201;
const FRAME_ATTAK308: i32 = 208;
const FRAME_ATTAK401: i32 = 209;
const FRAME_ATTAK405: i32 = 213;
const FRAME_ATTAK413: i32 = 221;
const FRAME_ATTAK421: i32 = 229;
const FRAME_ATTAK426: i32 = 234;
const FRAME_ATTAK501: i32 = 235;
const FRAME_ATTAK516: i32 = 250;
const FRAME_DEATH201: i32 = 251;
const FRAME_DEATH295: i32 = 345;
const FRAME_DEATH301: i32 = 346;
const FRAME_DEATH320: i32 = 365;
const FRAME_PAIN401: i32 = 379;
const FRAME_PAIN404: i32 = 382;
const FRAME_PAIN501: i32 = 383;
const FRAME_PAIN504: i32 = 386;
const FRAME_PAIN601: i32 = 387;
const FRAME_PAIN627: i32 = 413;
const FRAME_STAND201: i32 = 414;
const FRAME_STAND260: i32 = 473;
const FRAME_WALK204: i32 = 477;
const FRAME_WALK213: i32 = 486;

/// C model scale (both `boss31.h` and `boss32.h`).
const MODEL_SCALE: f32 = 1.0;

const RIDER_MODEL: &str = "models/monsters/boss3/rider/tris.md2";

// Jorg sounds. C ref: SP_monster_jorg.
const JORG_PAIN1: &str = "boss3/bs3pain1.wav";
const JORG_PAIN2: &str = "boss3/bs3pain2.wav";
const JORG_PAIN3: &str = "boss3/bs3pain3.wav";
const JORG_DEATH: &str = "boss3/bs3deth1.wav";
const JORG_ATTACK1: &str = "boss3/bs3atck1.wav";
const JORG_ATTACK2: &str = "boss3/bs3atck2.wav";
const JORG_SEARCH1: &str = "boss3/bs3srch1.wav";
const JORG_SEARCH2: &str = "boss3/bs3srch2.wav";
const JORG_SEARCH3: &str = "boss3/bs3srch3.wav";
const JORG_IDLE: &str = "boss3/bs3idle1.wav";
const JORG_STEP_LEFT: &str = "boss3/step1.wav";
const JORG_STEP_RIGHT: &str = "boss3/step2.wav";
const JORG_FIREGUN: &str = "boss3/xfire.wav";
const JORG_DEATH_HIT: &str = "boss3/d_hit.wav";
const JORG_SOUNDS: [&str; 14] = [
    JORG_PAIN1,
    JORG_PAIN2,
    JORG_PAIN3,
    JORG_DEATH,
    JORG_ATTACK1,
    JORG_ATTACK2,
    JORG_SEARCH1,
    JORG_SEARCH2,
    JORG_SEARCH3,
    JORG_IDLE,
    JORG_STEP_LEFT,
    JORG_STEP_RIGHT,
    JORG_FIREGUN,
    JORG_DEATH_HIT,
];

// Makron sounds. C ref: MakronPrecache.
const MAKRON_PAIN4: &str = "makron/pain3.wav";
const MAKRON_PAIN5: &str = "makron/pain2.wav";
const MAKRON_PAIN6: &str = "makron/pain1.wav";
const MAKRON_DEATH: &str = "makron/death.wav";
const MAKRON_STEP_LEFT: &str = "makron/step1.wav";
const MAKRON_STEP_RIGHT: &str = "makron/step2.wav";
const MAKRON_ATTACK_BFG: &str = "makron/bfg_fire.wav";
const MAKRON_BRAINSPLORCH: &str = "makron/brain1.wav";
const MAKRON_PRERAILGUN: &str = "makron/rail_up.wav";
const MAKRON_POPUP: &str = "makron/popup.wav";
const MAKRON_TAUNT1: &str = "makron/voice4.wav";
const MAKRON_TAUNT2: &str = "makron/voice3.wav";
const MAKRON_TAUNT3: &str = "makron/voice.wav";
const MAKRON_HIT: &str = "makron/bhit.wav";
const MAKRON_SOUNDS: [&str; 14] = [
    MAKRON_PAIN4,
    MAKRON_PAIN5,
    MAKRON_PAIN6,
    MAKRON_DEATH,
    MAKRON_STEP_LEFT,
    MAKRON_STEP_RIGHT,
    MAKRON_ATTACK_BFG,
    MAKRON_BRAINSPLORCH,
    MAKRON_PRERAILGUN,
    MAKRON_POPUP,
    MAKRON_TAUNT1,
    MAKRON_TAUNT2,
    MAKRON_TAUNT3,
    MAKRON_HIT,
];

// ---------------------------------------------------------------------------
// Moves. C ref: boss31.c `jorg_frames_*` / `jorg_move_*` and boss32.c
// `makron_frames_*` / `makron_move_*`. C's unused `jorg_move_start_walk`,
// `jorg_move_end_walk` and `makron_move_death3` are left out.
// ---------------------------------------------------------------------------

static JORG_FRAMES_STAND: [MonsterFrame; 51] = [
    frame(ai::ai_stand, 0.0, Some(jorg_idle)),
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
    frame(ai::ai_stand, 19.0, None),
    frame(ai::ai_stand, 11.0, Some(jorg_step_left)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 6.0, None),
    frame(ai::ai_stand, 9.0, Some(jorg_step_right)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -2.0, None),
    frame(ai::ai_stand, -17.0, Some(jorg_step_left)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -12.0, None),
    frame(ai::ai_stand, -14.0, Some(jorg_step_right)),
];

static JORG_FRAMES_RUN: [MonsterFrame; 14] = [
    frame(ai::ai_run, 17.0, Some(jorg_step_left)),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 8.0, None),
    frame(ai::ai_run, 10.0, None),
    frame(ai::ai_run, 33.0, Some(jorg_step_right)),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 9.0, None),
    frame(ai::ai_run, 9.0, None),
    frame(ai::ai_run, 9.0, None),
];

static JORG_FRAMES_WALK: [MonsterFrame; 14] = [
    frame(ai::ai_walk, 17.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 12.0, None),
    frame(ai::ai_walk, 8.0, None),
    frame(ai::ai_walk, 10.0, None),
    frame(ai::ai_walk, 33.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, 9.0, None),
];

static JORG_FRAMES_PAIN3: [MonsterFrame; 25] = [
    frame(ai::ai_move, -28.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -3.0, Some(jorg_step_left)),
    frame(ai::ai_move, -9.0, None),
    frame(ai::ai_move, 0.0, Some(jorg_step_right)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, -11.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 10.0, None),
    frame(ai::ai_move, 11.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 10.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 10.0, None),
    frame(ai::ai_move, 7.0, Some(jorg_step_left)),
    frame(ai::ai_move, 17.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(jorg_step_right)),
];

static JORG_FRAMES_PAIN2: [MonsterFrame; 3] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static JORG_FRAMES_PAIN1: [MonsterFrame; 3] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static JORG_FRAMES_DEATH1: [MonsterFrame; 50] = [
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
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(makron_toss)),
    frame(ai::ai_move, 0.0, Some(boss_explode)), // C: BossExplode (supertank.c)
];

static JORG_FRAMES_ATTACK2: [MonsterFrame; 13] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(jorg_bfg)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static JORG_FRAMES_START_ATTACK1: [MonsterFrame; 8] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

static JORG_FRAMES_ATTACK1: [MonsterFrame; 6] = [
    frame(ai::ai_charge, 0.0, Some(jorg_firebullet)),
    frame(ai::ai_charge, 0.0, Some(jorg_firebullet)),
    frame(ai::ai_charge, 0.0, Some(jorg_firebullet)),
    frame(ai::ai_charge, 0.0, Some(jorg_firebullet)),
    frame(ai::ai_charge, 0.0, Some(jorg_firebullet)),
    frame(ai::ai_charge, 0.0, Some(jorg_firebullet)),
];

static JORG_FRAMES_END_ATTACK1: [MonsterFrame; 4] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static JORG_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND51,
    frames: &JORG_FRAMES_STAND,
    endfunc: None,
};

static JORG_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK06,
    lastframe: FRAME_WALK19,
    frames: &JORG_FRAMES_RUN,
    endfunc: None,
};

static JORG_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK06,
    lastframe: FRAME_WALK19,
    frames: &JORG_FRAMES_WALK,
    endfunc: None,
};

static JORG_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN325,
    frames: &JORG_FRAMES_PAIN3,
    endfunc: Some(jorg_run),
};

static JORG_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN203,
    frames: &JORG_FRAMES_PAIN2,
    endfunc: Some(jorg_run),
};

static JORG_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN103,
    frames: &JORG_FRAMES_PAIN1,
    endfunc: Some(jorg_run),
};

static JORG_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH01,
    lastframe: FRAME_DEATH50,
    frames: &JORG_FRAMES_DEATH1,
    endfunc: Some(jorg_dead),
};

static JORG_MOVE_ATTACK2: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK213,
    frames: &JORG_FRAMES_ATTACK2,
    endfunc: Some(jorg_run),
};

static JORG_MOVE_START_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK108,
    frames: &JORG_FRAMES_START_ATTACK1,
    endfunc: Some(jorg_attack1),
};

static JORG_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK109,
    lastframe: FRAME_ATTAK114,
    frames: &JORG_FRAMES_ATTACK1,
    endfunc: Some(jorg_reattack1),
};

static JORG_MOVE_END_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK115,
    lastframe: FRAME_ATTAK118,
    frames: &JORG_FRAMES_END_ATTACK1,
    endfunc: Some(jorg_run),
};

static MAKRON_FRAMES_STAND: [MonsterFrame; 60] = [
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

static MAKRON_FRAMES_RUN: [MonsterFrame; 10] = [
    frame(ai::ai_run, 3.0, Some(makron_step_left)),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 8.0, None),
    frame(ai::ai_run, 8.0, None),
    frame(ai::ai_run, 8.0, Some(makron_step_right)),
    frame(ai::ai_run, 6.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 9.0, None),
    frame(ai::ai_run, 6.0, None),
    frame(ai::ai_run, 12.0, None),
];

static MAKRON_FRAMES_WALK: [MonsterFrame; 10] = [
    frame(ai::ai_walk, 3.0, Some(makron_step_left)),
    frame(ai::ai_walk, 12.0, None),
    frame(ai::ai_walk, 8.0, None),
    frame(ai::ai_walk, 8.0, None),
    frame(ai::ai_walk, 8.0, Some(makron_step_right)),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 12.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 12.0, None),
];

static MAKRON_FRAMES_PAIN6: [MonsterFrame; 27] = [
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
    frame(ai::ai_move, 0.0, Some(makron_popup)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(makron_taunt)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static MAKRON_FRAMES_PAIN5: [MonsterFrame; 4] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static MAKRON_FRAMES_PAIN4: [MonsterFrame; 4] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static MAKRON_FRAMES_DEATH2: [MonsterFrame; 95] = [
    frame(ai::ai_move, -15.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, -12.0, None),
    frame(ai::ai_move, 0.0, Some(makron_step_left)),
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
    frame(ai::ai_move, 11.0, None),
    frame(ai::ai_move, 12.0, None),
    frame(ai::ai_move, 11.0, Some(makron_step_right)),
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
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, 7.0, None),
    frame(ai::ai_move, 6.0, Some(makron_step_left)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 2.0, None),
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
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -6.0, Some(makron_step_right)),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -4.0, Some(makron_step_left)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -3.0, Some(makron_step_right)),
    frame(ai::ai_move, -8.0, None),
    frame(ai::ai_move, -3.0, Some(makron_step_left)),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -4.0, Some(makron_step_right)),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, 0.0, Some(makron_step_left)),
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
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 27.0, Some(makron_hit)),
    frame(ai::ai_move, 26.0, None),
    frame(ai::ai_move, 0.0, Some(makron_brainsplorch)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static MAKRON_FRAMES_SIGHT: [MonsterFrame; 13] = [
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

static MAKRON_FRAMES_ATTACK3: [MonsterFrame; 8] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(makron_bfg)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static MAKRON_FRAMES_ATTACK4: [MonsterFrame; 26] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, Some(makron_hyperblaster)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static MAKRON_FRAMES_ATTACK5: [MonsterFrame; 16] = [
    frame(ai::ai_charge, 0.0, Some(makron_prerailgun)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(makron_saveloc)),
    frame(ai::ai_move, 0.0, Some(makron_railgun)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static MAKRON_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND201,
    lastframe: FRAME_STAND260,
    frames: &MAKRON_FRAMES_STAND,
    endfunc: None,
};

static MAKRON_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK204,
    lastframe: FRAME_WALK213,
    frames: &MAKRON_FRAMES_RUN,
    endfunc: None,
};

static MAKRON_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK204,
    lastframe: FRAME_WALK213,
    frames: &MAKRON_FRAMES_WALK,
    endfunc: None,
};

static MAKRON_MOVE_PAIN6: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN601,
    lastframe: FRAME_PAIN627,
    frames: &MAKRON_FRAMES_PAIN6,
    endfunc: Some(makron_run),
};

static MAKRON_MOVE_PAIN5: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN501,
    lastframe: FRAME_PAIN504,
    frames: &MAKRON_FRAMES_PAIN5,
    endfunc: Some(makron_run),
};

static MAKRON_MOVE_PAIN4: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN401,
    lastframe: FRAME_PAIN404,
    frames: &MAKRON_FRAMES_PAIN4,
    endfunc: Some(makron_run),
};

static MAKRON_MOVE_DEATH2: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH201,
    lastframe: FRAME_DEATH295,
    frames: &MAKRON_FRAMES_DEATH2,
    endfunc: Some(makron_dead),
};

static MAKRON_MOVE_SIGHT: MonsterMove = MonsterMove {
    firstframe: FRAME_ACTIVE01,
    lastframe: FRAME_ACTIVE13,
    frames: &MAKRON_FRAMES_SIGHT,
    endfunc: Some(makron_run),
};

static MAKRON_MOVE_ATTACK3: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK301,
    lastframe: FRAME_ATTAK308,
    frames: &MAKRON_FRAMES_ATTACK3,
    endfunc: Some(makron_run),
};

static MAKRON_MOVE_ATTACK4: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK401,
    lastframe: FRAME_ATTAK426,
    frames: &MAKRON_FRAMES_ATTACK4,
    endfunc: Some(makron_run),
};

static MAKRON_MOVE_ATTACK5: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK501,
    lastframe: FRAME_ATTAK516,
    frames: &MAKRON_FRAMES_ATTACK5,
    endfunc: Some(makron_run),
};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn monsterinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MonsterInfo> {
    world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_deref_mut())
}

fn frame_of(world: &GameWorld, key: EntityKey) -> i32 {
    world.entities.get(key).map_or(0, |e| e.state.frame)
}

fn set_loop_sound(world: &mut GameWorld, key: EntityKey, index: i32) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.sound = index;
    }
}

/// The live enemy's origin, eye height and velocity.
fn enemy_of(world: &GameWorld, key: EntityKey) -> Option<(EntityKey, Vec3f, f32, Vec3f)> {
    let enemy = world.entities.get(key)?.game.enemy?;
    let e = world.entities.get(enemy)?;
    Some((enemy, e.state.origin, e.game.viewheight, e.velocity))
}

/// Half health shows the damaged skin. C: the first lines of both pains.
fn damaged_skin(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        if ent.game.health < ent.game.max_health / 2 {
            ent.state.skinnum = 1;
        }
    }
}

/// Jorg and Makron's `checkattack`: like `M_CheckAttack` but with fixed
/// chances (0.4 near or standing ground, 0.2 mid, never far), no skill
/// scaling, always attacking in melee range (with the missile, since
/// neither boss has a melee), and a shot check that ignores windows. C ref: `Jorg_CheckAttack` / `Makron_CheckAttack`, identical.
fn boss3_checkattack(world: &mut GameWorld, key: EntityKey) -> bool {
    let Some((enemy, enemy_origin, enemy_viewheight, _)) = enemy_of(world, key) else {
        return false;
    };
    let Some(me) = world.entities.get(key) else {
        return false;
    };
    let origin = me.state.origin;
    if world.entities.get(enemy).is_some_and(|e| e.game.health > 0) {
        // See if any entities are in the way of the shot.
        let spot1 = origin + Vec3f::new(0.0, 0.0, me.game.viewheight);
        let spot2 = enemy_origin + Vec3f::new(0.0, 0.0, enemy_viewheight);
        let tr = world.gi.trace(
            spot1,
            Vec3f::ZERO,
            Vec3f::ZERO,
            spot2,
            world.number_of(key),
            CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_SLIME | CONTENTS_LAVA,
        );
        // Do we have a clear shot?
        if tr.ent_index != world.number_of(enemy) {
            return false;
        }
    }

    let enemy_range = ai::range(world, key, enemy);
    let enemy_yaw = vectoyaw(enemy_origin - origin);
    let time = world.level.time;
    let fly = world
        .entities
        .get(key)
        .is_some_and(|e| e.game.flags.contains(EntityFlags::FLY));
    let Some(ent) = world.entities.get_mut(key) else {
        return false;
    };
    ent.game.ideal_yaw = enemy_yaw;
    let Some(mi) = ent.monsterinfo.as_deref_mut() else {
        return false;
    };

    // Melee attack.
    if enemy_range == Range::Melee {
        mi.attack_state = if mi.melee.is_some() {
            AttackState::Melee
        } else {
            AttackState::Missile
        };
        return true;
    }

    // Missile attack.
    if mi.attack.is_none() || time < mi.attack_finished || enemy_range == Range::Far {
        return false;
    }
    let chance = if mi.aiflags.contains(AiFlags::STAND_GROUND) || enemy_range == Range::Near {
        0.4
    } else if enemy_range == Range::Mid {
        0.2
    } else {
        return false;
    };

    if world.random() < chance {
        let wait = 2.0 * world.random();
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.attack_state = AttackState::Missile;
            mi.attack_finished = time + wait;
        }
        return true;
    }

    if fly {
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

// ---------------------------------------------------------------------------
// monster_boss3_stand. C ref: boss3.c.
// ---------------------------------------------------------------------------

/// Teleport away: the boss teleport effect, then gone. C ref: `Use_Boss3`.
pub fn use_boss3(world: &mut GameWorld, key: EntityKey, _other: EntityKey, _activator: EntityKey) {
    let Some(origin) = world.entities.get(key).map(|e| e.state.origin) else {
        return;
    };
    world.gi.write_byte(SvcOp::TempEntity as i32);
    world.gi.write_byte(TE_BOSSTPORT);
    world.gi.write_position(origin);
    world.gi.multicast(origin, Multicast::PVS);
    world.free_entity(key);
}

/// Cycle the rider's stand frames. C ref: `Think_Boss3Stand`.
pub fn think_boss3_stand(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.state.frame == FRAME_STAND260 {
        ent.state.frame = FRAME_STAND201;
    } else {
        ent.state.frame += 1;
    }
    ent.game.nextthink = time + FRAMETIME;
}

/// The Makron rider that stands until used, then teleports away; not a
/// monster. The `EntityStorage` half of C `SP_monster_boss3_stand`.
pub fn sp_monster_boss3_stand(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.game.model = RIDER_MODEL.to_string();
    ent.state.frame = FRAME_STAND201;
    ent.mins = Vec3f::new(-32.0, -32.0, 0.0);
    ent.maxs = Vec3f::new(32.0, 32.0, 90.0);
    ent.use_fn = Some(use_boss3);
    ent.think = Some(think_boss3_stand);
}

/// The `GameWorld` half of C `SP_monster_boss3_stand`: gone in
/// deathmatch, else precache the teleport sound and start cycling.
pub fn boss3_stand_finish(
    world: &mut GameWorld,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if world.game.deathmatch {
        world.free_entity(key);
        return;
    }
    world.gi.sound_index("misc/bigtele.wav");
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + FRAMETIME;
    }
}

// ---------------------------------------------------------------------------
// Jorg. C ref: boss31.c.
// ---------------------------------------------------------------------------

/// C ref: `jorg_search`.
pub fn jorg_search(world: &mut GameWorld, key: EntityKey) {
    let r = world.random();
    let name = if r <= 0.3 {
        JORG_SEARCH1
    } else if r <= 0.6 {
        JORG_SEARCH2
    } else {
        JORG_SEARCH3
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
}

/// C ref: `jorg_idle`.
pub fn jorg_idle(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, JORG_IDLE, ATTN_NORM);
}

/// Unused by C's moves, kept as C has it. C ref: `jorg_death_hit`.
pub fn jorg_death_hit(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, JORG_DEATH_HIT, ATTN_NORM);
}

/// C ref: `jorg_step_left`.
pub fn jorg_step_left(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, JORG_STEP_LEFT, ATTN_NORM);
}

/// C ref: `jorg_step_right`.
pub fn jorg_step_right(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, JORG_STEP_RIGHT, ATTN_NORM);
}

/// C ref: `jorg_stand`.
pub fn jorg_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &JORG_MOVE_STAND);
}

/// C ref: `jorg_walk`.
pub fn jorg_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &JORG_MOVE_WALK);
}

/// C ref: `jorg_run`.
pub fn jorg_run(world: &mut GameWorld, key: EntityKey) {
    let stand = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    let mv = if stand {
        &JORG_MOVE_STAND
    } else {
        &JORG_MOVE_RUN
    };
    set_move(world, key, mv);
}

/// Keep the chain guns going nine times in ten while the enemy is in
/// sight, else stop the gun loop and wind down. C ref: `jorg_reattack1`.
pub fn jorg_reattack1(world: &mut GameWorld, key: EntityKey) {
    let in_sight = enemy_of(world, key).is_some_and(|(enemy, ..)| ai::visible(world, key, enemy));
    if in_sight && world.random() < 0.9 {
        set_move(world, key, &JORG_MOVE_ATTACK1);
    } else {
        set_loop_sound(world, key, 0);
        set_move(world, key, &JORG_MOVE_END_ATTACK1);
    }
}

/// C ref: `jorg_attack1`.
pub fn jorg_attack1(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &JORG_MOVE_ATTACK1);
}

/// Hits of 40 or less are ignored 60% of the time; attack frames add
/// only a negligible extra chance (0.5% or 0.005%) to ignore pain. Then
/// a pain move by damage, none in nightmare. C ref: `jorg_pain`.
pub fn jorg_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    damage: i32,
) {
    damaged_skin(world, key);
    set_loop_sound(world, key, 0);
    let time = world.level.time;
    if world
        .entities
        .get(key)
        .is_some_and(|e| time < e.game.pain_debounce_time)
    {
        return;
    }
    // Lessen the chance of him going into his pain frames if he takes
    // little damage.
    if damage <= 40 && world.random() <= 0.6 {
        return;
    }
    // If he's entering his attack1 or using attack1, lessen the chance
    // of him going into pain.
    let frame = frame_of(world, key);
    if (FRAME_ATTAK101..=FRAME_ATTAK108).contains(&frame) && world.random() <= 0.005 {
        return;
    }
    if (FRAME_ATTAK109..=FRAME_ATTAK114).contains(&frame) && world.random() <= 0.00005 {
        return;
    }
    if (FRAME_ATTAK201..=FRAME_ATTAK208).contains(&frame) && world.random() <= 0.005 {
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.pain_debounce_time = time + 3.0;
    }
    if world.game.skill == crate::constants::SKILL_HARDPLUS {
        return; // No pain anims in nightmare.
    }
    if damage <= 50 {
        sound(world, key, CHAN_VOICE, JORG_PAIN1, ATTN_NORM);
        set_move(world, key, &JORG_MOVE_PAIN1);
    } else if damage <= 100 {
        sound(world, key, CHAN_VOICE, JORG_PAIN2, ATTN_NORM);
        set_move(world, key, &JORG_MOVE_PAIN2);
    } else if world.random() <= 0.3 {
        sound(world, key, CHAN_VOICE, JORG_PAIN3, ATTN_NORM);
        set_move(world, key, &JORG_MOVE_PAIN3);
    }
}

/// C ref: `jorgBFG`.
pub fn jorg_bfg(world: &mut GameWorld, key: EntityKey) {
    let (Some((_, enemy_origin, viewheight, _)), Some((start, _, _))) = (
        enemy_of(world, key),
        monster_flash_source(world, key, MZ2_JORG_BFG_1),
    ) else {
        return;
    };
    let target = enemy_origin + Vec3f::new(0.0, 0.0, viewheight);
    let dir = (target - start).normalize_or_zero();
    sound(world, key, CHAN_VOICE, JORG_ATTACK2, ATTN_NORM);
    monster_fire_bfg(world, key, start, dir, 50, 300, 100, 200.0, MZ2_JORG_BFG_1);
}

/// One chain gun leading the enemy by a fifth of a second.
/// C ref: `jorg_firebullet_left` / `jorg_firebullet_right`.
fn jorg_firebullet_from(world: &mut GameWorld, key: EntityKey, flash: i32) {
    let (Some((_, enemy_origin, viewheight, velocity)), Some((start, _, _))) = (
        enemy_of(world, key),
        monster_flash_source(world, key, flash),
    ) else {
        return;
    };
    let mut target = enemy_origin + velocity * -0.2;
    target.z += viewheight;
    let forward = (target - start).normalize_or_zero();
    monster_fire_bullet(
        world,
        key,
        start,
        forward,
        6,
        4,
        DEFAULT_BULLET_HSPREAD,
        DEFAULT_BULLET_VSPREAD,
        flash,
    );
}

/// Both chain guns, left first. C ref: `jorg_firebullet`.
pub fn jorg_firebullet(world: &mut GameWorld, key: EntityKey) {
    jorg_firebullet_from(world, key, MZ2_JORG_MACHINEGUN_L1);
    jorg_firebullet_from(world, key, MZ2_JORG_MACHINEGUN_R1);
}

/// Chain guns (with their loop sound) three times in four, else the BFG.
/// C ref: `jorg_attack`.
pub fn jorg_attack(world: &mut GameWorld, key: EntityKey) {
    if world.random() <= 0.75 {
        sound(world, key, CHAN_VOICE, JORG_ATTACK1, ATTN_NORM);
        let index = world.gi.sound_index("boss3/w_loop.wav");
        set_loop_sound(world, key, index);
        set_move(world, key, &JORG_MOVE_START_ATTACK1);
    } else {
        sound(world, key, CHAN_VOICE, JORG_ATTACK2, ATTN_NORM);
        set_move(world, key, &JORG_MOVE_ATTACK2);
    }
}

/// The death move's end function; does nothing in C either (the move's
/// last frame starts `BossExplode`). C ref: `jorg_dead`.
pub fn jorg_dead(_world: &mut GameWorld, _key: EntityKey) {}

/// C ref: `jorg_die`.
pub fn jorg_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    sound(world, key, CHAN_VOICE, JORG_DEATH, ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::No;
        ent.state.sound = 0;
        ent.game.count = 0;
    }
    set_move(world, key, &JORG_MOVE_DEATH);
}

/// C ref: `Jorg_CheckAttack`.
pub fn jorg_checkattack(world: &mut GameWorld, key: EntityKey) -> bool {
    boss3_checkattack(world, key)
}

/// The `EntityStorage` half of C `SP_monster_jorg`.
pub fn sp_monster_jorg(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-80.0, -80.0, 0.0);
    ent.maxs = Vec3f::new(80.0, 80.0, 140.0);
    ent.game.health = 3000;
    ent.game.gib_health = -2000;
    ent.game.mass = 1000;
    ent.pain = Some(jorg_pain);
    ent.die = Some(jorg_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(jorg_stand),
        walk: Some(jorg_walk),
        run: Some(jorg_run),
        attack: Some(jorg_attack),
        search: Some(jorg_search),
        checkattack: Some(jorg_checkattack),
        currentmove: Some(JORG_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_jorg`: precache Jorg and the
/// Makron he carries, and draw the rider on top of him.
pub fn jorg_finish(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    for name in JORG_SOUNDS {
        world.gi.sound_index(name);
    }
    makron_precache(world);
    let rider = world.gi.model_index(RIDER_MODEL);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.modelindex2 = rider;
    }
}

// ---------------------------------------------------------------------------
// Makron. C ref: boss32.c.
// ---------------------------------------------------------------------------

/// C ref: `makron_taunt`.
pub fn makron_taunt(world: &mut GameWorld, key: EntityKey) {
    let r = world.random();
    let name = if r <= 0.3 {
        MAKRON_TAUNT1
    } else if r <= 0.6 {
        MAKRON_TAUNT2
    } else {
        MAKRON_TAUNT3
    };
    sound(world, key, CHAN_AUTO, name, ATTN_NONE);
}

/// C ref: `makron_stand`.
pub fn makron_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MAKRON_MOVE_STAND);
}

/// C ref: `makron_hit`.
pub fn makron_hit(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_AUTO, MAKRON_HIT, ATTN_NONE);
}

/// C ref: `makron_popup`.
pub fn makron_popup(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, MAKRON_POPUP, ATTN_NONE);
}

/// C ref: `makron_step_left`.
pub fn makron_step_left(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, MAKRON_STEP_LEFT, ATTN_NORM);
}

/// C ref: `makron_step_right`.
pub fn makron_step_right(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, MAKRON_STEP_RIGHT, ATTN_NORM);
}

/// C ref: `makron_brainsplorch`.
pub fn makron_brainsplorch(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, MAKRON_BRAINSPLORCH, ATTN_NORM);
}

/// C ref: `makron_prerailgun`.
pub fn makron_prerailgun(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, MAKRON_PRERAILGUN, ATTN_NORM);
}

/// C ref: `makron_walk`.
pub fn makron_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MAKRON_MOVE_WALK);
}

/// C ref: `makron_run`.
pub fn makron_run(world: &mut GameWorld, key: EntityKey) {
    let stand = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    let mv = if stand {
        &MAKRON_MOVE_STAND
    } else {
        &MAKRON_MOVE_RUN
    };
    set_move(world, key, mv);
}

/// C ref: `makronBFG`.
pub fn makron_bfg(world: &mut GameWorld, key: EntityKey) {
    let (Some((_, enemy_origin, viewheight, _)), Some((start, _, _))) = (
        enemy_of(world, key),
        monster_flash_source(world, key, MZ2_MAKRON_BFG),
    ) else {
        return;
    };
    let target = enemy_origin + Vec3f::new(0.0, 0.0, viewheight);
    let dir = (target - start).normalize_or_zero();
    sound(world, key, CHAN_VOICE, MAKRON_ATTACK_BFG, ATTN_NORM);
    monster_fire_bfg(world, key, start, dir, 50, 300, 100, 300.0, MZ2_MAKRON_BFG);
}

/// Remember where the enemy's eyes are for the railgun shot on the next
/// frame. `game.move_origin` stands in for C `pos1`, which the entity
/// does not carry. C ref: `MakronSaveloc`.
pub fn makron_saveloc(world: &mut GameWorld, key: EntityKey) {
    let Some((_, enemy_origin, viewheight, _)) = enemy_of(world, key) else {
        return;
    };
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.move_origin = enemy_origin + Vec3f::new(0.0, 0.0, viewheight);
    }
}

/// Fire at the spot `makron_saveloc` saved. C ref: `MakronRailgun`.
pub fn makron_railgun(world: &mut GameWorld, key: EntityKey) {
    let Some((start, _, _)) = monster_flash_source(world, key, MZ2_MAKRON_RAILGUN_1) else {
        return;
    };
    let Some(pos1) = world.entities.get(key).map(|e| e.game.move_origin) else {
        return;
    };
    let dir = (pos1 - start).normalize_or_zero();
    monster_fire_railgun(world, key, start, dir, 50, 100, MZ2_MAKRON_RAILGUN_1);
}

/// One bolt of the sweep: pitch at the enemy, yaw swinging 10 degrees a
/// frame across the front. C ref: `MakronHyperblaster`.
pub fn makron_hyperblaster(world: &mut GameWorld, key: EntityKey) {
    let frame = frame_of(world, key);
    let flash = MZ2_MAKRON_BLASTER_1 + (frame - FRAME_ATTAK405);
    let Some((start, _, _)) = monster_flash_source(world, key, flash) else {
        return;
    };
    let Some(angles) = world.entities.get(key).map(|e| e.state.angles) else {
        return;
    };
    let pitch = match enemy_of(world, key) {
        Some((_, enemy_origin, viewheight, _)) => {
            let target = enemy_origin + Vec3f::new(0.0, 0.0, viewheight);
            crate::weapons::vectoangles(target - start).x
        }
        None => 0.0,
    };
    let yaw = if frame <= FRAME_ATTAK413 {
        angles.y - 10.0 * (frame - FRAME_ATTAK413) as f32
    } else {
        angles.y + 10.0 * (frame - FRAME_ATTAK421) as f32
    };
    let (forward, _, _) = crate::physics::angle_vectors(Vec3f::new(pitch, yaw, 0.0));
    // C projects from the frame's offset but always sends BLASTER_1, so
    // the client draws every bolt's flash at the first muzzle.
    monster_fire_blaster(
        world,
        key,
        start,
        forward,
        15,
        1000,
        MZ2_MAKRON_BLASTER_1,
        EF_BLASTER,
    );
}

/// Small hits are shrugged off one time in five; then a pain move by
/// damage (the big one only sometimes), none in nightmare.
/// C ref: `makron_pain`.
pub fn makron_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    damage: i32,
) {
    damaged_skin(world, key);
    let time = world.level.time;
    if world
        .entities
        .get(key)
        .is_some_and(|e| time < e.game.pain_debounce_time)
    {
        return;
    }
    // Lessen the chance of him going into his pain frames.
    if damage <= 25 && world.random() < 0.2 {
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.pain_debounce_time = time + 3.0;
    }
    if world.game.skill == crate::constants::SKILL_HARDPLUS {
        return; // No pain anims in nightmare.
    }
    if damage <= 40 {
        sound(world, key, CHAN_VOICE, MAKRON_PAIN4, ATTN_NONE);
        set_move(world, key, &MAKRON_MOVE_PAIN4);
    } else if damage <= 110 {
        sound(world, key, CHAN_VOICE, MAKRON_PAIN5, ATTN_NONE);
        set_move(world, key, &MAKRON_MOVE_PAIN5);
    } else {
        let chance = if damage <= 150 { 0.45 } else { 0.35 };
        if world.random() <= chance {
            sound(world, key, CHAN_VOICE, MAKRON_PAIN6, ATTN_NONE);
            set_move(world, key, &MAKRON_MOVE_PAIN6);
        }
    }
}

/// BFG, hyperblaster sweep or railgun, a third each. C ref: `makron_attack`.
pub fn makron_attack(world: &mut GameWorld, key: EntityKey) {
    let r = world.random();
    let mv = if r <= 0.3 {
        &MAKRON_MOVE_ATTACK3
    } else if r <= 0.6 {
        &MAKRON_MOVE_ATTACK4
    } else {
        &MAKRON_MOVE_ATTACK5
    };
    set_move(world, key, mv);
}

/// The fallen torso writhes on its own until shot apart, and goes away if
/// its Makron is revived. C ref: `makron_torso_think`.
pub fn makron_torso_think(world: &mut GameWorld, key: EntityKey) {
    let Some(owner) = world.entities.get(key).and_then(|e| e.owner) else {
        return advance_torso(world, key);
    };
    // Detach from the Makron if the legs are gone completely.
    match world.entities.get(owner) {
        Some(legs) if legs.in_use && legs.game.health > legs.game.gib_health => {
            // If the Makron is revived the torso was put back on him.
            if legs.game.deadflag != DeadFlag::Dead {
                world.free_entity(key);
                return;
            }
        }
        _ => {
            if let Some(ent) = world.entities.get_mut(key) {
                ent.owner = None;
            }
        }
    }
    advance_torso(world, key);
}

fn advance_torso(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.state.frame += 1;
    if ent.state.frame >= FRAME_DEATH320 {
        ent.state.frame = FRAME_DEATH301;
    }
    ent.game.nextthink = time + FRAMETIME;
}

/// Place the torso up to 84 units behind the Makron.
/// C ref: `makron_torso_origin`.
fn makron_torso_origin(world: &mut GameWorld, key: EntityKey, torso: EntityKey) {
    let (Some(me), Some(t)) = (world.entities.get(key), world.entities.get(torso)) else {
        return;
    };
    let (forward, _, _) = crate::physics::angle_vectors(me.state.angles);
    let end = me.state.origin + forward * -84.0;
    let tr = world.gi.trace(
        me.state.origin,
        t.mins,
        t.maxs,
        end,
        world.number_of(key),
        MASK_SOLID,
    );
    if let Some(t) = world.entities.get_mut(torso) {
        t.state.origin = tr.endpos;
    }
}

/// Only enough damage gibs the torso. C ref: `makron_torso_die`.
pub fn makron_torso_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.game.health > e.game.gib_health)
    {
        return;
    }
    sound(world, key, CHAN_VOICE, "misc/udeath.wav", ATTN_NORM);
    throw_gib(
        world,
        key,
        "models/objects/gibs/sm_meat/tris.md2",
        damage,
        GibType::Organic,
    );
    for _ in 0..4 {
        throw_gib(
            world,
            key,
            "models/objects/gibs/sm_metal/tris.md2",
            damage,
            GibType::Metallic,
        );
    }
    world.free_entity(key);
}

/// The torso falls off the dying Makron as its own dead entity.
/// C ref: `makron_torso`.
pub fn makron_torso(world: &mut GameWorld, key: EntityKey) {
    let Some(angles) = world.entities.get(key).map(|e| e.state.angles) else {
        return;
    };
    let Some(torso) = world.spawn() else {
        return;
    };
    if let Some(t) = world.entities.get_mut(torso) {
        t.state.angles = angles;
        t.mins = Vec3f::new(-24.0, -24.0, 0.0);
        t.maxs = Vec3f::new(24.0, 24.0, 16.0);
    }
    makron_torso_origin(world, key, torso);
    let modelindex = world.gi.model_index(RIDER_MODEL);
    let spine = world.gi.sound_index("makron/spine.wav");
    let time = world.level.time;
    if let Some(t) = world.entities.get_mut(torso) {
        t.game.gib_health = -800;
        t.game.takedamage = TakeDamage::Yes;
        t.die = Some(makron_torso_die);
        t.game.deadflag = DeadFlag::Dead;
        t.owner = Some(key);
        t.game.movetype = MoveType::Toss;
        t.solid = Solid::Bbox;
        t.svflags = SvFlags::MONSTER.bits() | SvFlags::DEADMONSTER.bits();
        t.clipmask = MASK_MONSTERSOLID;
        t.state.frame = FRAME_DEATH301;
        t.state.modelindex = modelindex;
        t.think = Some(makron_torso_think);
        t.game.nextthink = time + 2.0 * FRAMETIME;
        t.state.sound = spine;
    }
    world.link_entity(torso);
}

/// The legs' corpse. C ref: `makron_dead`.
pub fn makron_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-48.0, -48.0, 0.0);
        ent.maxs = Vec3f::new(48.0, 48.0, 24.0);
    }
    ai::monster_dead(world, key);
}

/// Gib at `gib_health`, else drop the torso and fall.
/// C ref: `makron_die`.
pub fn makron_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    set_loop_sound(world, key, 0);
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    // Check for gib.
    if ent.game.health <= ent.game.gib_health {
        sound(world, key, CHAN_VOICE, "misc/udeath.wav", ATTN_NORM);
        throw_gib(
            world,
            key,
            "models/objects/gibs/sm_meat/tris.md2",
            damage,
            GibType::Organic,
        );
        for _ in 0..4 {
            throw_gib(
                world,
                key,
                "models/objects/gibs/sm_metal/tris.md2",
                damage,
                GibType::Metallic,
            );
        }
        throw_head(
            world,
            key,
            "models/objects/gibs/gear/tris.md2",
            damage,
            GibType::Metallic,
        );
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.deadflag = DeadFlag::Dead;
        }
        return;
    }
    if ent.game.deadflag == DeadFlag::Dead {
        return;
    }

    // Regular death.
    sound(world, key, CHAN_VOICE, MAKRON_DEATH, ATTN_NONE);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    makron_torso(world, key);
    // Lower the box since the torso is gone.
    if let Some(ent) = world.entities.get_mut(key) {
        ent.maxs.z = 64.0;
    }
    world.link_entity(key);
    set_move(world, key, &MAKRON_MOVE_DEATH2);
}

/// C ref: `Makron_CheckAttack`.
pub fn makron_checkattack(world: &mut GameWorld, key: EntityKey) -> bool {
    boss3_checkattack(world, key)
}

/// C ref: `MakronPrecache`.
fn makron_precache(world: &mut GameWorld) {
    for name in MAKRON_SOUNDS {
        world.gi.sound_index(name);
    }
    world.gi.model_index(RIDER_MODEL);
}

/// The `EntityStorage` half of C `SP_monster_makron`. It starts on its
/// jump-out (sight) move.
pub fn sp_monster_makron(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-30.0, -30.0, 0.0);
    ent.maxs = Vec3f::new(30.0, 30.0, 90.0);
    ent.game.health = 3000;
    ent.game.gib_health = -2000;
    ent.game.mass = 500;
    ent.pain = Some(makron_pain);
    ent.die = Some(makron_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(makron_stand),
        walk: Some(makron_walk),
        run: Some(makron_run),
        attack: Some(makron_attack),
        checkattack: Some(makron_checkattack),
        currentmove: Some(MAKRON_MOVE_SIGHT.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_makron`.
pub fn makron_finish(world: &mut GameWorld, _key: EntityKey, _fields: &HashMap<String, String>) {
    makron_precache(world);
}

fn alive(world: &GameWorld, key: Option<EntityKey>) -> Option<EntityKey> {
    key.filter(|&k| {
        world
            .entities
            .get(k)
            .is_some_and(|e| e.in_use && e.game.deadflag != DeadFlag::Dead)
    })
}

/// The Makron jumps out of the dying Jorg: spawn him in place as a map
/// would, keep Jorg's enemies, and leap at the enemy (or forward).
/// C ref: `MakronSpawn`.
pub fn makron_spawn(world: &mut GameWorld, key: EntityKey) {
    // Spawning can mess with enemy state so clear it temporarily.
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let enemy = ent.game.enemy.take();
    let oldenemy = ent.game.oldenemy.take();

    // C SP_monster_makron, as GameWorld::call_spawn runs a spawn row.
    sp_monster_makron(&mut world.entities, key, &HashMap::new());
    makron_finish(world, key, &HashMap::new());
    let modelindex = world.gi.model_index(RIDER_MODEL);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.modelindex = modelindex;
    }
    if !ai::monster_start(world, key) {
        return;
    }
    if let Some(think) = world.entities.get(key).and_then(|e| e.think) {
        think(world, key);
    }

    // And re-link enemy state now that he's spawned.
    let enemy = alive(world, enemy);
    let oldenemy = alive(world, oldenemy);
    let (enemy, oldenemy) = match enemy {
        Some(e) => (Some(e), oldenemy),
        None => (oldenemy, None),
    };
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.enemy = enemy;
        ent.game.oldenemy = oldenemy;
    }

    if enemy.is_some() {
        ai::found_target(world, key);
        let pos1 = world
            .entities
            .get(key)
            .map_or(Vec3f::ZERO, |e| e.game.move_origin);
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.last_sighting = pos1;
        }
    }

    let Some(me) = world.entities.get(key) else {
        return;
    };
    let (origin, angles) = (me.state.origin, me.state.angles);
    let toward = enemy
        .filter(|&e| ai::visible(world, key, e))
        .and_then(|e| world.entities.get(e))
        .map(|e| e.state.origin - origin);
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let dir = match toward {
        Some(vec) => {
            ent.state.angles.y = vectoyaw(vec);
            vec.normalize_or_zero()
        }
        None => crate::physics::angle_vectors(angles).0,
    };
    ent.velocity = dir * 400.0;
    // The jump frames are fixed length so best to normalize the up speed.
    // C reads sv_gravity, which is not a cvar here.
    ent.velocity.z = 200.0 * (DEFAULT_GRAVITY / 800.0);
    ent.game.ground_entity = None;
    ent.state.origin.z += 1.0;
    ent.game.pain_debounce_time = time + 1.0;
    world.link_entity(key);
    set_move(world, key, &MAKRON_MOVE_SIGHT);
}

/// Jorg is just about dead: a placeholder entity that becomes the Makron
/// 0.8 s later where Jorg stands. `game.move_origin` carries C `pos1`
/// (Jorg's last sighting) to `makron_spawn`. C ref: `MakronToss`.
pub fn makron_toss(world: &mut GameWorld, key: EntityKey) {
    let Some(jorg) = world.entities.get(key) else {
        return;
    };
    let target = jorg.game.target.clone();
    let (origin, angles) = (jorg.state.origin, jorg.state.angles);
    let last_sighting = jorg
        .monsterinfo
        .as_ref()
        .map_or(Vec3f::ZERO, |mi| mi.last_sighting);
    let (enemy, oldenemy) = (jorg.game.enemy, jorg.game.oldenemy);
    let time = world.level.time;
    let Some(makron) = world.spawn() else {
        return;
    };
    let Some(ent) = world.entities.get_mut(makron) else {
        return;
    };
    ent.game.classname = "monster_makron".to_string();
    ent.game.nextthink = time + 0.8;
    ent.think = Some(makron_spawn);
    ent.game.target = target;
    ent.state.origin = origin;
    ent.state.angles = angles;
    ent.game.move_origin = last_sighting;
    ent.game.enemy = enemy;
    ent.game.oldenemy = oldenemy;
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    const NAMES: &[(ThinkFn, &str)] = &[
        (jorg_idle, "jorg_idle"),
        (jorg_step_left, "jorg_step_left"),
        (jorg_step_right, "jorg_step_right"),
        (makron_toss, "makron_toss"),
        (boss_explode, "boss_explode"),
        (jorg_bfg, "jorg_bfg"),
        (jorg_firebullet, "jorg_firebullet"),
        (jorg_run, "jorg_run"),
        (jorg_dead, "jorg_dead"),
        (jorg_attack1, "jorg_attack1"),
        (jorg_reattack1, "jorg_reattack1"),
        (makron_step_left, "makron_step_left"),
        (makron_step_right, "makron_step_right"),
        (makron_popup, "makron_popup"),
        (makron_taunt, "makron_taunt"),
        (makron_hit, "makron_hit"),
        (makron_brainsplorch, "makron_brainsplorch"),
        (makron_bfg, "makron_bfg"),
        (makron_hyperblaster, "makron_hyperblaster"),
        (makron_prerailgun, "makron_prerailgun"),
        (makron_saveloc, "makron_saveloc"),
        (makron_railgun, "makron_railgun"),
        (makron_run, "makron_run"),
        (makron_dead, "makron_dead"),
    ];

    use crate::entity::ThinkFn;
    use crate::misc::wire::{wire_world, Wire, WireImport};
    use crate::monster::test_support::{assert_moves_match_c, CMoveRow};
    use crate::world::{test_world, test_world_recording, MockGameImport};

    #[test]
    fn jorg_moves_match_c_boss31_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&JORG_MOVE_STAND, 112, 162, "-", "stand:0:jorg_idle stand:0:-*32 stand:19:- stand:11:jorg_step_left stand:0:-*2 stand:6:- stand:9:jorg_step_right stand:0:-*7 stand:-2:- stand:-17:jorg_step_left stand:0:- stand:-12:- stand:-14:jorg_step_right"),
            (&JORG_MOVE_RUN, 168, 181, "-", "run:17:jorg_step_left run:0:-*3 run:12:- run:8:- run:10:- run:33:jorg_step_right run:0:-*3 run:9:-*3"),
            (&JORG_MOVE_WALK, 168, 181, "-", "walk:17:- walk:0:-*3 walk:12:- walk:8:- walk:10:- walk:33:- walk:0:-*3 walk:9:-*3"),
            (&JORG_MOVE_PAIN3, 87, 111, "jorg_run", "move:-28:- move:-6:- move:-3:jorg_step_left move:-9:- move:0:jorg_step_right move:0:-*3 move:-7:- move:1:- move:-11:- move:-4:- move:0:-*2 move:10:- move:11:- move:0:- move:10:- move:3:- move:10:- move:7:jorg_step_left move:17:- move:0:-*2 move:0:jorg_step_right"),
            (&JORG_MOVE_PAIN2, 84, 86, "jorg_run", "move:0:-*3"),
            (&JORG_MOVE_PAIN1, 81, 83, "jorg_run", "move:0:-*3"),
            (&JORG_MOVE_DEATH, 31, 80, "jorg_dead", "move:0:-*48 move:0:makron_toss move:0:boss_explode"),
            (&JORG_MOVE_ATTACK2, 18, 30, "jorg_run", "charge:0:-*6 charge:0:jorg_bfg move:0:-*6"),
            (&JORG_MOVE_START_ATTACK1, 0, 7, "jorg_attack1", "charge:0:-*8"),
            (&JORG_MOVE_ATTACK1, 8, 13, "jorg_reattack1", "charge:0:jorg_firebullet*6"),
            (&JORG_MOVE_END_ATTACK1, 14, 17, "jorg_run", "move:0:-*4"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    #[test]
    fn makron_moves_match_c_boss32_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&MAKRON_MOVE_STAND, 414, 473, "-", "stand:0:-*60"),
            (&MAKRON_MOVE_RUN, 477, 486, "-", "run:3:makron_step_left run:12:- run:8:-*2 run:8:makron_step_right run:6:- run:12:- run:9:- run:6:- run:12:-"),
            (&MAKRON_MOVE_WALK, 477, 486, "-", "walk:3:makron_step_left walk:12:- walk:8:-*2 walk:8:makron_step_right walk:6:- walk:12:- walk:9:- walk:6:- walk:12:-"),
            (&MAKRON_MOVE_PAIN6, 387, 413, "makron_run", "move:0:-*15 move:0:makron_popup move:0:-*7 move:0:makron_taunt move:0:-*3"),
            (&MAKRON_MOVE_PAIN5, 383, 386, "makron_run", "move:0:-*4"),
            (&MAKRON_MOVE_PAIN4, 379, 382, "makron_run", "move:0:-*4"),
            (&MAKRON_MOVE_DEATH2, 251, 345, "makron_dead", "move:-15:- move:3:- move:-12:- move:0:makron_step_left move:0:-*11 move:11:- move:12:- move:11:makron_step_right move:0:-*15 move:5:- move:7:- move:6:makron_step_left move:0:-*2 move:-1:- move:2:- move:0:-*13 move:-6:- move:-4:- move:-6:makron_step_right move:-4:- move:-4:makron_step_left move:0:-*4 move:-2:- move:-5:- move:-3:makron_step_right move:-8:- move:-3:makron_step_left move:-7:- move:-4:- move:-4:makron_step_right move:-6:- move:-7:- move:0:makron_step_left move:0:-*12 move:-2:- move:0:-*2 move:2:- move:0:- move:27:makron_hit move:26:- move:0:makron_brainsplorch move:0:-*2"),
            (&MAKRON_MOVE_SIGHT, 188, 200, "makron_run", "move:0:-*13"),
            (&MAKRON_MOVE_ATTACK3, 201, 208, "makron_run", "charge:0:-*3 charge:0:makron_bfg move:0:-*4"),
            (&MAKRON_MOVE_ATTACK4, 209, 234, "makron_run", "charge:0:-*4 move:0:makron_hyperblaster*17 move:0:-*5"),
            (&MAKRON_MOVE_ATTACK5, 235, 250, "makron_run", "charge:0:makron_prerailgun charge:0:-*6 charge:0:makron_saveloc move:0:makron_railgun move:0:-*7"),
        ];
        assert_moves_match_c(c, NAMES);
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

    type SpawnFn = fn(&mut EntityStorage, EntityKey, &HashMap<String, String>);

    fn spawned(world: &mut GameWorld, spawn: SpawnFn) -> EntityKey {
        let key = world.spawn().unwrap();
        spawn(&mut world.entities, key, &HashMap::new());
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
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
    }

    fn plays(world: &GameWorld, key: EntityKey, mv: &MonsterMove) -> bool {
        mi(world, key)
            .currentmove
            .as_ref()
            .is_some_and(|cm| std::ptr::eq(cm.frames, mv.frames))
    }

    /// Names of the sounds `key`'s world played, in order.
    fn played(gi: &MockGameImport) -> Vec<String> {
        let names = gi.sounds.borrow();
        gi.sound_calls
            .borrow()
            .iter()
            .map(|c| names[c.2 as usize - 1].clone())
            .collect()
    }

    /// (channel, name, attenuation) of every sound a wire world played.
    fn wire_sounds(gi: &WireImport) -> Vec<(i32, String, f32)> {
        let names = gi.inner.sounds.borrow();
        gi.sounds
            .borrow()
            .iter()
            .map(|&(_, chan, index, attn)| (chan, names[index as usize - 1].clone(), attn))
            .collect()
    }

    /// The flash numbers of every `svc_muzzleflash2` on the wire.
    fn flashes(gi: &WireImport) -> Vec<i32> {
        let wire = gi.wire.borrow();
        wire.windows(3)
            .filter_map(|w| match w {
                [Wire::Byte(op), Wire::Short(_), Wire::Byte(flash)]
                    if *op == SvcOp::MuzzleFlash2 as i32 =>
                {
                    Some(*flash)
                }
                _ => None,
            })
            .collect()
    }

    /// A target at `at` with eyes 22 up, the enemy of `key`.
    fn enemy_at(world: &mut GameWorld, key: EntityKey, at: Vec3f) -> EntityKey {
        let enemy = world.spawn().unwrap();
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = at;
        e.game.viewheight = 22.0;
        e.game.health = 100;
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        enemy
    }

    fn hit(world: &GameWorld, ent: EntityKey) -> Trace {
        Trace {
            fraction: 0.5,
            ent_index: world.number_of(ent),
            ..Trace::default()
        }
    }

    // ----- monster_boss3_stand -----

    #[test]
    fn boss3_stand_is_a_rider_prop_cycling_stand201_to_260() {
        let mut world = test_world();
        world.level.time = 2.0;
        let key = spawned(&mut world, sp_monster_boss3_stand);
        boss3_stand_finish(&mut world, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert!(ent.monsterinfo.is_none());
        assert_eq!(
            (ent.game.movetype, ent.solid),
            (MoveType::Step, Solid::Bbox)
        );
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-32.0, -32.0, 0.0), Vec3f::new(32.0, 32.0, 90.0))
        );
        assert_eq!(ent.state.frame, FRAME_STAND201);
        assert_eq!(ent.game.nextthink, 2.1);
        think_boss3_stand(&mut world, key);
        assert_eq!(
            world.entities.get(key).unwrap().state.frame,
            FRAME_STAND201 + 1
        );
        world.entities.get_mut(key).unwrap().state.frame = FRAME_STAND260;
        think_boss3_stand(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().state.frame, FRAME_STAND201);
    }

    #[test]
    fn boss3_stand_is_removed_in_deathmatch() {
        let mut world = test_world();
        world.game.deathmatch = true;
        let key = spawned(&mut world, sp_monster_boss3_stand);
        boss3_stand_finish(&mut world, key, &HashMap::new());
        assert!(!world.entities.is_live(key));
    }

    #[test]
    fn using_boss3_stand_teleports_it_away() {
        let (mut world, gi) = wire_world();
        let key = spawned(&mut world, sp_monster_boss3_stand);
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(1.0, 2.0, 3.0);
        let use_fn = world.entities.get(key).unwrap().use_fn.unwrap();
        use_fn(&mut world, key, key, key);
        let at = Vec3f::new(1.0, 2.0, 3.0);
        assert_eq!(
            *gi.wire.borrow(),
            [
                Wire::Byte(SvcOp::TempEntity as i32),
                Wire::Byte(22),
                Wire::Position(at),
                Wire::Multicast(at, Multicast::PVS)
            ]
        );
        assert!(!world.entities.is_live(key));
    }

    // ----- Jorg -----

    #[test]
    fn jorg_spawns_with_c_values_and_precaches_the_makron() {
        let (mut world, gi) = wire_world();
        let key = spawned(&mut world, sp_monster_jorg);
        jorg_finish(&mut world, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.health, ent.game.mass), (3000, 1000));
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-80.0, -80.0, 0.0), Vec3f::new(80.0, 80.0, 140.0))
        );
        assert_eq!(
            (ent.game.movetype, ent.solid),
            (MoveType::Step, Solid::Bbox)
        );
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        assert_ne!(ent.state.modelindex2, 0, "the rider");
        let mi = mi(&world, key);
        assert!(mi.melee.is_none() && mi.sight.is_none() && mi.dodge.is_none());
        assert!(std::ptr::fn_addr_eq(
            mi.checkattack.unwrap(),
            jorg_checkattack as fn(&mut GameWorld, EntityKey) -> bool
        ));
        assert!(mi.search.is_some() && mi.attack.is_some());
        assert_eq!(mi.scale, 1.0);
        assert!(plays(&world, key, &JORG_MOVE_STAND));
        let names = gi.inner.sounds.borrow();
        assert_eq!(names.len(), 28, "Jorg's 14 and the Makron's 14");
        assert!(names.contains(&"boss3/xfire.wav".to_string()));
        assert!(names.contains(&"makron/bhit.wav".to_string()));
    }

    #[test]
    fn jorg_search_picks_one_of_three_by_thirds() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_jorg);
        for (roll, name) in [
            (0.0..0.3, JORG_SEARCH1),
            (0.31..0.6, JORG_SEARCH2),
            (0.61..1.01, JORG_SEARCH3),
        ] {
            world.level.rand_state = seed_rolls(&[roll]);
            jorg_search(&mut world, key);
            assert_eq!(played(&gi).last().unwrap(), name);
        }
    }

    #[test]
    fn jorg_run_stands_its_ground_or_runs() {
        let mut world = test_world();
        let key = spawned(&mut world, sp_monster_jorg);
        jorg_run(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_RUN));
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        jorg_run(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_STAND));
        jorg_walk(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_WALK));
    }

    #[test]
    fn jorg_attack_picks_chain_guns_with_their_loop_or_the_bfg() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_jorg);
        world.level.rand_state = seed_rolls(&[0.0..0.75]);
        jorg_attack(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_START_ATTACK1));
        assert_eq!(played(&gi), [JORG_ATTACK1]);
        let loop_index = world.gi.sound_index("boss3/w_loop.wav");
        assert_eq!(world.entities.get(key).unwrap().state.sound, loop_index);

        world.level.rand_state = seed_rolls(&[0.76..1.01]);
        jorg_attack(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_ATTACK2));
        assert_eq!(played(&gi)[1], JORG_ATTACK2);
    }

    #[test]
    fn jorg_reattack1_keeps_firing_at_a_visible_enemy_nine_times_in_ten() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_jorg);
        enemy_at(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        world.entities.get_mut(key).unwrap().state.sound = 7;
        world.level.rand_state = seed_rolls(&[0.0..0.9]);
        jorg_reattack1(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_ATTACK1));
        assert_eq!(world.entities.get(key).unwrap().state.sound, 7);

        world.level.rand_state = seed_rolls(&[0.9..1.01]);
        jorg_reattack1(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_END_ATTACK1));
        assert_eq!(world.entities.get(key).unwrap().state.sound, 0);

        world.entities.get_mut(key).unwrap().state.sound = 7;
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        world.level.rand_state = seed_rolls(&[0.0..0.9]);
        jorg_reattack1(&mut world, key);
        assert!(plays(&world, key, &JORG_MOVE_END_ATTACK1), "out of sight");
        assert_eq!(world.entities.get(key).unwrap().state.sound, 0);
    }

    #[test]
    fn jorg_pain_shows_damage_and_picks_a_pain_by_damage() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 10.0;
        let key = spawned(&mut world, sp_monster_jorg);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.max_health = 3000;
        ent.game.health = 1499;
        ent.state.sound = 5;
        jorg_pain(&mut world, key, key, 0.0, 50);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.state.skinnum, ent.state.sound), (1, 0));
        assert_eq!(ent.game.pain_debounce_time, 13.0);
        assert!(plays(&world, key, &JORG_MOVE_PAIN1));
        assert_eq!(played(&gi), [JORG_PAIN1]);

        world.level.time = 12.0;
        jorg_pain(&mut world, key, key, 0.0, 100);
        assert!(plays(&world, key, &JORG_MOVE_PAIN1), "debounced");

        world.level.time = 20.0;
        jorg_pain(&mut world, key, key, 0.0, 100);
        assert!(plays(&world, key, &JORG_MOVE_PAIN2));
        world.level.time = 30.0;
        world.level.rand_state = seed_rolls(&[0.0..0.3]);
        jorg_pain(&mut world, key, key, 0.0, 101);
        assert!(plays(&world, key, &JORG_MOVE_PAIN3));
        assert_eq!(played(&gi).last().unwrap(), JORG_PAIN3);
        jorg_stand(&mut world, key);
        world.level.time = 40.0;
        world.level.rand_state = seed_rolls(&[0.31..1.01]);
        jorg_pain(&mut world, key, key, 0.0, 101);
        assert!(
            plays(&world, key, &JORG_MOVE_STAND),
            "big hits only sometimes"
        );
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            43.0
        );
    }

    #[test]
    fn jorg_pain_ignores_small_hits_and_rarely_breaks_an_attack() {
        let mut world = test_world();
        let key = spawned(&mut world, sp_monster_jorg);
        world.level.rand_state = seed_rolls(&[0.0..0.6]);
        jorg_pain(&mut world, key, key, 0.0, 40);
        assert!(plays(&world, key, &JORG_MOVE_STAND));
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            0.0
        );
        world.level.rand_state = seed_rolls(&[0.61..1.01]);
        jorg_pain(&mut world, key, key, 0.0, 40);
        assert!(plays(&world, key, &JORG_MOVE_PAIN1));

        for (frame, chance) in [
            (FRAME_ATTAK101, 0.005),
            (FRAME_ATTAK114, 0.00005),
            (FRAME_ATTAK208, 0.005),
        ] {
            jorg_stand(&mut world, key);
            world.level.time += 5.0;
            world.entities.get_mut(key).unwrap().state.frame = frame;
            world.level.rand_state = seed_rolls(&[0.0..chance]);
            jorg_pain(&mut world, key, key, 0.0, 41);
            assert!(plays(&world, key, &JORG_MOVE_STAND), "frame {frame}");
            world.level.rand_state = seed_rolls(&[0.5..1.0]);
            jorg_pain(&mut world, key, key, 0.0, 41);
            assert!(plays(&world, key, &JORG_MOVE_PAIN1), "frame {frame}");
        }

        jorg_stand(&mut world, key);
        world.level.time += 5.0;
        world.entities.get_mut(key).unwrap().state.frame = 0;
        world.game.skill = crate::constants::SKILL_HARDPLUS;
        jorg_pain(&mut world, key, key, 0.0, 200);
        assert!(
            plays(&world, key, &JORG_MOVE_STAND),
            "no pain anims in nightmare"
        );
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            world.level.time + 3.0
        );
    }

    #[test]
    fn jorg_die_starts_the_death_move_and_stops_taking_damage() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_jorg);
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.sound = 3;
        ent.game.count = 9;
        jorg_die(&mut world, key, key, key, 10, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage),
            (DeadFlag::Dead, TakeDamage::No)
        );
        assert_eq!((ent.state.sound, ent.game.count), (0, 0));
        assert!(plays(&world, key, &JORG_MOVE_DEATH));
        assert_eq!(played(&gi), [JORG_DEATH]);
    }

    #[test]
    fn jorg_bfg_and_chain_guns_fire_from_their_muzzles() {
        let (mut world, gi) = wire_world();
        let key = spawned(&mut world, sp_monster_jorg);
        enemy_at(&mut world, key, Vec3f::new(400.0, 0.0, 0.0));
        jorg_firebullet(&mut world, key);
        assert_eq!(
            flashes(&gi),
            [MZ2_JORG_MACHINEGUN_L1, MZ2_JORG_MACHINEGUN_R1]
        );
        jorg_bfg(&mut world, key);
        assert_eq!(flashes(&gi)[2], MZ2_JORG_BFG_1);
        assert!(world.find_by_classname(None, "bfg blast").is_some());
        assert_eq!(wire_sounds(&gi).last().unwrap().1, JORG_ATTACK2);
    }

    #[test]
    fn jorg_checkattack_needs_a_clear_shot_through_everything_but_windows() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_jorg);
        enemy_at(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        assert!(
            !jorg_checkattack(&mut world, key),
            "the default trace hits nothing"
        );
        assert_eq!(
            gi.trace_masks.borrow().last(),
            Some(&(CONTENTS_SOLID | CONTENTS_MONSTER | CONTENTS_SLIME | CONTENTS_LAVA))
        );
    }

    #[test]
    fn jorg_checkattack_always_answers_melee_range_with_a_missile() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_jorg);
        let enemy = enemy_at(&mut world, key, Vec3f::new(0.0, 50.0, 0.0));
        *gi.next_trace.borrow_mut() = Some(hit(&world, enemy));
        let state = world.level.rand_state;
        assert!(jorg_checkattack(&mut world, key));
        assert_eq!(mi(&world, key).attack_state, AttackState::Missile);
        assert_eq!(world.entities.get(key).unwrap().game.ideal_yaw, 90.0);
        assert_eq!(world.level.rand_state, state, "no roll");
    }

    #[test]
    fn jorg_checkattack_fires_by_range_chance_without_skill_scaling() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 5.0;
        world.game.skill = 0;
        let key = spawned(&mut world, sp_monster_jorg);
        let enemy = enemy_at(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        for (x, roll, fires) in [
            (300.0, 0.39..0.4, true),
            (300.0, 0.4..0.41, false),
            (700.0, 0.19..0.2, true),
            (700.0, 0.2..0.21, false),
        ] {
            world.entities.get_mut(enemy).unwrap().state.origin.x = x;
            mi_mut(&mut world, key).attack_finished = 0.0;
            mi_mut(&mut world, key).attack_state = AttackState::Straight;
            *gi.next_trace.borrow_mut() = Some(hit(&world, enemy));
            world.level.rand_state = seed_rolls(&[roll, 0.5..0.51]);
            assert_eq!(jorg_checkattack(&mut world, key), fires, "x {x}");
            if fires {
                assert_eq!(mi(&world, key).attack_state, AttackState::Missile);
                let wait = mi(&world, key).attack_finished - 5.0;
                assert!((1.0..1.02).contains(&wait), "{wait}");
            } else {
                assert_eq!(
                    mi(&world, key).attack_state,
                    AttackState::Straight,
                    "walkers do not slide"
                );
            }
        }

        world.entities.get_mut(enemy).unwrap().state.origin.x = 700.0;
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        *gi.next_trace.borrow_mut() = Some(hit(&world, enemy));
        world.level.rand_state = seed_rolls(&[0.39..0.4]);
        assert!(
            jorg_checkattack(&mut world, key),
            "standing ground: 0.4 at mid range"
        );

        mi_mut(&mut world, key).attack_finished = 6.0;
        *gi.next_trace.borrow_mut() = Some(hit(&world, enemy));
        assert!(!jorg_checkattack(&mut world, key), "still waiting");
        mi_mut(&mut world, key).attack_finished = 0.0;
        world.entities.get_mut(enemy).unwrap().state.origin.x = 1000.0;
        *gi.next_trace.borrow_mut() = Some(hit(&world, enemy));
        assert!(!jorg_checkattack(&mut world, key), "far");
    }

    #[test]
    fn boss3_checkattack_lets_a_flyer_slide_when_it_holds_fire() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_makron);
        world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::FLY;
        let enemy = enemy_at(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        *gi.next_trace.borrow_mut() = Some(hit(&world, enemy));
        world.level.rand_state = seed_rolls(&[0.5..1.01, 0.0..0.3]);
        assert!(!makron_checkattack(&mut world, key));
        assert_eq!(mi(&world, key).attack_state, AttackState::Sliding);
    }

    #[test]
    fn boss_explode_blows_up_eight_times_then_gibs() {
        let (mut world, gi) = wire_world();
        world.level.time = 3.0;
        let key = spawned(&mut world, sp_monster_jorg);
        world.entities.get_mut(key).unwrap().state.sound = 4;
        for n in 0..8 {
            gi.wire.borrow_mut().clear();
            boss_explode(&mut world, key);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.count, n + 1);
            assert_eq!(ent.game.nextthink, 3.1);
            assert!(std::ptr::fn_addr_eq(
                ent.think.unwrap(),
                boss_explode as ThinkFn
            ));
            let wire = gi.wire.borrow();
            assert_eq!(wire[1], Wire::Byte(TE_EXPLOSION1));
            let Wire::Position(org) = wire[2] else {
                panic!()
            };
            let (dx, dy) = [
                (-24.0, -24.0),
                (24.0, 24.0),
                (24.0, -24.0),
                (-24.0, 24.0),
                (-48.0, -48.0),
                (48.0, 48.0),
                (-48.0, 48.0),
                (48.0, -48.0),
            ][n as usize];
            assert_eq!((org.x, org.y), (dx, dy));
            assert!((24.0..=39.0).contains(&org.z));
        }
        let before = world.entities.count();
        boss_explode(&mut world, key);
        assert_eq!(
            world.entities.count(),
            before + 13,
            "4 meat, 8 metal, chest"
        );
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.state.sound, 0);
        assert!(ent.state.effects & EF_GIB != 0, "the gear head");
    }

    // ----- Makron -----

    #[test]
    fn makron_spawns_with_c_values_on_its_sight_move() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_makron);
        makron_finish(&mut world, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.health, ent.game.mass), (3000, 500));
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-30.0, -30.0, 0.0), Vec3f::new(30.0, 30.0, 90.0))
        );
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        let mi = mi(&world, key);
        assert!(mi.melee.is_none() && mi.sight.is_none() && mi.search.is_none());
        assert!(std::ptr::fn_addr_eq(
            mi.checkattack.unwrap(),
            makron_checkattack as fn(&mut GameWorld, EntityKey) -> bool
        ));
        assert!(plays(&world, key, &MAKRON_MOVE_SIGHT));
        assert_eq!(gi.sounds.borrow().len(), 14);
        assert!(
            gi.models.borrow().is_empty() || gi.models.borrow().contains(&RIDER_MODEL.to_string())
        );
    }

    #[test]
    fn makron_attack_and_taunt_pick_by_thirds() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_makron);
        for (roll, mv, taunt) in [
            (0.0..0.3, &MAKRON_MOVE_ATTACK3, MAKRON_TAUNT1),
            (0.31..0.6, &MAKRON_MOVE_ATTACK4, MAKRON_TAUNT2),
            (0.61..1.01, &MAKRON_MOVE_ATTACK5, MAKRON_TAUNT3),
        ] {
            world.level.rand_state = seed_rolls(std::slice::from_ref(&roll));
            makron_attack(&mut world, key);
            assert!(plays(&world, key, mv));
            world.level.rand_state = seed_rolls(&[roll]);
            makron_taunt(&mut world, key);
            assert_eq!(played(&gi).last().unwrap(), taunt);
        }
        makron_run(&mut world, key);
        assert!(plays(&world, key, &MAKRON_MOVE_RUN));
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        makron_run(&mut world, key);
        assert!(plays(&world, key, &MAKRON_MOVE_STAND));
        makron_walk(&mut world, key);
        assert!(plays(&world, key, &MAKRON_MOVE_WALK));
    }

    #[test]
    fn makron_sounds_use_c_channels_and_attenuation() {
        let (mut world, gi) = wire_world();
        let key = spawned(&mut world, sp_monster_makron);
        let calls: [(ThinkFn, i32, &str, f32); 6] = [
            (makron_hit, CHAN_AUTO, MAKRON_HIT, ATTN_NONE),
            (makron_popup, CHAN_BODY, MAKRON_POPUP, ATTN_NONE),
            (makron_step_left, CHAN_BODY, MAKRON_STEP_LEFT, ATTN_NORM),
            (makron_step_right, CHAN_BODY, MAKRON_STEP_RIGHT, ATTN_NORM),
            (
                makron_brainsplorch,
                CHAN_VOICE,
                MAKRON_BRAINSPLORCH,
                ATTN_NORM,
            ),
            (makron_prerailgun, CHAN_WEAPON, MAKRON_PRERAILGUN, ATTN_NORM),
        ];
        for (f, ..) in calls {
            f(&mut world, key);
        }
        let want: Vec<_> = calls
            .iter()
            .map(|&(_, c, n, a)| (c, n.to_string(), a))
            .collect();
        assert_eq!(wire_sounds(&gi), want);
    }

    #[test]
    fn makron_pain_picks_a_pain_by_damage() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_makron);
        world.entities.get_mut(key).unwrap().game.max_health = 3000;
        world.level.rand_state = seed_rolls(&[0.0..0.2]);
        makron_pain(&mut world, key, key, 0.0, 25);
        assert!(
            plays(&world, key, &MAKRON_MOVE_SIGHT),
            "small hits shrugged off"
        );
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            0.0
        );

        for (damage, rolls, mv) in [
            (25, 0.2..1.01, &MAKRON_MOVE_PAIN4),
            (40, 0.0..1.01, &MAKRON_MOVE_PAIN4),
            (110, 0.0..1.01, &MAKRON_MOVE_PAIN5),
            (150, 0.0..0.45, &MAKRON_MOVE_PAIN6),
            (151, 0.0..0.35, &MAKRON_MOVE_PAIN6),
        ] {
            makron_stand(&mut world, key);
            world.level.time += 5.0;
            world.level.rand_state = seed_rolls(&[rolls]);
            makron_pain(&mut world, key, key, 0.0, damage);
            assert!(plays(&world, key, mv), "damage {damage}");
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.pain_debounce_time, world.level.time + 3.0);
        }
        assert_eq!(
            played(&gi),
            [
                MAKRON_PAIN4,
                MAKRON_PAIN4,
                MAKRON_PAIN5,
                MAKRON_PAIN6,
                MAKRON_PAIN6
            ]
        );
        assert!(gi.sound_calls.borrow().iter().all(|c| c.4 == ATTN_NONE));

        for (damage, roll) in [(150, 0.46..1.01), (151, 0.36..1.01)] {
            makron_stand(&mut world, key);
            world.level.time += 5.0;
            world.level.rand_state = seed_rolls(&[roll]);
            makron_pain(&mut world, key, key, 0.0, damage);
            assert!(plays(&world, key, &MAKRON_MOVE_STAND), "damage {damage}");
        }

        world.level.time += 1.0;
        makron_pain(&mut world, key, key, 0.0, 200);
        assert!(plays(&world, key, &MAKRON_MOVE_STAND), "debounced");
        world.level.time += 5.0;
        world.game.skill = crate::constants::SKILL_HARDPLUS;
        world.entities.get_mut(key).unwrap().game.health = 1000;
        makron_pain(&mut world, key, key, 0.0, 200);
        assert!(plays(&world, key, &MAKRON_MOVE_STAND), "nightmare");
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
    }

    #[test]
    fn makron_weapons_fire_bfg_rail_at_the_saved_spot_and_a_sweeping_blaster() {
        let (mut world, gi) = wire_world();
        let key = spawned(&mut world, sp_monster_makron);
        let enemy = enemy_at(&mut world, key, Vec3f::new(500.0, 0.0, 0.0));
        makron_bfg(&mut world, key);
        assert!(world.find_by_classname(None, "bfg blast").is_some());
        assert_eq!(wire_sounds(&gi)[0].1, MAKRON_ATTACK_BFG);

        makron_saveloc(&mut world, key);
        assert_eq!(
            world.entities.get(key).unwrap().game.move_origin,
            Vec3f::new(500.0, 0.0, 22.0)
        );
        world.entities.get_mut(enemy).unwrap().state.origin.y = 300.0;
        makron_railgun(&mut world, key);

        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK405 + 3;
        makron_hyperblaster(&mut world, key);
        assert_eq!(
            flashes(&gi),
            [MZ2_MAKRON_BFG, MZ2_MAKRON_RAILGUN_1, MZ2_MAKRON_BLASTER_1]
        );
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        // The bolt starts at the frame's own muzzle, though the flash on
        // the wire is always the first one.
        let (start, _, _) = monster_flash_source(&world, key, MZ2_MAKRON_BLASTER_1 + 3).unwrap();
        assert_eq!(world.entities.get(bolt).unwrap().state.origin, start);
        let v = world.entities.get(bolt).unwrap().velocity;
        // Frame 408 is 5 frames before 413: 50 degrees to the left.
        let yaw = v.y.atan2(v.x).to_degrees();
        assert!((yaw - 50.0).abs() < 0.5, "{yaw}");
    }

    #[test]
    fn makron_hyperblaster_sweeps_right_after_frame_413_and_fires_level_without_an_enemy() {
        let (mut world, _gi) = wire_world();
        let key = spawned(&mut world, sp_monster_makron);
        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK421 - 2;
        makron_hyperblaster(&mut world, key);
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        let v = world.entities.get(bolt).unwrap().velocity;
        let yaw = v.y.atan2(v.x).to_degrees();
        assert!((yaw + 20.0).abs() < 0.5, "{yaw}");
        assert!(v.z.abs() < 1e-3, "no enemy: level");
    }

    #[test]
    fn makron_die_gibs_at_c_gib_health() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world, sp_monster_makron);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = -2000;
        ent.state.sound = 9;
        let before = world.entities.count();
        makron_die(&mut world, key, key, key, 500, Vec3f::ZERO);
        assert_eq!(world.entities.count(), before + 5, "meat and 4 metal");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert!(ent.state.effects & EF_GIB != 0);
        assert_eq!(played(&gi), ["misc/udeath.wav"]);
    }

    #[test]
    fn makron_die_drops_the_torso_and_lowers_the_box_once() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 4.0;
        let key = spawned(&mut world, sp_monster_makron);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = -1999;
        ent.state.angles.y = 90.0;
        ent.state.origin = Vec3f::new(0.0, 0.0, 10.0);
        makron_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage),
            (DeadFlag::Dead, TakeDamage::Yes)
        );
        assert_eq!(ent.maxs.z, 64.0);
        assert!(plays(&world, key, &MAKRON_MOVE_DEATH2));
        assert_eq!(played(&gi), [MAKRON_DEATH]);
        assert_eq!(gi.sound_calls.borrow()[0].4, ATTN_NONE);

        let torso = world
            .entities
            .iter()
            .find(|(_, e)| e.owner == Some(key))
            .map(|(k, _)| k)
            .unwrap();
        let t = world.entities.get(torso).unwrap();
        assert!(
            (t.state.origin - Vec3f::new(0.0, -84.0, 10.0)).length() < 1e-3,
            "{:?}",
            t.state.origin
        );
        assert_eq!(
            (t.mins, t.maxs),
            (Vec3f::new(-24.0, -24.0, 0.0), Vec3f::new(24.0, 24.0, 16.0))
        );
        assert_eq!(
            (t.game.movetype, t.solid, t.state.frame),
            (MoveType::Toss, Solid::Bbox, FRAME_DEATH301)
        );
        assert_eq!(
            t.svflags,
            SvFlags::MONSTER.bits() | SvFlags::DEADMONSTER.bits()
        );
        assert_eq!(
            (t.game.deadflag, t.game.takedamage),
            (DeadFlag::Dead, TakeDamage::Yes)
        );
        assert_eq!(t.game.nextthink, 4.2);
        assert_eq!(t.state.sound, world.gi.sound_index("makron/spine.wav"));
        assert_eq!(gi.trace_masks.borrow().last(), Some(&MASK_SOLID));

        let count = world.entities.count();
        makron_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        assert_eq!(world.entities.count(), count, "already dead");
    }

    #[test]
    fn makron_torso_writhes_until_gibbed_and_leaves_if_the_makron_revives() {
        let mut world = test_world();
        let key = spawned(&mut world, sp_monster_makron);
        world.entities.get_mut(key).unwrap().game.deadflag = DeadFlag::Dead;
        makron_torso(&mut world, key);
        let torso = world
            .entities
            .iter()
            .find(|(_, e)| e.owner == Some(key))
            .unwrap()
            .0;
        makron_torso_think(&mut world, torso);
        assert_eq!(
            world.entities.get(torso).unwrap().state.frame,
            FRAME_DEATH301 + 1
        );
        world.entities.get_mut(torso).unwrap().state.frame = FRAME_DEATH320 - 1;
        makron_torso_think(&mut world, torso);
        assert_eq!(
            world.entities.get(torso).unwrap().state.frame,
            FRAME_DEATH301
        );

        // Gibbed legs: the torso lets go and keeps writhing.
        world.entities.get_mut(key).unwrap().game.health = -2000;
        world.entities.get_mut(key).unwrap().game.deadflag = DeadFlag::No;
        makron_torso_think(&mut world, torso);
        assert_eq!(world.entities.get(torso).unwrap().owner, None);

        // Revived legs take the torso back.
        let torso2 = {
            world.entities.get_mut(key).unwrap().game.health = 10;
            world.entities.get_mut(key).unwrap().game.deadflag = DeadFlag::Dead;
            makron_torso(&mut world, key);
            world
                .entities
                .iter()
                .find(|(_, e)| e.owner == Some(key))
                .unwrap()
                .0
        };
        world.entities.get_mut(key).unwrap().game.deadflag = DeadFlag::No;
        makron_torso_think(&mut world, torso2);
        assert!(!world.entities.is_live(torso2));

        let torso_die = world.entities.get(torso).unwrap().die.unwrap();
        world.entities.get_mut(torso).unwrap().game.health = -799;
        torso_die(&mut world, torso, torso, torso, 100, Vec3f::ZERO);
        assert!(world.entities.is_live(torso));
        world.entities.get_mut(torso).unwrap().game.health = -800;
        let count = world.entities.count();
        torso_die(&mut world, torso, torso, torso, 100, Vec3f::ZERO);
        assert!(!world.entities.is_live(torso));
        assert_eq!(world.entities.count(), count + 5 - 1, "5 gibs, torso gone");
    }

    #[test]
    fn makron_dead_lays_out_the_legs() {
        let mut world = test_world();
        let key = spawned(&mut world, sp_monster_makron);
        makron_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-48.0, -48.0, 0.0), Vec3f::new(48.0, 48.0, 24.0))
        );
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
    }

    // ----- Jorg's death throws out the Makron -----

    #[test]
    fn makron_toss_leaves_a_makron_to_spawn_where_jorg_stood() {
        let mut world = test_world();
        world.level.time = 7.0;
        let jorg = spawned(&mut world, sp_monster_jorg);
        let enemy = enemy_at(&mut world, jorg, Vec3f::new(300.0, 0.0, 0.0));
        let ent = world.entities.get_mut(jorg).unwrap();
        ent.state.origin = Vec3f::new(10.0, 20.0, 30.0);
        ent.state.angles.y = 45.0;
        ent.game.target = "t1".to_string();
        ent.game.oldenemy = Some(jorg);
        mi_mut(&mut world, jorg).last_sighting = Vec3f::new(1.0, 2.0, 3.0);
        makron_toss(&mut world, jorg);
        let makron = world.find_by_classname(None, "monster_makron").unwrap();
        let m = world.entities.get(makron).unwrap();
        assert!(std::ptr::fn_addr_eq(
            m.think.unwrap(),
            makron_spawn as ThinkFn
        ));
        assert!((m.game.nextthink - 7.8).abs() < 1e-5);
        assert_eq!(m.game.target, "t1");
        assert_eq!(
            (m.state.origin, m.state.angles.y),
            (Vec3f::new(10.0, 20.0, 30.0), 45.0)
        );
        assert_eq!(m.game.move_origin, Vec3f::new(1.0, 2.0, 3.0));
        assert_eq!((m.game.enemy, m.game.oldenemy), (Some(enemy), Some(jorg)));
        assert!(m.monsterinfo.is_none(), "not yet a monster");
    }

    #[test]
    fn makron_spawn_leaps_at_jorgs_enemy_as_a_live_monster() {
        let mut world = test_world();
        world.level.time = 7.0;
        let key = world.spawn().unwrap();
        let enemy = world.spawn_test_client(crate::entity::ClientData::default());
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = Vec3f::new(0.0, 300.0, 0.0);
        e.game.health = 100;
        let m = world.entities.get_mut(key).unwrap();
        m.game.classname = "monster_makron".to_string();
        m.game.enemy = Some(enemy);
        m.game.move_origin = Vec3f::new(5.0, 6.0, 7.0);
        let total = world.level.total_monsters;
        makron_spawn(&mut world, key);

        let m = world.entities.get(key).unwrap();
        assert_eq!((m.game.health, m.game.mass), (3000, 500));
        assert!(m.svflags & SvFlags::MONSTER.bits() != 0);
        assert_eq!(m.game.takedamage, TakeDamage::Aim);
        assert_eq!(world.level.total_monsters, total + 1);
        assert_eq!(m.game.enemy, Some(enemy));
        assert_eq!(m.state.angles.y, 90.0);
        assert!(
            (m.velocity - Vec3f::new(0.0, 400.0, 200.0)).length() < 1e-3,
            "{:?}",
            m.velocity
        );
        assert_eq!((m.state.origin.z, m.game.ground_entity), (1.0, None));
        assert_eq!(m.game.pain_debounce_time, 8.0);
        assert!(std::ptr::fn_addr_eq(
            m.think.unwrap(),
            ai::monster_think as ThinkFn
        ));
        assert!(plays(&world, key, &MAKRON_MOVE_SIGHT));
        assert_eq!(mi(&world, key).last_sighting, Vec3f::new(5.0, 6.0, 7.0));
    }

    #[test]
    fn makron_spawn_drops_a_dead_enemy_for_the_old_one_or_jumps_forward() {
        let mut world = test_world();
        world.level.time = 7.0;
        let key = world.spawn().unwrap();
        let dead = world.spawn().unwrap();
        world.entities.get_mut(dead).unwrap().game.deadflag = DeadFlag::Dead;
        let old = world.spawn().unwrap();
        world.entities.get_mut(old).unwrap().state.origin = Vec3f::new(-100.0, 0.0, 0.0);
        let m = world.entities.get_mut(key).unwrap();
        m.game.enemy = Some(dead);
        m.game.oldenemy = Some(old);
        makron_spawn(&mut world, key);
        let m = world.entities.get(key).unwrap();
        assert_eq!((m.game.enemy, m.game.oldenemy), (Some(old), None));
        assert!(
            (m.velocity - Vec3f::new(-400.0, 0.0, 200.0)).length() < 1e-2,
            "{:?}",
            m.velocity
        );

        let lone = world.spawn().unwrap();
        world.entities.get_mut(lone).unwrap().state.angles.y = 0.0;
        makron_spawn(&mut world, lone);
        let m = world.entities.get(lone).unwrap();
        assert_eq!(m.game.enemy, None);
        assert!(
            (m.velocity - Vec3f::new(400.0, 0.0, 200.0)).length() < 1e-2,
            "forward"
        );
    }

    #[test]
    fn jorg_death_move_tosses_the_makron_then_explodes() {
        let mv = &JORG_MOVE_DEATH;
        assert!(std::ptr::fn_addr_eq(
            mv.frames[48].thinkfunc.unwrap(),
            makron_toss as ThinkFn
        ));
        assert!(std::ptr::fn_addr_eq(
            mv.frames[49].thinkfunc.unwrap(),
            boss_explode as ThinkFn
        ));
    }

    /// Jorg's death plays once: one Makron is tossed at frame 49 and
    /// `BossExplode` takes over the think at frame 50, so the death move
    /// never wraps round to toss another.
    #[test]
    fn jorg_death_tosses_one_makron_then_explodes_instead_of_looping() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = spawned(&mut world, sp_monster_jorg);
        assert!(ai::monster_start(&mut world, key));
        let think = world.entities.get(key).unwrap().think.unwrap();
        think(&mut world, key);
        jorg_die(&mut world, key, key, key, 100, Vec3f::ZERO);
        for _ in 0..55 {
            world.level.time += FRAMETIME;
            let ent = world.entities.get(key).unwrap();
            if ent.game.nextthink <= 0.0 || ent.game.nextthink > world.level.time + 0.001 {
                continue;
            }
            let think = ent.think.unwrap();
            think(&mut world, key);
        }
        let makrons = world
            .entities
            .iter()
            .filter(|(_, e)| e.game.classname == "monster_makron")
            .count();
        assert_eq!(makrons, 1);
        let ent = world.entities.get(key).unwrap();
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            boss_explode as ThinkFn
        ));
        assert!(
            (1..=8).contains(&ent.game.count),
            "still exploding: {}",
            ent.game.count
        );
    }

    /// Jorg on a synthetic world with a player in plain view: he sights
    /// the player, runs, picks an attack, and fires (a BFG blast or chain
    /// gun shots).
    #[test]
    fn jorg_sights_runs_and_attacks_a_player_in_view() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 1.0;
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        let ent = world.entities.get_mut(player).unwrap();
        ent.game.classname = "player".to_string();
        ent.state.origin = Vec3f::new(400.0, 0.0, 0.0);
        ent.game.health = 100;
        world.link_entity(player);
        world.level.sight_client_key = Some(player);
        let key = spawned(&mut world, sp_monster_jorg);
        jorg_finish(&mut world, key, &HashMap::new());
        assert!(ai::monster_start(&mut world, key));

        let mut attacked = false;
        let mut ran = false;
        for _ in 0..300 {
            world.level.time += FRAMETIME;
            world.level.framenum += 1;
            world.level.sight_client_key = Some(player);
            // This frame's shot check sees the player: visible first,
            // then the clear-shot trace.
            gi.queued_traces.borrow_mut().clear();
            if ran {
                let clear = Trace {
                    fraction: 1.0,
                    ..Trace::default()
                };
                gi.queued_traces
                    .borrow_mut()
                    .extend([clear, hit(&world, player)]);
            }
            let think = world.entities.get(key).unwrap().think.unwrap();
            think(&mut world, key);
            ran |= plays(&world, key, &JORG_MOVE_RUN);
            let shot = gi.trace_masks.borrow().contains(&MASK_SHOT);
            if shot || world.find_by_classname(None, "bfg blast").is_some() {
                attacked = true;
                break;
            }
        }
        assert_eq!(
            world.entities.get(key).unwrap().game.enemy,
            Some(player),
            "sighted"
        );
        assert!(ran, "ran at the player");
        assert!(attacked, "fired");
    }
}
