//! Mutant: claws in melee and a leap at enemies further off.
//! C ref: `~/Qwasm2/src/game/monster/mutant/mutant.c`.

use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::monster::movement::m_check_bottom;
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frames. C ref: mutant.h.
// ---------------------------------------------------------------------------

const FRAME_ATTACK01: i32 = 0;
const FRAME_ATTACK02: i32 = 1;
const FRAME_ATTACK05: i32 = 4;
const FRAME_ATTACK08: i32 = 7;
const FRAME_ATTACK09: i32 = 8;
const FRAME_ATTACK15: i32 = 14;
const FRAME_DEATH101: i32 = 15;
const FRAME_DEATH109: i32 = 23;
const FRAME_DEATH201: i32 = 24;
const FRAME_DEATH210: i32 = 33;
const FRAME_PAIN101: i32 = 34;
const FRAME_PAIN105: i32 = 38;
const FRAME_PAIN201: i32 = 39;
const FRAME_PAIN206: i32 = 44;
const FRAME_PAIN301: i32 = 45;
const FRAME_PAIN311: i32 = 55;
const FRAME_RUN03: i32 = 56;
const FRAME_RUN08: i32 = 61;
const FRAME_STAND101: i32 = 62;
const FRAME_STAND151: i32 = 112;
const FRAME_STAND152: i32 = 113;
const FRAME_STAND155: i32 = 116;
const FRAME_STAND164: i32 = 125;
const FRAME_WALK01: i32 = 126;
const FRAME_WALK04: i32 = 129;
const FRAME_WALK05: i32 = 130;
const FRAME_WALK16: i32 = 141;

/// C `MODEL_SCALE` (mutant.h).
const MODEL_SCALE: f32 = 1.0;

// C SP_monster_mutant values.
const HEALTH: i32 = 300;
const MINS: Vec3f = Vec3f::new(-32.0, -32.0, -24.0);
const MAXS: Vec3f = Vec3f::new(32.0, 32.0, 48.0);
const MASS: i32 = 300;

const SOUND_SWING: &str = "mutant/mutatck1.wav";
const SOUND_HIT: &str = "mutant/mutatck2.wav";
const SOUND_HIT2: &str = "mutant/mutatck3.wav";
const SOUND_DEATH: &str = "mutant/mutdeth1.wav";
const SOUND_IDLE: &str = "mutant/mutidle1.wav";
const SOUND_PAIN1: &str = "mutant/mutpain1.wav";
const SOUND_PAIN2: &str = "mutant/mutpain2.wav";
const SOUND_SIGHT: &str = "mutant/mutsght1.wav";
const SOUND_SEARCH: &str = "mutant/mutsrch1.wav";
const SOUND_STEP1: &str = "mutant/step1.wav";
const SOUND_STEP2: &str = "mutant/step2.wav";
const SOUND_STEP3: &str = "mutant/step3.wav";
const SOUND_THUD: &str = "mutant/thud1.wav";

/// The sounds C `SP_monster_mutant` precaches, in its order.
const PRECACHE: [&str; 13] = [
    SOUND_SWING,
    SOUND_HIT,
    SOUND_HIT2,
    SOUND_DEATH,
    SOUND_IDLE,
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_SIGHT,
    SOUND_SEARCH,
    SOUND_STEP1,
    SOUND_STEP2,
    SOUND_STEP3,
    SOUND_THUD,
];

// ---------------------------------------------------------------------------
// Moves. C ref: mutant.c `mutant_frames_*` / `mutant_move_*`.
// ---------------------------------------------------------------------------

const fn frame(aifunc: AnimFrameFn, dist: f32, thinkfunc: Option<ThinkFn>) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

static MUTANT_FRAMES_STAND: [MonsterFrame; 51] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
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

static MUTANT_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND101,
    lastframe: FRAME_STAND151,
    frames: &MUTANT_FRAMES_STAND,
    endfunc: None,
};

static MUTANT_FRAMES_IDLE: [MonsterFrame; 13] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, Some(mutant_idle_loop)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
];

static MUTANT_MOVE_IDLE: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND152,
    lastframe: FRAME_STAND164,
    frames: &MUTANT_FRAMES_IDLE,
    endfunc: Some(mutant_stand),
};

static MUTANT_FRAMES_WALK: [MonsterFrame; 12] = [
    frame(ai::ai_walk, 3.0, Some(mutant_step)),
    frame(ai::ai_walk, 1.0, None),
    frame(ai::ai_walk, 5.0, Some(mutant_step)),
    frame(ai::ai_walk, 10.0, None),
    frame(ai::ai_walk, 13.0, None),
    frame(ai::ai_walk, 10.0, None),
    frame(ai::ai_walk, 0.0, Some(mutant_step)),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 16.0, None),
    frame(ai::ai_walk, 15.0, None),
    frame(ai::ai_walk, 6.0, None),
];

static MUTANT_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK05,
    lastframe: FRAME_WALK16,
    frames: &MUTANT_FRAMES_WALK,
    endfunc: None,
};

static MUTANT_FRAMES_START_WALK: [MonsterFrame; 4] = [
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, -2.0, Some(mutant_step)),
    frame(ai::ai_walk, 1.0, None),
];

