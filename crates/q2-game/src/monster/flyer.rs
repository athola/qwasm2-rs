//! Flyer: flies, strafes with twin blasters, slashes with its wing blades
//! up close, explodes when killed.
//! C ref: `~/Qwasm2/src/game/monster/flyer/flyer.c`.

use q2_shared::constants::{EF_HYPERBLASTER, MZ2_FLYER_BLASTER_1, MZ2_FLYER_BLASTER_2};
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::misc::become_explosion1;
use crate::monster::{fire::monster_fire_blaster, flash::monster_flash_source};
use crate::world::GameWorld;
use std::collections::HashMap;

const SOUND_SIGHT: &str = "flyer/flysght1.wav";
const SOUND_IDLE: &str = "flyer/flysrch1.wav";
const SOUND_PAIN1: &str = "flyer/flypain1.wav";
const SOUND_PAIN2: &str = "flyer/flypain2.wav";
const SOUND_SLASH: &str = "flyer/flyatck2.wav";
const SOUND_SPROING: &str = "flyer/flyatck1.wav";
const SOUND_DIE: &str = "flyer/flydeth1.wav";
/// Every sound C `SP_monster_flyer` indexes; `flyatck3` is precached but
/// never played.
const SOUNDS: [&str; 8] = [
    SOUND_SIGHT,
    SOUND_IDLE,
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_SLASH,
    SOUND_SPROING,
    SOUND_DIE,
    "flyer/flyatck3.wav",
];
/// C `self->s.sound`, the wing buzz.
const SOUND_IDLE_LOOP: &str = "flyer/flyidle1.wav";

/// C `MODEL_SCALE` (flyer.h).
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

// C ref: flyer.h (`FRAME_*`, model frame numbers).

const FRAME_START01: i32 = 0;
const FRAME_START06: i32 = 5;
const FRAME_STOP01: i32 = 6;
const FRAME_STOP07: i32 = 12;
const FRAME_STAND01: i32 = 13;
const FRAME_STAND45: i32 = 57;
const FRAME_ATTAK101: i32 = 58;
const FRAME_ATTAK106: i32 = 63;
const FRAME_ATTAK107: i32 = 64;
const FRAME_ATTAK118: i32 = 75;
const FRAME_ATTAK119: i32 = 76;
const FRAME_ATTAK121: i32 = 78;
const FRAME_ATTAK201: i32 = 79;
const FRAME_ATTAK204: i32 = 82;
const FRAME_ATTAK207: i32 = 85;
const FRAME_ATTAK210: i32 = 88;
const FRAME_ATTAK217: i32 = 95;
const FRAME_BANKL01: i32 = 96;
const FRAME_BANKL07: i32 = 102;
const FRAME_BANKR01: i32 = 103;
const FRAME_BANKR07: i32 = 109;
const FRAME_ROLLF01: i32 = 110;
const FRAME_ROLLF09: i32 = 118;
const FRAME_ROLLR01: i32 = 119;
const FRAME_ROLLR09: i32 = 127;
const FRAME_DEFENS01: i32 = 128;
const FRAME_DEFENS06: i32 = 133;
const FRAME_PAIN101: i32 = 134;
const FRAME_PAIN109: i32 = 142;
const FRAME_PAIN201: i32 = 143;
const FRAME_PAIN204: i32 = 146;
const FRAME_PAIN301: i32 = 147;
const FRAME_PAIN304: i32 = 150;

static FLYER_FRAMES_STAND: [MonsterFrame; 45] = [frame(ai::ai_stand, 0.0, None); 45];

pub static FLYER_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND45,
    frames: &FLYER_FRAMES_STAND,
    endfunc: None,
};

static FLYER_FRAMES_WALK: [MonsterFrame; 45] = [frame(ai::ai_walk, 5.0, None); 45];

pub static FLYER_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND45,
    frames: &FLYER_FRAMES_WALK,
    endfunc: None,
};

static FLYER_FRAMES_RUN: [MonsterFrame; 45] = [frame(ai::ai_run, 10.0, None); 45];

pub static FLYER_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND45,
    frames: &FLYER_FRAMES_RUN,
    endfunc: None,
};

