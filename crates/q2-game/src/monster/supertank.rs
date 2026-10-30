//! Supertank (the boss1 model), and `BossExplode`, which boss2 shares.
//! C ref: `~/Qwasm2/src/game/monster/supertank/supertank.c`

use q2_shared::constants::{
    MZ2_SUPERTANK_MACHINEGUN_1, MZ2_SUPERTANK_ROCKET_1, MZ2_SUPERTANK_ROCKET_2,
    MZ2_SUPERTANK_ROCKET_3,
};
use q2_shared::protocol::SvcOp;
use q2_shared::types::*;
use std::collections::HashMap;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::fire::{monster_fire_bullet, monster_fire_rocket};
use crate::monster::flash::{monster_flash_source, project_source, MONSTER_FLASH_OFFSET};
use crate::monster::{enemy_eye, frame, set_move, sound};
use crate::world::GameWorld;

/// C supertank.h `MODEL_SCALE`.
const MODEL_SCALE: f32 = 1.0;

// Frames the callbacks name besides the move bounds.
const FRAME_ATTAK2_8: i32 = 27;
const FRAME_ATTAK2_11: i32 = 30;
const FRAME_ATTAK2_14: i32 = 33;

// Moves. C ref: supertank.c `supertank_frames_*` / `supertank_move_*`.
// `turn_right`, `turn_left`, `backward`, `attack3` and `attack4` exist
// in C but nothing selects them.
const FRAME_ATTAK1_1: i32 = 0;
const FRAME_ATTAK1_6: i32 = 5;
const FRAME_ATTAK1_7: i32 = 6;
const FRAME_ATTAK1_20: i32 = 19;
const FRAME_ATTAK2_1: i32 = 20;
const FRAME_ATTAK2_27: i32 = 46;
const FRAME_ATTAK3_1: i32 = 47;
const FRAME_ATTAK3_27: i32 = 73;
const FRAME_ATTAK4_1: i32 = 74;
const FRAME_ATTAK4_6: i32 = 79;
const FRAME_BACKWD_1: i32 = 80;
const FRAME_BACKWD_18: i32 = 97;
const FRAME_DEATH_1: i32 = 98;
const FRAME_DEATH_24: i32 = 121;
const FRAME_FORWRD_1: i32 = 128;
const FRAME_FORWRD_18: i32 = 145;
const FRAME_LEFT_1: i32 = 146;
const FRAME_LEFT_18: i32 = 163;
const FRAME_PAIN1_1: i32 = 164;
const FRAME_PAIN1_4: i32 = 167;
const FRAME_PAIN2_5: i32 = 168;
const FRAME_PAIN2_8: i32 = 171;
const FRAME_PAIN3_9: i32 = 172;
const FRAME_PAIN3_12: i32 = 175;
const FRAME_RIGHT_1: i32 = 176;
const FRAME_RIGHT_18: i32 = 193;
const FRAME_STAND_1: i32 = 194;
const FRAME_STAND_60: i32 = 253;

static SUPERTANK_FRAMES_STAND: [MonsterFrame; 60] = [frame(ai::ai_stand, 0.0, None); 60];
static SUPERTANK_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND_1,
    lastframe: FRAME_STAND_60,
    frames: &SUPERTANK_FRAMES_STAND,
    endfunc: None,
};

static SUPERTANK_FRAMES_RUN: [MonsterFrame; 18] = [
    frame(ai::ai_run, 12.0, Some(tread_sound)),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
    frame(ai::ai_run, 12.0, None),
];
static SUPERTANK_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_FORWRD_1,
    lastframe: FRAME_FORWRD_18,
    frames: &SUPERTANK_FRAMES_RUN,
    endfunc: None,
};

static SUPERTANK_FRAMES_FORWARD: [MonsterFrame; 18] = [
    frame(ai::ai_walk, 4.0, Some(tread_sound)),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
];
static SUPERTANK_MOVE_FORWARD: MonsterMove = MonsterMove {
    firstframe: FRAME_FORWRD_1,
    lastframe: FRAME_FORWRD_18,
    frames: &SUPERTANK_FRAMES_FORWARD,
    endfunc: None,
};

