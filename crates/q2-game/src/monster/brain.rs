//! Brain: claws and chest tentacles in melee, behind a power screen.
//! C ref: `~/Qwasm2/src/game/monster/brain/brain.c`.

use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::{frame, set_move, sound};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: brain.h.
// ---------------------------------------------------------------------------

const FRAME_WALK101: i32 = 0;
const FRAME_WALK111: i32 = 10;
const FRAME_ATTAK101: i32 = 53;
const FRAME_ATTAK118: i32 = 70;
const FRAME_ATTAK201: i32 = 71;
const FRAME_ATTAK217: i32 = 87;
const FRAME_PAIN101: i32 = 88;
const FRAME_PAIN121: i32 = 108;
const FRAME_PAIN201: i32 = 109;
const FRAME_PAIN208: i32 = 116;
const FRAME_PAIN301: i32 = 117;
const FRAME_PAIN306: i32 = 122;
const FRAME_DEATH101: i32 = 123;
const FRAME_DEATH118: i32 = 140;
const FRAME_DEATH201: i32 = 141;
const FRAME_DEATH205: i32 = 145;
const FRAME_DUCK01: i32 = 146;
const FRAME_DUCK08: i32 = 153;
const FRAME_DEFENS01: i32 = 154;
const FRAME_DEFENS08: i32 = 161;
const FRAME_STAND01: i32 = 162;
const FRAME_STAND30: i32 = 191;
const FRAME_STAND31: i32 = 192;
const FRAME_STAND60: i32 = 221;

/// C `MODEL_SCALE` (brain.h).
const MODEL_SCALE: f32 = 1.0;

// C SP_monster_brain values.
const HEALTH: i32 = 300;
const MINS: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const MAXS: Vec3f = Vec3f::new(16.0, 16.0, 32.0);
const MASS: i32 = 400;
const POWER_ARMOR_POWER: i32 = 100;

/// The spawnflag the tentacles set on a grab, so the chest closes into
/// the claw attack. C ref: `brain_tentacle_attack` (65536).
const SPAWNFLAG_GRABBED: i32 = 65536;

const SOUND_CHEST_OPEN: &str = "brain/brnatck1.wav";
const SOUND_TENTACLES_EXTEND: &str = "brain/brnatck2.wav";
const SOUND_TENTACLES_RETRACT: &str = "brain/brnatck3.wav";
const SOUND_DEATH: &str = "brain/brndeth1.wav";
const SOUND_IDLE1: &str = "brain/brnidle1.wav";
const SOUND_IDLE2: &str = "brain/brnidle2.wav";
const SOUND_IDLE3: &str = "brain/brnlens1.wav";
const SOUND_PAIN1: &str = "brain/brnpain1.wav";
const SOUND_PAIN2: &str = "brain/brnpain2.wav";
const SOUND_SIGHT: &str = "brain/brnsght1.wav";
const SOUND_SEARCH: &str = "brain/brnsrch1.wav";
const SOUND_MELEE1: &str = "brain/melee1.wav";
const SOUND_MELEE2: &str = "brain/melee2.wav";
const SOUND_MELEE3: &str = "brain/melee3.wav";

/// The sounds C `SP_monster_brain` precaches, in its order. C never plays
/// the tentacle extend sound or the first two idles.
const PRECACHE: [&str; 14] = [
    SOUND_CHEST_OPEN,
    SOUND_TENTACLES_EXTEND,
    SOUND_TENTACLES_RETRACT,
    SOUND_DEATH,
    SOUND_IDLE1,
    SOUND_IDLE2,
    SOUND_IDLE3,
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_SIGHT,
    SOUND_SEARCH,
    SOUND_MELEE1,
    SOUND_MELEE2,
    SOUND_MELEE3,
];

// ---------------------------------------------------------------------------
// Moves. C ref: brain.c `brain_frames_*` / `brain_move_*`.
// ---------------------------------------------------------------------------

static BRAIN_FRAMES_STAND: [MonsterFrame; 30] = [
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

static BRAIN_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND30,
    frames: &BRAIN_FRAMES_STAND,
    endfunc: None,
};

static BRAIN_FRAMES_IDLE: [MonsterFrame; 30] = [
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

static BRAIN_MOVE_IDLE: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND31,
    lastframe: FRAME_STAND60,
    frames: &BRAIN_FRAMES_IDLE,
    endfunc: Some(brain_stand),
};

static BRAIN_FRAMES_WALK1: [MonsterFrame; 11] = [
    frame(ai::ai_walk, 7.0, None),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 3.0, None),
    frame(ai::ai_walk, 3.0, Some(brain_footstep)),
    frame(ai::ai_walk, 1.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, -4.0, None),
    frame(ai::ai_walk, -1.0, Some(brain_footstep)),
    frame(ai::ai_walk, 2.0, None),
];