static FLYER_FRAMES_START: [MonsterFrame; 6] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(flyer_nextmove)),
];

pub static FLYER_MOVE_START: MonsterMove = MonsterMove {
    firstframe: FRAME_START01,
    lastframe: FRAME_START06,
    frames: &FLYER_FRAMES_START,
    endfunc: None,
};

static FLYER_FRAMES_STOP: [MonsterFrame; 7] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(flyer_nextmove)),
];

pub static FLYER_MOVE_STOP: MonsterMove = MonsterMove {
    firstframe: FRAME_STOP01,
    lastframe: FRAME_STOP07,
    frames: &FLYER_FRAMES_STOP,
    endfunc: None,
};

static FLYER_FRAMES_ROLLRIGHT: [MonsterFrame; 9] = [frame(ai::ai_move, 0.0, None); 9];

pub static FLYER_MOVE_ROLLRIGHT: MonsterMove = MonsterMove {
    firstframe: FRAME_ROLLR01,
    lastframe: FRAME_ROLLR09,
    frames: &FLYER_FRAMES_ROLLRIGHT,
    endfunc: None,
};

static FLYER_FRAMES_ROLLLEFT: [MonsterFrame; 9] = [frame(ai::ai_move, 0.0, None); 9];

pub static FLYER_MOVE_ROLLLEFT: MonsterMove = MonsterMove {
    firstframe: FRAME_ROLLF01,
    lastframe: FRAME_ROLLF09,
    frames: &FLYER_FRAMES_ROLLLEFT,
    endfunc: None,
};

static FLYER_FRAMES_PAIN3: [MonsterFrame; 4] = [frame(ai::ai_move, 0.0, None); 4];

pub static FLYER_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN304,
    frames: &FLYER_FRAMES_PAIN3,
    endfunc: Some(flyer_run),
};

static FLYER_FRAMES_PAIN2: [MonsterFrame; 4] = [frame(ai::ai_move, 0.0, None); 4];

pub static FLYER_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN204,
    frames: &FLYER_FRAMES_PAIN2,
    endfunc: Some(flyer_run),
};

static FLYER_FRAMES_PAIN1: [MonsterFrame; 9] = [frame(ai::ai_move, 0.0, None); 9];

pub static FLYER_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN109,
    frames: &FLYER_FRAMES_PAIN1,
    endfunc: Some(flyer_run),
};

static FLYER_FRAMES_DEFENSE: [MonsterFrame; 6] = [frame(ai::ai_move, 0.0, None); 6];

pub static FLYER_MOVE_DEFENSE: MonsterMove = MonsterMove {
    firstframe: FRAME_DEFENS01,
    lastframe: FRAME_DEFENS06,
    frames: &FLYER_FRAMES_DEFENSE,
    endfunc: None,
};

static FLYER_FRAMES_BANKRIGHT: [MonsterFrame; 7] = [frame(ai::ai_move, 0.0, None); 7];

pub static FLYER_MOVE_BANKRIGHT: MonsterMove = MonsterMove {
    firstframe: FRAME_BANKR01,
    lastframe: FRAME_BANKR07,
    frames: &FLYER_FRAMES_BANKRIGHT,
    endfunc: None,
};

static FLYER_FRAMES_BANKLEFT: [MonsterFrame; 7] = [frame(ai::ai_move, 0.0, None); 7];

pub static FLYER_MOVE_BANKLEFT: MonsterMove = MonsterMove {
    firstframe: FRAME_BANKL01,
    lastframe: FRAME_BANKL07,
    frames: &FLYER_FRAMES_BANKLEFT,
    endfunc: None,
};

static FLYER_FRAMES_ATTACK2: [MonsterFrame; 17] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, -10.0, Some(flyer_fireleft)),
    frame(ai::ai_charge, -10.0, Some(flyer_fireright)),
    frame(ai::ai_charge, -10.0, Some(flyer_fireleft)),
    frame(ai::ai_charge, -10.0, Some(flyer_fireright)),
    frame(ai::ai_charge, -10.0, Some(flyer_fireleft)),
    frame(ai::ai_charge, -10.0, Some(flyer_fireright)),
    frame(ai::ai_charge, -10.0, Some(flyer_fireleft)),
    frame(ai::ai_charge, -10.0, Some(flyer_fireright)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

pub static FLYER_MOVE_ATTACK2: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK217,
    frames: &FLYER_FRAMES_ATTACK2,
    endfunc: Some(flyer_run),
};