pub static SUPERTANK_FRAMES_TURN_RIGHT: [MonsterFrame; 18] = [
    frame(ai::ai_move, 0.0, Some(tread_sound)),
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
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static SUPERTANK_MOVE_TURN_RIGHT: MonsterMove = MonsterMove {
    firstframe: FRAME_RIGHT_1,
    lastframe: FRAME_RIGHT_18,
    frames: &SUPERTANK_FRAMES_TURN_RIGHT,
    endfunc: Some(supertank_run),
};

pub static SUPERTANK_FRAMES_TURN_LEFT: [MonsterFrame; 18] = [
    frame(ai::ai_move, 0.0, Some(tread_sound)),
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
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static SUPERTANK_MOVE_TURN_LEFT: MonsterMove = MonsterMove {
    firstframe: FRAME_LEFT_1,
    lastframe: FRAME_LEFT_18,
    frames: &SUPERTANK_FRAMES_TURN_LEFT,
    endfunc: Some(supertank_run),
};

static SUPERTANK_FRAMES_PAIN3: [MonsterFrame; 4] = [frame(ai::ai_move, 0.0, None); 4];
static SUPERTANK_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN3_9,
    lastframe: FRAME_PAIN3_12,
    frames: &SUPERTANK_FRAMES_PAIN3,
    endfunc: Some(supertank_run),
};

static SUPERTANK_FRAMES_PAIN2: [MonsterFrame; 4] = [frame(ai::ai_move, 0.0, None); 4];
static SUPERTANK_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN2_5,
    lastframe: FRAME_PAIN2_8,
    frames: &SUPERTANK_FRAMES_PAIN2,
    endfunc: Some(supertank_run),
};

static SUPERTANK_FRAMES_PAIN1: [MonsterFrame; 4] = [frame(ai::ai_move, 0.0, None); 4];
static SUPERTANK_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN1_1,
    lastframe: FRAME_PAIN1_4,
    frames: &SUPERTANK_FRAMES_PAIN1,
    endfunc: Some(supertank_run),
};

static SUPERTANK_FRAMES_DEATH1: [MonsterFrame; 24] = [
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
    frame(ai::ai_move, 0.0, Some(boss_explode)),
];
static SUPERTANK_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH_1,
    lastframe: FRAME_DEATH_24,
    frames: &SUPERTANK_FRAMES_DEATH1,
    endfunc: Some(supertank_dead),
};

pub static SUPERTANK_FRAMES_BACKWARD: [MonsterFrame; 18] = [
    frame(ai::ai_walk, 0.0, Some(tread_sound)),
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
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static SUPERTANK_MOVE_BACKWARD: MonsterMove = MonsterMove {
    firstframe: FRAME_BACKWD_1,
    lastframe: FRAME_BACKWD_18,
    frames: &SUPERTANK_FRAMES_BACKWARD,
    endfunc: None,
};

pub static SUPERTANK_FRAMES_ATTACK4: [MonsterFrame; 6] = [frame(ai::ai_move, 0.0, None); 6];
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static SUPERTANK_MOVE_ATTACK4: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK4_1,
    lastframe: FRAME_ATTAK4_6,
    frames: &SUPERTANK_FRAMES_ATTACK4,
    endfunc: Some(supertank_run),
};

pub static SUPERTANK_FRAMES_ATTACK3: [MonsterFrame; 27] = [frame(ai::ai_move, 0.0, None); 27];
/// Unreferenced in C; `pub` so the unused table is kept without a warning.
pub static SUPERTANK_MOVE_ATTACK3: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK3_1,
    lastframe: FRAME_ATTAK3_27,
    frames: &SUPERTANK_FRAMES_ATTACK3,
    endfunc: Some(supertank_run),
};

static SUPERTANK_FRAMES_ATTACK2: [MonsterFrame; 27] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(supertank_rocket)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(supertank_rocket)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(supertank_rocket)),
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
static SUPERTANK_MOVE_ATTACK2: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK2_1,
    lastframe: FRAME_ATTAK2_27,
    frames: &SUPERTANK_FRAMES_ATTACK2,
    endfunc: Some(supertank_run),
};

static SUPERTANK_FRAMES_ATTACK1: [MonsterFrame; 6] =
    [frame(ai::ai_charge, 0.0, Some(supertank_machine_gun)); 6];
static SUPERTANK_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK1_1,
    lastframe: FRAME_ATTAK1_6,
    frames: &SUPERTANK_FRAMES_ATTACK1,
    endfunc: Some(supertank_reattack1),
};

static SUPERTANK_FRAMES_END_ATTACK1: [MonsterFrame; 14] = [frame(ai::ai_move, 0.0, None); 14];
static SUPERTANK_MOVE_END_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK1_7,
    lastframe: FRAME_ATTAK1_20,
    frames: &SUPERTANK_FRAMES_END_ATTACK1,
    endfunc: Some(supertank_run),
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The enemy, alive or not.
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
// Sounds and states
// ---------------------------------------------------------------------------

/// C ref: `TreadSound`.
pub fn tread_sound(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, "bosstank/btkengn1.wav", ATTN_NORM);
}

/// C ref: `supertank_search`.
pub fn supertank_search(world: &mut GameWorld, key: EntityKey) {
    let name = if world.random() < 0.5 {
        "bosstank/btkunqv1.wav"
    } else {
        "bosstank/btkunqv2.wav"
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
}

/// C ref: `supertank_stand`.
pub fn supertank_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &SUPERTANK_MOVE_STAND);
}

