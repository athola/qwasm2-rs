//! Infantry monster: machinegun at range, a rifle-butt smack in melee,
//! and a death that sprays the machinegun.
//!
//! C ref: `~/Qwasm2/src/game/monster/infantry/` (infantry.c, infantry.h).

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
// Animation frames. C ref: infantry.h (`FRAME_*`, model frame numbers).
// ---------------------------------------------------------------------------

const FRAME_STAND01: i32 = 1;
const FRAME_STAND49: i32 = 49;
const FRAME_STAND50: i32 = 50;
const FRAME_STAND71: i32 = 71;
const FRAME_WALK03: i32 = 74;
const FRAME_WALK14: i32 = 85;
const FRAME_RUN01: i32 = 92;
const FRAME_RUN08: i32 = 99;
const FRAME_PAIN101: i32 = 100;
const FRAME_PAIN110: i32 = 109;
const FRAME_PAIN201: i32 = 110;
const FRAME_PAIN210: i32 = 119;
const FRAME_DUCK01: i32 = 120;
const FRAME_DUCK05: i32 = 124;
const FRAME_DEATH101: i32 = 125;
const FRAME_DEATH120: i32 = 144;
const FRAME_DEATH201: i32 = 145;
const FRAME_DEATH225: i32 = 169;
const FRAME_DEATH301: i32 = 170;
const FRAME_DEATH309: i32 = 178;
const FRAME_ATTAK101: i32 = 184;
const FRAME_ATTAK115: i32 = 198;
const FRAME_ATTAK201: i32 = 199;
const FRAME_ATTAK208: i32 = 206;
const FRAME_DEATH211: i32 = 155;
const FRAME_ATTAK111: i32 = 194;

/// C model scale, multiplied into every frame's distance.
const MODEL_SCALE: f32 = 1.0;

// C SP_monster_infantry values.
const HEALTH: i32 = 100;
const MINS: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const MAXS: Vec3f = Vec3f::new(16.0, 16.0, 32.0);
const MASS: i32 = 200;

/// C cvar `g_monsterfootsteps`, which defaults to "0"; the game has no
/// cvar access, so footsteps stay off as in a default C game.
const G_MONSTERFOOTSTEPS: bool = false;

const SOUND_PAIN1: &str = "infantry/infpain1.wav";
const SOUND_PAIN2: &str = "infantry/infpain2.wav";
const SOUND_DIE1: &str = "infantry/infdeth1.wav";
const SOUND_DIE2: &str = "infantry/infdeth2.wav";
const SOUND_GUNSHOT: &str = "infantry/infatck1.wav";
const SOUND_WEAPON_COCK: &str = "infantry/infatck3.wav";
const SOUND_PUNCH_SWING: &str = "infantry/infatck2.wav";
const SOUND_PUNCH_HIT: &str = "infantry/melee2.wav";
const SOUND_SIGHT: &str = "infantry/infsght1.wav";
const SOUND_SEARCH: &str = "infantry/infsrch1.wav";
const SOUND_IDLE: &str = "infantry/infidle1.wav";
const SOUND_STEPS: [&str; 2] = ["infantry/step1.wav", "infantry/step2.wav"];

// ---------------------------------------------------------------------------
// Moves. C ref: infantry.c `infantry_frames_*` / `infantry_move_*`, one
// entry per model frame in C order.
// ---------------------------------------------------------------------------

static INFANTRY_FRAMES_STAND: [MonsterFrame; 22] = [frame(ai::ai_stand, 0.0, None); 22];
static INFANTRY_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND50,
    lastframe: FRAME_STAND71,
    frames: &INFANTRY_FRAMES_STAND,
    endfunc: None,
};

static INFANTRY_FRAMES_FIDGET: [MonsterFrame; 49] = [
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 3.0, None),
    frame(ai::ai_stand, 6.0, None),
    frame(ai::ai_stand, 3.0, Some(infantry_footstep)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -2.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, -1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -1.0, None),
    frame(ai::ai_stand, -1.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, -3.0, None),
    frame(ai::ai_stand, -2.0, None),
    frame(ai::ai_stand, -3.0, None),
    frame(ai::ai_stand, -3.0, Some(infantry_footstep)),
    frame(ai::ai_stand, -2.0, None),
];
static INFANTRY_MOVE_FIDGET: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND49,
    frames: &INFANTRY_FRAMES_FIDGET,
    endfunc: Some(infantry_stand),
};

static INFANTRY_FRAMES_WALK: [MonsterFrame; 12] = [
    frame(ai::ai_walk, 5.0, Some(infantry_footstep)),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 5.0, None),
    frame(ai::ai_walk, 6.0, Some(infantry_footstep)),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 4.0, None),
    frame(ai::ai_walk, 5.0, None),
];
static INFANTRY_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALK03,
    lastframe: FRAME_WALK14,
    frames: &INFANTRY_FRAMES_WALK,
    endfunc: None,
};

static INFANTRY_FRAMES_RUN: [MonsterFrame; 8] = [
    frame(ai::ai_run, 10.0, None),
    frame(ai::ai_run, 20.0, Some(infantry_footstep)),
    frame(ai::ai_run, 5.0, None),
    frame(ai::ai_run, 7.0, None),
    frame(ai::ai_run, 30.0, None),
    frame(ai::ai_run, 35.0, Some(infantry_footstep)),
    frame(ai::ai_run, 2.0, None),
    frame(ai::ai_run, 6.0, None),
];
static INFANTRY_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN01,
    lastframe: FRAME_RUN08,
    frames: &INFANTRY_FRAMES_RUN,
    endfunc: None,
};

