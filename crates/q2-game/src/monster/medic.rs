//! Medic: shoots a blaster, and hooks dead monsters with its cable to
//! bring them back.
//! C ref: `~/Qwasm2/src/game/monster/medic/medic.c`

use q2_shared::constants::{EF_BLASTER, EF_HYPERBLASTER, MZ2_MEDIC_BLASTER_1};
use q2_shared::protocol::SvcOp;
use q2_shared::types::*;
use std::collections::HashMap;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::monster::fire::monster_fire_blaster;
use crate::monster::flash::{monster_flash_source, project_source};
use crate::monster::{footstep, frame, set_move, sound, throw_organic_gibs, MONSTER_FOOTSTEPS};
use crate::world::GameWorld;

// C's sound_* names, one per file, shared by the call sites and
// medic_precache.
const SOUND_IDLE: &str = "medic/idle.wav";
const SOUND_PAIN1: &str = "medic/medpain1.wav";
const SOUND_PAIN2: &str = "medic/medpain2.wav";
const SOUND_DIE: &str = "medic/meddeth1.wav";
const SOUND_SIGHT: &str = "medic/medsght1.wav";
const SOUND_SEARCH: &str = "medic/medsrch1.wav";
const SOUND_HOOK_LAUNCH: &str = "medic/medatck2.wav";
const SOUND_HOOK_HIT: &str = "medic/medatck3.wav";
const SOUND_HOOK_HEAL: &str = "medic/medatck4.wav";
const SOUND_HOOK_RETRACT: &str = "medic/medatck5.wav";
/// C precaches it but plays it nowhere.
const SOUND_ATTACK1: &str = "medic/medatck1.wav";
pub(crate) const SOUND_STEPS: [&str; 2] = ["medic/step1.wav", "medic/step2.wav"];

/// C medic.h `MODEL_SCALE`.
const MODEL_SCALE: f32 = 1.0;
/// How far the medic looks for a patient (C `findradius(..., 1024)`).
const PATIENT_RADIUS: f32 = 1024.0;

// Frames the callbacks name besides the move bounds.
const FRAME_ATTACK9: i32 = 185;
const FRAME_ATTACK12: i32 = 188;
const FRAME_ATTACK19: i32 = 195;
const FRAME_ATTACK22: i32 = 198;
const FRAME_ATTACK25: i32 = 201;
const FRAME_ATTACK28: i32 = 204;
const FRAME_ATTACK42: i32 = 218;
const FRAME_ATTACK43: i32 = 219;
const FRAME_ATTACK44: i32 = 220;
const FRAME_ATTACK50: i32 = 226;

/// Where the cable leaves the medic on each frame from attack42.
/// C ref: `medic_cable_offsets`.
static MEDIC_CABLE_OFFSETS: [[f32; 3]; 10] = [
    [45.0, -9.2, 15.5],
    [48.4, -9.7, 15.2],
    [47.8, -9.8, 15.8],
    [47.3, -9.3, 14.3],
    [45.4, -10.1, 13.1],
    [41.9, -12.7, 12.0],
    [37.8, -15.8, 11.2],
    [34.3, -18.4, 10.7],
    [32.7, -19.7, 10.4],
    [32.7, -19.7, 10.4],
];

// Moves. C ref: medic.c `medic_frames_*` / `medic_move_*`.
const FRAME_WALK1: i32 = 0;
const FRAME_WALK12: i32 = 11;
const FRAME_WAIT1: i32 = 12;
const FRAME_WAIT90: i32 = 101;
const FRAME_RUN1: i32 = 102;
const FRAME_RUN6: i32 = 107;
const FRAME_PAINA1: i32 = 108;
const FRAME_PAINA8: i32 = 115;
const FRAME_PAINB1: i32 = 116;
const FRAME_PAINB15: i32 = 130;
const FRAME_DUCK1: i32 = 131;
const FRAME_DUCK16: i32 = 146;
const FRAME_DEATH1: i32 = 147;
const FRAME_DEATH30: i32 = 176;
const FRAME_ATTACK1: i32 = 177;
const FRAME_ATTACK14: i32 = 190;
const FRAME_ATTACK15: i32 = 191;
const FRAME_ATTACK30: i32 = 206;
const FRAME_ATTACK33: i32 = 209;
const FRAME_ATTACK60: i32 = 236;

static MEDIC_FRAMES_STAND: [MonsterFrame; 90] = [
    frame(ai::ai_stand, 0.0, Some(medic_idle)),
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
static MEDIC_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_WAIT1,
    lastframe: FRAME_WAIT90,
    frames: &MEDIC_FRAMES_STAND,
    endfunc: None,
};

static MEDIC_FRAMES_WALK: [MonsterFrame; 12] = [
    frame(ai::ai_walk, 6.2, None),
    frame(ai::ai_walk, 18.1, Some(medic_footstep)),
    frame(ai::ai_walk, 1.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, 10.0, None),
    frame(ai::ai_walk, 9.0, None),
    frame(ai::ai_walk, 11.0, None),
    frame(ai::ai_walk, 11.6, Some(medic_footstep)),
    frame(ai::ai_walk, 2.0, None),
    frame(ai::ai_walk, 9.9, None),
    frame(ai::ai_walk, 14.0, None),
    frame(ai::ai_walk, 9.3, None),
];
static MEDIC_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK1,
    lastframe: FRAME_WALK12,
    frames: &MEDIC_FRAMES_WALK,
    endfunc: None,
};

static MEDIC_FRAMES_RUN: [MonsterFrame; 6] = [
    frame(ai::ai_run, 18.0, Some(medic_footstep)),
    frame(ai::ai_run, 22.5, None),
    frame(ai::ai_run, 25.4, None),
    frame(ai::ai_run, 23.4, None),
    frame(ai::ai_run, 24.0, Some(medic_footstep)),
    frame(ai::ai_run, 35.6, None),
];
static MEDIC_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN1,
    lastframe: FRAME_RUN6,
    frames: &MEDIC_FRAMES_RUN,
    endfunc: None,
};

static MEDIC_FRAMES_PAIN1: [MonsterFrame; 8] = [frame(ai::ai_move, 0.0, None); 8];
static MEDIC_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAINA1,
    lastframe: FRAME_PAINA8,
    frames: &MEDIC_FRAMES_PAIN1,
    endfunc: Some(medic_run),
};

static MEDIC_FRAMES_PAIN2: [MonsterFrame; 15] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(medic_footstep)),
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
    frame(ai::ai_move, 0.0, Some(medic_footstep)),
];
static MEDIC_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAINB1,
    lastframe: FRAME_PAINB15,
    frames: &MEDIC_FRAMES_PAIN2,
    endfunc: Some(medic_run),
};