/// C ref: `supertank_forward` (nothing calls it in C either).
pub fn supertank_forward(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &SUPERTANK_MOVE_FORWARD);
}

/// C ref: `supertank_walk`.
pub fn supertank_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &SUPERTANK_MOVE_FORWARD);
}

/// C ref: `supertank_run`.
pub fn supertank_run(world: &mut GameWorld, key: EntityKey) {
    let stand_ground = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    let next = if stand_ground {
        &SUPERTANK_MOVE_STAND
    } else {
        &SUPERTANK_MOVE_RUN
    };
    set_move(world, key, next);
}

/// Keep the machine gun going nine times in ten while the enemy is in
/// sight. C ref: `supertank_reattack1`.
pub fn supertank_reattack1(world: &mut GameWorld, key: EntityKey) {
    let visible = enemy_of(world, key).is_some_and(|e| ai::visible(world, key, e));
    let next = if visible && world.random() < 0.9 {
        &SUPERTANK_MOVE_ATTACK1
    } else {
        &SUPERTANK_MOVE_END_ATTACK1
    };
    set_move(world, key, next);
}

/// C ref: `supertank_pain`.
pub fn supertank_pain(
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
        ent.state.skinnum = 1;
    }
    if time < ent.game.pain_debounce_time {
        return;
    }
    // Lessen the chance of him going into his pain frames.
    if damage <= 25 && world.random() < 0.2 {
        return;
    }
    // Don't go into pain if he's firing his rockets.
    if skill >= crate::constants::SKILL_HARD
        && (FRAME_ATTAK2_1..=FRAME_ATTAK2_14).contains(&frame_of(world, key))
    {
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.pain_debounce_time = time + 3.0;
    }
    if skill == crate::constants::SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    let (name, next) = if damage <= 10 {
        ("bosstank/btkpain1.wav", &SUPERTANK_MOVE_PAIN1)
    } else if damage <= 25 {
        ("bosstank/btkpain3.wav", &SUPERTANK_MOVE_PAIN2)
    } else {
        ("bosstank/btkpain2.wav", &SUPERTANK_MOVE_PAIN3)
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
    set_move(world, key, next);
}

// ---------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------

/// A rocket at the enemy's eye. C ref: `supertankRocket`.
pub fn supertank_rocket(world: &mut GameWorld, key: EntityKey) {
    let flash = match frame_of(world, key) {
        FRAME_ATTAK2_8 => MZ2_SUPERTANK_ROCKET_1,
        FRAME_ATTAK2_11 => MZ2_SUPERTANK_ROCKET_2,
        _ => MZ2_SUPERTANK_ROCKET_3,
    };
    let (Some((start, _, _)), Some(eye)) = (
        monster_flash_source(world, key, flash),
        enemy_eye(world, key),
    ) else {
        return;
    };
    let dir = (eye - start).normalize_or_zero();
    monster_fire_rocket(world, key, start, dir, 50, 500, flash);
}

/// A bullet from the level gun at the enemy's eye, or straight ahead
/// without one. C ref: `supertankMachineGun`.
pub fn supertank_machine_gun(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let flash = MZ2_SUPERTANK_MACHINEGUN_1 + (ent.state.frame - FRAME_ATTAK1_1);
    let (forward, right, _) =
        crate::physics::angle_vectors(Vec3f::new(0.0, ent.state.angles.y, 0.0));
    let start = project_source(
        ent.state.origin,
        MONSTER_FLASH_OFFSET[flash as usize],
        forward,
        right,
    );
    // C adds the enemy's velocity times 0: no lead.
    let dir = enemy_eye(world, key).map_or(forward, |eye| (eye - start).normalize_or_zero());
    monster_fire_bullet(
        world,
        key,
        start,
        dir,
        6,
        4,
        DEFAULT_BULLET_HSPREAD,
        DEFAULT_BULLET_VSPREAD,
        flash,
    );
}

/// The machine gun within 160 units, else rockets seven times in ten.
/// C ref: `supertank_attack`.
pub fn supertank_attack(world: &mut GameWorld, key: EntityKey) {
    let (Some(ent), Some(enemy)) = (world.entities.get(key), enemy_of(world, key)) else {
        return;
    };
    let origin = ent.state.origin;
    let range = world
        .entities
        .get(enemy)
        .map_or(0.0, |e| (e.state.origin - origin).length());
    let next = if range <= 160.0 || world.random() < 0.3 {
        &SUPERTANK_MOVE_ATTACK1
    } else {
        &SUPERTANK_MOVE_ATTACK2
    };
    set_move(world, key, next);
}