static FLYER_FRAMES_START_MELEE: [MonsterFrame; 6] = [
    frame(ai::ai_charge, 0.0, Some(flyer_pop_blades)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

pub static FLYER_MOVE_START_MELEE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK106,
    frames: &FLYER_FRAMES_START_MELEE,
    endfunc: Some(flyer_loop_melee),
};

static FLYER_FRAMES_END_MELEE: [MonsterFrame; 3] = [frame(ai::ai_charge, 0.0, None); 3];

pub static FLYER_MOVE_END_MELEE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK119,
    lastframe: FRAME_ATTAK121,
    frames: &FLYER_FRAMES_END_MELEE,
    endfunc: Some(flyer_run),
};

static FLYER_FRAMES_LOOP_MELEE: [MonsterFrame; 12] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(flyer_slash_left)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(flyer_slash_right)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

pub static FLYER_MOVE_LOOP_MELEE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK107,
    lastframe: FRAME_ATTAK118,
    frames: &FLYER_FRAMES_LOOP_MELEE,
    endfunc: Some(flyer_check_melee),
};

// ---------------------------------------------------------------------------
// Callbacks. C ref: flyer.c.
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

/// C ref: `flyer_sight`.
pub fn flyer_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `flyer_idle`.
pub fn flyer_idle(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
}

/// C ref: `flyer_pop_blades`.
pub fn flyer_pop_blades(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SPROING, ATTN_NORM);
}

/// C ref: `flyer_run`.
pub fn flyer_run(world: &mut GameWorld, key: EntityKey) {
    let stand_ground = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    let mv = if stand_ground {
        &FLYER_MOVE_STAND
    } else {
        &FLYER_MOVE_RUN
    };
    set_move(world, key, mv);
}

/// C ref: `flyer_walk`.
pub fn flyer_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLYER_MOVE_WALK);
}

/// C ref: `flyer_stand`.
pub fn flyer_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLYER_MOVE_STAND);
}

/// Last frame of the start and stop moves. C switches on the file static
/// `nextmove`, which only `flyer_setstart` sets and nothing calls that,
/// so it stays `ACTION_nothing` and no move is chosen; those unreachable
/// setters are not ported. C ref: `flyer_nextmove`.
pub fn flyer_nextmove(_world: &mut GameWorld, _key: EntityKey) {}

/// A bolt from gun `flash` at the enemy's eye; frames attak204, attak207
/// and attak210 fire hyperblaster ones. C ref: `flyer_fire`.
fn flyer_fire(world: &mut GameWorld, key: EntityKey, flash: i32) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let Some(enemy) = ent.game.enemy.and_then(|e| world.entities.get(e)) else {
        return;
    };
    let effect = if [FRAME_ATTAK204, FRAME_ATTAK207, FRAME_ATTAK210].contains(&ent.state.frame) {
        EF_HYPERBLASTER
    } else {
        0
    };
    let mut end = enemy.state.origin;
    end.z += enemy.game.viewheight;
    let Some((start, _, _)) = monster_flash_source(world, key, flash) else {
        return;
    };
    monster_fire_blaster(world, key, start, end - start, 1, 1000, flash, effect);
}

/// C ref: `flyer_fireleft`.
pub fn flyer_fireleft(world: &mut GameWorld, key: EntityKey) {
    flyer_fire(world, key, MZ2_FLYER_BLASTER_1);
}

/// C ref: `flyer_fireright`.
pub fn flyer_fireright(world: &mut GameWorld, key: EntityKey) {
    flyer_fire(world, key, MZ2_FLYER_BLASTER_2);
}

/// A wing strike from the side of the box at `side` (its x extent).
/// C ref: the shared body of `flyer_slash_left`/`flyer_slash_right`.
fn flyer_slash(world: &mut GameWorld, key: EntityKey, side: fn(&crate::entity::Entity) -> f32) {
    let Some(right) = world.entities.get(key).map(side) else {
        return;
    };
    world.fire_hit(key, Vec3f::new(ai::MELEE_DISTANCE, right, 0.0), 5, 0);
    sound(world, key, CHAN_WEAPON, SOUND_SLASH, ATTN_NORM);
}

