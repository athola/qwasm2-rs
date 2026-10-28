//! Parasite: walks, and drains the enemy through its proboscis from up to
//! 256 units away. C ref: `~/Qwasm2/src/game/monster/parasite/parasite.c`.

use q2_shared::protocol::SvcOp;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::flash::project_source;
use crate::world::GameWorld;
use std::collections::HashMap;

const SOUND_PAIN1: &str = "parasite/parpain1.wav";
const SOUND_PAIN2: &str = "parasite/parpain2.wav";
const SOUND_DIE: &str = "parasite/pardeth1.wav";
const SOUND_LAUNCH: &str = "parasite/paratck1.wav";
const SOUND_IMPACT: &str = "parasite/paratck2.wav";
const SOUND_SUCK: &str = "parasite/paratck3.wav";
const SOUND_REELIN: &str = "parasite/paratck4.wav";
const SOUND_SIGHT: &str = "parasite/parsght1.wav";
const SOUND_TAP: &str = "parasite/paridle1.wav";
const SOUND_SCRATCH: &str = "parasite/paridle2.wav";
const SOUND_SEARCH: &str = "parasite/parsrch1.wav";
/// Every sound C `SP_monster_parasite` indexes.
const SOUNDS: [&str; 11] = [
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_DIE,
    SOUND_LAUNCH,
    SOUND_IMPACT,
    SOUND_SUCK,
    SOUND_REELIN,
    SOUND_SIGHT,
    SOUND_TAP,
    SOUND_SCRATCH,
    SOUND_SEARCH,
];

/// C `g_monsterfootsteps` (default "0"). The cvar is not ported, so
/// footsteps stay at C's default: silent.
const MONSTER_FOOTSTEPS: bool = false;

/// C `MODEL_SCALE` (parasite.h).
const MODEL_SCALE: f32 = 1.0;

const fn frame(aifunc: AnimFrameFn, dist: f32, thinkfunc: Option<ThinkFn>) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

// ---------------------------------------------------------------------------
// Moves. C ref: the `mframe_t`/`mmove_t` tables, frame by frame.
// ---------------------------------------------------------------------------

// C ref: parasite.h (`FRAME_*`, model frame numbers).

const FRAME_BREAK01: i32 = 0;
const FRAME_BREAK32: i32 = 31;
const FRAME_DEATH101: i32 = 32;
const FRAME_DEATH107: i32 = 38;
const FRAME_DRAIN01: i32 = 39;
const FRAME_DRAIN03: i32 = 41;
const FRAME_DRAIN04: i32 = 42;
const FRAME_DRAIN18: i32 = 56;
const FRAME_PAIN101: i32 = 57;
const FRAME_PAIN111: i32 = 67;
const FRAME_RUN01: i32 = 68;
const FRAME_RUN02: i32 = 69;
const FRAME_RUN03: i32 = 70;
const FRAME_RUN09: i32 = 76;
const FRAME_RUN10: i32 = 77;
const FRAME_RUN15: i32 = 82;
const FRAME_STAND01: i32 = 83;
const FRAME_STAND17: i32 = 99;
const FRAME_STAND18: i32 = 100;
const FRAME_STAND21: i32 = 103;
const FRAME_STAND22: i32 = 104;
const FRAME_STAND27: i32 = 109;
const FRAME_STAND28: i32 = 110;
const FRAME_STAND35: i32 = 117;

static PARASITE_FRAMES_START_FIDGET: [MonsterFrame; 4] = [frame(ai::ai_stand, 0.0, None); 4];

pub static PARASITE_MOVE_START_FIDGET: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND18,
    lastframe: FRAME_STAND21,
    frames: &PARASITE_FRAMES_START_FIDGET,
    endfunc: Some(parasite_do_fidget),
};

static PARASITE_FRAMES_FIDGET: [MonsterFrame; 6] = [
    frame(ai::ai_stand, 0.0, Some(parasite_scratch)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(parasite_scratch)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
];

pub static PARASITE_MOVE_FIDGET: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND22,
    lastframe: FRAME_STAND27,
    frames: &PARASITE_FRAMES_FIDGET,
    endfunc: Some(parasite_refidget),
};

static PARASITE_FRAMES_END_FIDGET: [MonsterFrame; 8] = [
    frame(ai::ai_stand, 0.0, Some(parasite_scratch)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
];

pub static PARASITE_MOVE_END_FIDGET: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND28,
    lastframe: FRAME_STAND35,
    frames: &PARASITE_FRAMES_END_FIDGET,
    endfunc: Some(parasite_stand),
};