// ---------------------------------------------------------------------------
// Death
// ---------------------------------------------------------------------------

/// The corpse box, then stop. Never reached: `boss_explode` takes over
/// the think on the death move's last frame. C ref: `supertank_dead`.
pub fn supertank_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-60.0, -60.0, 0.0);
        ent.maxs = Vec3f::new(60.0, 60.0, 72.0);
    }
    ai::monster_dead(world, key);
}

/// Where `count` (0..=7) puts the next explosion around the boss.
const EXPLOSION_OFFSETS: [(f32, f32); 8] = [
    (-24.0, -24.0),
    (24.0, 24.0),
    (24.0, -24.0),
    (-24.0, 24.0),
    (-48.0, -48.0),
    (48.0, 48.0),
    (-48.0, 48.0),
    (48.0, -48.0),
];

/// Eight explosions a tenth of a second apart, then burst into gibs and a
/// gear head. The supertank's, boss2's and Jorg's last death frame.
/// C ref: `BossExplode`.
pub(crate) fn boss_explode(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.think = Some(boss_explode);
    let count = ent.game.count;
    ent.game.count += 1;
    let origin = ent.state.origin;
    let rise = 24.0 + (world.random_u32() & 15) as f32;
    let (dx, dy) = usize::try_from(count)
        .ok()
        .and_then(|i| EXPLOSION_OFFSETS.get(i))
        .copied()
        .unwrap_or_default();
    let org = origin + Vec3f::new(dx, dy, rise);
    if count == 8 {
        if let Some(ent) = world.entities.get_mut(key) {
            ent.state.sound = 0;
        }
        for _ in 0..4 {
            throw_gib(
                world,
                key,
                "models/objects/gibs/sm_meat/tris.md2",
                500,
                GibType::Organic,
            );
        }
        for _ in 0..8 {
            throw_gib(
                world,
                key,
                "models/objects/gibs/sm_metal/tris.md2",
                500,
                GibType::Metallic,
            );
        }
        throw_gib(
            world,
            key,
            "models/objects/gibs/chest/tris.md2",
            500,
            GibType::Organic,
        );
        throw_head(
            world,
            key,
            "models/objects/gibs/gear/tris.md2",
            500,
            GibType::Metallic,
        );
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.deadflag = DeadFlag::Dead;
        }
        return;
    }
    world.gi.write_byte(SvcOp::TempEntity as i32);
    world.gi.write_byte(TE_EXPLOSION1);
    world.gi.write_position(org);
    world.gi.multicast(origin, Multicast::PVS);
    let time = world.level.time;
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = time + FRAMETIME;
    }
}