static BRAIN_MOVE_WALK1: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK101,
    lastframe: FRAME_WALK111,
    frames: &BRAIN_FRAMES_WALK1,
    endfunc: None,
};

static BRAIN_FRAMES_DEFENSE: [MonsterFrame; 9] = [
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

/// Never selected in C either.
pub static BRAIN_MOVE_DEFENSE: MonsterMove = MonsterMove {
    firstframe: FRAME_DEFENS01,
    lastframe: FRAME_DEFENS08,
    frames: &BRAIN_FRAMES_DEFENSE,
    endfunc: None,
};

static BRAIN_FRAMES_PAIN3: [MonsterFrame; 6] = [
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -4.0, None),
];

static BRAIN_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN306,
    frames: &BRAIN_FRAMES_PAIN3,
    endfunc: Some(brain_run),
};

static BRAIN_FRAMES_PAIN2: [MonsterFrame; 8] = [
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, -2.0, None),
];

static BRAIN_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN208,
    frames: &BRAIN_FRAMES_PAIN2,
    endfunc: Some(brain_run),
};

static BRAIN_FRAMES_PAIN1: [MonsterFrame; 21] = [
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -6.0, Some(brain_footstep)),
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
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 7.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 3.0, Some(brain_footstep)),
    frame(ai::ai_move, -1.0, None),
];

static BRAIN_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN121,
    frames: &BRAIN_FRAMES_PAIN1,
    endfunc: Some(brain_run),
};

static BRAIN_FRAMES_DUCK: [MonsterFrame; 8] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, Some(brain_duck_down)),
    frame(ai::ai_move, 17.0, Some(brain_duck_hold)),
    frame(ai::ai_move, -3.0, Some(brain_footstep)),
    frame(ai::ai_move, -1.0, Some(brain_duck_up)),
    frame(ai::ai_move, -5.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -6.0, Some(brain_footstep)),
];

static BRAIN_MOVE_DUCK: MonsterMove = MonsterMove {
    firstframe: FRAME_DUCK01,
    lastframe: FRAME_DUCK08,
    frames: &BRAIN_FRAMES_DUCK,
    endfunc: Some(brain_run),
};

static BRAIN_FRAMES_DEATH2: [MonsterFrame; 5] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 9.0, None),
    frame(ai::ai_move, 0.0, None),
];

static BRAIN_MOVE_DEATH2: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH201,
    lastframe: FRAME_DEATH205,
    frames: &BRAIN_FRAMES_DEATH2,
    endfunc: Some(brain_dead),
};

static BRAIN_FRAMES_DEATH1: [MonsterFrame; 18] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 9.0, None),
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

static BRAIN_MOVE_DEATH1: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH118,
    frames: &BRAIN_FRAMES_DEATH1,
    endfunc: Some(brain_dead),
};

static BRAIN_FRAMES_ATTACK1: [MonsterFrame; 18] = [
    frame(ai::ai_charge, 8.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, 0.0, Some(brain_footstep)),
    frame(ai::ai_charge, -3.0, Some(brain_swing_right)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -5.0, None),
    frame(ai::ai_charge, -7.0, Some(brain_hit_right)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 6.0, Some(brain_swing_left)),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 2.0, Some(brain_hit_left)),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 6.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, -11.0, Some(brain_footstep)),
];

static BRAIN_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK118,
    frames: &BRAIN_FRAMES_ATTACK1,
    endfunc: Some(brain_run),
};

static BRAIN_FRAMES_ATTACK2: [MonsterFrame; 17] = [
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, -4.0, None),
    frame(ai::ai_charge, -4.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 0.0, Some(brain_chest_open)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 13.0, Some(brain_tentacle_attack)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -9.0, Some(brain_chest_closed)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 4.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, -6.0, None),
];

static BRAIN_MOVE_ATTACK2: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK217,
    frames: &BRAIN_FRAMES_ATTACK2,
    endfunc: Some(brain_run),
};

static BRAIN_FRAMES_RUN: [MonsterFrame; 11] = [
    frame(ai::ai_run, 9.0, None),
    frame(ai::ai_run, 2.0, None),
    frame(ai::ai_run, 3.0, None),
    frame(ai::ai_run, 3.0, Some(brain_footstep)),
    frame(ai::ai_run, 1.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 10.0, None),
    frame(ai::ai_run, -4.0, None),
    frame(ai::ai_run, -1.0, Some(brain_footstep)),
    frame(ai::ai_run, 2.0, None),
];