static INFANTRY_FRAMES_PAIN1: [MonsterFrame; 10] = [
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, -1.0, Some(infantry_footstep)),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 6.0, None),
    frame(ai::ai_move, 2.0, Some(infantry_footstep)),
];
static INFANTRY_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN110,
    frames: &INFANTRY_FRAMES_PAIN1,
    endfunc: Some(infantry_run),
};

static INFANTRY_FRAMES_PAIN2: [MonsterFrame; 10] = [
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -2.0, Some(infantry_footstep)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, 2.0, Some(infantry_footstep)),
];
static INFANTRY_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN210,
    frames: &INFANTRY_FRAMES_PAIN2,
    endfunc: Some(infantry_run),
};

static INFANTRY_FRAMES_DEATH1: [MonsterFrame; 20] = [
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -4.0, Some(infantry_footstep)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, Some(infantry_footstep)),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 9.0, None),
    frame(ai::ai_move, 9.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -3.0, None),
];
static INFANTRY_MOVE_DEATH1: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH120,
    frames: &INFANTRY_FRAMES_DEATH1,
    endfunc: Some(infantry_dead),
};

static INFANTRY_FRAMES_DEATH2: [MonsterFrame; 25] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 1.0, Some(infantry_footstep)),
    frame(ai::ai_move, 1.0, Some(infantry_footstep)),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -2.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -3.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -1.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -2.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, 0.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, 2.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, 2.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, 3.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -10.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -7.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -8.0, Some(infantry_machine_gun)),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 0.0, None),
];
static INFANTRY_MOVE_DEATH2: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH201,
    lastframe: FRAME_DEATH225,
    frames: &INFANTRY_FRAMES_DEATH2,
    endfunc: Some(infantry_dead),
};

static INFANTRY_FRAMES_DEATH3: [MonsterFrame; 9] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -11.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, -11.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];
static INFANTRY_MOVE_DEATH3: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH301,
    lastframe: FRAME_DEATH309,
    frames: &INFANTRY_FRAMES_DEATH3,
    endfunc: Some(infantry_dead),
};

static INFANTRY_FRAMES_DUCK: [MonsterFrame; 5] = [
    frame(ai::ai_move, -2.0, Some(infantry_duck_down)),
    frame(ai::ai_move, -5.0, Some(infantry_duck_hold)),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 4.0, Some(infantry_duck_up)),
    frame(ai::ai_move, 0.0, Some(infantry_footstep)),
];
static INFANTRY_MOVE_DUCK: MonsterMove = MonsterMove {
    firstframe: FRAME_DUCK01,
    lastframe: FRAME_DUCK05,
    frames: &INFANTRY_FRAMES_DUCK,
    endfunc: Some(infantry_run),
};

static INFANTRY_FRAMES_ATTACK1: [MonsterFrame; 15] = [
    frame(ai::ai_charge, 4.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, 0.0, Some(infantry_cock_gun)),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 1.0, None),
    frame(ai::ai_charge, 2.0, None),
    frame(ai::ai_charge, -2.0, None),
    frame(ai::ai_charge, -3.0, None),
    frame(ai::ai_charge, 1.0, Some(infantry_fire)),
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, -1.0, None),
    frame(ai::ai_charge, -2.0, None),
    frame(ai::ai_charge, -3.0, None),
];
static INFANTRY_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK115,
    frames: &INFANTRY_FRAMES_ATTACK1,
    endfunc: Some(infantry_run),
};

static INFANTRY_FRAMES_ATTACK2: [MonsterFrame; 8] = [
    frame(ai::ai_charge, 3.0, None),
    frame(ai::ai_charge, 6.0, None),
    frame(ai::ai_charge, 0.0, Some(infantry_swing)),
    frame(ai::ai_charge, 8.0, Some(infantry_footstep)),
    frame(ai::ai_charge, 5.0, None),
    frame(ai::ai_charge, 8.0, Some(infantry_smack)),
    frame(ai::ai_charge, 6.0, None),
    frame(ai::ai_charge, 3.0, None),
];
static INFANTRY_MOVE_ATTACK2: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK208,
    frames: &INFANTRY_FRAMES_ATTACK2,
    endfunc: Some(infantry_run),
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
// Stand, walk, run
// ---------------------------------------------------------------------------

/// C ref: `infantry_footstep`; silent unless `g_monsterfootsteps` is set.
pub fn infantry_footstep(world: &mut GameWorld, key: EntityKey) {
    if !G_MONSTERFOOTSTEPS {
        return;
    }
    let step = SOUND_STEPS[(world.random_u32() % 2) as usize];
    sound(world, key, CHAN_BODY, step, ATTN_NORM);
}

/// C ref: `infantry_stand`.
pub fn infantry_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &INFANTRY_MOVE_STAND);
}

/// The idle callback: fidget and mutter. C ref: `infantry_fidget`.
pub fn infantry_fidget(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &INFANTRY_MOVE_FIDGET);
    sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
}

/// C ref: `infantry_walk`.
pub fn infantry_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &INFANTRY_MOVE_WALK);
}

/// C ref: `infantry_run`.
pub fn infantry_run(world: &mut GameWorld, key: EntityKey) {
    let stand_ground =
        monsterinfo(world, key).is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    if stand_ground {
        set_move(world, key, &INFANTRY_MOVE_STAND);
    } else {
        set_move(world, key, &INFANTRY_MOVE_RUN);
    }
}

// ---------------------------------------------------------------------------
// Pain
// ---------------------------------------------------------------------------