static MEDIC_FRAMES_DEATH: [MonsterFrame; 30] = [frame(ai::ai_move, 0.0, None); 30];
static MEDIC_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH1,
    lastframe: FRAME_DEATH30,
    frames: &MEDIC_FRAMES_DEATH,
    endfunc: Some(medic_dead),
};

static MEDIC_FRAMES_DUCK: [MonsterFrame; 16] = [
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, Some(medic_duck_down)),
    frame(ai::ai_move, -1.0, Some(medic_duck_hold)),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, Some(medic_duck_up)),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
];
static MEDIC_MOVE_DUCK: MonsterMove = MonsterMove {
    firstframe: FRAME_DUCK1,
    lastframe: FRAME_DUCK16,
    frames: &MEDIC_FRAMES_DUCK,
    endfunc: Some(medic_run),
};

static MEDIC_FRAMES_ATTACK_HYPER_BLASTER: [MonsterFrame; 16] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
];
static MEDIC_MOVE_ATTACK_HYPER_BLASTER: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK15,
    lastframe: FRAME_ATTACK30,
    frames: &MEDIC_FRAMES_ATTACK_HYPER_BLASTER,
    endfunc: Some(medic_run),
};

static MEDIC_FRAMES_ATTACK_BLASTER: [MonsterFrame; 14] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(medic_fire_blaster)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(medic_continue)),
];
static MEDIC_MOVE_ATTACK_BLASTER: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK1,
    lastframe: FRAME_ATTACK14,
    frames: &MEDIC_FRAMES_ATTACK_BLASTER,
    endfunc: Some(medic_run),
};

static MEDIC_FRAMES_ATTACK_CABLE: [MonsterFrame; 28] = [
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, 4.4, None),
    frame(ai::ai_charge, 4.7, None),
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, 6.0, None),
    frame(ai::ai_charge, 4.0, Some(medic_footstep)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_move, 0.0, Some(medic_hook_launch)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, 0.0, Some(medic_cable_attack)),
    frame(ai::ai_move, -15.0, Some(medic_hook_retract)),
    frame(ai::ai_move, -1.5, None),
    frame(ai::ai_move, -1.2, Some(medic_footstep)),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 0.3, None),
    frame(ai::ai_move, 0.7, None),
    frame(ai::ai_move, 1.2, None),
    frame(ai::ai_move, 1.3, None),
];
static MEDIC_MOVE_ATTACK_CABLE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK33,
    lastframe: FRAME_ATTACK60,
    frames: &MEDIC_FRAMES_ATTACK_CABLE,
    endfunc: Some(medic_run),
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mi_mut(world: &mut GameWorld, key: EntityKey) -> Option<&mut MonsterInfo> {
    world.entities.get_mut(key)?.monsterinfo.as_deref_mut()
}

fn aiflags(world: &GameWorld, key: EntityKey) -> AiFlags {
    world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .map_or(AiFlags::empty(), |mi| mi.aiflags)
}

fn enemy_of(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
    world
        .entities
        .get(key)?
        .game
        .enemy
        .filter(|&e| world.entities.get(e).is_some())
}

fn frame_of(world: &GameWorld, key: EntityKey) -> i32 {
    world.entities.get(key).map_or(0, |e| e.state.frame)
}

// ---------------------------------------------------------------------------
// Finding patients
// ---------------------------------------------------------------------------

/// The unclaimed, finished-dying, visible monster corpse within 1024
/// units with the most max health (the first found on a tie).
/// C ref: `medic_FindDeadMonster`.
pub fn medic_find_dead_monster(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
    let origin = world.entities.get(key)?.state.origin;
    let mut best: Option<(EntityKey, i32)> = None;
    let mut next = None;
    while let Some(other) = world.find_radius(next, origin, PATIENT_RADIUS) {
        next = Some(other);
        let Some(ent) = world.entities.get(other) else {
            continue;
        };
        let good_guy = ent
            .monsterinfo
            .as_ref()
            .is_some_and(|mi| mi.aiflags.contains(AiFlags::GOOD_GUY));
        if other == key
            || ent.svflags & SvFlags::MONSTER.bits() == 0
            || good_guy
            || ent.owner.is_some()
            || ent.game.health > 0
            || ent.game.nextthink != 0.0
            || !ai::visible(world, key, other)
        {
            continue;
        }
        if best.is_none_or(|(_, max)| ent.game.max_health > max) {
            best = Some((other, ent.game.max_health));
        }
    }
    best.map(|(k, _)| k)
}

/// Claim `patient` and go to it. `keep_enemy` saves the current enemy in
/// `oldenemy` first, as `medic_search` and `medic_run` do.
fn take_patient(world: &mut GameWorld, key: EntityKey, patient: EntityKey, keep_enemy: bool) {
    if let Some(ent) = world.entities.get_mut(key) {
        if keep_enemy {
            ent.game.oldenemy = ent.game.enemy;
        }
        ent.game.enemy = Some(patient);
    }
    if let Some(ent) = world.entities.get_mut(patient) {
        ent.owner = Some(key);
    }
    if let Some(mi) = mi_mut(world, key) {
        mi.aiflags |= AiFlags::MEDIC;
    }
    ai::found_target(world, key);
}

/// C ref: `medic_footstep`; silent unless `g_monsterfootsteps` is set.
pub fn medic_footstep(world: &mut GameWorld, key: EntityKey) {
    footstep(world, key, MONSTER_FOOTSTEPS, &SOUND_STEPS, 1.0);
}

/// C ref: `medic_idle`.
pub fn medic_idle(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
    if let Some(patient) = medic_find_dead_monster(world, key) {
        take_patient(world, key, patient, false);
    }
}

/// C ref: `medic_search`.
pub fn medic_search(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SEARCH, ATTN_IDLE);
    if world
        .entities
        .get(key)
        .is_some_and(|e| e.game.oldenemy.is_none())
    {
        if let Some(patient) = medic_find_dead_monster(world, key) {
            take_patient(world, key, patient, true);
        }
    }
}

/// C ref: `medic_sight`.
pub fn medic_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// C ref: `medic_stand`.
pub fn medic_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MEDIC_MOVE_STAND);
}

/// C ref: `medic_walk`.
pub fn medic_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MEDIC_MOVE_WALK);
}