/// C ref: `flyer_slash_left`.
pub fn flyer_slash_left(world: &mut GameWorld, key: EntityKey) {
    flyer_slash(world, key, |e| e.mins.x);
}

/// C ref: `flyer_slash_right`.
pub fn flyer_slash_right(world: &mut GameWorld, key: EntityKey) {
    flyer_slash(world, key, |e| e.maxs.x);
}

/// C ref: `flyer_loop_melee`.
pub fn flyer_loop_melee(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLYER_MOVE_LOOP_MELEE);
}

/// C ref: `flyer_attack`.
pub fn flyer_attack(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLYER_MOVE_ATTACK2);
}

/// C ref: `flyer_melee`.
pub fn flyer_melee(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLYER_MOVE_START_MELEE);
}

/// Slash again eight times in ten while the enemy is in melee range,
/// else stop. C ref: `flyer_check_melee`.
pub fn flyer_check_melee(world: &mut GameWorld, key: EntityKey) {
    let enemy = world.entities.get(key).and_then(|e| e.game.enemy);
    let in_melee = enemy.is_some_and(|enemy| ai::range(world, key, enemy) == Range::Melee);
    if in_melee && world.random() <= 0.8 {
        set_move(world, key, &FLYER_MOVE_LOOP_MELEE);
    } else {
        set_move(world, key, &FLYER_MOVE_END_MELEE);
    }
}

/// Hurt skin below half health; one of three pains at most every 3 s,
/// none in nightmare. C ref: `flyer_pain`.
pub fn flyer_pain(
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
    let (name, mv) = match world.random_u32() % 3 {
        0 => (SOUND_PAIN1, &FLYER_MOVE_PAIN1),
        1 => (SOUND_PAIN2, &FLYER_MOVE_PAIN2),
        _ => (SOUND_PAIN1, &FLYER_MOVE_PAIN3),
    };
    sound(world, key, CHAN_VOICE, name, ATTN_NORM);
    set_move(world, key, mv);
}

/// C ref: `flyer_die`.
pub fn flyer_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    sound(world, key, CHAN_VOICE, SOUND_DIE, ATTN_NORM);
    become_explosion1(world, key);
}

// ---------------------------------------------------------------------------
// Spawn. C ref: `SP_monster_flyer`, split: the model is the
// `spawn::SPAWNS` row's; `finish_flyer` does the jail5 fix and sounds.
// ---------------------------------------------------------------------------