/// Bloody skin below half health; then, at most every 3 s, one of two
/// flinches (none on nightmare). C ref: `infantry_pain`.
pub fn infantry_pain(
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
    if world.random_u32().is_multiple_of(2) {
        set_move(world, key, &INFANTRY_MOVE_PAIN1);
        sound(world, key, CHAN_VOICE, SOUND_PAIN1, ATTN_NORM);
    } else {
        set_move(world, key, &INFANTRY_MOVE_PAIN2);
        sound(world, key, CHAN_VOICE, SOUND_PAIN2, ATTN_NORM);
    }
}

// ---------------------------------------------------------------------------
// Attacks
// ---------------------------------------------------------------------------

/// How far (pitch, yaw) above and right of its facing each dying burst
/// fires, frame by frame from `FRAME_death211`: C subtracts them from
/// `s.angles`, and a negative pitch looks up. C ref: `aimangles`.
const AIMANGLES: [[f32; 3]; 12] = [
    [0.0, 5.0, 0.0],
    [10.0, 15.0, 0.0],
    [20.0, 25.0, 0.0],
    [25.0, 35.0, 0.0],
    [30.0, 40.0, 0.0],
    [30.0, 45.0, 0.0],
    [25.0, 50.0, 0.0],
    [20.0, 40.0, 0.0],
    [15.0, 35.0, 0.0],
    [40.0, 35.0, 0.0],
    [70.0, 35.0, 0.0],
    [90.0, 35.0, 0.0],
];

/// One machinegun bullet: in the attack, at where the enemy was 0.2 s
/// ago (ahead without one); while dying, along the frame's sweep.
/// C ref: `InfantryMachineGun`.
pub fn infantry_machine_gun(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (frame, angles) = (ent.state.frame, ent.state.angles);
    let (flash_number, start, forward) = if frame == FRAME_ATTAK111 {
        let flash_number = MZ2_INFANTRY_MACHINEGUN_1;
        let Some((start, forward, _)) = flash::monster_flash_source(world, key, flash_number)
        else {
            return;
        };
        let target =
            ent.game.enemy.and_then(|e| world.entities.get(e)).map(|e| {
                e.state.origin + e.velocity * -0.2 + Vec3f::new(0.0, 0.0, e.game.viewheight)
            });
        let forward = match target {
            Some(target) => (target - start).normalize_or_zero(),
            None => forward,
        };
        (flash_number, start, forward)
    } else {
        let flash_number = MZ2_INFANTRY_MACHINEGUN_2 + (frame - FRAME_DEATH211);
        let Some(aim) = AIMANGLES.get((flash_number - MZ2_INFANTRY_MACHINEGUN_2) as usize) else {
            return; // C only calls this on attak111 and death211..222
        };
        let Some((start, _, _)) = flash::monster_flash_source(world, key, flash_number) else {
            return;
        };
        let (forward, _, _) = crate::physics::angle_vectors(angles - Vec3f::from(*aim));
        (flash_number, start, forward)
    };
    fire::monster_fire_bullet(
        world,
        key,
        start,
        forward,
        3,
        4,
        DEFAULT_BULLET_HSPREAD,
        DEFAULT_BULLET_VSPREAD,
        flash_number,
    );
}

/// C ref: `infantry_sight`.
pub fn infantry_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_BODY, SOUND_SIGHT, ATTN_NORM);
}

/// The corpse box, then the flies. C ref: `infantry_dead`.
/// Unlike `ai::monster_dead` it leaves `nextthink` alone, so a failed
/// fly check is retried on the next frame.
pub fn infantry_dead(world: &mut GameWorld, key: EntityKey) {
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

/// Gib when hit hard enough, else one of three deaths. The corpse stays
/// damageable so it can still be gibbed. C ref: `infantry_die`.
pub fn infantry_die(
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
        ent.state.skinnum = 1; // bloody skin
    }
    let (mv, cry) = match world.random_u32() % 3 {
        0 => (&INFANTRY_MOVE_DEATH1, SOUND_DIE2),
        1 => (&INFANTRY_MOVE_DEATH2, SOUND_DIE1),
        _ => (&INFANTRY_MOVE_DEATH3, SOUND_DIE2),
    };
    set_move(world, key, mv);
    sound(world, key, CHAN_VOICE, cry, ATTN_NORM);
}

/// Crouch: 32 units shorter and hittable but no longer aimed at, for a
/// second. C ref: `infantry_duck_down`.
pub fn infantry_duck_down(world: &mut GameWorld, key: EntityKey) {
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

/// C ref: `infantry_duck_hold`.
pub fn infantry_duck_hold(world: &mut GameWorld, key: EntityKey) {
    hold_until_pausetime(world, key);
}

/// C ref: `infantry_duck_up`.
pub fn infantry_duck_up(world: &mut GameWorld, key: EntityKey) {
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

/// Duck one shot in four, turning on an attacker it had not noticed.
/// C ref: `infantry_dodge`.
pub fn infantry_dodge(world: &mut GameWorld, key: EntityKey, attacker: EntityKey, _eta: f32) {
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
    set_move(world, key, &INFANTRY_MOVE_DUCK);
}

/// Cock the gun and pick a burst of 10-25 frames.
/// C ref: `infantry_cock_gun`.
pub fn infantry_cock_gun(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_WEAPON_COCK, ATTN_NORM);
    let n = (world.random_u32() & 15) + 3 + 7;
    let time = world.level.time;
    if let Some(mi) = monsterinfo_mut(world, key) {
        mi.pausetime = time + n as f32 * FRAMETIME;
    }
}

/// Fire, holding the frame for the burst. C ref: `infantry_fire`.
pub fn infantry_fire(world: &mut GameWorld, key: EntityKey) {
    infantry_machine_gun(world, key);
    hold_until_pausetime(world, key);
}

/// C ref: `infantry_swing`.
pub fn infantry_swing(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_PUNCH_SWING, ATTN_NORM);
}