/// Go to a patient if one is in sight, else run (or hold ground).
/// C ref: `medic_run`.
pub fn medic_run(world: &mut GameWorld, key: EntityKey) {
    if !aiflags(world, key).contains(AiFlags::MEDIC) {
        if let Some(patient) = medic_find_dead_monster(world, key) {
            take_patient(world, key, patient, true);
            return;
        }
    }
    let next = if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        &MEDIC_MOVE_STAND
    } else {
        &MEDIC_MOVE_RUN
    };
    set_move(world, key, next);
}

/// C ref: `medic_pain`.
pub fn medic_pain(
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
    if world.game.skill == crate::constants::SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    let (next, name) = if world.random() < 0.5 {
        (&MEDIC_MOVE_PAIN1, SOUND_PAIN1)
    } else {
        (&MEDIC_MOVE_PAIN2, SOUND_PAIN2)
    };
    set_move(world, key, next);
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
}

// ---------------------------------------------------------------------------
// Blaster
// ---------------------------------------------------------------------------

/// A 2 damage bolt at the enemy's eye, drawn as a blaster bolt on the
/// first attack's shots and a hyperblaster bolt on some of the second's.
/// C ref: `medic_fire_blaster`.
pub fn medic_fire_blaster(world: &mut GameWorld, key: EntityKey) {
    let effect = match frame_of(world, key) {
        FRAME_ATTACK9 | FRAME_ATTACK12 => EF_BLASTER,
        FRAME_ATTACK19 | FRAME_ATTACK22 | FRAME_ATTACK25 | FRAME_ATTACK28 => EF_HYPERBLASTER,
        _ => 0,
    };
    let Some((start, _, _)) = monster_flash_source(world, key, MZ2_MEDIC_BLASTER_1) else {
        return;
    };
    let Some(enemy) = enemy_of(world, key).and_then(|e| world.entities.get(e)) else {
        return;
    };
    let end = enemy.state.origin + Vec3f::new(0.0, 0.0, enemy.game.viewheight);
    monster_fire_blaster(
        world,
        key,
        start,
        end - start,
        2,
        1000,
        MZ2_MEDIC_BLASTER_1,
        effect,
    );
}

/// Go on to the hyperblaster burst 95 times in 100 while the enemy is in
/// sight. C ref: `medic_continue`.
pub fn medic_continue(world: &mut GameWorld, key: EntityKey) {
    let visible = enemy_of(world, key).is_some_and(|e| ai::visible(world, key, e));
    if visible && world.random() <= 0.95 {
        set_move(world, key, &MEDIC_MOVE_ATTACK_HYPER_BLASTER);
    }
}

// ---------------------------------------------------------------------------
// Death
// ---------------------------------------------------------------------------

/// The corpse box, then stop. C ref: `medic_dead`.
pub fn medic_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// Free the patient for another medic, then gib or fall over (once).
/// C ref: `medic_die`.
pub fn medic_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    damage: i32,
    _point: Vec3f,
) {
    if let Some(patient) = enemy_of(world, key) {
        if let Some(p) = world
            .entities
            .get_mut(patient)
            .filter(|p| p.owner == Some(key))
        {
            p.owner = None;
        }
    }
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
    set_move(world, key, &MEDIC_MOVE_DEATH);
}

// ---------------------------------------------------------------------------
// Ducking
// ---------------------------------------------------------------------------

/// C ref: `medic_duck_down`.
pub fn medic_duck_down(world: &mut GameWorld, key: EntityKey) {
    if aiflags(world, key).contains(AiFlags::DUCKED) {
        return;
    }
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.maxs.z -= 32.0;
    ent.game.takedamage = TakeDamage::Yes;
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.aiflags |= AiFlags::DUCKED;
        mi.pausetime = time + 1.0;
    }
    world.link_entity(key);
}

/// Hold the duck until `pausetime`. C ref: `medic_duck_hold`.
pub fn medic_duck_hold(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    if let Some(mi) = mi_mut(world, key) {
        let hold = time < mi.pausetime;
        mi.aiflags.set(AiFlags::HOLD_FRAME, hold);
    }
}

/// C ref: `medic_duck_up`.
pub fn medic_duck_up(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.maxs.z += 32.0;
    ent.game.takedamage = TakeDamage::Aim;
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.aiflags.remove(AiFlags::DUCKED);
    }
    world.link_entity(key);
}

/// Duck one shot in four, turning on the shooter if idle.
/// C ref: `medic_dodge`.
pub fn medic_dodge(world: &mut GameWorld, key: EntityKey, attacker: EntityKey, _eta: f32) {
    if world.random() > 0.25 {
        return;
    }
    if let Some(ent) = world
        .entities
        .get_mut(key)
        .filter(|e| e.game.enemy.is_none())
    {
        ent.game.enemy = Some(attacker);
        ai::found_target(world, key);
    }
    set_move(world, key, &MEDIC_MOVE_DUCK);
}

// ---------------------------------------------------------------------------
// Cable
// ---------------------------------------------------------------------------

/// C ref: `medic_hook_launch`.
pub fn medic_hook_launch(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_HOOK_LAUNCH, ATTN_NORM);
}

/// C `ED_CallSpawn(ent)` on a corpse. The `st.item` drop is not re-read,
/// as C's spawn temp is stale by then.
fn respawn(world: &mut GameWorld, key: EntityKey) {
    let Some(classname) = world.entities.get(key).map(|e| e.game.classname.clone()) else {
        return;
    };
    let fields = HashMap::from([("classname".to_string(), classname.clone())]);
    world.call_spawn(key, &classname, &fields);
}

/// Bring the patient back to life on attack50.
fn resurrect(world: &mut GameWorld, key: EntityKey, patient: EntityKey) {
    if let Some(p) = world.entities.get_mut(patient) {
        p.game.spawnflags = 0;
        p.game.target.clear();
        p.game.targetname.clear();
        p.game.combattarget.clear();
        p.game.deathtarget.clear();
        p.owner = Some(key);
        if let Some(mi) = p.monsterinfo.as_mut() {
            mi.aiflags = AiFlags::empty();
        }
    }
    respawn(world, patient);
    let time = world.level.time;
    let Some(p) = world.entities.get_mut(patient) else {
        return;
    };
    p.owner = None;
    if let Some(think) = p.think {
        p.game.nextthink = time;
        think(world, patient);
    }
    if let Some(mi) = mi_mut(world, patient) {
        mi.aiflags |= AiFlags::RESURRECTING;
    }
    let oldenemy = world
        .entities
        .get(key)
        .and_then(|e| e.game.oldenemy)
        .filter(|&o| world.entities.get(o).is_some_and(|o| o.client.is_some()));
    if let Some(oldenemy) = oldenemy {
        if let Some(p) = world.entities.get_mut(patient) {
            p.game.enemy = Some(oldenemy);
        }
        ai::found_target(world, patient);
    }
}

