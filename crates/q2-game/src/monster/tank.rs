//! Tank and Tank Commander.
//! C ref: `~/Qwasm2/src/game/monster/tank/tank.c`

use q2_shared::constants::{
    EF_BLASTER, MZ2_TANK_BLASTER_1, MZ2_TANK_BLASTER_2, MZ2_TANK_BLASTER_3, MZ2_TANK_MACHINEGUN_1,
    MZ2_TANK_ROCKET_1, MZ2_TANK_ROCKET_2, MZ2_TANK_ROCKET_3,
};
use q2_shared::types::*;
use std::collections::HashMap;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::fire::{monster_fire_blaster, monster_fire_bullet, monster_fire_rocket};
use crate::monster::flash::monster_flash_source;
use crate::world::GameWorld;

/// C tank.h `MODEL_SCALE`.
const MODEL_SCALE: f32 = 1.0;
const COMMANDER: &str = "monster_tank_commander";

// Frames the callbacks name besides the move bounds.
const FRAME_ATTAK110: i32 = 64;
const FRAME_ATTAK113: i32 = 67;
const FRAME_ATTAK324: i32 = 138;
const FRAME_ATTAK327: i32 = 141;
const FRAME_ATTAK406: i32 = 173;
const FRAME_ATTAK411: i32 = 178;
const FRAME_ATTAK415: i32 = 182;
const FRAME_ATTAK419: i32 = 186;

const fn frame(aifunc: AnimFrameFn, dist: f32, thinkfunc: Option<ThinkFn>) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

// Moves. C ref: tank.c `tank_frames_*` / `tank_move_*`. `start_walk`,
// `stop_walk` and `stop_run` exist in C but nothing selects them.
const FRAME_STAND01: i32 = 0;
const FRAME_STAND30: i32 = 29;
const FRAME_WALK01: i32 = 30;
const FRAME_WALK04: i32 = 33;
const FRAME_WALK05: i32 = 34;
const FRAME_WALK20: i32 = 49;
const FRAME_WALK21: i32 = 50;
const FRAME_WALK25: i32 = 54;
const FRAME_ATTAK101: i32 = 55;
const FRAME_ATTAK111: i32 = 65;
const FRAME_ATTAK116: i32 = 70;
const FRAME_ATTAK117: i32 = 71;
const FRAME_ATTAK122: i32 = 76;
const FRAME_ATTAK201: i32 = 77;
const FRAME_ATTAK238: i32 = 114;
const FRAME_ATTAK301: i32 = 115;
const FRAME_ATTAK321: i32 = 135;
const FRAME_ATTAK322: i32 = 136;
const FRAME_ATTAK330: i32 = 144;
const FRAME_ATTAK331: i32 = 145;
const FRAME_ATTAK353: i32 = 167;
const FRAME_ATTAK401: i32 = 168;
const FRAME_ATTAK429: i32 = 196;
const FRAME_PAIN101: i32 = 197;
const FRAME_PAIN104: i32 = 200;
const FRAME_PAIN201: i32 = 201;
const FRAME_PAIN205: i32 = 205;
const FRAME_PAIN301: i32 = 206;
const FRAME_PAIN316: i32 = 221;
const FRAME_DEATH101: i32 = 222;
const FRAME_DEATH132: i32 = 253;

static TANK_FRAMES_STAND: [MonsterFrame; 30] = [frame(ai::ai_stand, 0.0, None); 30];
static TANK_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND30,
    frames: &TANK_FRAMES_STAND,
    endfunc: None,
};

pub static TANK_FRAMES_START_WALK: [MonsterFrame; 4] = [
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 11.0, Some(tank_footstep)),
];
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static TANK_MOVE_START_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK01,
    lastframe: FRAME_WALK04,
    frames: &TANK_FRAMES_START_WALK,
    endfunc: Some(tank_walk),
};

static TANK_FRAMES_WALK: [MonsterFrame; 16] = [
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, Some(tank_footstep)),
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 6.0, Some(tank_footstep)),
];
static TANK_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK05,
    lastframe: FRAME_WALK20,
    frames: &TANK_FRAMES_WALK,
    endfunc: None,
};

pub static TANK_FRAMES_STOP_WALK: [MonsterFrame; 5] = [
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 4.0, Some(tank_footstep)),
];
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static TANK_MOVE_STOP_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK21,
    lastframe: FRAME_WALK25,
    frames: &TANK_FRAMES_STOP_WALK,
    endfunc: Some(tank_stand),
};

static TANK_FRAMES_START_RUN: [MonsterFrame; 4] = [
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 6.0, None),
    frame(ai::ai_run, 6.0, None),
    frame(ai::ai_run, 11.0, Some(tank_footstep)),
];
static TANK_MOVE_START_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK01,
    lastframe: FRAME_WALK04,
    frames: &TANK_FRAMES_START_RUN,
    endfunc: Some(tank_run),
};

static TANK_FRAMES_RUN: [MonsterFrame; 16] = [
    frame(ai::ai_run, 4.0, None),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 3.0, None),
    frame(ai::ai_run, 2.0, None),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 4.0, None),
    frame(ai::ai_run, 4.0, Some(tank_footstep)),
    frame(ai::ai_run, 3.0, None),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 4.0, None),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 7.0, None),
    frame(ai::ai_run, 7.0, None),
    frame(ai::ai_run, 6.0, None),
    frame(ai::ai_run, 6.0, Some(tank_footstep)),
];
static TANK_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK05,
    lastframe: FRAME_WALK20,
    frames: &TANK_FRAMES_RUN,
    endfunc: None,
};

pub static TANK_FRAMES_STOP_RUN: [MonsterFrame; 5] = [
    frame(ai::ai_run, 3.0, None),
    frame(ai::ai_run, 3.0, None),
    frame(ai::ai_run, 2.0, None),
    frame(ai::ai_run, 2.0, None),
    frame(ai::ai_run, 4.0, Some(tank_footstep)),
];
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static TANK_MOVE_STOP_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK21,
    lastframe: FRAME_WALK25,
    frames: &TANK_FRAMES_STOP_RUN,
    endfunc: Some(tank_walk),
};

static TANK_FRAMES_PAIN1: [MonsterFrame; 4] = [frame(ai::ai_move, 0.0, None); 4];
static TANK_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN104,
    frames: &TANK_FRAMES_PAIN1,
    endfunc: Some(tank_run),
};

static TANK_FRAMES_PAIN2: [MonsterFrame; 5] = [frame(ai::ai_move, 0.0, None); 5];
static TANK_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN205,
    frames: &TANK_FRAMES_PAIN2,
    endfunc: Some(tank_run),
};

static TANK_FRAMES_PAIN3: [MonsterFrame; 16] = [
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(tank_footstep)),
];
static TANK_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN316,
    frames: &TANK_FRAMES_PAIN3,
    endfunc: Some(tank_run),
};