pub fn sp_monster_flyer(
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
        .unwrap_or_else(|| "monster_flyer".to_string());
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.game.health = 50;
    ent.game.mass = 50;
    ent.pain = Some(flyer_pain);
    ent.die = Some(flyer_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(flyer_stand),
        walk: Some(flyer_walk),
        run: Some(flyer_run),
        attack: Some(flyer_attack),
        melee: Some(flyer_melee),
        sight: Some(flyer_sight),
        idle: Some(flyer_idle),
        currentmove: Some(FLYER_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::flymonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_flyer`: in jail5 the flyer at
/// z -104 is a map bug, its `target` meant as its `targetname`; then
/// precache the sounds and start the wing buzz.
pub fn finish_flyer(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    let jail5 = world.level.mapname.eq_ignore_ascii_case("jail5");
    if let Some(ent) = world.entities.get_mut(key) {
        if jail5 && ent.state.origin.z == -104.0 {
            ent.game.targetname = std::mem::take(&mut ent.game.target);
        }
    }
    for name in SOUNDS {
        world.gi.sound_index(name);
    }
    let buzz = world.gi.sound_index(SOUND_IDLE_LOOP);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.sound = buzz;
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
        (flyer_run, "flyer_run"),
        (flyer_nextmove, "flyer_nextmove"),
        (flyer_fireleft, "flyer_fireleft"),
        (flyer_fireright, "flyer_fireright"),
        (flyer_pop_blades, "flyer_pop_blades"),
        (flyer_loop_melee, "flyer_loop_melee"),
        (flyer_slash_left, "flyer_slash_left"),
        (flyer_slash_right, "flyer_slash_right"),
        (flyer_check_melee, "flyer_check_melee"),
    ];

    #[test]
    fn flyer_moves_match_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&FLYER_MOVE_STAND, 13, 57, "-", "stand:0:-*45"),
            (&FLYER_MOVE_WALK, 13, 57, "-", "walk:5:-*45"),
            (&FLYER_MOVE_RUN, 13, 57, "-", "run:10:-*45"),
            (&FLYER_MOVE_START, 0, 5, "-", "move:0:-*5 move:0:flyer_nextmove"),
            (&FLYER_MOVE_STOP, 6, 12, "-", "move:0:-*6 move:0:flyer_nextmove"),
            (&FLYER_MOVE_ROLLRIGHT, 119, 127, "-", "move:0:-*9"),
            (&FLYER_MOVE_ROLLLEFT, 110, 118, "-", "move:0:-*9"),
            (&FLYER_MOVE_PAIN3, 147, 150, "flyer_run", "move:0:-*4"),
            (&FLYER_MOVE_PAIN2, 143, 146, "flyer_run", "move:0:-*4"),
            (&FLYER_MOVE_PAIN1, 134, 142, "flyer_run", "move:0:-*9"),
            (&FLYER_MOVE_DEFENSE, 128, 133, "-", "move:0:-*6"),
            (&FLYER_MOVE_BANKRIGHT, 103, 109, "-", "move:0:-*7"),
            (&FLYER_MOVE_BANKLEFT, 96, 102, "-", "move:0:-*7"),
            (&FLYER_MOVE_ATTACK2, 79, 95, "flyer_run", "charge:0:-*3 charge:-10:flyer_fireleft charge:-10:flyer_fireright charge:-10:flyer_fireleft charge:-10:flyer_fireright charge:-10:flyer_fireleft charge:-10:flyer_fireright charge:-10:flyer_fireleft charge:-10:flyer_fireright charge:0:-*6"),
            (&FLYER_MOVE_START_MELEE, 58, 63, "flyer_loop_melee", "charge:0:flyer_pop_blades charge:0:-*5"),
            (&FLYER_MOVE_END_MELEE, 76, 78, "flyer_run", "charge:0:-*3"),
            (&FLYER_MOVE_LOOP_MELEE, 64, 75, "flyer_check_melee", "charge:0:-*2 charge:0:flyer_slash_left charge:0:-*4 charge:0:flyer_slash_right charge:0:-*4"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    fn flyer(world: &mut GameWorld) -> EntityKey {
        spawn_monster(world, "monster_flyer", Vec3f::new(0.0, 0.0, 64.0))
    }

    #[test]
    fn flyer_spawns_with_the_c_values_and_flies() {
        let (mut world, gi) = test_world_recording();
        let key = flyer(&mut world);
        let hum = world.gi.sound_index(SOUND_IDLE_LOOP);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.health, ent.game.max_health, ent.game.mass),
            (50, 50, 50)
        );
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-16.0, -16.0, -24.0),
                Vec3f::new(16.0, 16.0, 32.0)
            )
        );
        assert!(ent.game.flags.contains(EntityFlags::FLY));
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::flymonster_start_go as ThinkFn)));
        assert_eq!(ent.state.sound, hum);
        let mi = mi(&world, key);
        assert_eq!(mi.scale, 1.0);
        assert!(mi
            .attack
            .is_some_and(|f| std::ptr::fn_addr_eq(f, flyer_attack as ThinkFn)));
        assert!(mi
            .melee
            .is_some_and(|f| std::ptr::fn_addr_eq(f, flyer_melee as ThinkFn)));
        assert!(mi.idle.is_some() && mi.sight.is_some() && mi.search.is_none());
        assert!(playing(&world, key, &FLYER_MOVE_STAND));
        for s in SOUNDS {
            assert!(gi.sounds.borrow().iter().any(|n| n == s), "{s} precached");
        }
    }

    #[test]
    fn flyer_in_jail5_at_z_minus_104_waits_to_be_triggered_by_its_target() {
        let mut world = test_world();
        world.level.mapname = "JAIL5".to_string();
        let &(_, sfn, _, finish) = crate::spawn::find_spawn("monster_flyer").unwrap();
        let fields = HashMap::new();
        for (z, fixed) in [(-104.0, true), (-103.0, false)] {
            let key = world.spawn().unwrap();
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.origin.z = z;
            ent.game.target = "t1".to_string();
            sfn(&mut world.entities, key, &fields);
            finish.unwrap()(&mut world, key, &fields);
            let game = &world.entities.get(key).unwrap().game;
            if fixed {
                assert_eq!((game.targetname.as_str(), game.target.as_str()), ("t1", ""));
            } else {
                assert_eq!((game.targetname.as_str(), game.target.as_str()), ("", "t1"));
            }
        }
    }

    #[test]
    fn flyer_state_callbacks_pick_the_c_moves() {
        let mut world = test_world();
        let key = flyer(&mut world);
        flyer_run(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_RUN));
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        flyer_run(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_STAND));
        flyer_walk(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_WALK));
        flyer_attack(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_ATTACK2));
        flyer_melee(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_START_MELEE));
        flyer_loop_melee(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_LOOP_MELEE));
        flyer_stand(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_STAND));
        // C's `nextmove` is never set, so start/stop end where they are.
        flyer_nextmove(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_STAND));
    }

    #[test]
    fn flyer_check_melee_loops_eight_times_in_ten_in_melee_range() {
        let mut world = test_world();
        let key = flyer(&mut world);
        let player = player_at(&mut world, Vec3f::new(60.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        let mut looped = 0;
        for _ in 0..1000 {
            flyer_check_melee(&mut world, key);
            if playing(&world, key, &FLYER_MOVE_LOOP_MELEE) {
                looped += 1;
            } else {
                assert!(playing(&world, key, &FLYER_MOVE_END_MELEE));
            }
        }
        assert!((750..850).contains(&looped), "{looped}");
        world.entities.get_mut(player).unwrap().state.origin.x = 100.0;
        flyer_check_melee(&mut world, key);
        assert!(playing(&world, key, &FLYER_MOVE_END_MELEE));
    }

    #[test]
    fn flyer_guns_alternate_flashes_and_three_bolts_are_hyper() {
        let mut world = test_world();
        let key = flyer(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        for frame in FRAME_ATTAK204..FRAME_ATTAK204 + 8 {
            world.entities.get_mut(key).unwrap().state.frame = frame;
            if (frame - FRAME_ATTAK204) % 2 == 0 {
                flyer_fireleft(&mut world, key);
            } else {
                flyer_fireright(&mut world, key);
            }
            let bolt = world.find_by_classname(None, "bolt").unwrap();
            let ent = world.entities.get(bolt).unwrap();
            let hyper = [FRAME_ATTAK204, FRAME_ATTAK207, FRAME_ATTAK210].contains(&frame);
            assert_eq!(
                ent.state.effects & EF_HYPERBLASTER != 0,
                hyper,
                "frame {frame}"
            );
            assert_eq!(ent.game.dmg, 1);
            let flash =
                [MZ2_FLYER_BLASTER_1, MZ2_FLYER_BLASTER_2][(frame - FRAME_ATTAK204) as usize % 2];
            let (start, _, _) =
                crate::monster::flash::monster_flash_source(&world, key, flash).unwrap();
            assert_eq!(ent.state.origin, start);
            world.free_entity(bolt);
        }
        // No enemy: no shot.
        world.entities.get_mut(key).unwrap().game.enemy = None;
        flyer_fireleft(&mut world, key);
        assert!(world.find_by_classname(None, "bolt").is_none());
    }

    #[test]
    fn flyer_slashes_hit_with_either_wing_and_sound() {
        let (mut world, gi) = hit_world(false);
        let key = flyer(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(50.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        flyer_slash_left(&mut world, key);
        assert_eq!(world.entities.get(player).unwrap().game.health, 95);
        flyer_slash_right(&mut world, key);
        assert_eq!(world.entities.get(player).unwrap().game.health, 90);
        let slash = (CHAN_WEAPON, SOUND_SLASH.to_string(), ATTN_NORM);
        assert_eq!(
            sounds_of(&world, &gi.floor.inner, key),
            [slash.clone(), slash]
        );
        flyer_pop_blades(&mut world, key);
        assert_eq!(
            sounds_of(&world, &gi.floor.inner, key)[2],
            (CHAN_VOICE, SOUND_SPROING.into(), ATTN_NORM)
        );
    }

    #[test]
    fn flyer_sight_and_idle_sounds() {
        let (mut world, gi) = test_world_recording();
        let key = flyer(&mut world);
        flyer_sight(&mut world, key, key);
        flyer_idle(&mut world, key);
        assert_eq!(
            sounds_of(&world, &gi, key),
            [
                (CHAN_VOICE, SOUND_SIGHT.into(), ATTN_NORM),
                (CHAN_VOICE, SOUND_IDLE.into(), ATTN_IDLE)
            ]
        );
    }

    #[test]
    fn flyer_pain_picks_one_of_three_moves_with_its_sound() {
        let (mut world, gi) = test_world_recording();
        let key = flyer(&mut world);
        let mut seen = [0; 3];
        for _ in 0..300 {
            world.entities.get_mut(key).unwrap().game.pain_debounce_time = 0.0;
            flyer_pain(&mut world, key, key, 0.0, 5);
            let sound = sounds_of(&world, &gi, key).pop().unwrap().1;
            let (i, expect) = if playing(&world, key, &FLYER_MOVE_PAIN1) {
                (0, SOUND_PAIN1)
            } else if playing(&world, key, &FLYER_MOVE_PAIN2) {
                (1, SOUND_PAIN2)
            } else {
                assert!(playing(&world, key, &FLYER_MOVE_PAIN3));
                (2, SOUND_PAIN1)
            };
            assert_eq!(sound, expect);
            seen[i] += 1;
        }
        assert!(seen.iter().all(|&n| n > 60), "{seen:?}");
    }

    #[test]
    fn flyer_pain_debounces_skins_and_skips_animations_in_nightmare() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 2.0;
        let key = flyer(&mut world);
        world.entities.get_mut(key).unwrap().game.health = 24;
        flyer_pain(&mut world, key, key, 0.0, 10);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.state.skinnum, ent.game.pain_debounce_time), (1, 5.0));
        let calls = gi.sound_calls.borrow().len();
        flyer_stand(&mut world, key);
        flyer_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(gi.sound_calls.borrow().len(), calls, "debounced");
        world.game.skill = 3;
        world.level.time = 6.0;
        flyer_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            9.0
        );
        assert_eq!(gi.sound_calls.borrow().len(), calls, "nightmare");
        assert!(playing(&world, key, &FLYER_MOVE_STAND));
    }

    #[test]
    fn flyer_die_explodes_with_the_death_sound() {
        let (mut world, gi) = test_world_recording();
        let key = flyer(&mut world);
        let number = world.number_of(key);
        flyer_die(&mut world, key, key, key, 60, Vec3f::ZERO);
        assert!(!world.entities.is_live(key));
        let index = world.gi.sound_index(SOUND_DIE);
        assert!(gi
            .sound_calls
            .borrow()
            .iter()
            .any(|c| (c.0, c.1, c.2) == (number, CHAN_VOICE, index)));
    }

    #[test]
    fn flyer_sights_a_player_and_shoots_blaster_bolts() {
        let (mut world, gi) = hit_world(false);
        world.game.skill = 2;
        let key = flyer(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(400.0, 0.0, 24.0));
        let fired = run_until(&mut world, 80, |w| {
            w.find_by_classname(None, "bolt").is_some()
        });
        assert!(fired.is_some(), "attacked");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(sounds_of(&world, &gi.floor.inner, key)
            .iter()
            .any(|s| s.1 == SOUND_SIGHT));
        assert!(playing(&world, key, &FLYER_MOVE_ATTACK2));
    }

    #[test]
    fn flyer_next_to_a_player_slashes_it() {
        let (mut world, gi) = hit_world(false);
        let key = flyer(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(60.0, 0.0, 64.0));
        let hurt = run_until(&mut world, 80, |w| {
            w.entities.get(player).unwrap().game.health < 100
        });
        assert!(hurt.is_some(), "slashed");
        assert!(playing(&world, key, &FLYER_MOVE_LOOP_MELEE));
    }
}