static MUTANT_MOVE_START_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK01,
    lastframe: FRAME_WALK04,
    frames: &MUTANT_FRAMES_START_WALK,
    endfunc: Some(mutant_walk_loop),
};

static MUTANT_FRAMES_RUN: [MonsterFrame; 6] = [
    frame(ai::ai_run, 40.0, None),
    frame(ai::ai_run, 40.0, Some(mutant_step)),
    frame(ai::ai_run, 24.0, None),
    frame(ai::ai_run, 5.0, Some(mutant_step)),
    frame(ai::ai_run, 17.0, None),
    frame(ai::ai_run, 10.0, None),
];

static MUTANT_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN03,
    lastframe: FRAME_RUN08,
    frames: &MUTANT_FRAMES_RUN,
    endfunc: None,
};

static MUTANT_FRAMES_ATTACK: [MonsterFrame; 7] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(mutant_hit_left)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(mutant_hit_right)),
    frame(ai::ai_charge, 0.0, Some(mutant_check_refire)),
];

static MUTANT_MOVE_ATTACK: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK09,
    lastframe: FRAME_ATTACK15,
    frames: &MUTANT_FRAMES_ATTACK,
    endfunc: Some(mutant_run),
};

static MUTANT_FRAMES_JUMP: [MonsterFrame; 8] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 17.0, None),
    frame(ai::ai_charge, 15.0, Some(mutant_jump_takeoff)),
    frame(ai::ai_charge, 15.0, None),
    frame(ai::ai_charge, 15.0, Some(mutant_check_landing)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 0.0, None),
];

static MUTANT_MOVE_JUMP: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTACK01,
    lastframe: FRAME_ATTACK08,
    frames: &MUTANT_FRAMES_JUMP,
    endfunc: Some(mutant_run),
};

static MUTANT_FRAMES_PAIN1: [MonsterFrame; 5] = [
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -8.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 5.0, None),
];

static MUTANT_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN105,
    frames: &MUTANT_FRAMES_PAIN1,
    endfunc: Some(mutant_run),
};

static MUTANT_FRAMES_PAIN2: [MonsterFrame; 6] = [
    frame(ai::ai_move, -24.0, None),
    frame(ai::ai_move, 11.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 4.0, None),
];

static MUTANT_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN206,
    frames: &MUTANT_FRAMES_PAIN2,
    endfunc: Some(mutant_run),
};

static MUTANT_FRAMES_PAIN3: [MonsterFrame; 11] = [
    frame(ai::ai_move, -22.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 1.0, None),
];

static MUTANT_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN311,
    frames: &MUTANT_FRAMES_PAIN3,
    endfunc: Some(mutant_run),
};

static MUTANT_FRAMES_DEATH1: [MonsterFrame; 9] = [
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

static MUTANT_MOVE_DEATH1: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH109,
    frames: &MUTANT_FRAMES_DEATH1,
    endfunc: Some(mutant_dead),
};

static MUTANT_FRAMES_DEATH2: [MonsterFrame; 10] = [
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

static MUTANT_MOVE_DEATH2: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH201,
    lastframe: FRAME_DEATH210,
    frames: &MUTANT_FRAMES_DEATH2,
    endfunc: Some(mutant_dead),
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

/// C `self->monsterinfo.nextframe = frame`.
fn set_nextframe(world: &mut GameWorld, key: EntityKey, frame: i32) {
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.nextframe = frame;
    }
}

fn aiflags(world: &GameWorld, key: EntityKey) -> AiFlags {
    world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .map_or(AiFlags::empty(), |mi| mi.aiflags)
}

/// The live enemy, if `self->enemy` is set and in use.
fn enemy_of(world: &GameWorld, key: EntityKey) -> Option<EntityKey> {
    world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .filter(|&e| world.entities.get(e).is_some_and(|e| e.in_use))
}

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

/// One of three step sounds; C plays these whatever `g_monsterfootsteps`
/// says. C ref: `mutant_step`.
pub fn mutant_step(world: &mut GameWorld, key: EntityKey) {
    let name = match world.random_u32().wrapping_add(1) % 3 {
        0 => SOUND_STEP1,
        1 => SOUND_STEP2,
        _ => SOUND_STEP3,
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
}

/// C ref: `mutant_sight`.
pub fn mutant_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `mutant_search`.
pub fn mutant_search(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SEARCH, ATTN_NORM);
}

/// Defined but unused in C too: no mutant frame calls it.
/// C ref: `mutant_swing`.
pub fn mutant_swing(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SWING, ATTN_NORM);
}

/// C ref: `mutant_stand`.
pub fn mutant_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MUTANT_MOVE_STAND);
}

/// Keep scratching three times in four. C ref: `mutant_idle_loop`.
pub fn mutant_idle_loop(world: &mut GameWorld, key: EntityKey) {
    if world.random() < 0.75 {
        set_nextframe(world, key, FRAME_STAND155);
    }
}

/// C ref: `mutant_idle`.
pub fn mutant_idle(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MUTANT_MOVE_IDLE);
    sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
}

/// C ref: `mutant_walk_loop`.
pub fn mutant_walk_loop(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MUTANT_MOVE_WALK);
}