static TANK_FRAMES_ATTACK_BLAST: [MonsterFrame; 16] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -2.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(tank_blaster)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(tank_blaster)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(tank_blaster)),
];
static TANK_MOVE_ATTACK_BLAST: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK116,
    frames: &TANK_FRAMES_ATTACK_BLAST,
    endfunc: Some(tank_reattack_blaster),
};

static TANK_FRAMES_REATTACK_BLAST: [MonsterFrame; 6] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(tank_blaster)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(tank_blaster)),
];
static TANK_MOVE_REATTACK_BLAST: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK111,
    lastframe: FRAME_ATTAK116,
    frames: &TANK_FRAMES_REATTACK_BLAST,
    endfunc: Some(tank_reattack_blaster),
};

static TANK_FRAMES_ATTACK_POST_BLAST: [MonsterFrame; 6] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, -2.0, Some(tank_footstep)),
];
static TANK_MOVE_ATTACK_POST_BLAST: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK117,
    lastframe: FRAME_ATTAK122,
    frames: &TANK_FRAMES_ATTACK_POST_BLAST,
    endfunc: Some(tank_run),
};

static TANK_FRAMES_ATTACK_STRIKE: [MonsterFrame; 38] = [
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 7.0, None),
    frame(ai::ai_move, 9.0, Some(tank_footstep)),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 2.0, Some(tank_footstep)),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 0.0, Some(tank_windup)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(tank_strike)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -10.0, None),
    frame(ai::ai_move, -10.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -2.0, Some(tank_footstep)),
];
static TANK_MOVE_ATTACK_STRIKE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK238,
    frames: &TANK_FRAMES_ATTACK_STRIKE,
    endfunc: Some(tank_poststrike),
};

static TANK_FRAMES_ATTACK_PRE_ROCKET: [MonsterFrame; 21] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, 7.0, None),
    frame(ai::ai_charge, 7.0, None),
    frame(ai::ai_charge, 7.0, Some(tank_footstep)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -3.0, None),
];
static TANK_MOVE_ATTACK_PRE_ROCKET: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK301,
    lastframe: FRAME_ATTAK321,
    frames: &TANK_FRAMES_ATTACK_PRE_ROCKET,
    endfunc: Some(tank_doattack_rocket),
};

static TANK_FRAMES_ATTACK_FIRE_ROCKET: [MonsterFrame; 9] = [
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(tank_rocket)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(tank_rocket)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -1.0, Some(tank_rocket)),
];
static TANK_MOVE_ATTACK_FIRE_ROCKET: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK322,
    lastframe: FRAME_ATTAK330,
    frames: &TANK_FRAMES_ATTACK_FIRE_ROCKET,
    endfunc: Some(tank_refire_rocket),
};

static TANK_FRAMES_ATTACK_POST_ROCKET: [MonsterFrame; 23] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 4.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -9.0, None),
    frame(ai::ai_charge, -8.0, None),
    frame(ai::ai_charge, -7.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -1.0, Some(tank_footstep)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];
static TANK_MOVE_ATTACK_POST_ROCKET: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK331,
    lastframe: FRAME_ATTAK353,
    frames: &TANK_FRAMES_ATTACK_POST_ROCKET,
    endfunc: Some(tank_run),
};

static TANK_FRAMES_ATTACK_CHAIN: [MonsterFrame; 29] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    MonsterFrame {
        aifunc: None,
        dist: 0.0,
        thinkfunc: Some(tank_machine_gun),
    },
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];
static TANK_MOVE_ATTACK_CHAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK401,
    lastframe: FRAME_ATTAK429,
    frames: &TANK_FRAMES_ATTACK_CHAIN,
    endfunc: Some(tank_run),
};

static TANK_FRAMES_DEATH1: [MonsterFrame; 32] = [
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, -15.0, Some(tank_thud)),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];
static TANK_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH132,
    frames: &TANK_FRAMES_DEATH1,
    endfunc: Some(tank_dead),
};

// ---------------------------------------------------------------------------
// Helpers
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

/// C compares `currentmove` pointers; the tables are statics.
fn is_current(mi: &MonsterInfo, mv: &MonsterMove) -> bool {
    mi.currentmove
        .as_ref()
        .is_some_and(|cm| std::ptr::eq(cm.frames, mv.frames))
}

/// C `gi.sound(self, channel, gi.soundindex(name), 1, attenuation, 0)`.
fn sound(world: &GameWorld, key: EntityKey, channel: i32, name: &str, attenuation: f32) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), channel, index, 1.0, attenuation, 0.0);
}

/// The enemy's eye: C `enemy->s.origin` plus `viewheight` on z.
fn enemy_eye(world: &GameWorld, key: EntityKey) -> Option<Vec3f> {
    let enemy = world.entities.get(key)?.game.enemy?;
    let enemy = world.entities.get(enemy)?;
    Some(enemy.state.origin + Vec3f::new(0.0, 0.0, enemy.game.viewheight))
}

fn frame_of(world: &GameWorld, key: EntityKey) -> i32 {
    world.entities.get(key).map_or(0, |e| e.state.frame)
}

/// Whether the enemy is alive and in sight: the refire checks.
fn enemy_alive_and_visible(world: &GameWorld, key: EntityKey) -> bool {
    let Some(enemy) = world.entities.get(key).and_then(|e| e.game.enemy) else {
        return false;
    };
    world.entities.get(enemy).is_some_and(|e| e.game.health > 0) && ai::visible(world, key, enemy)
}

// ---------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------

/// C ref: `tank_sight`.
pub fn tank_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, "tank/sight1.wav", ATTN_NORM);
}

/// C ref: `tank_footstep`.
pub fn tank_footstep(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, "tank/step.wav", ATTN_NORM);
}

/// C ref: `tank_thud`.
pub fn tank_thud(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, "tank/tnkdeth2.wav", ATTN_NORM);
}

/// C ref: `tank_windup`.
pub fn tank_windup(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, "tank/tnkatck4.wav", ATTN_NORM);
}

/// C ref: `tank_idle`.
pub fn tank_idle(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, "tank/tnkidle1.wav", ATTN_IDLE);
}

/// C ref: `TankStrike`.
pub fn tank_strike(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, "tank/tnkatck5.wav", ATTN_NORM);
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// C ref: `tank_stand`.
pub fn tank_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &TANK_MOVE_STAND);
}

/// C ref: `tank_walk`.
pub fn tank_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &TANK_MOVE_WALK);
}