static PARASITE_FRAMES_STAND: [MonsterFrame; 17] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(parasite_tap)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(parasite_tap)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(parasite_tap)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(parasite_tap)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(parasite_tap)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(parasite_tap)),
];

pub static PARASITE_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND17,
    frames: &PARASITE_FRAMES_STAND,
    endfunc: Some(parasite_stand),
};

static PARASITE_FRAMES_RUN: [MonsterFrame; 7] = [
    frame(ai::ai_run, 30.0, None),
    frame(ai::ai_run, 30.0, None),
    frame(ai::ai_run, 22.0, Some(parasite_footstep)),
    frame(ai::ai_run, 19.0, Some(parasite_footstep)),
    frame(ai::ai_run, 24.0, None),
    frame(ai::ai_run, 28.0, Some(parasite_footstep)),
    frame(ai::ai_run, 25.0, None),
];

pub static PARASITE_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN03,
    lastframe: FRAME_RUN09,
    frames: &PARASITE_FRAMES_RUN,
    endfunc: None,
};

static PARASITE_FRAMES_START_RUN: [MonsterFrame; 2] =
    [frame(ai::ai_run, 0.0, None), frame(ai::ai_run, 30.0, None)];

pub static PARASITE_MOVE_START_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN01,
    lastframe: FRAME_RUN02,
    frames: &PARASITE_FRAMES_START_RUN,
    endfunc: Some(parasite_run),
};

static PARASITE_FRAMES_STOP_RUN: [MonsterFrame; 6] = [
    frame(ai::ai_run, 20.0, None),
    frame(ai::ai_run, 20.0, None),
    frame(ai::ai_run, 12.0, Some(parasite_footstep)),
    frame(ai::ai_run, 10.0, None),
    frame(ai::ai_run, 0.0, None),
    frame(ai::ai_run, 0.0, None),
];

pub static PARASITE_MOVE_STOP_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN10,
    lastframe: FRAME_RUN15,
    frames: &PARASITE_FRAMES_STOP_RUN,
    endfunc: None,
};

static PARASITE_FRAMES_WALK: [MonsterFrame; 7] = [
    frame(ai::ai_walk, 30.0, None),
    frame(ai::ai_walk, 30.0, None),
    frame(ai::ai_walk, 22.0, Some(parasite_footstep)),
    frame(ai::ai_walk, 19.0, None),
    frame(ai::ai_walk, 24.0, Some(parasite_footstep)),
    frame(ai::ai_walk, 28.0, Some(parasite_footstep)),
    frame(ai::ai_walk, 25.0, None),
];

pub static PARASITE_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN03,
    lastframe: FRAME_RUN09,
    frames: &PARASITE_FRAMES_WALK,
    endfunc: Some(parasite_walk),
};

static PARASITE_FRAMES_START_WALK: [MonsterFrame; 2] = [
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 30.0, Some(parasite_walk)),
];

pub static PARASITE_MOVE_START_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN01,
    lastframe: FRAME_RUN02,
    frames: &PARASITE_FRAMES_START_WALK,
    endfunc: None,
};

static PARASITE_FRAMES_STOP_WALK: [MonsterFrame; 6] = [
    frame(ai::ai_walk, 20.0, None),
    frame(ai::ai_walk, 20.0, None),
    frame(ai::ai_walk, 12.0, Some(parasite_footstep)),
    frame(ai::ai_walk, 10.0, None),
    frame(ai::ai_walk, 0.0, None),
    frame(ai::ai_walk, 0.0, None),
];

pub static PARASITE_MOVE_STOP_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN10,
    lastframe: FRAME_RUN15,
    frames: &PARASITE_FRAMES_STOP_WALK,
    endfunc: None,
};

static PARASITE_FRAMES_PAIN1: [MonsterFrame; 11] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 16.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -7.0, None),
    frame(ai::ai_move, 0.0, None),
];

pub static PARASITE_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN111,
    frames: &PARASITE_FRAMES_PAIN1,
    endfunc: Some(parasite_start_run),
};

static PARASITE_FRAMES_DRAIN: [MonsterFrame; 18] = [
    frame(ai::ai_charge, 0.0, Some(parasite_launch)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 15.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, 0.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, 0.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, 0.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, 0.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, -2.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, -2.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, -3.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, -2.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, 0.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, -1.0, Some(parasite_drain_attack)),
    frame(ai::ai_charge, 0.0, Some(parasite_reel_in)),
    frame(ai::ai_charge, -2.0, None),
    frame(ai::ai_charge, -2.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 0.0, None),
];