static BRAIN_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK101,
    lastframe: FRAME_WALK111,
    frames: &BRAIN_FRAMES_RUN,
    endfunc: None,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn monsterinfo_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MonsterInfo> {
    world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_deref_mut())
}

fn set_power_armor(world: &mut GameWorld, key: EntityKey, kind: i32) {
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.power_armor_type = kind;
    }
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

/// C `g_monsterfootsteps` defaults to 0 and is not ported, so the step
/// sounds (`brain/step1.wav`, `step2.wav`) never play.
/// C ref: `brain_footstep`.
pub fn brain_footstep(_world: &mut GameWorld, _key: EntityKey) {}

/// C ref: `brain_sight`.
pub fn brain_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `brain_search`.
pub fn brain_search(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SEARCH, ATTN_NORM);
}

/// C ref: `brain_stand`.
pub fn brain_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &BRAIN_MOVE_STAND);
}

/// C ref: `brain_idle`.
pub fn brain_idle(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_AUTO, SOUND_IDLE3, ATTN_IDLE);
    set_move(world, key, &BRAIN_MOVE_IDLE);
}

/// C ref: `brain_walk`.
pub fn brain_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &BRAIN_MOVE_WALK1);
}

/// Crouch 32 units. C ref: `brain_duck_down`.
pub fn brain_duck_down(world: &mut GameWorld, key: EntityKey) {
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
    ent.maxs.z -= 32.0;
    ent.game.takedamage = TakeDamage::Yes;
    world.link_entity(key);
}

/// Hold the crouch frame until `pausetime`. C ref: `brain_duck_hold`.
pub fn brain_duck_hold(world: &mut GameWorld, key: EntityKey) {
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

/// C ref: `brain_duck_up`.
pub fn brain_duck_up(world: &mut GameWorld, key: EntityKey) {
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

/// Duck one shot in four until half a second after it would land,
/// turning on an attacker if it had no enemy. C ref: `brain_dodge`.
pub fn brain_dodge(world: &mut GameWorld, key: EntityKey, attacker: EntityKey, eta: f32) {
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
    let time = world.level.time;
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.pausetime = time + eta + 0.5;
    }
    set_move(world, key, &BRAIN_MOVE_DUCK);
}

/// C ref: `brain_swing_right`.
pub fn brain_swing_right(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, SOUND_MELEE1, ATTN_NORM);
}

/// A claw for 15 to 19 from the `right` side; a sound only on a hit.
fn brain_hit(world: &mut GameWorld, key: EntityKey, right: f32) {
    let aim = Vec3f::new(ai::MELEE_DISTANCE, right, 8.0);
    let damage = 15 + (world.random_u32() % 5) as i32;
    if world.fire_hit(key, aim, damage, 40) {
        sound(world, key, CHAN_WEAPON, SOUND_MELEE3, ATTN_NORM);
    }
}

/// C ref: `brain_hit_right`.
pub fn brain_hit_right(world: &mut GameWorld, key: EntityKey) {
    let Some(maxs_x) = world.entities.get(key).map(|e| e.maxs.x) else {
        return;
    };
    brain_hit(world, key, maxs_x);
}

/// C ref: `brain_swing_left`.
pub fn brain_swing_left(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, SOUND_MELEE2, ATTN_NORM);
}

/// C ref: `brain_hit_left`.
pub fn brain_hit_left(world: &mut GameWorld, key: EntityKey) {
    let Some(mins_x) = world.entities.get(key).map(|e| e.mins.x) else {
        return;
    };
    brain_hit(world, key, mins_x);
}

/// Drop the power screen to open the chest. C ref: `brain_chest_open`.
pub fn brain_chest_open(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.spawnflags &= !SPAWNFLAG_GRABBED;
    }
    set_power_armor(world, key, POWER_ARMOR_NONE);
    sound(world, key, CHAN_BODY, SOUND_CHEST_OPEN, ATTN_NORM);
}

/// Tentacles for 10 to 14 that pull the enemy in (kick -600); a grab
/// above easy chains into the claws. C ref: `brain_tentacle_attack`.
pub fn brain_tentacle_attack(world: &mut GameWorld, key: EntityKey) {
    let aim = Vec3f::new(ai::MELEE_DISTANCE, 0.0, 8.0);
    let damage = 10 + (world.random_u32() % 5) as i32;
    if world.fire_hit(key, aim, damage, -600) && world.game.skill > SKILL_EASY {
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.spawnflags |= SPAWNFLAG_GRABBED;
        }
    }
    sound(world, key, CHAN_WEAPON, SOUND_TENTACLES_RETRACT, ATTN_NORM);
}