/// Brutal against players; hold ground, keep running, or start running.
/// C ref: `tank_run`.
pub fn tank_run(world: &mut GameWorld, key: EntityKey) {
    let enemy_is_client = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .and_then(|e| world.entities.get(e))
        .is_some_and(|e| e.client.is_some());
    let Some(mi) = world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_mut())
    else {
        return;
    };
    mi.aiflags.set(AiFlags::BRUTAL, enemy_is_client);
    let next = if mi.aiflags.contains(AiFlags::STAND_GROUND) {
        &TANK_MOVE_STAND
    } else if is_current(mi, &TANK_MOVE_WALK) || is_current(mi, &TANK_MOVE_START_RUN) {
        &TANK_MOVE_RUN
    } else {
        &TANK_MOVE_START_RUN
    };
    set_move(world, key, next);
}

/// C ref: `tank_pain`.
pub fn tank_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    damage: i32,
) {
    let time = world.level.time;
    let skill = world.game.skill;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.health < ent.game.max_health / 2 {
        ent.state.skinnum |= 1;
    }
    if damage <= 10 || time < ent.game.pain_debounce_time {
        return;
    }
    if damage <= 30 && world.random() > 0.2 {
        return;
    }
    // On hard or nightmare, no pain while attacking.
    let frame = frame_of(world, key);
    if skill >= crate::constants::SKILL_HARD
        && ((FRAME_ATTAK301..=FRAME_ATTAK330).contains(&frame)
            || (FRAME_ATTAK101..=FRAME_ATTAK116).contains(&frame))
    {
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.pain_debounce_time = time + 3.0;
    }
    sound(world, key, CHAN_VOICE, "tank/tnkpain2.wav", ATTN_NORM);
    if skill == crate::constants::SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    let next = if damage <= 30 {
        &TANK_MOVE_PAIN1
    } else if damage <= 60 {
        &TANK_MOVE_PAIN2
    } else {
        &TANK_MOVE_PAIN3
    };
    set_move(world, key, next);
}

// ---------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------

/// A bolt from the gun arm at the enemy's eye. C ref: `TankBlaster`.
pub fn tank_blaster(world: &mut GameWorld, key: EntityKey) {
    let flash = match frame_of(world, key) {
        FRAME_ATTAK110 => MZ2_TANK_BLASTER_1,
        FRAME_ATTAK113 => MZ2_TANK_BLASTER_2,
        _ => MZ2_TANK_BLASTER_3,
    };
    let (Some((start, _, _)), Some(end)) = (
        monster_flash_source(world, key, flash),
        enemy_eye(world, key),
    ) else {
        return;
    };
    monster_fire_blaster(world, key, start, end - start, 30, 800, flash, EF_BLASTER);
}

/// A rocket from the shoulder launcher at the enemy's eye.
/// C ref: `TankRocket`.
pub fn tank_rocket(world: &mut GameWorld, key: EntityKey) {
    let flash = match frame_of(world, key) {
        FRAME_ATTAK324 => MZ2_TANK_ROCKET_1,
        FRAME_ATTAK327 => MZ2_TANK_ROCKET_2,
        _ => MZ2_TANK_ROCKET_3,
    };
    let (Some((start, _, _)), Some(eye)) = (
        monster_flash_source(world, key, flash),
        enemy_eye(world, key),
    ) else {
        return;
    };
    let dir = (eye - start).normalize_or_zero();
    monster_fire_rocket(world, key, start, dir, 50, 550, flash);
}

/// One bullet of the chaingun sweep: pitched at the enemy, yawed 8
/// degrees a frame from right to left and back. C ref: `TankMachineGun`.
pub fn tank_machine_gun(world: &mut GameWorld, key: EntityKey) {
    let frame = frame_of(world, key);
    let flash = MZ2_TANK_MACHINEGUN_1 + (frame - FRAME_ATTAK406);
    let Some((start, _, _)) = monster_flash_source(world, key, flash) else {
        return;
    };
    let pitch = enemy_eye(world, key).map_or(0.0, |eye| crate::weapons::vectoangles(eye - start).x);
    let yaw = sweep_yaw(
        frame,
        world.entities.get(key).map_or(0.0, |e| e.state.angles.y),
    );
    let (forward, _, _) = crate::physics::angle_vectors(Vec3f::new(pitch, yaw, 0.0));
    monster_fire_bullet(
        world,
        key,
        start,
        forward,
        20,
        4,
        DEFAULT_BULLET_HSPREAD,
        DEFAULT_BULLET_VSPREAD,
        flash,
    );
}

/// The chaingun's yaw on `frame`: from 40 degrees left of `yaw` at
/// attak406 down to 32 right at attak415, then back. C ref: `TankMachineGun`.
fn sweep_yaw(frame: i32, yaw: f32) -> f32 {
    if frame <= FRAME_ATTAK415 {
        yaw - 8.0 * (frame - FRAME_ATTAK411) as f32
    } else {
        yaw + 8.0 * (frame - FRAME_ATTAK419) as f32
    }
}

/// Blast again on hard or nightmare, six times in ten.
/// C ref: `tank_reattack_blaster`.
pub fn tank_reattack_blaster(world: &mut GameWorld, key: EntityKey) {
    if world.game.skill >= crate::constants::SKILL_HARD
        && enemy_alive_and_visible(world, key)
        && world.random() <= 0.6
    {
        set_move(world, key, &TANK_MOVE_REATTACK_BLAST);
        return;
    }
    set_move(world, key, &TANK_MOVE_ATTACK_POST_BLAST);
}

/// After punching a corpse, forget it. C ref: `tank_poststrike`.
pub fn tank_poststrike(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.enemy = None;
    }
    tank_run(world, key);
}

/// Rocket again on hard or nightmare, four times in ten.
/// C ref: `tank_refire_rocket`.
pub fn tank_refire_rocket(world: &mut GameWorld, key: EntityKey) {
    if world.game.skill >= crate::constants::SKILL_HARD
        && enemy_alive_and_visible(world, key)
        && world.random() <= 0.4
    {
        set_move(world, key, &TANK_MOVE_ATTACK_FIRE_ROCKET);
        return;
    }
    set_move(world, key, &TANK_MOVE_ATTACK_POST_ROCKET);
}

/// C ref: `tank_doattack_rocket`.
pub fn tank_doattack_rocket(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &TANK_MOVE_ATTACK_FIRE_ROCKET);
}