/// C ref: `mutant_walk`.
pub fn mutant_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MUTANT_MOVE_START_WALK);
}

/// C ref: `mutant_run`.
pub fn mutant_run(world: &mut GameWorld, key: EntityKey) {
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        set_move(world, key, &MUTANT_MOVE_STAND);
    } else {
        set_move(world, key, &MUTANT_MOVE_RUN);
    }
}

/// A claw for 10 to 14 from the `right` side of the box.
fn mutant_hit(world: &mut GameWorld, key: EntityKey, right: f32, hit_sound: &str) {
    let aim = Vec3f::new(ai::MELEE_DISTANCE, right, 8.0);
    let damage = 10 + (world.random_u32() % 5) as i32;
    if world.fire_hit(key, aim, damage, 100) {
        sound(world, key, CHAN_WEAPON, hit_sound, ATTN_NORM);
    } else {
        sound(world, key, CHAN_WEAPON, SOUND_SWING, ATTN_NORM);
    }
}

/// C ref: `mutant_hit_left`.
pub fn mutant_hit_left(world: &mut GameWorld, key: EntityKey) {
    let Some(mins_x) = world.entities.get(key).map(|e| e.mins.x) else {
        return;
    };
    mutant_hit(world, key, mins_x, SOUND_HIT);
}

/// C ref: `mutant_hit_right`.
pub fn mutant_hit_right(world: &mut GameWorld, key: EntityKey) {
    let Some(maxs_x) = world.entities.get(key).map(|e| e.maxs.x) else {
        return;
    };
    mutant_hit(world, key, maxs_x, SOUND_HIT2);
}

/// Claw again while the enemy is in reach, or on even odds in nightmare.
/// C ref: `mutant_check_refire`.
pub fn mutant_check_refire(world: &mut GameWorld, key: EntityKey) {
    let Some(enemy) = enemy_of(world, key) else {
        return;
    };
    if world.entities.get(enemy).is_none_or(|e| e.game.health <= 0) {
        return;
    }
    if (world.game.skill == SKILL_HARDPLUS && world.random() < 0.5)
        || ai::range(world, key, enemy) == Range::Melee
    {
        set_nextframe(world, key, FRAME_ATTACK09);
    }
}

/// C ref: `mutant_melee`.
pub fn mutant_melee(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MUTANT_MOVE_ATTACK);
}

/// Land on a damageable entity at over 400 units/s for 40 to 50 damage;
/// off a ledge with ground under us, go back to the leap.
/// C ref: `mutant_jump_touch`.
pub fn mutant_jump_touch(
    world: &mut GameWorld,
    key: EntityKey,
    other: EntityKey,
    _plane: Option<&Plane>,
    _surf: Option<&Surface>,
) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.health <= 0 {
        ent.touch = None;
        return;
    }
    let (origin, velocity, maxs_x) = (ent.state.origin, ent.velocity, ent.maxs.x);
    let takes_damage = world
        .entities
        .get(other)
        .is_some_and(|o| o.game.takedamage != TakeDamage::No);
    if takes_damage && velocity.length() > 400.0 {
        let normal = velocity.normalize_or_zero();
        let point = origin + normal * maxs_x;
        let damage = (40.0 + 10.0 * world.random()) as i32;
        world.t_damage(
            other,
            key,
            key,
            velocity,
            point,
            normal,
            damage,
            damage,
            DamageFlags::empty(),
            MeansOfDeath::Unknown,
        );
    }
    if !m_check_bottom(world, key) {
        let Some(ent) = world.entities.get_mut(key) else {
            return;
        };
        if ent.game.ground_entity.is_some() {
            ent.touch = None;
            set_nextframe(world, key, FRAME_ATTACK02);
        }
        return;
    }
    if let Some(ent) = world.entities.get_mut(key) {
        ent.touch = None;
    }
}

/// Leap forward at 600 and up at 250, ducked, for up to 3 seconds.
/// C ref: `mutant_jump_takeoff`.
pub fn mutant_jump_takeoff(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let (forward, _, _) = crate::physics::angle_vectors(ent.state.angles);
    ent.state.origin.z += 1.0;
    ent.velocity = forward * 600.0;
    ent.velocity.z = 250.0;
    ent.game.ground_entity = None;
    ent.touch = Some(mutant_jump_touch);
    if let Some(mi) = ent.monsterinfo.as_mut() {
        mi.aiflags |= AiFlags::DUCKED;
        mi.attack_finished = time + 3.0;
    }
}

/// Thud down once grounded; in the air, hold the falling frames, or
/// restart the leap once the 3 seconds are up. C ref: `mutant_check_landing`.
pub fn mutant_check_landing(world: &mut GameWorld, key: EntityKey) {
    if world
        .entities
        .get(key)
        .is_some_and(|e| e.game.ground_entity.is_some())
    {
        sound(world, key, CHAN_WEAPON, SOUND_THUD, ATTN_NORM);
        if let Some(mi) = monsterinfo_mut(world, key) {
            mi.attack_finished = 0.0;
            mi.aiflags.remove(AiFlags::DUCKED);
        }
        return;
    }
    let time = world.level.time;
    let Some(mi) = monsterinfo_mut(world, key) else {
        return;
    };
    mi.nextframe = if time > mi.attack_finished {
        FRAME_ATTACK02
    } else {
        FRAME_ATTACK05
    };
}