/// A 5-9 damage rifle butt. C ref: `infantry_smack`.
pub fn infantry_smack(world: &mut GameWorld, key: EntityKey) {
    let aim = Vec3f::new(ai::MELEE_DISTANCE, 0.0, 0.0);
    let damage = 5 + (world.random_u32() % 5) as i32;
    if world.fire_hit(key, aim, damage, 50) {
        sound(world, key, CHAN_WEAPON, SOUND_PUNCH_HIT, ATTN_NORM);
    }
}

/// Smack in melee range, else the machinegun. C ref: `infantry_attack`.
pub fn infantry_attack(world: &mut GameWorld, key: EntityKey) {
    let melee = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .is_some_and(|enemy| ai::range(world, key, enemy) == Range::Melee);
    if melee {
        set_move(world, key, &INFANTRY_MOVE_ATTACK2);
    } else {
        set_move(world, key, &INFANTRY_MOVE_ATTACK1);
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_infantry`.
pub fn sp_monster_infantry(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "monster_infantry".to_string());
        ent.game.movetype = MoveType::Step;
        ent.solid = Solid::Bbox;
        ent.mins = MINS;
        ent.maxs = MAXS;
        ent.game.health = HEALTH;
        ent.game.gib_health = -40;
        ent.game.max_health = HEALTH;
        ent.game.mass = MASS;

        ent.pain = Some(infantry_pain);
        ent.die = Some(infantry_die);
        ent.monsterinfo = Some(Box::new(MonsterInfo {
            stand: Some(infantry_stand),
            walk: Some(infantry_walk),
            run: Some(infantry_run),
            dodge: Some(infantry_dodge),
            attack: Some(infantry_attack),
            melee: None,
            sight: Some(infantry_sight),
            idle: Some(infantry_fidget),
            currentmove: Some(INFANTRY_MOVE_STAND.clone()),
            scale: MODEL_SCALE,
            ..Default::default()
        }));
        ai::walkmonster_start(ent);
    }
}

/// The `GameWorld` half of C `SP_monster_infantry`: its sounds.
pub fn finish_infantry(world: &mut GameWorld, _key: EntityKey, _fields: &HashMap<String, String>) {
    for name in [
        SOUND_PAIN1,
        SOUND_PAIN2,
        SOUND_DIE1,
        SOUND_DIE2,
        SOUND_GUNSHOT,
        SOUND_WEAPON_COCK,
        SOUND_PUNCH_SWING,
        SOUND_PUNCH_HIT,
        SOUND_SIGHT,
        SOUND_SEARCH,
        SOUND_IDLE,
    ] {
        world.gi.sound_index(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ThinkFn;
    use crate::monster::test_support::{
        check_move, player_arena, seed, sounds, think_for, ArenaImport,
    };
    use crate::world::{test_world, test_world_recording, MockGameImport};
    use std::rc::Rc;

    #[test]
    fn every_infantry_move_matches_c() {
        check_move(&INFANTRY_MOVE_STAND, (50, 71), ai::ai_stand, &[], &[], None);
        check_move(
            &INFANTRY_MOVE_FIDGET,
            (1, 49),
            ai::ai_stand,
            &[
                (0, 1.0),
                (2, 1.0),
                (3, 3.0),
                (4, 6.0),
                (5, 3.0),
                (10, 1.0),
                (15, 1.0),
                (17, -1.0),
                (20, 1.0),
                (22, -2.0),
                (23, 1.0),
                (24, 1.0),
                (25, 1.0),
                (26, -1.0),
                (29, -1.0),
                (35, -1.0),
                (38, 1.0),
                (41, -1.0),
                (42, -1.0),
                (44, -3.0),
                (45, -2.0),
                (46, -3.0),
                (47, -3.0),
                (48, -2.0),
            ],
            &[
                (5, infantry_footstep as ThinkFn),
                (47, infantry_footstep as ThinkFn),
            ],
            Some(infantry_stand as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_WALK,
            (74, 85),
            ai::ai_walk,
            &[
                (0, 5.0),
                (1, 4.0),
                (2, 4.0),
                (3, 5.0),
                (4, 4.0),
                (5, 5.0),
                (6, 6.0),
                (7, 4.0),
                (8, 4.0),
                (9, 4.0),
                (10, 4.0),
                (11, 5.0),
            ],
            &[
                (0, infantry_footstep as ThinkFn),
                (6, infantry_footstep as ThinkFn),
            ],
            None,
        );
        check_move(
            &INFANTRY_MOVE_RUN,
            (92, 99),
            ai::ai_run,
            &[
                (0, 10.0),
                (1, 20.0),
                (2, 5.0),
                (3, 7.0),
                (4, 30.0),
                (5, 35.0),
                (6, 2.0),
                (7, 6.0),
            ],
            &[
                (1, infantry_footstep as ThinkFn),
                (5, infantry_footstep as ThinkFn),
            ],
            None,
        );
        check_move(
            &INFANTRY_MOVE_PAIN1,
            (100, 109),
            ai::ai_move,
            &[
                (0, -3.0),
                (1, -2.0),
                (2, -1.0),
                (3, -2.0),
                (4, -1.0),
                (5, 1.0),
                (6, -1.0),
                (7, 1.0),
                (8, 6.0),
                (9, 2.0),
            ],
            &[
                (4, infantry_footstep as ThinkFn),
                (9, infantry_footstep as ThinkFn),
            ],
            Some(infantry_run as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_PAIN2,
            (110, 119),
            ai::ai_move,
            &[
                (0, -3.0),
                (1, -3.0),
                (3, -1.0),
                (4, -2.0),
                (7, 2.0),
                (8, 5.0),
                (9, 2.0),
            ],
            &[
                (4, infantry_footstep as ThinkFn),
                (9, infantry_footstep as ThinkFn),
            ],
            Some(infantry_run as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_DEATH1,
            (125, 144),
            ai::ai_move,
            &[
                (0, -4.0),
                (3, -1.0),
                (4, -4.0),
                (8, -1.0),
                (9, 3.0),
                (10, 1.0),
                (11, 1.0),
                (12, -2.0),
                (13, 2.0),
                (14, 2.0),
                (15, 9.0),
                (16, 9.0),
                (17, 5.0),
                (18, -3.0),
                (19, -3.0),
            ],
            &[
                (4, infantry_footstep as ThinkFn),
                (8, infantry_footstep as ThinkFn),
            ],
            Some(infantry_dead as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_DEATH2,
            (145, 169),
            ai::ai_move,
            &[
                (1, 1.0),
                (2, 5.0),
                (3, -1.0),
                (5, 1.0),
                (6, 1.0),
                (7, 4.0),
                (8, 3.0),
                (10, -2.0),
                (11, -2.0),
                (12, -3.0),
                (13, -1.0),
                (14, -2.0),
                (16, 2.0),
                (17, 2.0),
                (18, 3.0),
                (19, -10.0),
                (20, -7.0),
                (21, -8.0),
                (22, -6.0),
                (23, 4.0),
            ],
            &[
                (5, infantry_footstep as ThinkFn),
                (6, infantry_footstep as ThinkFn),
                (10, infantry_machine_gun as ThinkFn),
                (11, infantry_machine_gun as ThinkFn),
                (12, infantry_machine_gun as ThinkFn),
                (13, infantry_machine_gun as ThinkFn),
                (14, infantry_machine_gun as ThinkFn),
                (15, infantry_machine_gun as ThinkFn),
                (16, infantry_machine_gun as ThinkFn),
                (17, infantry_machine_gun as ThinkFn),
                (18, infantry_machine_gun as ThinkFn),
                (19, infantry_machine_gun as ThinkFn),
                (20, infantry_machine_gun as ThinkFn),
                (21, infantry_machine_gun as ThinkFn),
            ],
            Some(infantry_dead as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_DEATH3,
            (170, 178),
            ai::ai_move,
            &[(3, -6.0), (4, -11.0), (5, -3.0), (6, -11.0)],
            &[],
            Some(infantry_dead as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_DUCK,
            (120, 124),
            ai::ai_move,
            &[(0, -2.0), (1, -5.0), (2, 3.0), (3, 4.0)],
            &[
                (0, infantry_duck_down as ThinkFn),
                (1, infantry_duck_hold as ThinkFn),
                (3, infantry_duck_up as ThinkFn),
                (4, infantry_footstep as ThinkFn),
            ],
            Some(infantry_run as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_ATTACK1,
            (184, 198),
            ai::ai_charge,
            &[
                (0, 4.0),
                (1, -1.0),
                (2, -1.0),
                (4, -1.0),
                (5, 1.0),
                (6, 1.0),
                (7, 2.0),
                (8, -2.0),
                (9, -3.0),
                (10, 1.0),
                (11, 5.0),
                (12, -1.0),
                (13, -2.0),
                (14, -3.0),
            ],
            &[
                (3, infantry_cock_gun as ThinkFn),
                (10, infantry_fire as ThinkFn),
            ],
            Some(infantry_run as ThinkFn),
        );
        check_move(
            &INFANTRY_MOVE_ATTACK2,
            (199, 206),
            ai::ai_charge,
            &[
                (0, 3.0),
                (1, 6.0),
                (3, 8.0),
                (4, 5.0),
                (5, 8.0),
                (6, 6.0),
                (7, 3.0),
            ],
            &[
                (2, infantry_swing as ThinkFn),
                (3, infantry_footstep as ThinkFn),
                (5, infantry_smack as ThinkFn),
            ],
            Some(infantry_run as ThinkFn),
        );
    }

    /// An infantry at the origin facing +X, its live enemy at (x, 0, 0),
    /// at level time 5.
    fn duel(x: f32) -> (GameWorld, Rc<MockGameImport>, EntityKey, EntityKey) {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        sp_monster_infantry(&mut world.entities, key, &HashMap::new());
        let enemy = world.spawn().unwrap();
        let e = world.entities.get_mut(enemy).unwrap();
        e.state.origin = Vec3f::new(x, 0.0, 0.0);
        e.game.health = 100;
        e.game.takedamage = TakeDamage::Aim;
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.enemy = Some(enemy);
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
        mi(world, key)
            .currentmove
            .as_ref()
            .is_some_and(|cm| std::ptr::eq(cm.frames, mv.frames))
    }

    fn snd(chan: i32, name: &str, attn: f32) -> (i32, String, f32) {
        (chan, name.to_string(), attn)
    }

    #[test]
    fn infantry_stand_walk_and_run_set_their_moves() {
        let (mut world, _, key, _) = duel(300.0);
        infantry_walk(&mut world, key);
        assert!(is(&world, key, &INFANTRY_MOVE_WALK));
        infantry_run(&mut world, key);
        assert!(is(&world, key, &INFANTRY_MOVE_RUN));
        infantry_stand(&mut world, key);
        assert!(is(&world, key, &INFANTRY_MOVE_STAND));
    }

    #[test]
    fn infantry_run_holding_ground_stands() {
        let (mut world, _, key, _) = duel(300.0);
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        infantry_run(&mut world, key);
        assert!(is(&world, key, &INFANTRY_MOVE_STAND));
    }

    #[test]
    fn infantry_fidget_fidgets_and_mutters() {
        let (mut world, gi, key, _) = duel(300.0);
        infantry_fidget(&mut world, key);
        assert!(is(&world, key, &INFANTRY_MOVE_FIDGET));
        assert_eq!(sounds(&gi), [snd(CHAN_VOICE, SOUND_IDLE, ATTN_IDLE)]);
    }

    #[test]
    fn infantry_footstep_is_silent_with_the_default_footsteps_cvar() {
        let (mut world, gi, key, _) = duel(300.0);
        let rand = world.level.rand_state;
        infantry_footstep(&mut world, key);
        assert!(sounds(&gi).is_empty());
        assert_eq!(world.level.rand_state, rand);
    }

    #[test]
    fn infantry_pain_turns_bloody_below_half_health() {
        let (mut world, _, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.health = 50;
        infantry_pain(&mut world, key, enemy, 0.0, 50);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 0);
        world.entities.get_mut(key).unwrap().game.health = 49;
        infantry_pain(&mut world, key, enemy, 0.0, 1);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
    }

    #[test]
    fn infantry_pain_flinches_one_of_two_ways_with_a_three_second_debounce() {
        for (n, mv, cry) in [
            (0, &INFANTRY_MOVE_PAIN1, SOUND_PAIN1),
            (1, &INFANTRY_MOVE_PAIN2, SOUND_PAIN2),
        ] {
            let (mut world, gi, key, enemy) = duel(300.0);
            seed(&mut world, |w| w.random_u32() % 2 == n);
            infantry_pain(&mut world, key, enemy, 0.0, 5);
            assert!(is(&world, key, mv));
            assert_eq!(sounds(&gi), [snd(CHAN_VOICE, cry, ATTN_NORM)]);
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                8.0
            );

            // Within the debounce: nothing.
            infantry_stand(&mut world, key);
            infantry_pain(&mut world, key, enemy, 0.0, 5);
            assert!(is(&world, key, &INFANTRY_MOVE_STAND));
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn infantry_pain_on_nightmare_is_silent_and_keeps_the_move() {
        let (mut world, gi, key, enemy) = duel(300.0);
        world.game.skill = crate::constants::SKILL_HARDPLUS;
        infantry_pain(&mut world, key, enemy, 0.0, 5);
        assert!(is(&world, key, &INFANTRY_MOVE_STAND));
        assert!(sounds(&gi).is_empty());
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            8.0
        );
    }

    /// An infantry facing +X in an arena whose player is `x` ahead.
    fn in_arena(x: f32) -> (GameWorld, Rc<ArenaImport>, EntityKey, EntityKey) {
        let (mut world, gi, player) = player_arena(x);
        let key = world.spawn().unwrap();
        sp_monster_infantry(&mut world.entities, key, &HashMap::new());
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        world.link_entity(key);
        world.level.time = 5.0;
        (world, gi, key, player)
    }

    fn health(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().game.health
    }

    #[test]
    fn infantry_machine_gun_in_the_attack_shoots_the_enemy_for_3() {
        let (mut world, _, key, player) = in_arena(200.0);
        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK111;
        infantry_machine_gun(&mut world, key);
        assert_eq!(health(&world, player), 97);
    }

    #[test]
    fn infantry_machine_gun_leads_a_moving_enemy_by_0_2_seconds_back() {
        // The player runs +Y at 500/s: the shot goes 100 units behind it.
        let (mut world, gi, key, player) = in_arena(200.0);
        world.entities.get_mut(player).unwrap().velocity = Vec3f::new(0.0, 500.0, 0.0);
        let eye = Vec3f::new(200.0, -100.0, 24.0 + 22.0);
        gi.target_at(eye, 16.0, world.number_of(player).unwrap());
        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK111;
        infantry_machine_gun(&mut world, key);
        assert_eq!(health(&world, player), 97);
    }

    #[test]
    fn infantry_machine_gun_without_an_enemy_fires_straight_ahead() {
        let (mut world, gi, key, player) = in_arena(200.0);
        world.entities.get_mut(key).unwrap().game.enemy = None;
        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK111;
        let (start, forward, _) =
            flash::monster_flash_source(&world, key, MZ2_INFANTRY_MACHINEGUN_1).unwrap();
        gi.target_at(
            start + forward * 100.0,
            8.0,
            world.number_of(player).unwrap(),
        );
        infantry_machine_gun(&mut world, key);
        assert_eq!(health(&world, player), 97);
    }

    #[test]
    fn infantry_dying_machine_gun_sweeps_by_the_c_aim_angles() {
        // death216 fires flash MACHINEGUN_7, 30 up and 45 right of ahead.
        let (mut world, gi, key, player) = in_arena(200.0);
        world.entities.get_mut(key).unwrap().game.enemy = None;
        world.entities.get_mut(key).unwrap().state.frame = FRAME_DEATH211 + 5;
        let flash = MZ2_INFANTRY_MACHINEGUN_2 + 5;
        let (start, _, _) = flash::monster_flash_source(&world, key, flash).unwrap();
        let (forward, _, _) = crate::physics::angle_vectors(Vec3f::new(-30.0, -45.0, 0.0));
        gi.target_at(
            start + forward * 100.0,
            8.0,
            world.number_of(player).unwrap(),
        );
        infantry_machine_gun(&mut world, key);
        assert_eq!(health(&world, player), 97);
        assert_eq!(AIMANGLES[5], [30.0, 45.0, 0.0]);
        assert_eq!(AIMANGLES[0], [0.0, 5.0, 0.0]);
        assert_eq!(AIMANGLES[11], [90.0, 35.0, 0.0]);
    }

    #[test]
    fn infantry_machine_gun_sends_the_frames_muzzle_flash() {
        for (frame, flash) in [
            (FRAME_ATTAK111, MZ2_INFANTRY_MACHINEGUN_1),
            (FRAME_DEATH211, MZ2_INFANTRY_MACHINEGUN_2),
            (FRAME_DEATH211 + 11, MZ2_INFANTRY_MACHINEGUN_13),
        ] {
            let (mut world, gi) = crate::misc::wire::wire_world();
            let key = world.spawn().unwrap();
            sp_monster_infantry(&mut world.entities, key, &HashMap::new());
            world.entities.get_mut(key).unwrap().state.frame = frame;
            infantry_machine_gun(&mut world, key);
            let wire = gi.wire.borrow();
            assert_eq!(
                wire[wire.len() - 2],
                crate::misc::wire::Wire::Byte(flash),
                "{frame}"
            );
        }
    }

    #[test]
    fn infantry_sight_calls_out_on_the_body_channel() {
        let (mut world, gi, key, enemy) = duel(300.0);
        infantry_sight(&mut world, key, enemy);
        assert_eq!(sounds(&gi), [snd(CHAN_BODY, SOUND_SIGHT, ATTN_NORM)]);
    }

    #[test]
    fn infantry_dead_leaves_a_corpse_for_the_flies() {
        let (mut world, _, key, _) = duel(300.0);
        seed(&mut world, |w| w.random() <= 0.5);
        infantry_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.mins, Vec3f::new(-16.0, -16.0, -24.0));
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_ne!(ent.svflags & SvFlags::DEADMONSTER.bits(), 0);
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::m_flies_on as ThinkFn)));
    }

    #[test]
    fn infantry_dead_keeps_thinking_when_the_fly_check_fails() {
        // C infantry_dead never touches nextthink, so a failed M_FlyCheck
        // is retried on the next frame of the death move.
        let (mut world, _, key, _) = duel(300.0);
        world.level.time = 2.0;
        world.entities.get_mut(key).unwrap().game.nextthink = 2.1;
        seed(&mut world, |w| w.random() > 0.5);
        infantry_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(ent.game.nextthink, 2.1);
    }

    #[test]
    fn infantry_dead_in_water_gets_no_flies_but_keeps_thinking() {
        let (mut world, _, key, _) = duel(300.0);
        world.level.time = 2.0;
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.nextthink = 2.1;
        ent.game.waterlevel = 1;
        infantry_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.nextthink, 2.1);
        assert!(!ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::m_flies_on as ThinkFn)));
    }

    #[test]
    fn infantry_die_gibs_at_minus_40_health() {
        let (mut world, gi, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.health = -40;
        let before = world.entities.count();
        infantry_die(&mut world, key, enemy, enemy, 50, Vec3f::ZERO);
        assert_eq!(sounds(&gi), [snd(CHAN_VOICE, "misc/udeath.wav", ATTN_NORM)]);
        assert_eq!(world.entities.count(), before + 6, "2 bones + 4 meat");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.maxs, Vec3f::ZERO, "now the head");
    }

    #[test]
    fn infantry_die_twice_does_nothing_more_short_of_a_gib() {
        let (mut world, gi, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.deadflag = DeadFlag::Dead;
        world.entities.get_mut(key).unwrap().game.health = -39;
        infantry_die(&mut world, key, enemy, enemy, 5, Vec3f::ZERO);
        assert!(sounds(&gi).is_empty());
        assert!(is(&world, key, &INFANTRY_MOVE_STAND));
    }

    #[test]
    fn infantry_die_picks_one_of_three_deaths_and_bloodies_the_skin() {
        for (n, mv, cry) in [
            (0, &INFANTRY_MOVE_DEATH1, SOUND_DIE2),
            (1, &INFANTRY_MOVE_DEATH2, SOUND_DIE1),
            (2, &INFANTRY_MOVE_DEATH3, SOUND_DIE2),
        ] {
            let (mut world, gi, key, enemy) = duel(300.0);
            seed(&mut world, |w| w.random_u32() % 3 == n);
            infantry_die(&mut world, key, enemy, enemy, 5, Vec3f::ZERO);
            assert!(is(&world, key, mv), "{n}");
            assert_eq!(sounds(&gi), [snd(CHAN_VOICE, cry, ATTN_NORM)]);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.state.skinnum, 1);
            assert_eq!(ent.game.deadflag, DeadFlag::Dead);
            assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        }
    }

    #[test]
    fn infantry_duck_crouches_once_holds_then_stands() {
        let (mut world, _, key, _) = duel(300.0);
        infantry_duck_down(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().maxs.z, 0.0);
        assert_eq!(
            world.entities.get(key).unwrap().game.takedamage,
            TakeDamage::Yes
        );
        assert_eq!(mi(&world, key).pausetime, 6.0);
        infantry_duck_down(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().maxs.z, 0.0);

        infantry_duck_hold(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        world.level.time = 6.0;
        infantry_duck_hold(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));

        infantry_duck_up(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs.z, 32.0);
        assert_eq!(ent.game.takedamage, TakeDamage::Aim);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::DUCKED));
    }

    #[test]
    fn infantry_dodge_ducks_one_shot_in_four_and_turns_on_the_attacker() {
        let (mut world, _, key, enemy) = duel(300.0);
        world.entities.get_mut(key).unwrap().game.enemy = None;
        seed(&mut world, |w| w.random() > 0.25);
        infantry_dodge(&mut world, key, enemy, 0.5);
        assert!(is(&world, key, &INFANTRY_MOVE_STAND));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, None);

        seed(&mut world, |w| w.random() <= 0.25);
        infantry_dodge(&mut world, key, enemy, 0.5);
        assert!(is(&world, key, &INFANTRY_MOVE_DUCK));
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(enemy));
    }

    #[test]
    fn infantry_cock_gun_clicks_and_picks_a_10_to_25_frame_burst() {
        for (low, frames) in [(0, 10.0), (15, 25.0)] {
            let (mut world, gi, key, _) = duel(300.0);
            seed(&mut world, |w| w.random_u32() & 15 == low);
            infantry_cock_gun(&mut world, key);
            assert_eq!(
                sounds(&gi),
                [snd(CHAN_WEAPON, SOUND_WEAPON_COCK, ATTN_NORM)]
            );
            assert!((mi(&world, key).pausetime - (5.0 + frames * 0.1)).abs() < 1e-4);
        }
    }

    #[test]
    fn infantry_fire_shoots_and_holds_the_frame_for_the_burst() {
        let (mut world, _, key, player) = in_arena(200.0);
        world.entities.get_mut(key).unwrap().state.frame = FRAME_ATTAK111;
        mi_mut(&mut world, key).pausetime = 5.5;
        infantry_fire(&mut world, key);
        assert!(mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
        assert_eq!(health(&world, player), 97);
        world.level.time = 5.5;
        infantry_fire(&mut world, key);
        assert!(!mi(&world, key).aiflags.contains(AiFlags::HOLD_FRAME));
    }

    #[test]
    fn infantry_swing_whooshes() {
        let (mut world, gi, key, _) = duel(50.0);
        infantry_swing(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [snd(CHAN_WEAPON, SOUND_PUNCH_SWING, ATTN_NORM)]
        );
    }

    #[test]
    fn infantry_smack_hits_for_5_to_9_in_reach_and_thuds() {
        for (roll, hurt) in [(0, 95), (4, 91)] {
            let (mut world, gi, key, player) = in_arena(50.0);
            seed(&mut world, |w| w.random_u32() % 5 == roll);
            infantry_smack(&mut world, key);
            assert_eq!(health(&world, player), hurt);
            assert_eq!(
                sounds(&gi.floor.inner),
                [snd(CHAN_WEAPON, SOUND_PUNCH_HIT, ATTN_NORM)]
            );
        }

        let (mut world, gi, key, player) = in_arena(150.0);
        infantry_smack(&mut world, key);
        assert_eq!(health(&world, player), 100);
        assert!(sounds(&gi.floor.inner).is_empty());
    }

    #[test]
    fn infantry_attack_smacks_in_melee_range_else_machineguns() {
        let (mut world, _, key, _) = duel(50.0);
        infantry_attack(&mut world, key);
        assert!(is(&world, key, &INFANTRY_MOVE_ATTACK2));
        let (mut world, _, key, _) = duel(300.0);
        infantry_attack(&mut world, key);
        assert!(is(&world, key, &INFANTRY_MOVE_ATTACK1));
    }

    #[test]
    fn infantry_spawns_with_the_c_callbacks_and_starts_as_a_walker() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_infantry(&mut world.entities, key, &HashMap::new());
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.game.health, ent.game.mass), (100, 200));
        assert_eq!((ent.mins, ent.maxs), (MINS, MAXS));
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert!(ent.pain.is_some() && ent.die.is_some());
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::walkmonster_start_go as ThinkFn)));
        let mi = ent.monsterinfo.as_deref().unwrap();
        assert_eq!(mi.scale, 1.0);
        assert!(mi.stand.is_some() && mi.walk.is_some() && mi.run.is_some());
        assert!(mi.dodge.is_some() && mi.attack.is_some() && mi.sight.is_some());
        assert!(mi
            .idle
            .is_some_and(|f| std::ptr::fn_addr_eq(f, infantry_fidget as ThinkFn)));
        assert!(mi.melee.is_none() && mi.search.is_none());
        assert!(is(&world, key, &INFANTRY_MOVE_STAND));
    }

    #[test]
    fn infantry_finish_precaches_its_sounds_and_runs_from_its_row() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        finish_infantry(&mut world, key, &HashMap::new());
        assert_eq!(gi.sounds.borrow().len(), 11);
        assert!(gi.sounds.borrow().iter().any(|s| s == SOUND_GUNSHOT));
        let row = crate::spawn::find_spawn("monster_infantry").unwrap();
        assert!(row
            .3
            .is_some_and(|f| std::ptr::fn_addr_eq(f, finish_infantry as crate::spawn::FinishFn)));
    }

    #[test]
    fn infantry_sights_the_player_and_machineguns_it() {
        let (mut world, gi, player) = player_arena(300.0);
        world.game.skill = 1;
        let key = world.spawn().unwrap();
        let fields: HashMap<String, String> =
            [("classname".to_string(), "monster_infantry".to_string())].into();
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
        let &(_, spawn, _, finish) = crate::spawn::find_spawn("monster_infantry").unwrap();
        spawn(&mut world.entities, key, &fields);
        finish.unwrap()(&mut world, key, &fields);
        ai::monster_start(&mut world, key);
        world.link_entity(key);

        think_for(&mut world, key, 150);

        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(sounds(&gi.floor.inner)
            .iter()
            .any(|(_, s, _)| s == SOUND_SIGHT));
        assert!(health(&world, player) < 100);
    }
}