/// One frame of the healing cable: within 256 units and 45 degrees of
/// pitch, with nothing in the way, hook the patient on attack43, revive
/// it on attack50, and draw the cable. C ref: `medic_cable_attack`.
pub fn medic_cable_attack(world: &mut GameWorld, key: EntityKey) {
    let Some(patient) = enemy_of(world, key).filter(|&e| world.entities.is_live(e)) else {
        return;
    };
    let (Some(me), Some(p)) = (world.entities.get(key), world.entities.get(patient)) else {
        return;
    };
    let frame = me.state.frame;
    let Some(offset) = usize::try_from(frame - FRAME_ATTACK42)
        .ok()
        .and_then(|i| MEDIC_CABLE_OFFSETS.get(i))
    else {
        return;
    };
    let (f, r, _) = crate::physics::angle_vectors(me.state.angles);
    let start = project_source(me.state.origin, *offset, f, r);
    let patient_origin = p.state.origin;

    // Check for max distance and min/max pitch.
    let dir = start - patient_origin;
    if dir.length() > 256.0 {
        return;
    }
    let mut pitch = crate::weapons::vectoangles(dir).x;
    if pitch < -180.0 {
        pitch += 360.0;
    }
    if pitch.abs() > 45.0 {
        return;
    }
    let tr = world.gi.trace(
        start,
        Vec3f::ZERO,
        Vec3f::ZERO,
        patient_origin,
        world.number_of(key),
        MASK_SHOT,
    );
    if tr.fraction != 1.0 && tr.ent_index != world.number_of(patient) {
        return;
    }

    if frame == FRAME_ATTACK43 {
        sound(world, patient, CHAN_AUTO, SOUND_HOOK_HIT, ATTN_NORM);
        if let Some(mi) = mi_mut(world, patient) {
            mi.aiflags |= AiFlags::RESURRECTING;
        }
    } else if frame == FRAME_ATTACK50 {
        resurrect(world, key, patient);
    } else if frame == FRAME_ATTACK44 {
        sound(world, key, CHAN_WEAPON, SOUND_HOOK_HEAL, ATTN_NORM);
    }

    // The beam starts mid-segment and ends mid-body, as the patient is
    // (or was) lying dead.
    let start = start + f * 8.0;
    let Some(p) = world.entities.get(patient) else {
        return;
    };
    let end = Vec3f::new(
        p.state.origin.x,
        p.state.origin.y,
        p.absmin.z + p.size.z / 2.0,
    );
    let origin = world.entities.get(key).map_or(start, |e| e.state.origin);
    world.gi.write_byte(SvcOp::TempEntity as i32);
    world.gi.write_byte(TE_MEDIC_CABLE_ATTACK);
    world
        .gi
        .write_short(world.number_of(key).map_or(0, |n| n as i32));
    world.gi.write_position(start);
    world.gi.write_position(end);
    world.gi.multicast(origin, Multicast::PVS);
}

/// C ref: `medic_hook_retract`.
pub fn medic_hook_retract(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_HOOK_RETRACT, ATTN_NORM);
    if let Some(patient) = enemy_of(world, key) {
        if let Some(mi) = mi_mut(world, patient) {
            mi.aiflags.remove(AiFlags::RESURRECTING);
        }
    }
}

/// The cable for a patient, else the blaster. C ref: `medic_attack`.
pub fn medic_attack(world: &mut GameWorld, key: EntityKey) {
    let next = if aiflags(world, key).contains(AiFlags::MEDIC) {
        &MEDIC_MOVE_ATTACK_CABLE
    } else {
        &MEDIC_MOVE_ATTACK_BLASTER
    };
    set_move(world, key, next);
}