/// C ref: `mutant_jump`.
pub fn mutant_jump(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &MUTANT_MOVE_JUMP);
}

/// C ref: `mutant_check_melee`.
fn mutant_check_melee(world: &GameWorld, key: EntityKey, enemy: EntityKey) -> bool {
    ai::range(world, key, enemy) == Range::Melee
}

/// Leap only at an enemy roughly level with us, 100 or more units off,
/// and then one time in ten (always at exactly 100). C ref: `mutant_check_jump`.
fn mutant_check_jump(world: &mut GameWorld, key: EntityKey, enemy: EntityKey) -> bool {
    let (Some(me), Some(them)) = (world.entities.get(key), world.entities.get(enemy)) else {
        return false;
    };
    if me.absmin.z > them.absmin.z + 0.75 * them.size.z {
        return false;
    }
    if me.absmax.z < them.absmin.z + 0.25 * them.size.z {
        return false;
    }
    let v = me.state.origin - them.state.origin;
    let distance = Vec3f::new(v.x, v.y, 0.0).length();
    if distance < 100.0 {
        return false;
    }
    if distance > 100.0 && world.random() < 0.9 {
        return false;
    }
    true
}

/// Melee in reach, else maybe leap. C ref: `mutant_checkattack`.
pub fn mutant_checkattack(world: &mut GameWorld, key: EntityKey) -> bool {
    let Some(enemy) = world.entities.get(key).and_then(|e| e.game.enemy) else {
        return false;
    };
    if world.entities.get(enemy).is_none_or(|e| e.game.health <= 0) {
        return false;
    }
    let state = if mutant_check_melee(world, key, enemy) {
        AttackState::Melee
    } else if mutant_check_jump(world, key, enemy) {
        AttackState::Missile
    } else {
        return false;
    };
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.attack_state = state;
    }
    true
}

/// C ref: `mutant_pain`.
pub fn mutant_pain(
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
        (SOUND_PAIN1, &MUTANT_MOVE_PAIN1)
    } else if r < 0.66 {
        (SOUND_PAIN2, &MUTANT_MOVE_PAIN2)
    } else {
        (SOUND_PAIN1, &MUTANT_MOVE_PAIN3)
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
    set_move(world, key, mv);
}

/// The corpse box, then maybe flies. Unlike the other `*_dead`, C leaves
/// `nextthink` alone, so the corpse keeps thinking and rerolls
/// `M_FlyCheck` each frame until the flies come. C ref: `mutant_dead`.
pub fn mutant_dead(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    ent.game.movetype = MoveType::Toss;
    ent.svflags |= SvFlags::DEADMONSTER.bits();
    world.link_entity(key);
    ai::m_fly_check(world, key);
}