/// Strike a dead enemy; otherwise chaingun or blaster up close, and
/// rockets too from beyond 250 units. C ref: `tank_attack`.
pub fn tank_attack(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let origin = ent.state.origin;
    let Some(enemy) = ent.game.enemy.and_then(|e| world.entities.get(e)) else {
        return;
    };
    let (enemy_health, range) = (enemy.game.health, (enemy.state.origin - origin).length());
    if enemy_health < 0 {
        set_move(world, key, &TANK_MOVE_ATTACK_STRIKE);
        if let Some(mi) = world
            .entities
            .get_mut(key)
            .and_then(|e| e.monsterinfo.as_mut())
        {
            mi.aiflags.remove(AiFlags::BRUTAL);
        }
        return;
    }
    let r = world.random();
    let next = if range <= 125.0 {
        if r < 0.4 {
            &TANK_MOVE_ATTACK_CHAIN
        } else {
            &TANK_MOVE_ATTACK_BLAST
        }
    } else if range <= 250.0 {
        if r < 0.5 {
            &TANK_MOVE_ATTACK_CHAIN
        } else {
            &TANK_MOVE_ATTACK_BLAST
        }
    } else if r < 0.33 {
        &TANK_MOVE_ATTACK_CHAIN
    } else if r < 0.66 {
        let time = world.level.time;
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.pain_debounce_time = time + 5.0; // no pain for a while
        }
        &TANK_MOVE_ATTACK_PRE_ROCKET
    } else {
        &TANK_MOVE_ATTACK_BLAST
    };
    set_move(world, key, next);
}

// ---------------------------------------------------------------------------
// Death
// ---------------------------------------------------------------------------

/// The corpse box, then stop. C ref: `tank_dead`.
pub fn tank_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -16.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 0.0);
    }
    ai::monster_dead(world, key);
}