/// Always "attack" a patient; otherwise `M_CheckAttack`.
/// C ref: `medic_checkattack`.
pub fn medic_checkattack(world: &mut GameWorld, key: EntityKey) -> bool {
    if aiflags(world, key).contains(AiFlags::MEDIC) {
        medic_attack(world, key);
        return true;
    }
    ai::m_check_attack(world, key)
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// C ref: `SP_monster_medic`.
pub fn sp_monster_medic(
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
        .unwrap_or_else(|| "monster_medic".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-24.0, -24.0, -24.0);
    ent.maxs = Vec3f::new(24.0, 24.0, 32.0);
    ent.game.health = 300;
    ent.game.gib_health = -130;
    ent.game.max_health = 300;
    ent.game.mass = 400;
    ent.pain = Some(medic_pain);
    ent.die = Some(medic_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(medic_stand),
        walk: Some(medic_walk),
        run: Some(medic_run),
        dodge: Some(medic_dodge),
        attack: Some(medic_attack),
        sight: Some(medic_sight),
        idle: Some(medic_idle),
        search: Some(medic_search),
        checkattack: Some(medic_checkattack),
        currentmove: Some(MEDIC_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The sounds `SP_monster_medic` indexes.
pub fn medic_precache(world: &mut GameWorld, _key: EntityKey, _fields: &HashMap<String, String>) {
    for name in [
        SOUND_IDLE,
        SOUND_PAIN1,
        SOUND_PAIN2,
        SOUND_DIE,
        SOUND_SIGHT,
        SOUND_SEARCH,
        SOUND_HOOK_LAUNCH,
        SOUND_HOOK_HIT,
        SOUND_HOOK_HEAL,
        SOUND_HOOK_RETRACT,
        SOUND_ATTACK1,
    ] {
        world.gi.sound_index(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NAMES: &[(ThinkFn, &str)] = &[
        (medic_footstep, "medic_footstep"),
        (medic_idle, "medic_idle"),
        (medic_run, "medic_run"),
        (medic_dead, "medic_dead"),
        (medic_duck_down, "medic_duck_down"),
        (medic_duck_hold, "medic_duck_hold"),
        (medic_duck_up, "medic_duck_up"),
        (medic_fire_blaster, "medic_fire_blaster"),
        (medic_continue, "medic_continue"),
        (medic_hook_launch, "medic_hook_launch"),
        (medic_cable_attack, "medic_cable_attack"),
        (medic_hook_retract, "medic_hook_retract"),
    ];

    use crate::entity::ThinkFn;
    use crate::misc::wire::{Wire, WireImport};
    use crate::monster::test_support::*;
    use std::rc::Rc;

    fn medic() -> (GameWorld, Rc<WireImport>, EntityKey) {
        monster_world(sp_monster_medic)
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

    /// A finished-dying tank corpse at `at` with `max_health`, as
    /// `tank_die` and `tank_dead` leave it.
    fn corpse(world: &mut GameWorld, at: Vec3f, max_health: i32) -> EntityKey {
        let key = world.spawn().unwrap();
        crate::monster::tank::sp_monster_tank(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = at;
        ent.svflags = SvFlags::MONSTER.bits() | SvFlags::DEADMONSTER.bits();
        ent.game.health = -5;
        ent.game.max_health = max_health;
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.nextthink = 0.0;
        ent.mins = Vec3f::new(-16.0, -16.0, -16.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 0.0);
        world.link_entity(key);
        key
    }

    #[test]
    fn medic_moves_match_c_medic_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&MEDIC_MOVE_STAND, 12, 101, "-", "stand:0:medic_idle stand:0:-*89"),
            (&MEDIC_MOVE_WALK, 0, 11, "-", "walk:6.2:- walk:18.1:medic_footstep walk:1:- walk:9:- walk:10:- walk:9:- walk:11:- walk:11.6:medic_footstep walk:2:- walk:9.9:- walk:14:- walk:9.3:-"),
            (&MEDIC_MOVE_RUN, 102, 107, "-", "run:18:medic_footstep run:22.5:- run:25.4:- run:23.4:- run:24:medic_footstep run:35.6:-"),
            (&MEDIC_MOVE_PAIN1, 108, 115, "medic_run", "move:0:-*8"),
            (&MEDIC_MOVE_PAIN2, 116, 130, "medic_run", "move:0:-*3 move:0:medic_footstep move:0:-*10 move:0:medic_footstep"),
            (&MEDIC_MOVE_DEATH, 147, 176, "medic_dead", "move:0:-*30"),
            (&MEDIC_MOVE_DUCK, 131, 146, "medic_run", "move:-1:-*2 move:-1:medic_duck_down move:-1:medic_duck_hold move:-1:-*2 move:-1:medic_duck_up move:-1:-*9"),
            (&MEDIC_MOVE_ATTACK_HYPER_BLASTER, 191, 206, "medic_run", "charge:0:-*4 charge:0:medic_fire_blaster*12"),
            (&MEDIC_MOVE_ATTACK_BLASTER, 177, 190, "medic_run", "charge:0:- charge:5:-*2 charge:3:- charge:2:- charge:0:-*3 charge:0:medic_fire_blaster charge:0:-*2 charge:0:medic_fire_blaster charge:0:- charge:0:medic_continue"),
            (&MEDIC_MOVE_ATTACK_CABLE, 209, 236, "medic_run", "move:2:- move:3:- move:5:- move:4.4:- charge:4.7:- charge:5:- charge:6:- charge:4:medic_footstep charge:0:- move:0:medic_hook_launch move:0:medic_cable_attack*9 move:-15:medic_hook_retract move:-1.5:- move:-1.2:medic_footstep move:-3:- move:-2:- move:0.3:- move:0.7:- move:1.2:- move:1.3:-"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    #[test]
    fn medic_spawns_with_c_health_box_and_every_callback() {
        let (world, _, key) = medic();
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.health, ent.game.mass), (300, 400));
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-24.0, -24.0, -24.0),
                Vec3f::new(24.0, 24.0, 32.0)
            )
        );
        assert_eq!(
            (ent.game.movetype, ent.solid),
            (MoveType::Step, Solid::Bbox)
        );
        assert!(ent
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, ai::walkmonster_start_go as ThinkFn)));
        let mi = mi(&world, key);
        assert!(
            mi.dodge.is_some() && mi.idle.is_some() && mi.search.is_some() && mi.sight.is_some()
        );
        assert!(mi.checkattack.is_some_and(|f| std::ptr::fn_addr_eq(
            f,
            medic_checkattack as crate::entity::MonsterCheckAttackFn
        )));
        assert!(mi.melee.is_none());
        assert!(playing(&world, key, &MEDIC_MOVE_STAND));
    }

    #[test]
    fn medic_precache_indexes_the_eleven_c_sounds() {
        let (mut world, gi, key) = medic();
        medic_precache(&mut world, key, &HashMap::new());
        assert_eq!(gi.inner.sounds.borrow().len(), 11);
    }

    #[test]
    fn medic_find_dead_monster_takes_the_biggest_visible_unclaimed_corpse() {
        let (mut world, _, key) = medic();
        let small = corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        let big = corpse(&mut world, Vec3f::new(-100.0, 0.0, 0.0), 750);
        let tie = corpse(&mut world, Vec3f::new(0.0, 100.0, 0.0), 750);
        assert_eq!(medic_find_dead_monster(&world, key), Some(big));
        world.entities.get_mut(big).unwrap().owner = Some(tie);
        assert_eq!(medic_find_dead_monster(&world, key), Some(tie));
        world.entities.get_mut(tie).unwrap().game.nextthink = 2.0;
        assert_eq!(medic_find_dead_monster(&world, key), Some(small));
    }

    #[test]
    fn medic_find_dead_monster_skips_every_c_case() {
        type Spoil = fn(&mut crate::entity::Entity);
        let spoils: [(&str, Spoil); 7] = [
            ("alive", |e| e.game.health = 1),
            ("not a monster", |e| e.svflags = 0),
            ("good guy", |e| {
                e.monsterinfo.as_mut().unwrap().aiflags = AiFlags::GOOD_GUY
            }),
            ("claimed", |e| {
                e.owner = Some(crate::entity::EntityKey::default())
            }),
            ("still dying", |e| e.game.nextthink = 1.1),
            ("not solid", |e| e.solid = Solid::Not),
            ("too far", |e| e.state.origin.x = 1100.0),
        ];
        for (why, spoil) in spoils {
            let (mut world, _, key) = medic();
            let body = corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
            spoil(world.entities.get_mut(body).unwrap());
            assert_eq!(medic_find_dead_monster(&world, key), None, "{why}");
        }
        let (mut world, gi, key) = medic();
        corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        *gi.inner.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        assert_eq!(medic_find_dead_monster(&world, key), None, "hidden");
        // The medic never treats itself.
        let (mut world, _, key) = medic();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = -5;
        ent.svflags = SvFlags::MONSTER.bits();
        assert_eq!(medic_find_dead_monster(&world, key), None, "self");
    }

    #[test]
    fn medic_idle_mutters_and_claims_a_patient_in_sight() {
        let (mut world, gi, key) = medic();
        medic_idle(&mut world, key);
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "medic/idle.wav".to_string(), ATTN_IDLE)]
        );
        assert!(world.entities.get(key).unwrap().game.enemy.is_none());
        let body = corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        medic_idle(&mut world, key);
        let me = world.entities.get(key).unwrap();
        assert_eq!((me.game.enemy, me.game.oldenemy), (Some(body), None));
        assert_eq!(world.entities.get(body).unwrap().owner, Some(key));
        assert!(mi(&world, key).aiflags.contains(AiFlags::MEDIC));
        // FoundTarget hunts the patient: medic_run, which with a patient runs.
        assert!(playing(&world, key, &MEDIC_MOVE_RUN));
    }

    #[test]
    fn medic_search_keeps_its_enemy_as_oldenemy_unless_it_already_has_one() {
        let (mut world, gi, key) = medic();
        let player = give_enemy(&mut world, key, Vec3f::new(500.0, 0.0, 0.0));
        let body = corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        medic_search(&mut world, key);
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "medic/medsrch1.wav".to_string(), ATTN_IDLE)]
        );
        let me = world.entities.get(key).unwrap();
        assert_eq!(
            (me.game.enemy, me.game.oldenemy),
            (Some(body), Some(player))
        );

        let (mut world, _, key) = medic();
        let player = give_enemy(&mut world, key, Vec3f::new(500.0, 0.0, 0.0));
        world.entities.get_mut(key).unwrap().game.oldenemy = Some(player);
        corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        medic_search(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
    }

    #[test]
    fn medic_run_goes_for_a_patient_before_running_or_holding_ground() {
        let (mut world, _, key) = medic();
        medic_run(&mut world, key);
        assert!(playing(&world, key, &MEDIC_MOVE_RUN));
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::STAND_GROUND;
        medic_run(&mut world, key);
        assert!(playing(&world, key, &MEDIC_MOVE_STAND));

        let (mut world, _, key) = medic();
        let player = give_enemy(&mut world, key, Vec3f::new(500.0, 0.0, 0.0));
        let body = corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        medic_run(&mut world, key);
        let me = world.entities.get(key).unwrap();
        assert_eq!(
            (me.game.enemy, me.game.oldenemy),
            (Some(body), Some(player))
        );
        // Already on a patient: no new search.
        let other = corpse(&mut world, Vec3f::new(0.0, 100.0, 0.0), 900);
        medic_run(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(body));
        assert!(world.entities.get(other).unwrap().owner.is_none());
    }

    #[test]
    fn medic_walk_and_stand_pick_their_moves() {
        let (mut world, _, key) = medic();
        medic_walk(&mut world, key);
        assert!(playing(&world, key, &MEDIC_MOVE_WALK));
        medic_stand(&mut world, key);
        assert!(playing(&world, key, &MEDIC_MOVE_STAND));
    }

    #[test]
    fn medic_pain_flinches_either_way_half_the_time_and_debounces() {
        for (roll, mv, name) in [
            (0.4, &MEDIC_MOVE_PAIN1, "medic/medpain1.wav"),
            (0.6, &MEDIC_MOVE_PAIN2, "medic/medpain2.wav"),
        ] {
            let (mut world, gi, key) = medic();
            seed_random(&mut world, |r| (r - roll).abs() < 0.01);
            medic_pain(&mut world, key, key, 0.0, 5);
            assert!(playing(&world, key, mv));
            assert_eq!(
                wire_sounds(&gi),
                [(CHAN_VOICE, name.to_string(), ATTN_NORM)]
            );
            medic_pain(&mut world, key, key, 0.0, 5);
            assert_eq!(gi.sounds.borrow().len(), 1, "debounced");
        }
    }

    #[test]
    fn medic_pain_skin_and_nightmare() {
        let (mut world, gi, key) = medic();
        world.game.skill = crate::constants::SKILL_HARDPLUS;
        world.entities.get_mut(key).unwrap().game.health = 149;
        medic_pain(&mut world, key, key, 0.0, 5);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.state.skinnum, ent.game.pain_debounce_time), (1, 4.0));
        assert!(playing(&world, key, &MEDIC_MOVE_STAND));
        assert!(gi.sounds.borrow().is_empty());
    }

    #[test]
    fn medic_fire_blaster_marks_bolts_by_frame() {
        for (frame, effect) in [
            (FRAME_ATTACK9, EF_BLASTER),
            (FRAME_ATTACK12, EF_BLASTER),
            (FRAME_ATTACK19, EF_HYPERBLASTER),
            (FRAME_ATTACK28, EF_HYPERBLASTER),
            (FRAME_ATTACK19 + 1, 0),
        ] {
            let (mut world, gi, key) = medic();
            give_enemy(&mut world, key, Vec3f::new(400.0, 0.0, 0.0));
            set_frame(&mut world, key, frame);
            medic_fire_blaster(&mut world, key);
            let bolt = world.entities.get(all_of(&world, "bolt")[0]).unwrap();
            assert_eq!(
                bolt.state.effects & (EF_BLASTER | EF_HYPERBLASTER),
                effect,
                "{frame}"
            );
            assert_eq!(bolt.game.dmg, 2);
            let (start, _, _) = monster_flash_source(&world, key, MZ2_MEDIC_BLASTER_1).unwrap();
            let want = (Vec3f::new(400.0, 0.0, 22.0) - start).normalize() * 1000.0;
            assert!((bolt.velocity - want).length() < 0.01);
            assert_eq!(muzzle_flashes(&gi), [MZ2_MEDIC_BLASTER_1]);
        }
    }

    #[test]
    fn medic_continue_goes_on_to_the_hyperblaster_95_times_in_100_in_sight() {
        let (mut world, gi, key) = medic();
        give_enemy(&mut world, key, Vec3f::new(400.0, 0.0, 0.0));
        let mut burst = 0;
        for _ in 0..1000 {
            medic_attack(&mut world, key);
            medic_continue(&mut world, key);
            if playing(&world, key, &MEDIC_MOVE_ATTACK_HYPER_BLASTER) {
                burst += 1;
            } else {
                assert!(playing(&world, key, &MEDIC_MOVE_ATTACK_BLASTER));
            }
        }
        assert!((930..975).contains(&burst), "{burst}");
        medic_attack(&mut world, key);
        *gi.inner.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        seed_random(&mut world, |r| r < 0.1);
        medic_continue(&mut world, key);
        assert!(playing(&world, key, &MEDIC_MOVE_ATTACK_BLASTER));
    }

    #[test]
    fn medic_die_frees_its_patient_for_another_medic() {
        let (mut world, _, key) = medic();
        let body = corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        world.entities.get_mut(key).unwrap().game.enemy = Some(body);
        world.entities.get_mut(body).unwrap().owner = Some(key);
        medic_die(&mut world, key, key, key, 10, Vec3f::ZERO);
        assert!(world.entities.get(body).unwrap().owner.is_none());
        // Someone else's patient stays claimed.
        let (mut world, _, key) = medic();
        let body = corpse(&mut world, Vec3f::new(100.0, 0.0, 0.0), 100);
        world.entities.get_mut(key).unwrap().game.enemy = Some(body);
        world.entities.get_mut(body).unwrap().owner = Some(body);
        medic_die(&mut world, key, key, key, 10, Vec3f::ZERO);
        assert_eq!(world.entities.get(body).unwrap().owner, Some(body));
    }

    #[test]
    fn medic_die_gibs_at_or_below_minus_130() {
        let (mut world, gi, key) = medic();
        world.entities.get_mut(key).unwrap().game.health = -130;
        medic_die(&mut world, key, key, key, 200, Vec3f::ZERO);
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "misc/udeath.wav".to_string(), ATTN_NORM)]
        );
        let models = gi.inner.models.borrow().clone();
        for model in ["bone", "sm_meat", "head2"] {
            assert!(models.iter().any(|m| m.contains(model)), "{model}");
        }
        assert_eq!(
            world.entities.get(key).unwrap().game.deadflag,
            DeadFlag::Dead
        );
    }

    #[test]
    fn medic_die_above_gib_health_falls_over_once() {
        let (mut world, gi, key) = medic();
        world.entities.get_mut(key).unwrap().game.health = -129;
        medic_die(&mut world, key, key, key, 200, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage),
            (DeadFlag::Dead, TakeDamage::Yes)
        );
        assert!(playing(&world, key, &MEDIC_MOVE_DEATH));
        medic_die(&mut world, key, key, key, 200, Vec3f::ZERO);
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "medic/meddeth1.wav".to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn medic_dead_sets_the_c_corpse_box() {
        let (mut world, _, key) = medic();
        medic_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-16.0, -16.0, -24.0),
                Vec3f::new(16.0, 16.0, -8.0)
            )
        );
        assert_eq!(ent.game.movetype, MoveType::Toss);
    }

    #[test]
    fn medic_duck_lowers_the_box_for_a_second_then_stands_up() {
        let (mut world, _, key) = medic();
        medic_duck_down(&mut world, key);
        medic_duck_down(&mut world, key); // once only
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.maxs.z, ent.game.takedamage), (0.0, TakeDamage::Yes));
        assert!(mi(&world, key).aiflags.contains(AiFlags::DUCKED));
        assert_eq!(mi(&world, key).pausetime, 2.0);
        medic_duck_hold(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        world.level.time = 2.0;
        medic_duck_hold(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        medic_duck_up(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.maxs.z, ent.game.takedamage), (32.0, TakeDamage::Aim));
        assert!(!mi(&world, key).aiflags.contains(AiFlags::DUCKED));
    }

    #[test]
    fn medic_dodge_ducks_one_shot_in_four_and_turns_on_an_unseen_shooter() {
        let (mut world, _, key) = medic();
        let shooter = world.spawn().unwrap();
        seed_random(&mut world, |r| r > 0.25);
        medic_dodge(&mut world, key, shooter, 0.5);
        assert!(playing(&world, key, &MEDIC_MOVE_STAND));
        seed_random(&mut world, |r| r <= 0.25);
        medic_dodge(&mut world, key, shooter, 0.5);
        assert!(playing(&world, key, &MEDIC_MOVE_DUCK));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(shooter));

        let (mut world, _, key) = medic();
        let enemy = give_enemy(&mut world, key, Vec3f::new(300.0, 0.0, 0.0));
        let shooter = world.spawn().unwrap();
        seed_random(&mut world, |r| r <= 0.25);
        medic_dodge(&mut world, key, shooter, 0.5);
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(enemy));
    }

    #[test]
    fn medic_attack_and_checkattack_hook_a_patient_and_blast_anything_else() {
        let (mut world, _, key) = medic();
        medic_attack(&mut world, key);
        assert!(playing(&world, key, &MEDIC_MOVE_ATTACK_BLASTER));
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::MEDIC;
        assert!(medic_checkattack(&mut world, key));
        assert!(playing(&world, key, &MEDIC_MOVE_ATTACK_CABLE));
        // Without a patient: M_CheckAttack, which needs an enemy.
        let (mut world, _, key) = medic();
        assert!(!medic_checkattack(&mut world, key));
    }

    #[test]
    fn medic_hook_sounds_and_retract_clears_resurrecting() {
        let (mut world, gi, key) = medic();
        let body = corpse(&mut world, Vec3f::new(60.0, 0.0, 0.0), 100);
        world.entities.get_mut(key).unwrap().game.enemy = Some(body);
        world
            .entities
            .get_mut(body)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::RESURRECTING;
        let other = world.spawn().unwrap();
        medic_sight(&mut world, key, other);
        medic_hook_launch(&mut world, key);
        medic_hook_retract(&mut world, key);
        assert!(!mi(&world, body).aiflags.contains(AiFlags::RESURRECTING));
        let names: Vec<(i32, String)> = wire_sounds(&gi)
            .into_iter()
            .map(|(c, n, _)| (c, n))
            .collect();
        assert_eq!(
            names,
            [
                (CHAN_VOICE, "medic/medsght1.wav".to_string()),
                (CHAN_WEAPON, "medic/medatck2.wav".to_string()),
                (CHAN_WEAPON, "medic/medatck5.wav".to_string()),
            ]
        );
    }

    /// The cable temp entities on the wire: (medic number, start, end).
    fn cables(gi: &WireImport) -> Vec<(i32, Vec3f, Vec3f)> {
        let wire = gi.wire.borrow();
        wire.windows(5)
            .filter_map(|w| match w {
                [Wire::Byte(op), Wire::Byte(TE_MEDIC_CABLE_ATTACK), Wire::Short(n), Wire::Position(s), Wire::Position(e)]
                    if *op == SvcOp::TempEntity as i32 =>
                {
                    Some((*n, *s, *e))
                }
                _ => None,
            })
            .collect()
    }

    /// A medic at the origin on `frame` with a corpse at `at` as patient.
    fn cabling(at: Vec3f, frame: i32) -> (GameWorld, Rc<WireImport>, EntityKey, EntityKey) {
        let (mut world, gi, key) = medic();
        let body = corpse(&mut world, at, 100);
        world.entities.get_mut(key).unwrap().game.enemy = Some(body);
        set_frame(&mut world, key, frame);
        (world, gi, key, body)
    }

    #[test]
    fn medic_cable_attack_draws_the_cable_from_mid_segment_to_mid_body() {
        let (mut world, gi, key, body) = cabling(Vec3f::new(100.0, 0.0, 0.0), FRAME_ATTACK42 + 2);
        medic_cable_attack(&mut world, key);
        let number = world.number_of(key).unwrap() as i32;
        // Yaw 0: right is -y, so the offset's -9.8 right is +9.8 y.
        let start = Vec3f::new(47.8 + 8.0, 9.8, 15.8);
        let b = world.entities.get(body).unwrap();
        let end = Vec3f::new(100.0, 0.0, b.absmin.z + b.size.z / 2.0);
        let drawn = cables(&gi);
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].0, number);
        assert!((drawn[0].1 - start).length() < 1e-4, "{:?}", drawn[0].1);
        assert_eq!(drawn[0].2, end);
        assert_eq!(gi.inner.trace_masks.borrow().as_slice(), [MASK_SHOT]);
    }

    #[test]
    fn medic_cable_attack_needs_the_patient_close_level_and_in_the_clear() {
        // Over 256 units from the cable start.
        let (mut world, gi, key, _) = cabling(Vec3f::new(320.0, 0.0, 0.0), FRAME_ATTACK43);
        medic_cable_attack(&mut world, key);
        assert!(cables(&gi).is_empty(), "far");
        // Patient far above: the cable would pitch up over 45 degrees.
        let (mut world, gi, key, _) = cabling(Vec3f::new(50.0, 0.0, 150.0), FRAME_ATTACK43);
        medic_cable_attack(&mut world, key);
        assert!(cables(&gi).is_empty(), "steep");
        // Something else in the way.
        let (mut world, gi, key, _) = cabling(Vec3f::new(100.0, 0.0, 0.0), FRAME_ATTACK43);
        *gi.inner.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        medic_cable_attack(&mut world, key);
        assert!(cables(&gi).is_empty(), "blocked");
        assert!(
            !mi(&world, world.entities.get(key).unwrap().game.enemy.unwrap())
                .aiflags
                .contains(AiFlags::RESURRECTING)
        );
        // A freed patient.
        let (mut world, gi, key, body) = cabling(Vec3f::new(100.0, 0.0, 0.0), FRAME_ATTACK43);
        world.free_entity(body);
        medic_cable_attack(&mut world, key);
        assert!(cables(&gi).is_empty(), "freed");
    }

    #[test]
    fn medic_cable_attack_hooks_on_attack43_and_heals_on_attack44() {
        let (mut world, gi, key, body) = cabling(Vec3f::new(100.0, 0.0, 0.0), FRAME_ATTACK43);
        medic_cable_attack(&mut world, key);
        assert!(mi(&world, body).aiflags.contains(AiFlags::RESURRECTING));
        set_frame(&mut world, key, FRAME_ATTACK44);
        medic_cable_attack(&mut world, key);
        let played = wire_sounds(&gi);
        assert_eq!(
            played,
            [
                (CHAN_AUTO, "medic/medatck3.wav".to_string(), ATTN_NORM),
                (CHAN_WEAPON, "medic/medatck4.wav".to_string(), ATTN_NORM),
            ]
        );
        let from: Vec<Option<usize>> = gi.sounds.borrow().iter().map(|s| s.0).collect();
        assert_eq!(from, [world.number_of(body), world.number_of(key)]);
        assert_eq!(cables(&gi).len(), 2);
    }

    #[test]
    fn medic_cable_attack_on_attack50_brings_the_patient_back_against_the_player() {
        let (mut world, _, key, body) = cabling(Vec3f::new(100.0, 0.0, 0.0), FRAME_ATTACK50);
        let player = player_at(&mut world, Vec3f::new(600.0, 0.0, 24.0));
        world.entities.get_mut(key).unwrap().game.oldenemy = Some(player);
        {
            let b = world.entities.get_mut(body).unwrap();
            b.game.spawnflags = 1;
            b.game.target = "t1".into();
            b.game.targetname = "corpse".into();
            b.game.combattarget = "c1".into();
            b.game.deathtarget = "d1".into();
            b.owner = Some(key);
        }
        let monsters = world.level.total_monsters;
        medic_cable_attack(&mut world, key);
        let b = world.entities.get(body).unwrap();
        assert_eq!(
            (b.game.health, b.game.deadflag, b.solid),
            (750, DeadFlag::No, Solid::Bbox)
        );
        assert_eq!(b.svflags & SvFlags::DEADMONSTER.bits(), 0);
        assert_eq!(
            (b.mins, b.maxs),
            (
                Vec3f::new(-32.0, -32.0, -16.0),
                Vec3f::new(32.0, 32.0, 72.0)
            )
        );
        assert_eq!(b.game.spawnflags, 0);
        assert!(b.game.target.is_empty() && b.game.targetname.is_empty());
        assert!(b.game.combattarget.is_empty() && b.game.deathtarget.is_empty());
        assert!(b.owner.is_none());
        assert_eq!(b.game.enemy, Some(player));
        // walkmonster_start_go ran at once: it is a thinking monster again.
        assert!(b
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, ai::monster_think as ThinkFn)));
        assert!(mi(&world, body).aiflags.contains(AiFlags::RESURRECTING));
        // C's monster_start counts it again.
        assert_eq!(world.level.total_monsters, monsters + 1);
    }

    #[test]
    fn medic_resurrection_without_a_player_oldenemy_leaves_the_patient_calm() {
        let (mut world, _, key, body) = cabling(Vec3f::new(100.0, 0.0, 0.0), FRAME_ATTACK50);
        medic_cable_attack(&mut world, key);
        assert!(world.entities.get(body).unwrap().game.enemy.is_none());
        assert_eq!(world.entities.get(body).unwrap().game.health, 750);
    }

    #[test]
    fn medic_sights_a_player_and_shoots_its_blaster() {
        let (mut world, gi, key) = medic();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 24.0));
        let at = world.entities.get(key).unwrap().state.origin;
        clear_shots_at(&gi, &world, player, at, 100_000);
        let fired = run_monster(&mut world, key, 600, |w| !all_of(w, "bolt").is_empty());
        assert!(fired, "no bolt");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(wire_sounds(&gi)
            .iter()
            .any(|(_, name, _)| name == "medic/medsght1.wav"));
    }

    #[test]
    fn medic_finds_a_dead_tank_and_brings_it_back() {
        let (mut world, _, key) = medic();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        let body = corpse(&mut world, Vec3f::new(90.0, 0.0, 16.0), 750);
        let revived = run_monster(&mut world, key, 600, |w| {
            w.entities.get(body).unwrap().game.health > 0
        });
        assert!(revived, "the tank stayed dead");
        assert_eq!(world.entities.get(body).unwrap().game.health, 750);
    }
}