pub static PARASITE_MOVE_DRAIN: MonsterMove = MonsterMove {
    firstframe: FRAME_DRAIN01,
    lastframe: FRAME_DRAIN18,
    frames: &PARASITE_FRAMES_DRAIN,
    endfunc: Some(parasite_start_run),
};

static PARASITE_FRAMES_BREAK: [MonsterFrame; 32] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -18.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 9.0, None),
    frame(ai::ai_charge, 6.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -18.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 8.0, None),
    frame(ai::ai_charge, 9.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -18.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 4.0, None),
    frame(ai::ai_charge, 11.0, None),
    frame(ai::ai_charge, -2.0, None),
    frame(ai::ai_charge, -5.0, None),
    frame(ai::ai_charge, 1.0, None),
];

pub static PARASITE_MOVE_BREAK: MonsterMove = MonsterMove {
    firstframe: FRAME_BREAK01,
    lastframe: FRAME_BREAK32,
    frames: &PARASITE_FRAMES_BREAK,
    endfunc: Some(parasite_start_run),
};

static PARASITE_FRAMES_DEATH: [MonsterFrame; 7] = [frame(ai::ai_move, 0.0, None); 7];

pub static PARASITE_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH107,
    frames: &PARASITE_FRAMES_DEATH,
    endfunc: Some(parasite_dead),
};

// ---------------------------------------------------------------------------
// Callbacks. C ref: parasite.c.
// ---------------------------------------------------------------------------

fn sound(world: &mut GameWorld, key: EntityKey, channel: i32, name: &str, attenuation: f32) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), channel, index, 1.0, attenuation, 0.0);
}

fn set_move(world: &mut GameWorld, key: EntityKey, mv: &MonsterMove) {
    if let Some(mi) = world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_mut())
    {
        mi.currentmove = Some(mv.clone());
    }
}

fn stands_ground(world: &GameWorld, key: EntityKey) -> bool {
    world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND))
}

/// C ref: `parasite_launch`.
pub fn parasite_launch(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_LAUNCH, ATTN_NORM);
}

/// C ref: `parasite_reel_in`.
pub fn parasite_reel_in(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_REELIN, ATTN_NORM);
}

/// C ref: `parasite_sight`.
pub fn parasite_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `parasite_tap`.
pub fn parasite_tap(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, SOUND_TAP, ATTN_IDLE);
}

/// C ref: `parasite_footstep`.
pub fn parasite_footstep(world: &mut GameWorld, key: EntityKey) {
    if MONSTER_FOOTSTEPS {
        parasite_tap(world, key);
    }
}

/// C ref: `parasite_scratch`.
pub fn parasite_scratch(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_BODY, SOUND_SCRATCH, ATTN_IDLE);
}

/// C defines it, but `SP_monster_parasite` never sets it as `search`.
/// C ref: `parasite_search`.
pub fn parasite_search(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SEARCH, ATTN_IDLE);
}

/// C ref: `parasite_end_fidget`.
pub fn parasite_end_fidget(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &PARASITE_MOVE_END_FIDGET);
}

/// C ref: `parasite_do_fidget`.
pub fn parasite_do_fidget(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &PARASITE_MOVE_FIDGET);
}

/// Fidget again eight times in ten. C ref: `parasite_refidget`.
pub fn parasite_refidget(world: &mut GameWorld, key: EntityKey) {
    let mv = if world.random() <= 0.8 {
        &PARASITE_MOVE_FIDGET
    } else {
        &PARASITE_MOVE_END_FIDGET
    };
    set_move(world, key, mv);
}

/// C ref: `parasite_idle`.
pub fn parasite_idle(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &PARASITE_MOVE_START_FIDGET);
}

/// C ref: `parasite_stand`.
pub fn parasite_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &PARASITE_MOVE_STAND);
}

/// C ref: `parasite_start_run`.
pub fn parasite_start_run(world: &mut GameWorld, key: EntityKey) {
    let mv = if stands_ground(world, key) {
        &PARASITE_MOVE_STAND
    } else {
        &PARASITE_MOVE_START_RUN
    };
    set_move(world, key, mv);
}