/// Gib when hit hard enough, else fall over (once). C ref: `tank_die`.
pub fn tank_die(
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
    sound(world, key, CHAN_VOICE, "tank/death.wav", ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    set_move(world, key, &TANK_MOVE_DEATH);
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// C ref: `SP_monster_tank` for both classnames.
fn spawn_tank(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
    classname: &str,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    ent.game.classname = fields
        .get("classname")
        .cloned()
        .unwrap_or_else(|| classname.to_string());
    ent.mins = Vec3f::new(-32.0, -32.0, -16.0);
    ent.maxs = Vec3f::new(32.0, 32.0, 72.0);
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    let commander = ent.game.classname == COMMANDER;
    ent.game.health = if commander { 1000 } else { 750 };
    ent.game.gib_health = if commander { -225 } else { -200 };
    ent.game.max_health = ent.game.health;
    ent.game.mass = 500;
    ent.pain = Some(tank_pain);
    ent.die = Some(tank_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(tank_stand),
        walk: Some(tank_walk),
        run: Some(tank_run),
        attack: Some(tank_attack),
        sight: Some(tank_sight),
        idle: Some(tank_idle),
        currentmove: Some(TANK_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
    if commander {
        ent.state.skinnum = 2;
    }
}

pub fn sp_monster_tank(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_tank(storage, key, fields, "monster_tank");
}

pub fn sp_monster_tank_commander(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_tank(storage, key, fields, COMMANDER);
}

/// The sounds `SP_monster_tank` indexes, including the attack sounds it
/// precaches without keeping. C ref: `SP_monster_tank`.
pub fn tank_precache(world: &mut GameWorld, _key: EntityKey, _fields: &HashMap<String, String>) {
    for name in [
        "tank/tnkpain2.wav",
        "tank/tnkdeth2.wav",
        "tank/tnkidle1.wav",
        "tank/death.wav",
        "tank/step.wav",
        "tank/tnkatck4.wav",
        "tank/tnkatck5.wav",
        "tank/sight1.wav",
        "tank/tnkatck1.wav",
        "tank/tnkatk2a.wav",
        "tank/tnkatk2b.wav",
        "tank/tnkatk2c.wav",
        "tank/tnkatk2d.wav",
        "tank/tnkatk2e.wav",
        "tank/tnkatck3.wav",
    ] {
        world.gi.sound_index(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::misc::wire::WireImport;
    use crate::monster::test_support::*;
    use std::rc::Rc;

    fn tank() -> (GameWorld, Rc<WireImport>, EntityKey) {
        monster_world(sp_monster_tank)
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

    fn set_frame(world: &mut GameWorld, key: EntityKey, frame: i32) {
        world.entities.get_mut(key).unwrap().state.frame = frame;
    }

    #[test]
    fn tank_moves_match_c_tank_c() {
        let walk = [
            4.0, 5.0, 3.0, 2.0, 5.0, 5.0, 4.0, 4.0, 3.0, 5.0, 4.0, 5.0, 7.0, 7.0, 6.0, 6.0,
        ]
        .to_vec();
        let step: ThinkFn = tank_footstep;
        let blaster: ThinkFn = tank_blaster;
        let rocket: ThinkFn = tank_rocket;
        let stand = Some(ai::ai_stand as AnimFrameFn);
        let walk_ai = Some(ai::ai_walk as AnimFrameFn);
        let run_ai = Some(ai::ai_run as AnimFrameFn);
        let move_ai = Some(ai::ai_move as AnimFrameFn);
        let charge = Some(ai::ai_charge as AnimFrameFn);
        let chain_thinks: Vec<(usize, ThinkFn)> =
            (5..24).map(|i| (i, tank_machine_gun as ThinkFn)).collect();
        let mut pre_rocket = zeros(21);
        pre_rocket[11..16].copy_from_slice(&[1.0, 2.0, 7.0, 7.0, 7.0]);
        pre_rocket[20] = -3.0;
        let mut post_rocket = zeros(23);
        post_rocket[1..8].copy_from_slice(&[-1.0, -1.0, 0.0, 2.0, 3.0, 4.0, 2.0]);
        post_rocket[11..16].copy_from_slice(&[-9.0, -8.0, -7.0, -1.0, -1.0]);
        let strike = [
            3.0, 2.0, 2.0, 1.0, 6.0, 7.0, 9.0, 2.0, 1.0, 2.0, 2.0, 2.0, 0.0, 0.0, 0.0, 0.0, -2.0,
            -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, -1.0, -1.0, -1.0, -3.0,
            -10.0, -10.0, -2.0, -3.0, -2.0,
        ];
        let death = [
            -7.0, -2.0, -2.0, 1.0, 3.0, 6.0, 1.0, 1.0, 2.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, -3.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -4.0, -6.0, -4.0, -5.0, -7.0, -15.0, -5.0, 0.0, 0.0, 0.0,
        ];
        let moves: Vec<(&str, &MonsterMove, CMove)> = vec![
            (
                "stand",
                &TANK_MOVE_STAND,
                CMove {
                    bounds: (0, 29),
                    ai: vec![(30, stand)],
                    dists: zeros(30),
                    thinks: vec![],
                    end: None,
                },
            ),
            (
                "start_walk",
                &TANK_MOVE_START_WALK,
                CMove {
                    bounds: (30, 33),
                    ai: vec![(4, walk_ai)],
                    dists: vec![0.0, 6.0, 6.0, 11.0],
                    thinks: vec![(3, step)],
                    end: Some(tank_walk),
                },
            ),
            (
                "walk",
                &TANK_MOVE_WALK,
                CMove {
                    bounds: (34, 49),
                    ai: vec![(16, walk_ai)],
                    dists: walk.clone(),
                    thinks: vec![(7, step), (15, step)],
                    end: None,
                },
            ),
            (
                "stop_walk",
                &TANK_MOVE_STOP_WALK,
                CMove {
                    bounds: (50, 54),
                    ai: vec![(5, walk_ai)],
                    dists: vec![3.0, 3.0, 2.0, 2.0, 4.0],
                    thinks: vec![(4, step)],
                    end: Some(tank_stand),
                },
            ),
            (
                "start_run",
                &TANK_MOVE_START_RUN,
                CMove {
                    bounds: (30, 33),
                    ai: vec![(4, run_ai)],
                    dists: vec![0.0, 6.0, 6.0, 11.0],
                    thinks: vec![(3, step)],
                    end: Some(tank_run),
                },
            ),
            (
                "run",
                &TANK_MOVE_RUN,
                CMove {
                    bounds: (34, 49),
                    ai: vec![(16, run_ai)],
                    dists: walk,
                    thinks: vec![(7, step), (15, step)],
                    end: None,
                },
            ),
            (
                "stop_run",
                &TANK_MOVE_STOP_RUN,
                CMove {
                    bounds: (50, 54),
                    ai: vec![(5, run_ai)],
                    dists: vec![3.0, 3.0, 2.0, 2.0, 4.0],
                    thinks: vec![(4, step)],
                    end: Some(tank_walk),
                },
            ),
            (
                "pain1",
                &TANK_MOVE_PAIN1,
                CMove {
                    bounds: (197, 200),
                    ai: vec![(4, move_ai)],
                    dists: zeros(4),
                    thinks: vec![],
                    end: Some(tank_run),
                },
            ),
            (
                "pain2",
                &TANK_MOVE_PAIN2,
                CMove {
                    bounds: (201, 205),
                    ai: vec![(5, move_ai)],
                    dists: zeros(5),
                    thinks: vec![],
                    end: Some(tank_run),
                },
            ),
            (
                "pain3",
                &TANK_MOVE_PAIN3,
                CMove {
                    bounds: (206, 221),
                    ai: vec![(16, move_ai)],
                    dists: vec![
                        -7.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        0.0,
                    ],
                    thinks: vec![(15, step)],
                    end: Some(tank_run),
                },
            ),
            (
                "attack_blast",
                &TANK_MOVE_ATTACK_BLAST,
                CMove {
                    bounds: (55, 70),
                    ai: vec![(16, charge)],
                    dists: vec![
                        0.0, 0.0, 0.0, 0.0, -1.0, -2.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        0.0, 0.0,
                    ],
                    thinks: vec![(9, blaster), (12, blaster), (15, blaster)],
                    end: Some(tank_reattack_blaster),
                },
            ),
            (
                "reattack_blast",
                &TANK_MOVE_REATTACK_BLAST,
                CMove {
                    bounds: (65, 70),
                    ai: vec![(6, charge)],
                    dists: zeros(6),
                    thinks: vec![(2, blaster), (5, blaster)],
                    end: Some(tank_reattack_blaster),
                },
            ),
            (
                "attack_post_blast",
                &TANK_MOVE_ATTACK_POST_BLAST,
                CMove {
                    bounds: (71, 76),
                    ai: vec![(6, move_ai)],
                    dists: vec![0.0, 0.0, 2.0, 3.0, 2.0, -2.0],
                    thinks: vec![(5, step)],
                    end: Some(tank_run),
                },
            ),
            (
                "attack_strike",
                &TANK_MOVE_ATTACK_STRIKE,
                CMove {
                    bounds: (77, 114),
                    ai: vec![(38, move_ai)],
                    dists: strike.to_vec(),
                    thinks: vec![
                        (6, step),
                        (10, step),
                        (18, tank_windup),
                        (25, tank_strike),
                        (37, step),
                    ],
                    end: Some(tank_poststrike),
                },
            ),
            (
                "attack_pre_rocket",
                &TANK_MOVE_ATTACK_PRE_ROCKET,
                CMove {
                    bounds: (115, 135),
                    ai: vec![(21, charge)],
                    dists: pre_rocket,
                    thinks: vec![(15, step)],
                    end: Some(tank_doattack_rocket),
                },
            ),
            (
                "attack_fire_rocket",
                &TANK_MOVE_ATTACK_FIRE_ROCKET,
                CMove {
                    bounds: (136, 144),
                    ai: vec![(9, charge)],
                    dists: vec![-3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0],
                    thinks: vec![(2, rocket), (5, rocket), (8, rocket)],
                    end: Some(tank_refire_rocket),
                },
            ),
            (
                "attack_post_rocket",
                &TANK_MOVE_ATTACK_POST_ROCKET,
                CMove {
                    bounds: (145, 167),
                    ai: vec![(23, charge)],
                    dists: post_rocket,
                    thinks: vec![(15, step)],
                    end: Some(tank_run),
                },
            ),
            (
                "attack_chain",
                &TANK_MOVE_ATTACK_CHAIN,
                CMove {
                    bounds: (168, 196),
                    ai: vec![(5, charge), (19, None), (5, charge)],
                    dists: zeros(29),
                    thinks: chain_thinks,
                    end: Some(tank_run),
                },
            ),
            (
                "death",
                &TANK_MOVE_DEATH,
                CMove {
                    bounds: (222, 253),
                    ai: vec![(32, move_ai)],
                    dists: death.to_vec(),
                    thinks: vec![(27, tank_thud)],
                    end: Some(tank_dead),
                },
            ),
        ];
        for (name, mv, c) in moves {
            assert_move(name, mv, c);
        }
    }

    #[test]
    fn tank_spawns_with_c_health_box_callbacks_and_starts_walking() {
        let (world, _, key) = tank();
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.health, ent.game.max_health, ent.game.mass),
            (750, 750, 500)
        );
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-32.0, -32.0, -16.0),
                Vec3f::new(32.0, 32.0, 72.0)
            )
        );
        assert_eq!(
            (ent.game.movetype, ent.solid, ent.state.skinnum),
            (MoveType::Step, Solid::Bbox, 0)
        );
        assert!(ent
            .pain
            .is_some_and(|f| std::ptr::fn_addr_eq(f, tank_pain as crate::entity::PainFn)));
        assert!(ent
            .die
            .is_some_and(|f| std::ptr::fn_addr_eq(f, tank_die as crate::entity::DieFn)));
        assert!(ent
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, ai::walkmonster_start_go as ThinkFn)));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(
            mi.walk.is_some()
                && mi.run.is_some()
                && mi.attack.is_some()
                && mi.sight.is_some()
                && mi.idle.is_some()
        );
        assert!(mi.melee.is_none() && mi.dodge.is_none() && mi.search.is_none());
        assert_eq!(mi.scale, 1.0);
        assert!(playing(&world, key, &TANK_MOVE_STAND));
    }

    #[test]
    fn tank_commander_has_more_health_and_skin_two() {
        let (world, _, key) = monster_world(sp_monster_tank_commander);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, COMMANDER);
        assert_eq!((ent.game.health, ent.state.skinnum), (1000, 2));
        assert_eq!(ent.game.gib_health, -225);
        let (world, _, key) = monster_world(sp_monster_tank);
        assert_eq!(world.entities.get(key).unwrap().game.gib_health, -200);
    }

    #[test]
    fn tank_precache_indexes_every_sound_sp_monster_tank_does() {
        let (mut world, gi, key) = tank();
        tank_precache(&mut world, key, &HashMap::new());
        let names = gi.inner.sounds.borrow();
        assert_eq!(names.len(), 15);
        assert!(names.iter().any(|n| n == "tank/tnkatk2e.wav"));
        assert!(names.iter().any(|n| n == "tank/sight1.wav"));
    }

    #[test]
    fn tank_sounds_play_on_the_c_channels() {
        let (mut world, gi, key) = tank();
        let other = world.spawn().unwrap();
        tank_sight(&mut world, key, other);
        tank_footstep(&mut world, key);
        tank_thud(&mut world, key);
        tank_windup(&mut world, key);
        tank_idle(&mut world, key);
        tank_strike(&mut world, key);
        assert_eq!(
            wire_sounds(&gi),
            [
                (CHAN_VOICE, "tank/sight1.wav".to_string(), ATTN_NORM),
                (CHAN_BODY, "tank/step.wav".to_string(), ATTN_NORM),
                (CHAN_BODY, "tank/tnkdeth2.wav".to_string(), ATTN_NORM),
                (CHAN_WEAPON, "tank/tnkatck4.wav".to_string(), ATTN_NORM),
                (CHAN_VOICE, "tank/tnkidle1.wav".to_string(), ATTN_IDLE),
                (CHAN_WEAPON, "tank/tnkatck5.wav".to_string(), ATTN_NORM),
            ]
        );
    }

    #[test]
    fn tank_run_is_brutal_only_against_a_player() {
        let (mut world, _, key) = tank();
        let player = player_at(&mut world, Vec3f::new(100.0, 0.0, 0.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        tank_run(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::BRUTAL));
        give_enemy(&mut world, key, Vec3f::ZERO);
        tank_run(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::BRUTAL));
    }

    #[test]
    fn tank_run_starts_running_keeps_running_or_holds_ground() {
        let (mut world, _, key) = tank();
        tank_run(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_START_RUN));
        tank_run(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_RUN));
        tank_walk(&mut world, key);
        tank_run(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_RUN));
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::STAND_GROUND;
        tank_run(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_STAND));
    }

    /// Hit the tank for `damage` after seeding the pain roll; whether it
    /// flinched (sound) and the move it is on afterwards.
    fn pain(
        world: &mut GameWorld,
        gi: &WireImport,
        key: EntityKey,
        damage: i32,
        roll: f32,
    ) -> bool {
        seed_random(world, |r| (r - roll).abs() < 0.01);
        let before = gi.sounds.borrow().len();
        let other = key;
        tank_pain(world, key, other, 0.0, damage);
        gi.sounds.borrow().len() > before
    }

    #[test]
    fn tank_pain_darkens_the_skin_below_half_health() {
        let (mut world, gi, key) = tank();
        world.entities.get_mut(key).unwrap().game.health = 374;
        pain(&mut world, &gi, key, 1, 0.5);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
    }

    #[test]
    fn tank_pain_ignores_ten_damage_and_hits_during_the_debounce() {
        let (mut world, gi, key) = tank();
        assert!(!pain(&mut world, &gi, key, 10, 0.1));
        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 1.5;
        assert!(!pain(&mut world, &gi, key, 100, 0.1));
        assert!(playing(&world, key, &TANK_MOVE_STAND));
    }

    #[test]
    fn tank_pain_flinches_from_light_hits_one_time_in_five() {
        let (mut world, gi, key) = tank();
        assert!(!pain(&mut world, &gi, key, 30, 0.5));
        assert!(pain(&mut world, &gi, key, 30, 0.1));
        assert!(playing(&world, key, &TANK_MOVE_PAIN1));
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            4.0
        );
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "tank/tnkpain2.wav".to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn tank_pain_picks_the_move_by_damage() {
        for (damage, mv) in [
            (31, &TANK_MOVE_PAIN2),
            (60, &TANK_MOVE_PAIN2),
            (61, &TANK_MOVE_PAIN3),
        ] {
            let (mut world, gi, key) = tank();
            assert!(pain(&mut world, &gi, key, damage, 0.9));
            assert!(playing(&world, key, mv), "{damage}");
        }
    }

    #[test]
    fn tank_pain_on_hard_never_interrupts_blaster_or_rockets() {
        for frame in [
            FRAME_ATTAK101,
            FRAME_ATTAK116,
            FRAME_ATTAK301,
            FRAME_ATTAK330,
        ] {
            let (mut world, gi, key) = tank();
            world.game.skill = crate::constants::SKILL_HARD;
            set_frame(&mut world, key, frame);
            assert!(!pain(&mut world, &gi, key, 100, 0.1), "{frame}");
        }
        let (mut world, gi, key) = tank();
        world.game.skill = crate::constants::SKILL_HARD;
        set_frame(&mut world, key, FRAME_ATTAK201);
        assert!(pain(&mut world, &gi, key, 100, 0.1));
        // On medium the attack frames do flinch.
        let (mut world, gi, key) = tank();
        set_frame(&mut world, key, FRAME_ATTAK101);
        assert!(pain(&mut world, &gi, key, 100, 0.1));
    }

    #[test]
    fn tank_pain_in_nightmare_cries_out_without_a_pain_move() {
        let (mut world, gi, key) = tank();
        world.game.skill = crate::constants::SKILL_HARDPLUS;
        assert!(pain(&mut world, &gi, key, 100, 0.1));
        assert!(playing(&world, key, &TANK_MOVE_STAND));
    }

    fn attack_at(range: f32, roll: f32) -> (GameWorld, EntityKey) {
        let (mut world, _, key) = tank();
        give_enemy(&mut world, key, Vec3f::new(range, 0.0, 0.0));
        seed_random(&mut world, |r| (r - roll).abs() < 0.005);
        tank_attack(&mut world, key);
        (world, key)
    }

    #[test]
    fn tank_attack_chooses_by_range_and_roll() {
        let cases: [(f32, f32, &MonsterMove); 9] = [
            (125.0, 0.39, &TANK_MOVE_ATTACK_CHAIN),
            (125.0, 0.41, &TANK_MOVE_ATTACK_BLAST),
            (250.0, 0.49, &TANK_MOVE_ATTACK_CHAIN),
            (250.0, 0.51, &TANK_MOVE_ATTACK_BLAST),
            (251.0, 0.32, &TANK_MOVE_ATTACK_CHAIN),
            (251.0, 0.34, &TANK_MOVE_ATTACK_PRE_ROCKET),
            (251.0, 0.65, &TANK_MOVE_ATTACK_PRE_ROCKET),
            (251.0, 0.67, &TANK_MOVE_ATTACK_BLAST),
            (900.0, 0.9, &TANK_MOVE_ATTACK_BLAST),
        ];
        for (range, roll, mv) in cases {
            let (world, key) = attack_at(range, roll);
            assert!(playing(&world, key, mv), "{range} {roll}");
        }
    }

    #[test]
    fn tank_attack_with_rockets_holds_off_pain_for_five_seconds() {
        let (world, key) = attack_at(400.0, 0.5);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            6.0
        );
        let (world, key) = attack_at(400.0, 0.9);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            0.0
        );
    }

    #[test]
    fn tank_attack_on_a_dead_enemy_strikes_and_is_no_longer_brutal() {
        let (mut world, _, key) = tank();
        let enemy = give_enemy(&mut world, key, Vec3f::new(50.0, 0.0, 0.0));
        world.entities.get_mut(enemy).unwrap().game.health = -1;
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::BRUTAL;
        tank_attack(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_ATTACK_STRIKE));
        assert!(!mi(&world, key).aiflags.contains(AiFlags::BRUTAL));
    }

    #[test]
    fn tank_poststrike_forgets_the_enemy_and_runs() {
        let (mut world, _, key) = tank();
        give_enemy(&mut world, key, Vec3f::ZERO);
        tank_poststrike(&mut world, key);
        assert!(world.entities.get(key).unwrap().game.enemy.is_none());
        assert!(playing(&world, key, &TANK_MOVE_START_RUN));
    }

    /// Run an end function `n` times with the enemy alive and in sight;
    /// how often it chose `again`.
    fn refires(skill: i32, end: ThinkFn, again: &MonsterMove, other: &MonsterMove) -> usize {
        let (mut world, _, key) = tank();
        world.game.skill = skill;
        give_enemy(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        let mut count = 0;
        for _ in 0..1000 {
            end(&mut world, key);
            if playing(&world, key, again) {
                count += 1;
            } else {
                assert!(playing(&world, key, other));
            }
        }
        count
    }

    #[test]
    fn tank_reattack_blaster_repeats_six_times_in_ten_on_hard_only() {
        let (again, post) = (&TANK_MOVE_REATTACK_BLAST, &TANK_MOVE_ATTACK_POST_BLAST);
        assert_eq!(refires(1, tank_reattack_blaster, again, post), 0);
        let hard = refires(
            crate::constants::SKILL_HARD,
            tank_reattack_blaster,
            again,
            post,
        );
        assert!((550..650).contains(&hard), "{hard}");
    }

    #[test]
    fn tank_refire_rocket_repeats_four_times_in_ten_on_hard_only() {
        let (again, post) = (&TANK_MOVE_ATTACK_FIRE_ROCKET, &TANK_MOVE_ATTACK_POST_ROCKET);
        assert_eq!(refires(1, tank_refire_rocket, again, post), 0);
        let hard = refires(
            crate::constants::SKILL_HARDPLUS,
            tank_refire_rocket,
            again,
            post,
        );
        assert!((350..450).contains(&hard), "{hard}");
    }

    #[test]
    fn tank_refires_stop_at_a_dead_or_hidden_enemy() {
        let (mut world, gi, key) = tank();
        world.game.skill = crate::constants::SKILL_HARD;
        let enemy = give_enemy(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        world.entities.get_mut(enemy).unwrap().game.health = 0;
        seed_random(&mut world, |r| r < 0.1);
        tank_reattack_blaster(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_ATTACK_POST_BLAST));
        world.entities.get_mut(enemy).unwrap().game.health = 100;
        *gi.inner.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        seed_random(&mut world, |r| r < 0.1);
        tank_refire_rocket(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_ATTACK_POST_ROCKET));
    }

    #[test]
    fn tank_doattack_rocket_fires_the_rocket_loop() {
        let (mut world, _, key) = tank();
        tank_doattack_rocket(&mut world, key);
        assert!(playing(&world, key, &TANK_MOVE_ATTACK_FIRE_ROCKET));
    }

    #[test]
    fn tank_blaster_fires_a_30_damage_bolt_from_the_frame_flash_at_the_eye() {
        for (frame, flash) in [
            (FRAME_ATTAK110, MZ2_TANK_BLASTER_1),
            (FRAME_ATTAK113, MZ2_TANK_BLASTER_2),
            (FRAME_ATTAK116, MZ2_TANK_BLASTER_3),
        ] {
            let (mut world, gi, key) = tank();
            give_enemy(&mut world, key, Vec3f::new(400.0, 0.0, 0.0));
            set_frame(&mut world, key, frame);
            tank_blaster(&mut world, key);
            let bolt = world.entities.get(all_of(&world, "bolt")[0]).unwrap();
            let (start, _, _) = monster_flash_source(&world, key, flash).unwrap();
            let want = (Vec3f::new(400.0, 0.0, 22.0) - start).normalize() * 800.0;
            assert!((bolt.velocity - want).length() < 0.01);
            assert_eq!(
                (bolt.game.dmg, bolt.state.effects & EF_BLASTER),
                (30, EF_BLASTER)
            );
            assert_eq!(muzzle_flashes(&gi), [flash]);
        }
    }

    #[test]
    fn tank_rocket_fires_a_50_damage_rocket_at_550() {
        for (frame, flash) in [
            (FRAME_ATTAK324, MZ2_TANK_ROCKET_1),
            (FRAME_ATTAK327, MZ2_TANK_ROCKET_2),
            (FRAME_ATTAK330, MZ2_TANK_ROCKET_3),
        ] {
            let (mut world, gi, key) = tank();
            give_enemy(&mut world, key, Vec3f::new(400.0, 0.0, 0.0));
            set_frame(&mut world, key, frame);
            tank_rocket(&mut world, key);
            let rocket = world.entities.get(all_of(&world, "rocket")[0]).unwrap();
            let (start, _, _) = monster_flash_source(&world, key, flash).unwrap();
            let want = (Vec3f::new(400.0, 0.0, 22.0) - start).normalize() * 550.0;
            assert!((rocket.velocity - want).length() < 0.01);
            assert_eq!(rocket.game.dmg, 50);
            assert_eq!(muzzle_flashes(&gi), [flash]);
        }
    }

    #[test]
    fn tank_machine_gun_flashes_each_frame_of_the_sweep() {
        let (mut world, gi, key) = tank();
        give_enemy(&mut world, key, Vec3f::new(400.0, 0.0, 0.0));
        for frame in FRAME_ATTAK406..=FRAME_ATTAK406 + 18 {
            set_frame(&mut world, key, frame);
            tank_machine_gun(&mut world, key);
        }
        let want: Vec<i32> = (MZ2_TANK_MACHINEGUN_1..MZ2_TANK_MACHINEGUN_1 + 19).collect();
        assert_eq!(muzzle_flashes(&gi), want);
    }

    /// A tank at (0, 0, 24) facing +x on attak406 (yaw + 40), with a
    /// player at (300, 0, `player_z`) as its enemy.
    fn tank_in_arena(player_z: f32) -> (GameWorld, Rc<ArenaImport>, EntityKey, EntityKey) {
        let (mut world, gi, player) = player_arena(300.0);
        world.entities.get_mut(player).unwrap().state.origin.z = player_z;
        world.link_entity(player);
        let key = world.spawn().unwrap();
        sp_monster_tank(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::new(0.0, 0.0, 24.0);
        ent.state.frame = FRAME_ATTAK406;
        ent.game.enemy = Some(player);
        world.link_entity(key);
        (world, gi, key, player)
    }

    /// Put the trace target 100 units along `(pitch, yaw)` from the
    /// muzzle, fire one chaingun bullet and return the player's damage.
    fn chaingun_damage_along(
        world: &mut GameWorld,
        gi: &ArenaImport,
        key: EntityKey,
        player: EntityKey,
        pitch: f32,
    ) -> i32 {
        let flash = MZ2_TANK_MACHINEGUN_1;
        let (start, _, _) = monster_flash_source(world, key, flash).unwrap();
        let (forward, _, _) = crate::physics::angle_vectors(Vec3f::new(pitch, 40.0, 0.0));
        gi.target_at(
            start + forward * 100.0,
            8.0,
            world.number_of(player).unwrap(),
        );
        let before = world.entities.get(player).unwrap().game.health;
        tank_machine_gun(world, key);
        before - world.entities.get(player).unwrap().game.health
    }

    #[test]
    fn tank_machine_gun_pitches_at_the_enemys_eye_for_20_damage() {
        let (mut world, gi, key, player) = tank_in_arena(224.0);
        let (start, _, _) = monster_flash_source(&world, key, MZ2_TANK_MACHINEGUN_1).unwrap();
        let p = world.entities.get(player).unwrap();
        let eye = p.state.origin + Vec3f::new(0.0, 0.0, p.game.viewheight);
        let pitch = crate::weapons::vectoangles(eye - start).x;
        assert!(pitch < 0.0, "the enemy is above: negative pitch looks up");
        assert_eq!(
            chaingun_damage_along(&mut world, &gi, key, player, pitch),
            20
        );
        // The same bullet fired level would miss a box on the pitched line.
        let (mut world, gi, key, player) = tank_in_arena(224.0);
        assert_eq!(chaingun_damage_along(&mut world, &gi, key, player, 0.0), 0);
    }

    #[test]
    fn tank_machine_gun_without_an_enemy_fires_level() {
        let (mut world, gi, key, player) = tank_in_arena(224.0);
        world.entities.get_mut(key).unwrap().game.enemy = None;
        assert_eq!(chaingun_damage_along(&mut world, &gi, key, player, 0.0), 20);
    }

    #[test]
    fn tank_machine_gun_sweeps_yaw_eight_degrees_a_frame_right_then_left() {
        assert_eq!(sweep_yaw(FRAME_ATTAK406, 90.0), 130.0);
        assert_eq!(sweep_yaw(FRAME_ATTAK411, 90.0), 90.0);
        assert_eq!(sweep_yaw(FRAME_ATTAK415, 90.0), 58.0);
        assert_eq!(sweep_yaw(FRAME_ATTAK415 + 1, 90.0), 66.0);
        assert_eq!(sweep_yaw(FRAME_ATTAK419, 90.0), 90.0);
        assert_eq!(sweep_yaw(FRAME_ATTAK406 + 18, 90.0), 130.0);
    }

    #[test]
    fn tank_die_gibs_at_or_below_gib_health() {
        let (mut world, gi, key) = tank();
        world.entities.get_mut(key).unwrap().game.health = -200;
        tank_die(&mut world, key, key, key, 300, Vec3f::ZERO);
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "misc/udeath.wav".to_string(), ATTN_NORM)]
        );
        // sm_meat, four sm_metal and the chest are new entities; the tank
        // itself becomes the gear head.
        let models = gi.inner.models.borrow().clone();
        for model in ["sm_meat", "sm_metal", "chest", "gear"] {
            assert!(models.iter().any(|m| m.contains(model)), "{model}");
        }
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert!(ent.monsterinfo.is_some() && ent.svflags & SvFlags::MONSTER.bits() == 0);
    }

    #[test]
    fn tank_die_above_gib_health_falls_over_once() {
        let (mut world, gi, key) = tank();
        world.entities.get_mut(key).unwrap().game.health = -199;
        tank_die(&mut world, key, key, key, 300, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage, ent.solid),
            (DeadFlag::Dead, TakeDamage::Yes, Solid::Bbox)
        );
        assert!(playing(&world, key, &TANK_MOVE_DEATH));
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "tank/death.wav".to_string(), ATTN_NORM)]
        );
        set_frame(&mut world, key, FRAME_DEATH101 + 5);
        tank_die(&mut world, key, key, key, 300, Vec3f::ZERO);
        assert_eq!(gi.sounds.borrow().len(), 1, "already dead");
    }

    #[test]
    fn tank_commander_needs_225_damage_past_death_to_gib() {
        let (mut world, gi, key) = monster_world(sp_monster_tank_commander);
        world.entities.get_mut(key).unwrap().game.health = -210;
        tank_die(&mut world, key, key, key, 300, Vec3f::ZERO);
        assert!(playing(&world, key, &TANK_MOVE_DEATH));
        assert!(gi.inner.models.borrow().is_empty());
    }

    #[test]
    fn tank_dead_leaves_a_low_corpse_box_that_falls() {
        let (mut world, _, key) = tank();
        tank_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-16.0, -16.0, -16.0), Vec3f::new(16.0, 16.0, 0.0))
        );
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
        assert_eq!(ent.game.nextthink, 0.0);
    }

    #[test]
    fn tank_sights_a_player_runs_and_shoots_at_it() {
        let (mut world, gi, key) = tank();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 16.0);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 24.0));
        let at = world.entities.get(key).unwrap().state.origin;
        clear_shots_at(&gi, &world, player, at, 100_000);
        let fired = run_monster(&mut world, key, 600, |w| {
            !all_of(w, "bolt").is_empty() || !all_of(w, "rocket").is_empty()
        });
        assert!(fired, "no bolt or rocket");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(wire_sounds(&gi)
            .iter()
            .any(|(_, name, _)| name == "tank/sight1.wav"));
        assert!(!muzzle_flashes(&gi).is_empty());
    }
}