/// Gib at `gib_health`, else fall one of two ways. C ref: `mutant_die`.
pub fn mutant_die(
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
    sound(world, key, CHAN_VOICE, SOUND_DEATH, ATTN_NORM);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
        ent.state.skinnum = 1;
    }
    if world.random() < 0.5 {
        set_move(world, key, &MUTANT_MOVE_DEATH1);
    } else {
        set_move(world, key, &MUTANT_MOVE_DEATH2);
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_mutant`; `monster_start`
/// (run by `call_spawn`) removes it in deathmatch.
pub fn sp_monster_mutant(
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
        .unwrap_or_else(|| "monster_mutant".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = MINS;
    ent.maxs = MAXS;
    ent.game.health = HEALTH;
    ent.game.gib_health = -120;
    ent.game.mass = MASS;
    ent.pain = Some(mutant_pain);
    ent.die = Some(mutant_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(mutant_stand),
        walk: Some(mutant_walk),
        run: Some(mutant_run),
        dodge: None,
        attack: Some(mutant_jump),
        melee: Some(mutant_melee),
        sight: Some(mutant_sight),
        search: Some(mutant_search),
        idle: Some(mutant_idle),
        checkattack: Some(mutant_checkattack),
        currentmove: Some(MUTANT_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_mutant`: its sound precaches.
pub fn sp_monster_mutant_finish(
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
    use crate::monster::test_support::{floor_world, walker};
    use crate::monster::test_support::{monster_arena, run, sounds};
    use crate::world::{test_world, test_world_recording};

    type CFrame = (AnimFrameFn, f32, Option<ThinkFn>);
    type CMove = (&'static MonsterMove, i32, i32, Vec<CFrame>, Option<ThinkFn>);

    /// (move, firstframe, lastframe, [(aifunc, dist, thinkfunc)], endfunc) from C.
    #[rustfmt::skip]
    fn c_moves() -> Vec<CMove> {
        vec![
            (&MUTANT_MOVE_STAND, 62, 112, vec![
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
            (&MUTANT_MOVE_IDLE, 113, 125, vec![
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, Some(mutant_idle_loop as ThinkFn)),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
            ], Some(mutant_stand as ThinkFn)),
            (&MUTANT_MOVE_WALK, 130, 141, vec![
                (ai::ai_walk as AnimFrameFn, 3.0, Some(mutant_step as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 1.0, None),
                (ai::ai_walk as AnimFrameFn, 5.0, Some(mutant_step as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 10.0, None),
                (ai::ai_walk as AnimFrameFn, 13.0, None),
                (ai::ai_walk as AnimFrameFn, 10.0, None),
                (ai::ai_walk as AnimFrameFn, 0.0, Some(mutant_step as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, 6.0, None),
                (ai::ai_walk as AnimFrameFn, 16.0, None),
                (ai::ai_walk as AnimFrameFn, 15.0, None),
                (ai::ai_walk as AnimFrameFn, 6.0, None),
            ], None),
            (&MUTANT_MOVE_START_WALK, 126, 129, vec![
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, 5.0, None),
                (ai::ai_walk as AnimFrameFn, -2.0, Some(mutant_step as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 1.0, None),
            ], Some(mutant_walk_loop as ThinkFn)),
            (&MUTANT_MOVE_RUN, 56, 61, vec![
                (ai::ai_run as AnimFrameFn, 40.0, None),
                (ai::ai_run as AnimFrameFn, 40.0, Some(mutant_step as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 24.0, None),
                (ai::ai_run as AnimFrameFn, 5.0, Some(mutant_step as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 17.0, None),
                (ai::ai_run as AnimFrameFn, 10.0, None),
            ], None),
            (&MUTANT_MOVE_ATTACK, 8, 14, vec![
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(mutant_hit_left as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(mutant_hit_right as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(mutant_check_refire as ThinkFn)),
            ], Some(mutant_run as ThinkFn)),
            (&MUTANT_MOVE_JUMP, 0, 7, vec![
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 17.0, None),
                (ai::ai_charge as AnimFrameFn, 15.0, Some(mutant_jump_takeoff as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 15.0, None),
                (ai::ai_charge as AnimFrameFn, 15.0, Some(mutant_check_landing as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 3.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
            ], Some(mutant_run as ThinkFn)),
            (&MUTANT_MOVE_PAIN1, 34, 38, vec![
                (ai::ai_move as AnimFrameFn, 4.0, None),
                (ai::ai_move as AnimFrameFn, -3.0, None),
                (ai::ai_move as AnimFrameFn, -8.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 5.0, None),
            ], Some(mutant_run as ThinkFn)),
            (&MUTANT_MOVE_PAIN2, 39, 44, vec![
                (ai::ai_move as AnimFrameFn, -24.0, None),
                (ai::ai_move as AnimFrameFn, 11.0, None),
                (ai::ai_move as AnimFrameFn, 5.0, None),
                (ai::ai_move as AnimFrameFn, -2.0, None),
                (ai::ai_move as AnimFrameFn, 6.0, None),
                (ai::ai_move as AnimFrameFn, 4.0, None),
            ], Some(mutant_run as ThinkFn)),
            (&MUTANT_MOVE_PAIN3, 45, 55, vec![
                (ai::ai_move as AnimFrameFn, -22.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
                (ai::ai_move as AnimFrameFn, 6.0, None),
                (ai::ai_move as AnimFrameFn, 3.0, None),
                (ai::ai_move as AnimFrameFn, 2.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 1.0, None),
            ], Some(mutant_run as ThinkFn)),
            (&MUTANT_MOVE_DEATH1, 15, 23, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(mutant_dead as ThinkFn)),
            (&MUTANT_MOVE_DEATH2, 24, 33, vec![
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
            ], Some(mutant_dead as ThinkFn)),
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
    fn mutant_moves_match_c_frame_by_frame() {
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
        sp_monster_mutant(&mut world.entities, key, &HashMap::new());
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

    /// A monster enemy at (`x`, 0, `z`), a player-sized box.
    fn enemy_at(world: &mut GameWorld, key: EntityKey, x: f32, z: f32) -> EntityKey {
        let enemy = world.spawn().unwrap();
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = Vec3f::new(x, 0.0, z);
        e.mins = Vec3f::new(-16.0, -16.0, -24.0);
        e.maxs = Vec3f::new(16.0, 16.0, 32.0);
        e.game.health = 100;
        e.game.takedamage = TakeDamage::Yes;
        e.svflags |= SvFlags::MONSTER.bits();
        world.link_entity(enemy);
        world.link_entity(key);
        world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
        enemy
    }

    #[test]
    fn mutant_spawns_with_its_c_pain_walk_run_and_melee_callbacks() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(ent.pain.is_some());
        assert!(mi.walk.is_some() && mi.run.is_some() && mi.melee.is_some());
    }

    #[test]
    fn mutant_spawns_with_the_c_values_and_callbacks() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "monster_mutant");
        assert_eq!((ent.game.health, ent.game.mass), (300, 300));
        assert_eq!((ent.mins, ent.maxs), (MINS, MAXS));
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert_eq!(mi.scale, 1.0);
        assert!(mi.dodge.is_none());
        assert!(std::ptr::fn_addr_eq(
            mi.attack.unwrap(),
            mutant_jump as ThinkFn
        ));
        assert!(std::ptr::fn_addr_eq(
            mi.idle.unwrap(),
            mutant_idle as ThinkFn
        ));
        assert!(std::ptr::fn_addr_eq(
            mi.checkattack.unwrap(),
            mutant_checkattack as crate::entity::MonsterCheckAttackFn
        ));
        assert!(mi.sight.is_some() && mi.search.is_some());
        assert!(is_move(&world, key, &MUTANT_MOVE_STAND));
    }

    #[test]
    fn mutant_finish_precaches_the_c_sounds_and_the_spawn_row_uses_it() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        let row = crate::spawn::find_spawn("monster_mutant").unwrap();
        assert_eq!(row.2, Some("models/monsters/mutant/tris.md2"));
        row.3.unwrap()(&mut world, key, &HashMap::new());
        assert_eq!(*gi.sounds.borrow(), PRECACHE);
    }

    #[test]
    fn mutant_step_plays_the_step_sound_after_randk_plus_one_mod_three() {
        for (n, name) in [(0, SOUND_STEP1), (1, SOUND_STEP2), (2, SOUND_STEP3)] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            seed_randk(&mut world, |r| r.wrapping_add(1) % 3 == n);
            mutant_step(&mut world, key);
            assert_eq!(sounds(&gi), [(CHAN_VOICE, name.to_string(), ATTN_NORM)]);
        }
    }

    #[test]
    fn mutant_sight_search_swing_and_idle_play_their_voice_sounds() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        mutant_sight(&mut world, key, key);
        mutant_search(&mut world, key);
        mutant_swing(&mut world, key);
        mutant_idle(&mut world, key);
        assert!(is_move(&world, key, &MUTANT_MOVE_IDLE));
        assert_eq!(
            sounds(&gi),
            [
                (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_SEARCH.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_SWING.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_IDLE.to_string(), ATTN_IDLE),
            ]
        );
    }

    #[test]
    fn mutant_idle_loop_rescratches_three_times_in_four() {
        let mut world = test_world();
        let key = spawned(&mut world);
        seed_random(&mut world, |r| r < 0.75);
        mutant_idle_loop(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, FRAME_STAND155);
        mi_mut(&mut world, key).nextframe = 0;
        seed_random(&mut world, |r| r >= 0.75);
        mutant_idle_loop(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, 0);
    }

    #[test]
    fn mutant_walk_starts_then_loops_and_run_obeys_stand_ground() {
        let mut world = test_world();
        let key = spawned(&mut world);
        mutant_walk(&mut world, key);
        assert!(is_move(&world, key, &MUTANT_MOVE_START_WALK));
        mutant_walk_loop(&mut world, key);
        assert!(is_move(&world, key, &MUTANT_MOVE_WALK));
        mutant_run(&mut world, key);
        assert!(is_move(&world, key, &MUTANT_MOVE_RUN));
        mutant_melee(&mut world, key);
        assert!(is_move(&world, key, &MUTANT_MOVE_ATTACK));
        mutant_jump(&mut world, key);
        assert!(is_move(&world, key, &MUTANT_MOVE_JUMP));
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        mutant_run(&mut world, key);
        assert!(is_move(&world, key, &MUTANT_MOVE_STAND));
    }

    #[test]
    fn mutant_claws_hit_for_10_to_14_with_their_own_hit_sounds() {
        for (claw, name) in [
            (mutant_hit_left as ThinkFn, SOUND_HIT),
            (mutant_hit_right as ThinkFn, SOUND_HIT2),
        ] {
            for k in 0..5 {
                let (mut world, gi) = test_world_recording();
                let key = spawned(&mut world);
                let enemy = enemy_at(&mut world, key, 40.0, 0.0);
                *gi.next_trace.borrow_mut() = Some(Trace {
                    fraction: 0.5,
                    ent_index: world.number_of(enemy),
                    ..Trace::default()
                });
                seed_randk(&mut world, |r| r % 5 == k);
                claw(&mut world, key);
                let health = world.entities.get(enemy).unwrap().game.health;
                assert_eq!(health, 100 - (10 + k as i32));
                assert_eq!(sounds(&gi), [(CHAN_WEAPON, name.to_string(), ATTN_NORM)]);
            }
        }
    }

    #[test]
    fn mutant_claw_out_of_reach_plays_the_swing_sound() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        enemy_at(&mut world, key, 200.0, 0.0);
        mutant_hit_left(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [(CHAN_WEAPON, SOUND_SWING.to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn mutant_refires_at_an_enemy_in_reach_or_on_even_odds_in_nightmare() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let enemy = enemy_at(&mut world, key, 60.0, 0.0);
        mutant_check_refire(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, FRAME_ATTACK09);

        mi_mut(&mut world, key).nextframe = 0;
        world.entities.get_mut(enemy).unwrap().state.origin.x = 300.0;
        world.link_entity(enemy);
        mutant_check_refire(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, 0);

        world.game.skill = 3;
        seed_random(&mut world, |r| r >= 0.5);
        mutant_check_refire(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, 0);
        seed_random(&mut world, |r| r < 0.5);
        mutant_check_refire(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, FRAME_ATTACK09);
    }

    #[test]
    fn mutant_never_refires_at_a_dead_or_missing_enemy() {
        let mut world = test_world();
        let key = spawned(&mut world);
        mutant_check_refire(&mut world, key);
        let enemy = enemy_at(&mut world, key, 60.0, 0.0);
        world.entities.get_mut(enemy).unwrap().game.health = 0;
        mutant_check_refire(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, 0);
    }

    #[test]
    fn mutant_jump_takeoff_leaps_forward_ducked_for_three_seconds() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.level.time = 4.0;
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.angles.y = 90.0;
        ent.game.ground_entity = Some(key);
        mutant_jump_takeoff(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.origin.z, 1.0);
        assert!((ent.velocity - Vec3f::new(0.0, 600.0, 250.0)).length() < 0.01);
        assert!(ent.game.ground_entity.is_none());
        assert!(ent.touch.is_some());
        assert!(mi(&world, key).aiflags.contains(AiFlags::DUCKED));
        assert_eq!(mi(&world, key).attack_finished, 7.0);
        assert_eq!(
            sounds(&gi),
            [(CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn mutant_landing_thuds_and_clears_the_leap() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        mi_mut(&mut world, key).aiflags = AiFlags::DUCKED;
        mi_mut(&mut world, key).attack_finished = 9.0;
        world.entities.get_mut(key).unwrap().game.ground_entity = Some(key);
        mutant_check_landing(&mut world, key);
        assert_eq!(mi(&world, key).attack_finished, 0.0);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::DUCKED));
        assert_eq!(
            sounds(&gi),
            [(CHAN_WEAPON, SOUND_THUD.to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn mutant_in_the_air_holds_the_fall_until_the_leap_times_out() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.level.time = 5.0;
        mi_mut(&mut world, key).attack_finished = 5.0;
        mutant_check_landing(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, FRAME_ATTACK05);
        world.level.time = 5.1;
        mutant_check_landing(&mut world, key);
        assert_eq!(mi(&world, key).nextframe, FRAME_ATTACK02);
    }

    #[test]
    fn mutant_leap_landing_on_a_target_over_400_hits_it_for_40_to_50() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        sp_monster_mutant(&mut world.entities, key, &HashMap::new());
        let enemy = enemy_at(&mut world, key, 60.0, 24.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.velocity = Vec3f::new(401.0, 0.0, 0.0);
        ent.touch = Some(mutant_jump_touch);
        seed_random(&mut world, |r| r > 0.99 && r < 1.0);
        mutant_jump_touch(&mut world, key, enemy, None, None);
        assert_eq!(world.entities.get(enemy).unwrap().game.health, 100 - 49);
        // Standing on the floor: the leap is over.
        assert!(world.entities.get(key).unwrap().touch.is_none());
    }

    #[test]
    fn mutant_leap_touch_at_400_or_on_an_undamageable_entity_hurts_nothing() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        sp_monster_mutant(&mut world.entities, key, &HashMap::new());
        let enemy = enemy_at(&mut world, key, 60.0, 24.0);
        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(400.0, 0.0, 0.0);
        mutant_jump_touch(&mut world, key, enemy, None, None);
        world.entities.get_mut(key).unwrap().velocity = Vec3f::new(900.0, 0.0, 0.0);
        world.entities.get_mut(enemy).unwrap().game.takedamage = TakeDamage::No;
        mutant_jump_touch(&mut world, key, enemy, None, None);
        assert_eq!(world.entities.get(enemy).unwrap().game.health, 100);
    }

    #[test]
    fn mutant_leap_touch_over_a_ledge_restarts_the_leap_only_with_ground() {
        // MockGameImport traces hit nothing: no bottom under the mutant.
        let mut world = test_world();
        let key = spawned(&mut world);
        let other = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().touch = Some(mutant_jump_touch);
        mutant_jump_touch(&mut world, key, other, None, None);
        assert!(world.entities.get(key).unwrap().touch.is_some());
        assert_eq!(mi(&world, key).nextframe, 0);

        world.entities.get_mut(key).unwrap().game.ground_entity = Some(other);
        mutant_jump_touch(&mut world, key, other, None, None);
        assert!(world.entities.get(key).unwrap().touch.is_none());
        assert_eq!(mi(&world, key).nextframe, FRAME_ATTACK02);
    }

    #[test]
    fn mutant_leap_touch_when_dead_just_stops_touching() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let other = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = 0;
        ent.game.ground_entity = Some(other);
        ent.touch = Some(mutant_jump_touch);
        mutant_jump_touch(&mut world, key, other, None, None);
        assert!(world.entities.get(key).unwrap().touch.is_none());
        assert_eq!(mi(&world, key).nextframe, 0);
    }

    #[test]
    fn mutant_checkattack_melees_in_reach_and_needs_a_live_enemy() {
        let mut world = test_world();
        let key = spawned(&mut world);
        assert!(!mutant_checkattack(&mut world, key));
        let enemy = enemy_at(&mut world, key, 60.0, 0.0);
        assert!(mutant_checkattack(&mut world, key));
        assert_eq!(mi(&world, key).attack_state, AttackState::Melee);
        world.entities.get_mut(enemy).unwrap().game.health = 0;
        assert!(!mutant_checkattack(&mut world, key));
    }

    #[test]
    fn mutant_checkattack_leaps_one_time_in_ten_beyond_100_units() {
        let mut world = test_world();
        let key = spawned(&mut world);
        enemy_at(&mut world, key, 300.0, 0.0);
        seed_random(&mut world, |r| r < 0.9);
        assert!(!mutant_checkattack(&mut world, key));
        seed_random(&mut world, |r| r >= 0.9);
        assert!(mutant_checkattack(&mut world, key));
        assert_eq!(mi(&world, key).attack_state, AttackState::Missile);
    }

    #[test]
    fn mutant_checkattack_always_leaps_at_exactly_100_units() {
        let mut world = test_world();
        let key = spawned(&mut world);
        // 100 units is beyond C melee range (80) but not over 100.
        enemy_at(&mut world, key, 100.0, 0.0);
        seed_random(&mut world, |r| r < 0.9);
        assert!(mutant_checkattack(&mut world, key));
        assert_eq!(mi(&world, key).attack_state, AttackState::Missile);
    }

    #[test]
    fn mutant_never_leaps_at_an_enemy_far_above_or_below_or_closer_than_100() {
        for (x, z) in [(300.0, -200.0), (300.0, 200.0), (99.0, 0.0)] {
            let mut world = test_world();
            let key = spawned(&mut world);
            let enemy = enemy_at(&mut world, key, x, z);
            seed_random(&mut world, |r| r >= 0.9);
            assert!(!mutant_check_jump(&mut world, key, enemy), "{x} {z}");
        }
    }

    #[test]
    fn mutant_pain_picks_by_roll_and_debounces_three_seconds() {
        for (rolls, name, mv) in [
            (0.0_f32..0.33, SOUND_PAIN1, &MUTANT_MOVE_PAIN1),
            (0.33..0.66, SOUND_PAIN2, &MUTANT_MOVE_PAIN2),
            (0.66..1.01, SOUND_PAIN1, &MUTANT_MOVE_PAIN3),
        ] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            world.level.time = 2.0;
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.max_health = 300;
            ent.game.health = 149;
            seed_random(&mut world, |r| rolls.contains(&r));
            mutant_pain(&mut world, key, key, 0.0, 10);
            assert_eq!(sounds(&gi), [(CHAN_VOICE, name.to_string(), ATTN_NORM)]);
            assert!(is_move(&world, key, mv));
            let ent = world.entities.get(key).unwrap();
            assert_eq!((ent.state.skinnum, ent.game.pain_debounce_time), (1, 5.0));

            mutant_pain(&mut world, key, key, 0.0, 10);
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn mutant_pain_in_nightmare_is_silent_and_still() {
        let (mut world, gi) = test_world_recording();
        world.game.skill = 3;
        let key = spawned(&mut world);
        mutant_pain(&mut world, key, key, 0.0, 50);
        assert!(sounds(&gi).is_empty());
        assert!(is_move(&world, key, &MUTANT_MOVE_STAND));
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            3.0
        );
    }

    #[test]
    fn mutant_at_its_c_gib_health_throws_six_gibs() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -120;
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
    fn mutant_above_gib_health_falls_one_of_two_ways_once_with_the_dead_skin() {
        for (low, mv) in [(true, &MUTANT_MOVE_DEATH1), (false, &MUTANT_MOVE_DEATH2)] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            world.entities.get_mut(key).unwrap().game.health = -119;
            seed_random(&mut world, |r| (r < 0.5) == low);
            mutant_die(&mut world, key, key, key, 100, Vec3f::ZERO);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.deadflag, DeadFlag::Dead);
            assert_eq!(ent.game.takedamage, TakeDamage::Yes);
            assert_eq!(ent.state.skinnum, 1);
            assert!(is_move(&world, key, mv));
            assert_eq!(
                sounds(&gi),
                [(CHAN_VOICE, SOUND_DEATH.to_string(), ATTN_NORM)]
            );
            mutant_die(&mut world, key, key, key, 10, Vec3f::ZERO);
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn mutant_dead_keeps_thinking_until_the_fly_check_brings_flies() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.level.time = 2.0;
        world.entities.get_mut(key).unwrap().game.nextthink = 2.1;
        seed_random(&mut world, |r| r > 0.5);
        mutant_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
        assert_eq!(ent.game.nextthink, 2.1);

        seed_random(&mut world, |r| r <= 0.5);
        mutant_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::m_flies_on as ThinkFn
        ));
        assert!(ent.game.nextthink >= 7.0);
    }

    #[test]
    fn mutant_sights_a_player_and_leaps_or_claws_it() {
        let (mut world, gi, key, player) =
            monster_arena(sp_monster_mutant, sp_monster_mutant_finish, 300.0);
        let (mut angry, mut attacked) = (false, false);
        run(&mut world, key, 80, |w| {
            angry |= w.entities.get(key).unwrap().game.enemy == Some(player);
            attacked |= is_move(w, key, &MUTANT_MOVE_ATTACK) || is_move(w, key, &MUTANT_MOVE_JUMP);
        });
        assert!(angry && attacked, "angry {angry} attacked {attacked}");
        let played = sounds(&gi.floor.inner);
        assert_eq!(played[0], (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM));
        assert!(world.entities.get(player).unwrap().game.health < 100);
    }
}