/// Raise the screen; after a grab, claw. C ref: `brain_chest_closed`.
pub fn brain_chest_closed(world: &mut GameWorld, key: EntityKey) {
    set_power_armor(world, key, POWER_ARMOR_SCREEN);
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.spawnflags & SPAWNFLAG_GRABBED != 0 {
        ent.game.spawnflags &= !SPAWNFLAG_GRABBED;
        set_move(world, key, &BRAIN_MOVE_ATTACK1);
    }
}

/// Claws or tentacles, evenly. C ref: `brain_melee`.
pub fn brain_melee(world: &mut GameWorld, key: EntityKey) {
    if world.random() <= 0.5 {
        set_move(world, key, &BRAIN_MOVE_ATTACK1);
    } else {
        set_move(world, key, &BRAIN_MOVE_ATTACK2);
    }
}

/// C ref: `brain_run`.
pub fn brain_run(world: &mut GameWorld, key: EntityKey) {
    set_power_armor(world, key, POWER_ARMOR_SCREEN);
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        set_move(world, key, &BRAIN_MOVE_STAND);
    } else {
        set_move(world, key, &BRAIN_MOVE_RUN);
    }
}

/// C ref: `brain_pain`.
pub fn brain_pain(
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
    if time < ent.game.pain_debounce_time {
        return;
    }
    ent.game.pain_debounce_time = time + 3.0;
    if world.game.skill == SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    let r = world.random();
    let (name, mv) = if r < 0.33 {
        (SOUND_PAIN1, &BRAIN_MOVE_PAIN1)
    } else if r < 0.66 {
        (SOUND_PAIN2, &BRAIN_MOVE_PAIN2)
    } else {
        (SOUND_PAIN1, &BRAIN_MOVE_PAIN3)
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
    set_move(world, key, mv);
}

/// The corpse box, then stop. C ref: `brain_dead`.
pub fn brain_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// Drop the screen and effects, then gib at `gib_health` or fall one of
/// two ways. C ref: `brain_die`.
pub fn brain_die(
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
    ent.state.effects = 0;
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.power_armor_type = POWER_ARMOR_NONE;
    }
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
    sound(world, key, CHAN_VOICE, SOUND_DEATH, ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    if world.random() <= 0.5 {
        set_move(world, key, &BRAIN_MOVE_DEATH1);
    } else {
        set_move(world, key, &BRAIN_MOVE_DEATH2);
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_brain`; `monster_start`
/// (run by `call_spawn`) removes it in deathmatch.
pub fn sp_monster_brain(
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
        .unwrap_or_else(|| "monster_brain".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = MINS;
    ent.maxs = MAXS;
    ent.game.health = HEALTH;
    ent.game.gib_health = -150;
    ent.game.mass = MASS;
    ent.pain = Some(brain_pain);
    ent.die = Some(brain_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(brain_stand),
        walk: Some(brain_walk),
        run: Some(brain_run),
        dodge: Some(brain_dodge),
        melee: Some(brain_melee),
        sight: Some(brain_sight),
        search: Some(brain_search),
        idle: Some(brain_idle),
        power_armor_type: POWER_ARMOR_SCREEN,
        power_armor_power: POWER_ARMOR_POWER,
        currentmove: Some(BRAIN_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_brain`: its sound precaches.
pub fn sp_monster_brain_finish(
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
    use crate::monster::test_support::{monster_arena, run, sounds};
    use crate::world::{test_world, test_world_recording, MockGameImport};

    type CFrame = (AnimFrameFn, f32, Option<ThinkFn>);
    type CMove = (&'static MonsterMove, i32, i32, Vec<CFrame>, Option<ThinkFn>);

    /// (move, firstframe, lastframe, [(aifunc, dist, thinkfunc)], endfunc) from C.
    #[rustfmt::skip]
    fn c_moves() -> Vec<CMove> {
        vec![
            (&BRAIN_MOVE_STAND, 162, 191, vec![
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
                (ai::ai_stand as AnimFrameFn, 0.0, None),
            ], None),
            (&BRAIN_MOVE_IDLE, 192, 221, vec![
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
                (ai::ai_stand as AnimFrameFn, 0.0, None),
            ], Some(brain_stand as ThinkFn)),
            (&BRAIN_MOVE_WALK1, 0, 10, vec![
                (ai::ai_walk as AnimFrameFn, 7.0, None),
                (ai::ai_walk as AnimFrameFn, 2.0, None),
                (ai::ai_walk as AnimFrameFn, 3.0, None),
                (ai::ai_walk as AnimFrameFn, 3.0, Some(brain_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 1.0, None),
                (ai::ai_walk as AnimFrameFn, 0.0, None),
                (ai::ai_walk as AnimFrameFn, 0.0, None),
                (ai::ai_walk as AnimFrameFn, 9.0, None),
                (ai::ai_walk as AnimFrameFn, -4.0, None),
                (ai::ai_walk as AnimFrameFn, -1.0, Some(brain_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 2.0, None),
            ], None),
            (&BRAIN_MOVE_DEFENSE, 154, 161, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], None),
            (&BRAIN_MOVE_PAIN3, 117, 122, vec![
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -4.0, None),
            ], Some(brain_run as ThinkFn)),
            (&BRAIN_MOVE_PAIN2, 109, 116, vec![
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, None),
            ], Some(brain_run as ThinkFn)),
            (&BRAIN_MOVE_PAIN1, 88, 108, vec![
                (ai::ai_move as AnimFrameFn, -6.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, -6.0, Some(brain_footstep as ThinkFn)),
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
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 7.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, Some(brain_footstep as ThinkFn)),
                (ai::ai_move as AnimFrameFn, -1.0, None),
            ], Some(brain_run as ThinkFn)),
            (&BRAIN_MOVE_DUCK, 146, 153, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, Some(brain_duck_down as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 17.0, Some(brain_duck_hold as ThinkFn)),
                (ai::ai_move as AnimFrameFn, -3.0, Some(brain_footstep as ThinkFn)),
                (ai::ai_move as AnimFrameFn, -1.0, Some(brain_duck_up as ThinkFn)),
                (ai::ai_move as AnimFrameFn, -5.0, None),
                (ai::ai_move as AnimFrameFn, -6.0, None),
                (ai::ai_move as AnimFrameFn, -6.0, Some(brain_footstep as ThinkFn)),
            ], Some(brain_run as ThinkFn)),
            (&BRAIN_MOVE_DEATH2, 141, 145, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 9.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(brain_dead as ThinkFn)),
            (&BRAIN_MOVE_DEATH1, 123, 140, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, 9.0, None),
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
            ], Some(brain_dead as ThinkFn)),
            (&BRAIN_MOVE_ATTACK1, 53, 70, vec![
                (ai::ai_charge as AnimFrameFn, 8.0, None),
                (ai::ai_charge as AnimFrameFn, 3.0, None),
                (ai::ai_charge as AnimFrameFn, 5.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(brain_footstep as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, -3.0, Some(brain_swing_right as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, -5.0, None),
                (ai::ai_charge as AnimFrameFn, -7.0, Some(brain_hit_right as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 6.0, Some(brain_swing_left as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 1.0, None),
                (ai::ai_charge as AnimFrameFn, 2.0, Some(brain_hit_left as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, -3.0, None),
                (ai::ai_charge as AnimFrameFn, 6.0, None),
                (ai::ai_charge as AnimFrameFn, -1.0, None),
                (ai::ai_charge as AnimFrameFn, -3.0, None),
                (ai::ai_charge as AnimFrameFn, 2.0, None),
                (ai::ai_charge as AnimFrameFn, -11.0, Some(brain_footstep as ThinkFn)),
            ], Some(brain_run as ThinkFn)),
            (&BRAIN_MOVE_ATTACK2, 71, 87, vec![
                (ai::ai_charge as AnimFrameFn, 5.0, None),
                (ai::ai_charge as AnimFrameFn, -4.0, None),
                (ai::ai_charge as AnimFrameFn, -4.0, None),
                (ai::ai_charge as AnimFrameFn, -3.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(brain_chest_open as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 13.0, Some(brain_tentacle_attack as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 2.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, -9.0, Some(brain_chest_closed as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 4.0, None),
                (ai::ai_charge as AnimFrameFn, 3.0, None),
                (ai::ai_charge as AnimFrameFn, 2.0, None),
                (ai::ai_charge as AnimFrameFn, -3.0, None),
                (ai::ai_charge as AnimFrameFn, -6.0, None),
            ], Some(brain_run as ThinkFn)),
            (&BRAIN_MOVE_RUN, 0, 10, vec![
                (ai::ai_run as AnimFrameFn, 9.0, None),
                (ai::ai_run as AnimFrameFn, 2.0, None),
                (ai::ai_run as AnimFrameFn, 3.0, None),
                (ai::ai_run as AnimFrameFn, 3.0, Some(brain_footstep as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 1.0, None),
                (ai::ai_run as AnimFrameFn, 0.0, None),
                (ai::ai_run as AnimFrameFn, 0.0, None),
                (ai::ai_run as AnimFrameFn, 10.0, None),
                (ai::ai_run as AnimFrameFn, -4.0, None),
                (ai::ai_run as AnimFrameFn, -1.0, Some(brain_footstep as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 2.0, None),
            ], None),
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
    fn brain_moves_match_c_frame_by_frame() {
        for (mv, first, last, frames, end) in c_moves() {
            assert_eq!((mv.firstframe, mv.lastframe), (first, last));
            assert_eq!(mv.frames.len(), frames.len(), "move at {first}");
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

    #[test]
    fn brain_defense_keeps_c_ninth_frame_that_the_eight_frame_range_never_plays() {
        let mv = &BRAIN_MOVE_DEFENSE;
        assert_eq!(mv.lastframe - mv.firstframe + 1, 8);
        assert_eq!(mv.frames.len(), 9);
    }

    fn spawned(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        sp_monster_brain(&mut world.entities, key, &HashMap::new());
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
        mi(world, key)
            .currentmove
            .as_ref()
            .is_some_and(|cm| std::ptr::eq(cm.frames, mv.frames))
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

    /// A monster enemy 40 units in front, and the next trace hitting it,
    /// as C's `fire_hit` trace does in a real map.
    fn enemy_in_reach(world: &mut GameWorld, gi: &MockGameImport, key: EntityKey) -> EntityKey {
        let enemy = world.spawn().unwrap();
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = Vec3f::new(40.0, 0.0, 0.0);
        e.mins = MINS;
        e.maxs = MAXS;
        e.game.health = 100;
        e.game.takedamage = TakeDamage::Yes;
        e.svflags |= SvFlags::MONSTER.bits();
        world.link_entity(enemy);
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ent_index: world.number_of(enemy),
            ..Trace::default()
        });
        enemy
    }

    #[test]
    fn brain_spawns_with_the_c_values_callbacks_and_power_screen() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "monster_brain");
        assert_eq!((ent.game.health, ent.game.mass), (300, 400));
        assert_eq!((ent.mins, ent.maxs), (MINS, MAXS));
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(std::ptr::fn_addr_eq(
            ent.pain.unwrap(),
            brain_pain as crate::entity::PainFn
        ));
        for (got, want) in [
            (mi.walk, brain_walk as ThinkFn),
            (mi.run, brain_run),
            (mi.melee, brain_melee),
        ] {
            assert!(std::ptr::fn_addr_eq(got.unwrap(), want));
        }
        assert_eq!(mi.scale, 1.0);
        assert_eq!((mi.power_armor_type, mi.power_armor_power), (1, 100));
        assert!(mi.attack.is_none());
        assert!(std::ptr::fn_addr_eq(
            mi.idle.unwrap(),
            brain_idle as ThinkFn
        ));
        assert!(std::ptr::fn_addr_eq(
            mi.dodge.unwrap(),
            brain_dodge as crate::entity::MonsterDodgeFn
        ));
        assert!(mi.sight.is_some() && mi.search.is_some());
        assert!(is_move(&world, key, &BRAIN_MOVE_STAND));
    }

    #[test]
    fn brain_finish_precaches_the_c_sounds_and_the_spawn_row_uses_it() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        let row = crate::spawn::find_spawn("monster_brain").unwrap();
        assert_eq!(row.2, Some("models/monsters/brain/tris.md2"));
        row.3.unwrap()(&mut world, key, &HashMap::new());
        assert_eq!(*gi.sounds.borrow(), PRECACHE);
    }

    #[test]
    fn brain_sight_search_idle_and_swings_play_their_sounds() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        brain_sight(&mut world, key, key);
        brain_search(&mut world, key);
        brain_idle(&mut world, key);
        assert!(is_move(&world, key, &BRAIN_MOVE_IDLE));
        brain_swing_right(&mut world, key);
        brain_swing_left(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [
                (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_SEARCH.to_string(), ATTN_NORM),
                (CHAN_AUTO, SOUND_IDLE3.to_string(), ATTN_IDLE),
                (CHAN_BODY, SOUND_MELEE1.to_string(), ATTN_NORM),
                (CHAN_BODY, SOUND_MELEE2.to_string(), ATTN_NORM),
            ]
        );
    }

    #[test]
    fn brain_run_raises_the_screen_and_obeys_stand_ground() {
        let mut world = test_world();
        let key = spawned(&mut world);
        brain_walk(&mut world, key);
        assert!(is_move(&world, key, &BRAIN_MOVE_WALK1));
        mi_mut(&mut world, key).power_armor_type = POWER_ARMOR_NONE;
        brain_run(&mut world, key);
        assert!(is_move(&world, key, &BRAIN_MOVE_RUN));
        assert_eq!(mi(&world, key).power_armor_type, POWER_ARMOR_SCREEN);
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        brain_run(&mut world, key);
        assert!(is_move(&world, key, &BRAIN_MOVE_STAND));
    }

    #[test]
    fn brain_melee_picks_claws_or_tentacles_evenly() {
        let mut world = test_world();
        let key = spawned(&mut world);
        seed_random(&mut world, |r| r <= 0.5);
        brain_melee(&mut world, key);
        assert!(is_move(&world, key, &BRAIN_MOVE_ATTACK1));
        seed_random(&mut world, |r| r > 0.5);
        brain_melee(&mut world, key);
        assert!(is_move(&world, key, &BRAIN_MOVE_ATTACK2));
    }

    #[test]
    fn brain_claws_hit_for_15_to_19_with_a_sound_only_on_a_hit() {
        for claw in [brain_hit_right as ThinkFn, brain_hit_left as ThinkFn] {
            for k in 0..5 {
                let (mut world, gi) = test_world_recording();
                let key = spawned(&mut world);
                let enemy = enemy_in_reach(&mut world, &gi, key);
                seed_randk(&mut world, |r| r % 5 == k);
                claw(&mut world, key);
                let health = world.entities.get(enemy).unwrap().game.health;
                assert_eq!(health, 100 - (15 + k as i32));
                assert_eq!(
                    sounds(&gi),
                    [(CHAN_WEAPON, SOUND_MELEE3.to_string(), ATTN_NORM)]
                );
            }
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            claw(&mut world, key);
            assert!(sounds(&gi).is_empty());
        }
    }

    #[test]
    fn brain_claws_knock_the_enemy_by_40_from_opposite_sides() {
        // Right claw at maxs[0], left at mins[0]: the push comes from the
        // brain's right (-y), then its left (+y).
        for (claw, push_y_positive) in [
            (brain_hit_right as ThinkFn, true),
            (brain_hit_left as ThinkFn, false),
        ] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            let enemy = enemy_in_reach(&mut world, &gi, key);
            claw(&mut world, key);
            let push = world.entities.get(enemy).unwrap().velocity;
            assert!((push.length() - 40.0).abs() < 0.01, "{push}");
            assert_eq!(push.y > 0.0, push_y_positive, "{push}");
        }
    }

    #[test]
    fn brain_chest_opens_without_its_screen() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.spawnflags = SPAWNFLAG_GRABBED | 1;
        brain_chest_open(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().game.spawnflags, 1);
        assert_eq!(mi(&world, key).power_armor_type, POWER_ARMOR_NONE);
        assert_eq!(
            sounds(&gi),
            [(CHAN_BODY, SOUND_CHEST_OPEN.to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn brain_tentacles_hit_for_10_to_14_and_grab_above_easy() {
        for (skill, grabs) in [(0, false), (1, true)] {
            let (mut world, gi) = test_world_recording();
            world.game.skill = skill;
            let key = spawned(&mut world);
            let enemy = enemy_in_reach(&mut world, &gi, key);
            seed_randk(&mut world, |r| r % 5 == 4);
            brain_tentacle_attack(&mut world, key);
            assert_eq!(world.entities.get(enemy).unwrap().game.health, 86);
            // Kick -600 pulls the enemy back toward the brain.
            let pull = world.entities.get(enemy).unwrap().velocity;
            assert!(
                (pull.length() - 600.0).abs() < 0.01 && pull.x < 0.0,
                "{pull}"
            );
            let grabbed = world.entities.get(key).unwrap().game.spawnflags & SPAWNFLAG_GRABBED;
            assert_eq!(grabbed != 0, grabs);
            assert_eq!(
                sounds(&gi),
                [(CHAN_WEAPON, SOUND_TENTACLES_RETRACT.to_string(), ATTN_NORM)]
            );
        }
    }

    #[test]
    fn brain_tentacles_that_miss_grab_nothing_but_still_retract() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        brain_tentacle_attack(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().game.spawnflags, 0);
        assert_eq!(sounds(&gi).len(), 1);
    }

    #[test]
    fn brain_chest_closes_with_its_screen_and_claws_after_a_grab() {
        let mut world = test_world();
        let key = spawned(&mut world);
        set_move(&mut world, key, &BRAIN_MOVE_ATTACK2);
        mi_mut(&mut world, key).power_armor_type = POWER_ARMOR_NONE;
        brain_chest_closed(&mut world, key);
        assert_eq!(mi(&world, key).power_armor_type, POWER_ARMOR_SCREEN);
        assert!(is_move(&world, key, &BRAIN_MOVE_ATTACK2));

        world.entities.get_mut(key).unwrap().game.spawnflags = SPAWNFLAG_GRABBED;
        brain_chest_closed(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().game.spawnflags, 0);
        assert!(is_move(&world, key, &BRAIN_MOVE_ATTACK1));
    }

    #[test]
    fn brain_ducks_32_units_once_and_holds_until_pausetime() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.level.time = 2.0;
        mi_mut(&mut world, key).pausetime = 2.5;
        brain_duck_down(&mut world, key);
        brain_duck_down(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 0.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert_eq!(mi(&world, key).pausetime, 2.5);
        brain_duck_hold(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        world.level.time = 2.5;
        brain_duck_hold(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        brain_duck_up(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 32.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Aim);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::DUCKED));
    }

    #[test]
    fn brain_dodges_one_shot_in_four_until_half_a_second_after_it_lands() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let attacker = world.spawn().unwrap();
        world.level.time = 2.0;
        seed_random(&mut world, |r| r > 0.25);
        brain_dodge(&mut world, key, attacker, 0.4);
        assert!(is_move(&world, key, &BRAIN_MOVE_STAND));
        assert!(world.entities.get(key).unwrap().game.enemy.is_none());

        seed_random(&mut world, |r| r <= 0.25);
        brain_dodge(&mut world, key, attacker, 0.4);
        assert!(is_move(&world, key, &BRAIN_MOVE_DUCK));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(attacker));
        assert!((mi(&world, key).pausetime - 2.9).abs() < 1e-5);
    }

    #[test]
    fn brain_pain_picks_by_roll_and_debounces_three_seconds() {
        for (rolls, name, mv) in [
            (0.0_f32..0.33, SOUND_PAIN1, &BRAIN_MOVE_PAIN1),
            (0.33..0.66, SOUND_PAIN2, &BRAIN_MOVE_PAIN2),
            (0.66..1.01, SOUND_PAIN1, &BRAIN_MOVE_PAIN3),
        ] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            world.level.time = 2.0;
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.max_health = 300;
            ent.game.health = 149;
            seed_random(&mut world, |r| rolls.contains(&r));
            brain_pain(&mut world, key, key, 0.0, 10);
            assert_eq!(sounds(&gi), [(CHAN_VOICE, name.to_string(), ATTN_NORM)]);
            assert!(is_move(&world, key, mv));
            let ent = world.entities.get(key).unwrap();
            assert_eq!((ent.state.skinnum, ent.game.pain_debounce_time), (1, 5.0));
            brain_pain(&mut world, key, key, 0.0, 10);
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn brain_pain_in_nightmare_is_silent_and_still() {
        let (mut world, gi) = test_world_recording();
        world.game.skill = 3;
        let key = spawned(&mut world);
        brain_pain(&mut world, key, key, 0.0, 50);
        assert!(sounds(&gi).is_empty());
        assert!(is_move(&world, key, &BRAIN_MOVE_STAND));
    }

    #[test]
    fn brain_at_its_c_gib_health_throws_six_gibs() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -150;
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
    fn brain_death_drops_screen_and_effects_and_falls_one_of_two_ways_once() {
        for (low, mv) in [(true, &BRAIN_MOVE_DEATH1), (false, &BRAIN_MOVE_DEATH2)] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.health = -149;
            ent.state.effects = 0x10;
            seed_random(&mut world, |r| (r <= 0.5) == low);
            brain_die(&mut world, key, key, key, 100, Vec3f::ZERO);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.state.effects, 0);
            assert_eq!(ent.game.deadflag, DeadFlag::Dead);
            assert_eq!(ent.game.takedamage, TakeDamage::Yes);
            assert_eq!(mi(&world, key).power_armor_type, POWER_ARMOR_NONE);
            assert!(is_move(&world, key, mv));
            assert_eq!(
                sounds(&gi),
                [(CHAN_VOICE, SOUND_DEATH.to_string(), ATTN_NORM)]
            );
            brain_die(&mut world, key, key, key, 10, Vec3f::ZERO);
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn brain_dead_shrinks_to_the_corpse_box_and_stops_thinking() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.nextthink = 9.0;
        brain_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(ent.game.nextthink, 0.0);
    }

    #[test]
    fn brain_sights_a_player_closes_in_and_attacks_it() {
        let (mut world, gi, key, player) =
            monster_arena(sp_monster_brain, sp_monster_brain_finish, 200.0);
        let (mut angry, mut attacked) = (false, false);
        run(&mut world, key, 150, |w| {
            angry |= w.entities.get(key).unwrap().game.enemy == Some(player);
            attacked |=
                is_move(w, key, &BRAIN_MOVE_ATTACK1) || is_move(w, key, &BRAIN_MOVE_ATTACK2);
        });
        assert!(angry && attacked, "angry {angry} attacked {attacked}");
        let played = sounds(&gi.floor.inner);
        assert_eq!(played[0], (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM));
        assert!(world.entities.get(player).unwrap().game.health < 100);
    }
}