/// C ref: `parasite_run`.
pub fn parasite_run(world: &mut GameWorld, key: EntityKey) {
    let mv = if stands_ground(world, key) {
        &PARASITE_MOVE_STAND
    } else {
        &PARASITE_MOVE_RUN
    };
    set_move(world, key, mv);
}

/// C ref: `parasite_start_walk`.
pub fn parasite_start_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &PARASITE_MOVE_START_WALK);
}

/// C ref: `parasite_walk`.
pub fn parasite_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &PARASITE_MOVE_WALK);
}

/// Hurt skin below half health; pain1 at most every 3 s, none in
/// nightmare. C ref: `parasite_pain`.
pub fn parasite_pain(
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
    if world.game.skill == 3 {
        return; // No pain anims in nightmare.
    }
    let name = if world.random() < 0.5 {
        SOUND_PAIN1
    } else {
        SOUND_PAIN2
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
    set_move(world, key, &PARASITE_MOVE_PAIN1);
}

/// Whether `end` is within 256 units and 30 degrees of pitch of `start`.
/// C ref: `parasite_drain_attack_ok`.
pub fn parasite_drain_attack_ok(start: Vec3f, end: Vec3f) -> bool {
    let dir = start - end;
    if dir.length() > 256.0 {
        return false;
    }
    let mut pitch = crate::weapons::vectoangles(dir).x;
    if pitch < -180.0 {
        pitch += 360.0;
    }
    pitch.abs() <= 30.0
}

/// Drain the enemy if its origin, the top or the bottom of its box is in
/// reach and a shot from the mouth to its origin hits it: 5 on the
/// frame the proboscis lands, 2 after. C ref: `parasite_drain_attack`.
pub fn parasite_drain_attack(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (origin, frame) = (ent.state.origin, ent.state.frame);
    let Some(enemy) = ent.game.enemy else {
        return;
    };
    let Some(en) = world.entities.get(enemy) else {
        return;
    };
    let (enemy_origin, enemy_mins, enemy_maxs) = (en.state.origin, en.mins, en.maxs);
    let (f, r, _) = crate::physics::angle_vectors(ent.state.angles);
    let start = project_source(origin, [24.0, 0.0, 6.0], f, r);

    let reach = [
        enemy_origin.z,
        enemy_origin.z + enemy_maxs.z - 8.0,
        enemy_origin.z + enemy_mins.z + 8.0,
    ];
    if !reach
        .iter()
        .any(|&z| parasite_drain_attack_ok(start, Vec3f::new(enemy_origin.x, enemy_origin.y, z)))
    {
        return;
    }

    // C aims at the origin whichever point was in reach.
    let end = enemy_origin;
    let tr = world.gi.trace(
        start,
        Vec3f::ZERO,
        Vec3f::ZERO,
        end,
        world.number_of(key),
        MASK_SHOT,
    );
    if tr.ent_index.and_then(|n| world.entities.key_for_number(n)) != Some(enemy) {
        return;
    }

    let damage = if frame == FRAME_DRAIN03 {
        sound(world, enemy, CHAN_AUTO, SOUND_IMPACT, ATTN_NORM);
        5
    } else {
        if frame == FRAME_DRAIN04 {
            sound(world, key, CHAN_WEAPON, SOUND_SUCK, ATTN_NORM);
        }
        2
    };

    let number = world.number_of(key).map_or(0, |n| n as i32);
    world.gi.write_byte(SvcOp::TempEntity as i32);
    world.gi.write_byte(TE_PARASITE_ATTACK);
    world.gi.write_short(number);
    world.gi.write_position(start);
    world.gi.write_position(end);
    world.gi.multicast(origin, Multicast::PVS);

    world.t_damage(
        enemy,
        key,
        key,
        start - end,
        enemy_origin,
        Vec3f::ZERO,
        damage,
        0,
        DamageFlags::NO_KNOCKBACK,
        MeansOfDeath::Unknown,
    );
}

/// C ref: `parasite_attack`.
pub fn parasite_attack(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &PARASITE_MOVE_DRAIN);
}