/// C ref: `supertank_die`.
pub fn supertank_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    sound(world, key, CHAN_VOICE, "bosstank/btkdeth1.wav", ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::No;
        ent.game.count = 0;
    }
    set_move(world, key, &SUPERTANK_MOVE_DEATH);
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// C ref: `SP_monster_supertank`. C's gib_health of -500 is never read:
/// the supertank always explodes.
pub fn sp_monster_supertank(
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
        .unwrap_or_else(|| "monster_supertank".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-64.0, -64.0, 0.0);
    ent.maxs = Vec3f::new(64.0, 64.0, 112.0);
    ent.game.health = 1500;
    ent.game.gib_health = -500;
    ent.game.max_health = 1500;
    ent.game.mass = 800;
    ent.pain = Some(supertank_pain);
    ent.die = Some(supertank_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(supertank_stand),
        walk: Some(supertank_walk),
        run: Some(supertank_run),
        attack: Some(supertank_attack),
        search: Some(supertank_search),
        currentmove: Some(SUPERTANK_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The sounds `SP_monster_supertank` indexes.
pub fn supertank_precache(
    world: &mut GameWorld,
    _key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    for name in [
        "bosstank/btkpain1.wav",
        "bosstank/btkpain2.wav",
        "bosstank/btkpain3.wav",
        "bosstank/btkdeth1.wav",
        "bosstank/btkunqv1.wav",
        "bosstank/btkunqv2.wav",
        "bosstank/btkengn1.wav",
    ] {
        world.gi.sound_index(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NAMES: &[(ThinkFn, &str)] = &[
        (tread_sound, "tread_sound"),
        (supertank_run, "supertank_run"),
        (boss_explode, "boss_explode"),
        (supertank_dead, "supertank_dead"),
        (supertank_rocket, "supertank_rocket"),
        (supertank_machine_gun, "supertank_machine_gun"),
        (supertank_reattack1, "supertank_reattack1"),
    ];

    use crate::entity::ThinkFn;
    use crate::misc::wire::{Wire, WireImport};
    use crate::monster::test_support::*;
    use std::rc::Rc;

    fn supertank() -> (GameWorld, Rc<WireImport>, EntityKey) {
        monster_world(sp_monster_supertank)
    }

    fn set_frame(world: &mut GameWorld, key: EntityKey, frame: i32) {
        world.entities.get_mut(key).unwrap().state.frame = frame;
    }

    #[test]
    fn supertank_moves_match_c_supertank_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&SUPERTANK_MOVE_STAND, 194, 253, "-", "stand:0:-*60"),
            (&SUPERTANK_MOVE_RUN, 128, 145, "-", "run:12:tread_sound run:12:-*17"),
            (&SUPERTANK_MOVE_FORWARD, 128, 145, "-", "walk:4:tread_sound walk:4:-*17"),
            (&SUPERTANK_MOVE_TURN_RIGHT, 176, 193, "supertank_run", "move:0:tread_sound move:0:-*17"),
            (&SUPERTANK_MOVE_TURN_LEFT, 146, 163, "supertank_run", "move:0:tread_sound move:0:-*17"),
            (&SUPERTANK_MOVE_PAIN3, 172, 175, "supertank_run", "move:0:-*4"),
            (&SUPERTANK_MOVE_PAIN2, 168, 171, "supertank_run", "move:0:-*4"),
            (&SUPERTANK_MOVE_PAIN1, 164, 167, "supertank_run", "move:0:-*4"),
            (&SUPERTANK_MOVE_DEATH, 98, 121, "supertank_dead", "move:0:-*23 move:0:boss_explode"),
            (&SUPERTANK_MOVE_BACKWARD, 80, 97, "-", "walk:0:tread_sound walk:0:-*17"),
            (&SUPERTANK_MOVE_ATTACK4, 74, 79, "supertank_run", "move:0:-*6"),
            (&SUPERTANK_MOVE_ATTACK3, 47, 73, "supertank_run", "move:0:-*27"),
            (&SUPERTANK_MOVE_ATTACK2, 20, 46, "supertank_run", "charge:0:-*7 charge:0:supertank_rocket move:0:-*2 move:0:supertank_rocket move:0:-*2 move:0:supertank_rocket move:0:-*13"),
            (&SUPERTANK_MOVE_ATTACK1, 0, 5, "supertank_reattack1", "charge:0:supertank_machine_gun*6"),
            (&SUPERTANK_MOVE_END_ATTACK1, 6, 19, "supertank_run", "move:0:-*14"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    #[test]
    fn supertank_spawns_with_c_health_box_and_callbacks() {
        let (world, _, key) = supertank();
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.health, ent.game.mass), (1500, 800));
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-64.0, -64.0, 0.0), Vec3f::new(64.0, 64.0, 112.0))
        );
        assert_eq!(
            (ent.game.movetype, ent.solid),
            (MoveType::Step, Solid::Bbox)
        );
        assert!(ent
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, ai::walkmonster_start_go as ThinkFn)));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(
            mi.walk.is_some() && mi.run.is_some() && mi.attack.is_some() && mi.search.is_some()
        );
        assert!(
            mi.sight.is_none() && mi.melee.is_none() && mi.dodge.is_none() && mi.idle.is_none()
        );
        assert!(playing(&world, key, &SUPERTANK_MOVE_STAND));
        assert!(ent.pain.is_some() && ent.die.is_some());
    }

    #[test]
    fn supertank_precache_indexes_the_seven_c_sounds() {
        let (mut world, gi, key) = supertank();
        supertank_precache(&mut world, key, &HashMap::new());
        assert_eq!(gi.inner.sounds.borrow().len(), 7);
    }

    #[test]
    fn supertank_search_plays_either_search_sound_and_tread_rumbles() {
        let (mut world, gi, key) = supertank();
        seed_random(&mut world, |r| r < 0.5);
        supertank_search(&mut world, key);
        seed_random(&mut world, |r| r >= 0.5);
        supertank_search(&mut world, key);
        tread_sound(&mut world, key);
        let names: Vec<String> = wire_sounds(&gi)
            .into_iter()
            .map(|(c, n, a)| {
                assert_eq!((c, a), (CHAN_VOICE, ATTN_NORM));
                n
            })
            .collect();
        assert_eq!(
            names,
            [
                "bosstank/btkunqv1.wav",
                "bosstank/btkunqv2.wav",
                "bosstank/btkengn1.wav"
            ]
        );
    }

    #[test]
    fn supertank_walk_run_and_stand_pick_their_moves() {
        let (mut world, _, key) = supertank();
        supertank_walk(&mut world, key);
        assert!(playing(&world, key, &SUPERTANK_MOVE_FORWARD));
        supertank_stand(&mut world, key);
        supertank_forward(&mut world, key);
        assert!(playing(&world, key, &SUPERTANK_MOVE_FORWARD));
        supertank_run(&mut world, key);
        assert!(playing(&world, key, &SUPERTANK_MOVE_RUN));
        world
            .entities
            .get_mut(key)
            .unwrap()
            .monsterinfo
            .as_mut()
            .unwrap()
            .aiflags = AiFlags::STAND_GROUND;
        supertank_run(&mut world, key);
        assert!(playing(&world, key, &SUPERTANK_MOVE_STAND));
    }

    #[test]
    fn supertank_reattack1_keeps_firing_nine_times_in_ten_while_in_sight() {
        let (mut world, gi, key) = supertank();
        give_enemy(&mut world, key, Vec3f::new(100.0, 0.0, 0.0));
        let mut again = 0;
        for _ in 0..1000 {
            supertank_reattack1(&mut world, key);
            again += playing(&world, key, &SUPERTANK_MOVE_ATTACK1) as usize;
        }
        assert!((860..940).contains(&again), "{again}");
        *gi.inner.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        seed_random(&mut world, |r| r < 0.1);
        supertank_reattack1(&mut world, key);
        assert!(playing(&world, key, &SUPERTANK_MOVE_END_ATTACK1));
    }

    fn pain(world: &mut GameWorld, key: EntityKey, damage: i32, roll: f32) {
        seed_random(world, |r| (r - roll).abs() < 0.01);
        supertank_pain(world, key, key, 0.0, damage);
    }

    #[test]
    fn supertank_pain_picks_move_and_sound_by_damage() {
        for (damage, name, mv) in [
            (10, "bosstank/btkpain1.wav", &SUPERTANK_MOVE_PAIN1),
            (11, "bosstank/btkpain3.wav", &SUPERTANK_MOVE_PAIN2),
            (25, "bosstank/btkpain3.wav", &SUPERTANK_MOVE_PAIN2),
            (26, "bosstank/btkpain2.wav", &SUPERTANK_MOVE_PAIN3),
        ] {
            let (mut world, gi, key) = supertank();
            pain(&mut world, key, damage, 0.5);
            assert!(playing(&world, key, mv), "{damage}");
            assert_eq!(
                wire_sounds(&gi),
                [(CHAN_VOICE, name.to_string(), ATTN_NORM)]
            );
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                4.0
            );
        }
    }

    #[test]
    fn supertank_pain_shrugs_off_light_hits_one_time_in_five_and_debounces() {
        let (mut world, gi, key) = supertank();
        pain(&mut world, key, 25, 0.1);
        pain(&mut world, key, 26, 0.1); // heavy hits always count
        assert!(playing(&world, key, &SUPERTANK_MOVE_PAIN3));
        pain(&mut world, key, 100, 0.5); // debounced
        assert_eq!(gi.sounds.borrow().len(), 1);
    }

    #[test]
    fn supertank_pain_sets_the_damaged_skin_below_half_health() {
        let (mut world, _, key) = supertank();
        world.entities.get_mut(key).unwrap().game.health = 749;
        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 5.0;
        pain(&mut world, key, 100, 0.5);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
    }

    #[test]
    fn supertank_pain_on_hard_spares_the_rocket_volley_and_nightmare_has_no_anim() {
        let (mut world, gi, key) = supertank();
        world.game.skill = crate::constants::SKILL_HARD;
        set_frame(&mut world, key, FRAME_ATTAK2_14);
        pain(&mut world, key, 100, 0.5);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            0.0
        );
        set_frame(&mut world, key, FRAME_ATTAK2_14 + 1);
        pain(&mut world, key, 100, 0.5);
        assert!(playing(&world, key, &SUPERTANK_MOVE_PAIN3));

        let (mut world, gi2, key) = supertank();
        world.game.skill = crate::constants::SKILL_HARDPLUS;
        pain(&mut world, key, 100, 0.5);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            4.0
        );
        assert!(playing(&world, key, &SUPERTANK_MOVE_STAND));
        assert!(
            gi2.sounds.borrow().is_empty(),
            "C plays no sound in nightmare"
        );
        assert_eq!(gi.sounds.borrow().len(), 1);
    }

    #[test]
    fn supertank_pain_on_hard_spares_the_volley_from_its_first_frame_only() {
        let (mut world, _, key) = supertank();
        world.game.skill = crate::constants::SKILL_HARD;
        set_frame(&mut world, key, FRAME_ATTAK2_1);
        pain(&mut world, key, 100, 0.5);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            0.0
        );
        set_frame(&mut world, key, FRAME_ATTAK2_1 - 1);
        pain(&mut world, key, 100, 0.5);
        assert!(playing(&world, key, &SUPERTANK_MOVE_PAIN3));
    }

    #[test]
    fn supertank_attack_uses_the_gun_close_and_mostly_rockets_far() {
        for (range, roll, mv) in [
            (160.0, 0.9, &SUPERTANK_MOVE_ATTACK1),
            (161.0, 0.29, &SUPERTANK_MOVE_ATTACK1),
            (161.0, 0.31, &SUPERTANK_MOVE_ATTACK2),
        ] {
            let (mut world, _, key) = supertank();
            give_enemy(&mut world, key, Vec3f::new(range, 0.0, 0.0));
            seed_random(&mut world, |r| (r - roll).abs() < 0.005);
            supertank_attack(&mut world, key);
            assert!(playing(&world, key, mv), "{range} {roll}");
        }
    }

    #[test]
    fn supertank_rocket_fires_from_the_frame_flash_at_the_eye() {
        for (frame, flash) in [
            (FRAME_ATTAK2_8, MZ2_SUPERTANK_ROCKET_1),
            (FRAME_ATTAK2_11, MZ2_SUPERTANK_ROCKET_2),
            (FRAME_ATTAK2_14, MZ2_SUPERTANK_ROCKET_3),
        ] {
            let (mut world, gi, key) = supertank();
            give_enemy(&mut world, key, Vec3f::new(500.0, 100.0, 0.0));
            set_frame(&mut world, key, frame);
            supertank_rocket(&mut world, key);
            let rocket = world.entities.get(all_of(&world, "rocket")[0]).unwrap();
            let (start, _, _) = monster_flash_source(&world, key, flash).unwrap();
            let want = (Vec3f::new(500.0, 100.0, 22.0) - start).normalize() * 500.0;
            assert!((rocket.velocity - want).length() < 0.01);
            assert_eq!(rocket.game.dmg, 50);
            assert_eq!(muzzle_flashes(&gi), [flash]);
        }
    }

    #[test]
    fn supertank_machine_gun_flashes_by_frame_and_fires_without_an_enemy() {
        let (mut world, gi, key) = supertank();
        for frame in FRAME_ATTAK1_1..=FRAME_ATTAK1_6 {
            set_frame(&mut world, key, frame);
            supertank_machine_gun(&mut world, key);
        }
        let want: Vec<i32> = (MZ2_SUPERTANK_MACHINEGUN_1..MZ2_SUPERTANK_MACHINEGUN_1 + 6).collect();
        assert_eq!(muzzle_flashes(&gi), want);
    }

    /// A supertank at (0, 0, 24) facing +x but pitched 20 degrees, on
    /// attak1_1, with a player at (300, 0, 24) as its enemy.
    fn pitched_supertank() -> (GameWorld, Rc<ArenaImport>, EntityKey, EntityKey) {
        let (mut world, gi, player) = player_arena(300.0);
        let key = world.spawn().unwrap();
        sp_monster_supertank(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = Vec3f::new(0.0, 0.0, 24.0);
        ent.state.angles = Vec3f::new(20.0, 0.0, 0.0);
        ent.state.frame = FRAME_ATTAK1_1;
        ent.game.enemy = Some(player);
        world.link_entity(key);
        (world, gi, key, player)
    }

    /// C projects flash offset (30, 30, 88.5) from (0, yaw, 0), ignoring
    /// the pitch: forward +x, right -y.
    const YAW_ONLY_START: Vec3f = Vec3f::new(30.0, -30.0, 112.5);

    fn gun_damage_with_target_at(
        world: &mut GameWorld,
        gi: &ArenaImport,
        key: EntityKey,
        player: EntityKey,
        at: Vec3f,
    ) -> i32 {
        gi.target_at(at, 4.0, world.number_of(player).unwrap());
        let before = world.entities.get(player).unwrap().game.health;
        supertank_machine_gun(world, key);
        before - world.entities.get(player).unwrap().game.health
    }

    #[test]
    fn supertank_machine_gun_fires_from_the_yaw_only_muzzle_at_the_eye_for_6() {
        let (mut world, gi, key, player) = pitched_supertank();
        let p = world.entities.get(player).unwrap();
        let eye = p.state.origin + Vec3f::new(0.0, 0.0, p.game.viewheight);
        // Close to the muzzle, where a pitched start would pass well clear.
        let at = YAW_ONLY_START + (eye - YAW_ONLY_START).normalize() * 20.0;
        assert_eq!(
            gun_damage_with_target_at(&mut world, &gi, key, player, at),
            6
        );
    }

    #[test]
    fn supertank_machine_gun_without_an_enemy_fires_level_ahead() {
        let (mut world, gi, key, player) = pitched_supertank();
        world.entities.get_mut(key).unwrap().game.enemy = None;
        let at = YAW_ONLY_START + Vec3f::X * 20.0;
        assert_eq!(
            gun_damage_with_target_at(&mut world, &gi, key, player, at),
            6
        );
    }

    #[test]
    fn supertank_die_starts_the_death_move_and_stops_taking_damage() {
        let (mut world, gi, key) = supertank();
        world.entities.get_mut(key).unwrap().game.count = 5;
        world.entities.get_mut(key).unwrap().game.takedamage = TakeDamage::Aim;
        supertank_die(&mut world, key, key, key, 100, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.deadflag, ent.game.takedamage, ent.game.count),
            (DeadFlag::Dead, TakeDamage::No, 0)
        );
        assert!(playing(&world, key, &SUPERTANK_MOVE_DEATH));
        assert_eq!(
            wire_sounds(&gi),
            [(CHAN_VOICE, "bosstank/btkdeth1.wav".to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn supertank_dead_sets_the_c_corpse_box() {
        let (mut world, _, key) = supertank();
        supertank_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-60.0, -60.0, 0.0), Vec3f::new(60.0, 60.0, 72.0))
        );
        assert_eq!(ent.game.movetype, MoveType::Toss);
    }

    /// The positions of every TE_EXPLOSION1 on the wire.
    fn explosions(gi: &WireImport) -> Vec<Vec3f> {
        let wire = gi.wire.borrow();
        wire.windows(3)
            .filter_map(|w| match w {
                [Wire::Byte(op), Wire::Byte(TE_EXPLOSION1), Wire::Position(p)]
                    if *op == SvcOp::TempEntity as i32 =>
                {
                    Some(*p)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn boss_explode_blasts_eight_times_around_the_boss_then_gibs_it() {
        let (mut world, gi, key) = supertank();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(100.0, 200.0, 0.0);
        world.entities.get_mut(key).unwrap().state.sound = 7;
        supertank_die(&mut world, key, key, key, 100, Vec3f::ZERO);
        boss_explode(&mut world, key);
        assert!(world
            .entities
            .get(key)
            .unwrap()
            .think
            .is_some_and(|f| std::ptr::fn_addr_eq(f, boss_explode as ThinkFn)));
        for _ in 0..8 {
            world.level.time += FRAMETIME;
            world.run_think(key);
        }
        let blasts = explosions(&gi);
        assert_eq!(blasts.len(), 8);
        let offsets: Vec<(f32, f32)> = blasts.iter().map(|p| (p.x - 100.0, p.y - 200.0)).collect();
        // C BossExplode's switch, cases 0..=7; case 8 gibs and returns
        // before the explosion write (supertank.c:820-840).
        let c = [
            (-24.0, -24.0),
            (24.0, 24.0),
            (24.0, -24.0),
            (-24.0, 24.0),
            (-48.0, -48.0),
            (48.0, 48.0),
            (-48.0, 48.0),
            (48.0, -48.0),
        ];
        assert_eq!(offsets, c);
        assert!(blasts.iter().all(|p| (24.0..=39.0).contains(&p.z)));
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.state.sound, ent.game.deadflag), (0, DeadFlag::Dead));
        // The boss is now the gear head, which ThrowHead leaves for 10 to
        // 20 seconds; BossExplode does not reschedule it.
        assert!(ent.game.nextthink >= world.level.time + 10.0);
        world.level.time += FRAMETIME;
        world.run_think(key);
        assert!(world.entities.is_live(key));
        assert_eq!(explosions(&gi).len(), 8);
    }

    #[test]
    fn boss_explode_throws_four_meat_eight_metal_a_chest_and_the_gear_head() {
        let (mut world, gi, key) = supertank();
        world.entities.get_mut(key).unwrap().game.count = 8;
        let before = world.entities.iter().count();
        boss_explode(&mut world, key);
        assert_eq!(world.entities.iter().count() - before, 13);
        let gibs: Vec<String> = world
            .entities
            .iter()
            .filter(|(k, _)| *k != key)
            .map(|(_, e)| e.state.modelindex)
            .map(|i| gi.inner.models.borrow()[i as usize - 1].clone())
            .collect();
        let count = |m: &str| gibs.iter().filter(|g| g.contains(m)).count();
        assert_eq!(
            (count("sm_meat"), count("sm_metal"), count("chest")),
            (4, 8, 1)
        );
        let head = world.entities.get(key).unwrap().state.modelindex;
        assert!(gi.inner.models.borrow()[head as usize - 1].contains("gear"));
    }

    #[test]
    fn supertank_sights_a_player_and_fires_at_it() {
        let (mut world, gi, key) = supertank();
        let player = player_at(&mut world, Vec3f::new(400.0, 0.0, 24.0));
        let at = world.entities.get(key).unwrap().state.origin;
        clear_shots_at(&gi, &world, player, at, 100_000);
        let fired = run_monster(&mut world, key, 600, |w| !all_of(w, "rocket").is_empty());
        assert!(fired, "no rocket");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(!muzzle_flashes(&gi).is_empty());
    }
}