/// The corpse box, then stop. C ref: `parasite_dead`.
pub fn parasite_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// Gib at `gib_health` or below, else play the death once.
/// C ref: `parasite_die`.
pub fn parasite_die(
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
    sound(world, key, CHAN_VOICE, SOUND_DIE, ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    set_move(world, key, &PARASITE_MOVE_DEATH);
}

// ---------------------------------------------------------------------------
// Spawn. C ref: `SP_monster_parasite`, split: the model is the
// `spawn::SPAWNS` row's and the sounds are indexed by `finish_parasite`.
// ---------------------------------------------------------------------------

pub fn sp_monster_parasite(
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
        .unwrap_or_else(|| "monster_parasite".to_string());
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 24.0);
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.game.health = 175;
    ent.game.gib_health = -50;
    ent.game.mass = 250;
    ent.game.viewheight = 16.0;
    ent.pain = Some(parasite_pain);
    ent.die = Some(parasite_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(parasite_stand),
        walk: Some(parasite_start_walk),
        run: Some(parasite_start_run),
        attack: Some(parasite_attack),
        sight: Some(parasite_sight),
        idle: Some(parasite_idle),
        currentmove: Some(PARASITE_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_parasite`: precache its sounds.
pub fn finish_parasite(world: &mut GameWorld, _key: EntityKey, _fields: &HashMap<String, String>) {
    for name in SOUNDS {
        world.gi.sound_index(name);
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster::test_support::*;
    use crate::world::{test_world, test_world_recording};

    const NAMES: &[(ThinkFn, &str)] = &[
        (parasite_do_fidget, "parasite_do_fidget"),
        (parasite_refidget, "parasite_refidget"),
        (parasite_stand, "parasite_stand"),
        (parasite_scratch, "parasite_scratch"),
        (parasite_tap, "parasite_tap"),
        (parasite_footstep, "parasite_footstep"),
        (parasite_run, "parasite_run"),
        (parasite_walk, "parasite_walk"),
        (parasite_start_run, "parasite_start_run"),
        (parasite_launch, "parasite_launch"),
        (parasite_drain_attack, "parasite_drain_attack"),
        (parasite_reel_in, "parasite_reel_in"),
        (parasite_dead, "parasite_dead"),
    ];

    #[test]
    fn parasite_moves_match_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&PARASITE_MOVE_START_FIDGET, 100, 103, "parasite_do_fidget", "stand:0:-*4"),
            (&PARASITE_MOVE_FIDGET, 104, 109, "parasite_refidget", "stand:0:parasite_scratch stand:0:-*2 stand:0:parasite_scratch stand:0:-*2"),
            (&PARASITE_MOVE_END_FIDGET, 110, 117, "parasite_stand", "stand:0:parasite_scratch stand:0:-*7"),
            (&PARASITE_MOVE_STAND, 83, 99, "parasite_stand", "stand:0:-*2 stand:0:parasite_tap stand:0:- stand:0:parasite_tap stand:0:-*3 stand:0:parasite_tap stand:0:- stand:0:parasite_tap stand:0:-*3 stand:0:parasite_tap stand:0:- stand:0:parasite_tap"),
            (&PARASITE_MOVE_RUN, 70, 76, "-", "run:30:-*2 run:22:parasite_footstep run:19:parasite_footstep run:24:- run:28:parasite_footstep run:25:-"),
            (&PARASITE_MOVE_START_RUN, 68, 69, "parasite_run", "run:0:- run:30:-"),
            (&PARASITE_MOVE_STOP_RUN, 77, 82, "-", "run:20:-*2 run:12:parasite_footstep run:10:- run:0:-*2"),
            (&PARASITE_MOVE_WALK, 70, 76, "parasite_walk", "walk:30:-*2 walk:22:parasite_footstep walk:19:- walk:24:parasite_footstep walk:28:parasite_footstep walk:25:-"),
            (&PARASITE_MOVE_START_WALK, 68, 69, "-", "walk:0:- walk:30:parasite_walk"),
            (&PARASITE_MOVE_STOP_WALK, 77, 82, "-", "walk:20:-*2 walk:12:parasite_footstep walk:10:- walk:0:-*2"),
            (&PARASITE_MOVE_PAIN1, 57, 67, "parasite_start_run", "move:0:-*6 move:6:- move:16:- move:-6:- move:-7:- move:0:-"),
            (&PARASITE_MOVE_DRAIN, 39, 56, "parasite_start_run", "charge:0:parasite_launch charge:0:- charge:15:parasite_drain_attack charge:0:parasite_drain_attack*4 charge:-2:parasite_drain_attack*2 charge:-3:parasite_drain_attack charge:-2:parasite_drain_attack charge:0:parasite_drain_attack charge:-1:parasite_drain_attack charge:0:parasite_reel_in charge:-2:-*2 charge:-3:- charge:0:-"),
            (&PARASITE_MOVE_BREAK, 0, 31, "parasite_start_run", "charge:0:- charge:-3:- charge:1:- charge:2:- charge:-3:- charge:1:-*2 charge:3:- charge:0:- charge:-18:- charge:3:- charge:9:- charge:6:- charge:0:- charge:-18:- charge:0:- charge:8:- charge:9:- charge:0:- charge:-18:- charge:0:-*7 charge:4:- charge:11:- charge:-2:- charge:-5:- charge:1:-"),
            (&PARASITE_MOVE_DEATH, 32, 38, "parasite_dead", "move:0:-*7"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    fn parasite(world: &mut GameWorld) -> EntityKey {
        spawn_monster(world, "monster_parasite", Vec3f::new(0.0, 0.0, 24.0))
    }

    #[test]
    fn parasite_spawns_with_the_c_values_and_walks() {
        let (mut world, gi) = test_world_recording();
        let key = parasite(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.health, ent.game.max_health, ent.game.mass),
            (175, 175, 250)
        );
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-16.0, -16.0, -24.0),
                Vec3f::new(16.0, 16.0, 24.0)
            )
        );
        assert_eq!(ent.game.viewheight, 16.0);
        assert!(!ent
            .game
            .flags
            .intersects(EntityFlags::FLY | EntityFlags::SWIM));
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::walkmonster_start_go as ThinkFn)));
        let mi = mi(&world, key);
        assert_eq!(mi.scale, 1.0);
        assert!(mi
            .walk
            .is_some_and(|f| std::ptr::fn_addr_eq(f, parasite_start_walk as ThinkFn)));
        assert!(mi
            .run
            .is_some_and(|f| std::ptr::fn_addr_eq(f, parasite_start_run as ThinkFn)));
        assert!(mi
            .attack
            .is_some_and(|f| std::ptr::fn_addr_eq(f, parasite_attack as ThinkFn)));
        assert!(mi
            .idle
            .is_some_and(|f| std::ptr::fn_addr_eq(f, parasite_idle as ThinkFn)));
        assert!(mi.melee.is_none() && mi.search.is_none(), "C sets neither");
        assert!(playing(&world, key, &PARASITE_MOVE_STAND));
        for s in SOUNDS {
            assert!(gi.sounds.borrow().iter().any(|n| n == s), "{s} precached");
        }
    }

    #[test]
    fn parasite_state_callbacks_pick_the_c_moves() {
        let mut world = test_world();
        let key = parasite(&mut world);
        parasite_start_run(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_START_RUN));
        parasite_run(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_RUN));
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        parasite_start_run(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_STAND));
        parasite_walk(&mut world, key);
        parasite_run(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_STAND));
        parasite_start_walk(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_START_WALK));
        parasite_walk(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_WALK));
        parasite_attack(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_DRAIN));
        parasite_idle(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_START_FIDGET));
        parasite_do_fidget(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_FIDGET));
        parasite_end_fidget(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_END_FIDGET));
        parasite_stand(&mut world, key);
        assert!(playing(&world, key, &PARASITE_MOVE_STAND));
    }

    #[test]
    fn parasite_refidget_fidgets_again_eight_times_in_ten() {
        let mut world = test_world();
        let key = parasite(&mut world);
        let mut again = 0;
        for _ in 0..1000 {
            parasite_refidget(&mut world, key);
            if playing(&world, key, &PARASITE_MOVE_FIDGET) {
                again += 1;
            } else {
                assert!(playing(&world, key, &PARASITE_MOVE_END_FIDGET));
            }
        }
        assert!((750..850).contains(&again), "{again}");
    }

    #[test]
    fn parasite_sounds_play_on_their_c_channels() {
        let (mut world, gi) = test_world_recording();
        let key = parasite(&mut world);
        parasite_launch(&mut world, key);
        parasite_reel_in(&mut world, key);
        parasite_sight(&mut world, key, key);
        parasite_tap(&mut world, key);
        parasite_scratch(&mut world, key);
        parasite_search(&mut world, key);
        // g_monsterfootsteps defaults to 0: footsteps are silent.
        parasite_footstep(&mut world, key);
        assert_eq!(
            sounds_of(&world, &gi, key),
            [
                (CHAN_WEAPON, SOUND_LAUNCH.into(), ATTN_NORM),
                (CHAN_WEAPON, SOUND_REELIN.into(), ATTN_NORM),
                (CHAN_VOICE, SOUND_SIGHT.into(), ATTN_NORM),
                (CHAN_BODY, SOUND_TAP.into(), ATTN_IDLE),
                (CHAN_BODY, SOUND_SCRATCH.into(), ATTN_IDLE),
                (CHAN_VOICE, SOUND_SEARCH.into(), ATTN_IDLE),
            ]
        );
    }

    #[test]
    fn parasite_drain_attack_ok_needs_256_units_and_30_degrees_of_pitch() {
        let at = Vec3f::new(0.0, 0.0, 0.0);
        assert!(parasite_drain_attack_ok(at, Vec3f::new(256.0, 0.0, 0.0)));
        assert!(!parasite_drain_attack_ok(at, Vec3f::new(257.0, 0.0, 0.0)));
        assert!(
            parasite_drain_attack_ok(at, Vec3f::new(100.0, 0.0, 57.0)),
            "29 degrees"
        );
        assert!(
            !parasite_drain_attack_ok(at, Vec3f::new(100.0, 0.0, 62.0)),
            "31 degrees up"
        );
        assert!(
            !parasite_drain_attack_ok(at, Vec3f::new(100.0, 0.0, -62.0)),
            "31 degrees down"
        );
    }

    /// A parasite at the origin facing +x with a player `dx` ahead and `dz` up.
    fn drain_setup(
        dx: f32,
        dz: f32,
    ) -> (GameWorld, std::rc::Rc<ArenaImport>, EntityKey, EntityKey) {
        let (mut world, gi) = hit_world(false);
        let key = parasite(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(dx, 0.0, 24.0 + dz));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        (world, gi, key, player)
    }

    fn health(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().game.health
    }

    #[test]
    fn parasite_drain_attack_bites_for_5_on_drain03_with_the_impact_on_the_enemy() {
        let (mut world, gi, key, player) = drain_setup(100.0, 0.0);
        world.entities.get_mut(key).unwrap().state.frame = FRAME_DRAIN03;
        parasite_drain_attack(&mut world, key);
        assert_eq!(health(&world, player), 95);
        assert_eq!(
            sounds_of(&world, &gi.floor.inner, player),
            [(CHAN_AUTO, SOUND_IMPACT.into(), ATTN_NORM)]
        );
        // C: svc_temp_entity, TE_PARASITE_ATTACK, self, start, end, PVS of self.
        let start = Vec3f::new(24.0, 0.0, 30.0);
        let number = world.number_of(key).unwrap();
        assert_eq!(
            *gi.wire.borrow(),
            [
                format!("byte {}", q2_shared::protocol::SvcOp::TempEntity as i32),
                "byte 16".to_string(),
                format!("short {number}"),
                format!("position {start:?}"),
                format!("position {:?}", Vec3f::new(100.0, 0.0, 24.0)),
                format!("multicast {:?} PVS", Vec3f::new(0.0, 0.0, 24.0)),
            ]
        );
    }

    #[test]
    fn parasite_drain_attack_drains_2_and_sucks_only_on_drain04() {
        let (mut world, gi, key, player) = drain_setup(100.0, 0.0);
        world.entities.get_mut(key).unwrap().state.frame = FRAME_DRAIN04;
        parasite_drain_attack(&mut world, key);
        assert_eq!(health(&world, player), 98);
        assert_eq!(
            sounds_of(&world, &gi.floor.inner, key),
            [(CHAN_WEAPON, SOUND_SUCK.into(), ATTN_NORM)]
        );
        world.entities.get_mut(key).unwrap().state.frame = FRAME_DRAIN04 + 1;
        parasite_drain_attack(&mut world, key);
        assert_eq!(health(&world, player), 96);
        assert_eq!(sounds_of(&world, &gi.floor.inner, key).len(), 1);
    }

    #[test]
    fn parasite_drain_attack_tries_the_enemy_head_and_feet_before_giving_up() {
        // Origin too steep, the top of the box (z + 32 - 8) too, the
        // bottom (z - 24 + 8) shallow enough: the drain still hits.
        let (mut world, _gi, key, player) = drain_setup(80.0, 50.0);
        parasite_drain_attack(&mut world, key);
        assert_eq!(health(&world, player), 98);
        // Every point too steep: no drain.
        let (mut world, _gi, key, player) = drain_setup(20.0, 100.0);
        parasite_drain_attack(&mut world, key);
        assert_eq!(health(&world, player), 100);
        // Too far: no drain.
        let (mut world, _gi, key, player) = drain_setup(300.0, 0.0);
        parasite_drain_attack(&mut world, key);
        assert_eq!(health(&world, player), 100);
    }

    #[test]
    fn parasite_drain_attack_misses_when_the_trace_hits_something_else() {
        let (mut world, gi, key, player) = drain_setup(100.0, 0.0);
        gi.target.set(None);
        parasite_drain_attack(&mut world, key);
        assert_eq!(health(&world, player), 100);
        assert!(gi.wire.borrow().is_empty());
    }

    #[test]
    fn parasite_pain_plays_pain1_with_either_sound() {
        let (mut world, gi) = test_world_recording();
        let key = parasite(&mut world);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..50 {
            world.entities.get_mut(key).unwrap().game.pain_debounce_time = 0.0;
            parasite_stand(&mut world, key);
            parasite_pain(&mut world, key, key, 0.0, 10);
            assert!(playing(&world, key, &PARASITE_MOVE_PAIN1));
            seen.insert(sounds_of(&world, &gi, key).pop().unwrap().1);
        }
        assert_eq!(
            seen,
            [SOUND_PAIN1.to_string(), SOUND_PAIN2.to_string()].into()
        );
    }

    #[test]
    fn parasite_pain_debounces_skins_and_skips_animations_in_nightmare() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 2.0;
        let key = parasite(&mut world);
        world.entities.get_mut(key).unwrap().game.health = 86;
        parasite_pain(&mut world, key, key, 0.0, 10);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.state.skinnum, ent.game.pain_debounce_time), (1, 5.0));
        let calls = gi.sound_calls.borrow().len();
        parasite_stand(&mut world, key);
        parasite_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(gi.sound_calls.borrow().len(), calls, "debounced");
        world.game.skill = 3;
        world.level.time = 6.0;
        parasite_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            9.0
        );
        assert_eq!(gi.sound_calls.borrow().len(), calls, "nightmare");
        assert!(playing(&world, key, &PARASITE_MOVE_STAND));
    }

    #[test]
    fn parasite_die_at_gib_health_throws_six_gibs_and_a_head() {
        let (mut world, gi) = test_world_recording();
        let key = parasite(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -50;
        let before = world.entities.iter().count();
        parasite_die(&mut world, key, key, key, 80, Vec3f::ZERO);
        assert_eq!(world.entities.iter().count(), before + 6);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.svflags & SvFlags::MONSTER.bits(), 0, "now a head");
        assert_eq!(sounds_of(&world, &gi, key)[0].1, "misc/udeath.wav");
    }

    #[test]
    fn parasite_die_above_gib_health_plays_the_death_once() {
        let (mut world, gi) = test_world_recording();
        let key = parasite(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -49;
        parasite_die(&mut world, key, key, key, 80, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage),
            (DeadFlag::Dead, TakeDamage::Yes)
        );
        assert!(playing(&world, key, &PARASITE_MOVE_DEATH));
        assert_eq!(
            sounds_of(&world, &gi, key),
            [(CHAN_VOICE, SOUND_DIE.into(), ATTN_NORM)]
        );
        parasite_stand(&mut world, key);
        parasite_die(&mut world, key, key, key, 80, Vec3f::ZERO);
        assert!(playing(&world, key, &PARASITE_MOVE_STAND), "already dead");
        assert_eq!(sounds_of(&world, &gi, key).len(), 1);
    }

    #[test]
    fn parasite_dead_shrinks_the_box_and_falls() {
        let mut world = test_world();
        let key = parasite(&mut world);
        parasite_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-16.0, -16.0, -24.0),
                Vec3f::new(16.0, 16.0, -8.0)
            )
        );
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_ne!(ent.svflags & SvFlags::DEADMONSTER.bits(), 0);
        assert_eq!(ent.game.nextthink, 0.0);
    }

    #[test]
    fn parasite_sights_a_player_and_drains_it() {
        let (mut world, gi) = hit_world(false);
        world.game.skill = 2;
        let key = parasite(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(150.0, 0.0, 24.0));
        let drained = run_until(&mut world, 100, |w| health(w, player) < 100);
        assert!(drained.is_some(), "drained");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(sounds_of(&world, &gi.floor.inner, key)
            .iter()
            .any(|s| s.1 == SOUND_SIGHT));
        assert!(playing(&world, key, &PARASITE_MOVE_DRAIN));
    }
}
