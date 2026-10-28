//! Berserker: a melee monster that swings a spike and a club.
//! C ref: `~/Qwasm2/src/game/monster/berserker/berserker.c`.

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
// Animation frames. C ref: berserker.h.
// ---------------------------------------------------------------------------

const FRAME_STAND1: i32 = 0;
const FRAME_STAND5: i32 = 4;
const FRAME_STANDB1: i32 = 5;
const FRAME_STANDB20: i32 = 24;
const FRAME_WALKC1: i32 = 25;
const FRAME_WALKC11: i32 = 35;
const FRAME_RUN1: i32 = 36;
const FRAME_RUN6: i32 = 41;
const FRAME_ATT_C1: i32 = 76;
const FRAME_ATT_C8: i32 = 83;
const FRAME_ATT_C9: i32 = 84;
const FRAME_ATT_C20: i32 = 95;
const FRAME_ATT_C21: i32 = 96;
const FRAME_ATT_C34: i32 = 109;
const FRAME_PAINC1: i32 = 199;
const FRAME_PAINC4: i32 = 202;
const FRAME_PAINB1: i32 = 203;
const FRAME_PAINB20: i32 = 222;
const FRAME_DEATH1: i32 = 223;
const FRAME_DEATH13: i32 = 235;
const FRAME_DEATHC1: i32 = 236;
const FRAME_DEATHC8: i32 = 243;

/// C `MODEL_SCALE` (berserker.h).
const MODEL_SCALE: f32 = 1.0;

// C SP_monster_berserk values.
const HEALTH: i32 = 240;
const MINS: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const MAXS: Vec3f = Vec3f::new(16.0, 16.0, 32.0);
const MASS: i32 = 250;

const SOUND_PAIN: &str = "berserk/berpain2.wav";
const SOUND_DIE: &str = "berserk/berdeth2.wav";
const SOUND_IDLE: &str = "berserk/beridle1.wav";
const SOUND_PUNCH: &str = "berserk/attack.wav";
const SOUND_SEARCH: &str = "berserk/bersrch1.wav";
const SOUND_SIGHT: &str = "berserk/sight.wav";

/// The sounds C `SP_monster_berserk` precaches, in its order.
const PRECACHE: [&str; 6] = [
    SOUND_PAIN,
    SOUND_DIE,
    SOUND_IDLE,
    SOUND_PUNCH,
    SOUND_SEARCH,
    SOUND_SIGHT,
];

// ---------------------------------------------------------------------------
// Moves. C ref: berserker.c `berserk_frames_*` / `berserk_move_*`.
// ---------------------------------------------------------------------------

const fn frame(aifunc: AnimFrameFn, dist: f32, thinkfunc: Option<ThinkFn>) -> MonsterFrame {
    MonsterFrame {
        aifunc: Some(aifunc),
        dist,
        thinkfunc,
    }
}

static BERSERK_FRAMES_STAND: [MonsterFrame; 5] = [
    frame(ai::ai_stand, 0.0, Some(berserk_fidget)),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
];

static BERSERK_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND1,
    lastframe: FRAME_STAND5,
    frames: &BERSERK_FRAMES_STAND,
    endfunc: None,
};

static BERSERK_FRAMES_STAND_FIDGET: [MonsterFrame; 20] = [
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
    frame(ai::ai_stand, 0.0, None),
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

static BERSERK_MOVE_STAND_FIDGET: MonsterMove = MonsterMove {
    firstframe: FRAME_STANDB1,
    lastframe: FRAME_STANDB20,
    frames: &BERSERK_FRAMES_STAND_FIDGET,
    endfunc: Some(berserk_stand),
};

static BERSERK_FRAMES_WALK: [MonsterFrame; 12] = [
    frame(ai::ai_walk, 9.1, None),
    frame(ai::ai_walk, 6.3, None),
    frame(ai::ai_walk, 4.9, None),
    frame(ai::ai_walk, 6.7, Some(berserk_footstep)),
    frame(ai::ai_walk, 6.0, None),
    frame(ai::ai_walk, 8.2, None),
    frame(ai::ai_walk, 7.2, None),
    frame(ai::ai_walk, 6.1, None),
    frame(ai::ai_walk, 4.9, Some(berserk_footstep)),
    frame(ai::ai_walk, 4.7, None),
    frame(ai::ai_walk, 4.7, None),
    frame(ai::ai_walk, 4.8, None),
];

static BERSERK_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_WALKC1,
    lastframe: FRAME_WALKC11,
    frames: &BERSERK_FRAMES_WALK,
    endfunc: None,
};

static BERSERK_FRAMES_RUN1: [MonsterFrame; 6] = [
    frame(ai::ai_run, 21.0, None),
    frame(ai::ai_run, 11.0, Some(berserk_footstep)),
    frame(ai::ai_run, 21.0, None),
    frame(ai::ai_run, 25.0, None),
    frame(ai::ai_run, 18.0, Some(berserk_footstep)),
    frame(ai::ai_run, 19.0, None),
];

static BERSERK_MOVE_RUN1: MonsterMove = MonsterMove {
    firstframe: FRAME_RUN1,
    lastframe: FRAME_RUN6,
    frames: &BERSERK_FRAMES_RUN1,
    endfunc: None,
};

static BERSERK_FRAMES_ATTACK_SPIKE: [MonsterFrame; 8] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(berserk_swing)),
    frame(ai::ai_charge, 0.0, Some(berserk_attack_spike)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

static BERSERK_MOVE_ATTACK_SPIKE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATT_C1,
    lastframe: FRAME_ATT_C8,
    frames: &BERSERK_FRAMES_ATTACK_SPIKE,
    endfunc: Some(berserk_run),
};

static BERSERK_FRAMES_ATTACK_CLUB: [MonsterFrame; 12] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(berserk_footstep)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(berserk_swing)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(berserk_attack_club)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

static BERSERK_MOVE_ATTACK_CLUB: MonsterMove = MonsterMove {
    firstframe: FRAME_ATT_C9,
    lastframe: FRAME_ATT_C20,
    frames: &BERSERK_FRAMES_ATTACK_CLUB,
    endfunc: Some(berserk_run),
};

static BERSERK_FRAMES_ATTACK_STRIKE: [MonsterFrame; 14] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(berserk_footstep)),
    frame(ai::ai_move, 0.0, Some(berserk_swing)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, Some(berserk_strike)),
    frame(ai::ai_move, 0.0, Some(berserk_footstep)),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 9.7, None),
    frame(ai::ai_move, 13.6, Some(berserk_footstep)),
];

/// Never selected in C either.
pub static BERSERK_MOVE_ATTACK_STRIKE: MonsterMove = MonsterMove {
    firstframe: FRAME_ATT_C21,
    lastframe: FRAME_ATT_C34,
    frames: &BERSERK_FRAMES_ATTACK_STRIKE,
    endfunc: Some(berserk_run),
};

static BERSERK_FRAMES_PAIN1: [MonsterFrame; 4] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static BERSERK_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAINC1,
    lastframe: FRAME_PAINC4,
    frames: &BERSERK_FRAMES_PAIN1,
    endfunc: Some(berserk_run),
};

static BERSERK_FRAMES_PAIN2: [MonsterFrame; 20] = [
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
];

static BERSERK_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAINB1,
    lastframe: FRAME_PAINB20,
    frames: &BERSERK_FRAMES_PAIN2,
    endfunc: Some(berserk_run),
};

static BERSERK_FRAMES_DEATH1: [MonsterFrame; 13] = [
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

static BERSERK_MOVE_DEATH1: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH1,
    lastframe: FRAME_DEATH13,
    frames: &BERSERK_FRAMES_DEATH1,
    endfunc: Some(berserk_dead),
};

static BERSERK_FRAMES_DEATH2: [MonsterFrame; 8] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
];

static BERSERK_MOVE_DEATH2: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATHC1,
    lastframe: FRAME_DEATHC8,
    frames: &BERSERK_FRAMES_DEATH2,
    endfunc: Some(berserk_dead),
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
    if let Some(mi) = world
        .entities
        .get_mut(key)
        .and_then(|e| e.monsterinfo.as_mut())
    {
        mi.currentmove = Some(mv.clone());
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
/// sounds (`berserk/step1.wav`, `step2.wav`) never play.
/// C ref: `berserk_footstep`.
pub fn berserk_footstep(_world: &mut GameWorld, _key: EntityKey) {}

/// C ref: `berserk_sight`.
pub fn berserk_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `berserk_search`.
pub fn berserk_search(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SEARCH, ATTN_NORM);
}

/// C ref: `berserk_stand`.
pub fn berserk_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &BERSERK_MOVE_STAND);
}

/// Fidget one stand in seven, unless angry or holding ground.
/// C ref: `berserk_fidget`.
pub fn berserk_fidget(world: &mut GameWorld, key: EntityKey) {
    if world
        .entities
        .get(key)
        .is_none_or(|e| e.game.enemy.is_some())
    {
        return;
    }
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        return;
    }
    if world.random() > 0.15 {
        return;
    }
    set_move(world, key, &BERSERK_MOVE_STAND_FIDGET);
    sound(world, key, CHAN_WEAPON, SOUND_IDLE, ATTN_IDLE);
}

/// C ref: `berserk_walk`.
pub fn berserk_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &BERSERK_MOVE_WALK);
}

/// C ref: `berserk_run`.
pub fn berserk_run(world: &mut GameWorld, key: EntityKey) {
    if aiflags(world, key).contains(AiFlags::STAND_GROUND) {
        set_move(world, key, &BERSERK_MOVE_STAND);
    } else {
        set_move(world, key, &BERSERK_MOVE_RUN1);
    }
}

/// The faster spike: 15 to 20 damage, upwards and backwards.
/// C ref: `berserk_attack_spike`.
pub fn berserk_attack_spike(world: &mut GameWorld, key: EntityKey) {
    let aim = Vec3f::new(ai::MELEE_DISTANCE, 0.0, -24.0);
    let damage = 15 + (world.random_u32() % 6) as i32;
    world.fire_hit(key, aim, damage, 400);
}

/// C ref: `berserk_swing`.
pub fn berserk_swing(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_PUNCH, ATTN_NORM);
}

/// The slower club: 5 to 10 damage. C ref: `berserk_attack_club`.
pub fn berserk_attack_club(world: &mut GameWorld, key: EntityKey) {
    let Some(mins_x) = world.entities.get(key).map(|e| e.mins.x) else {
        return;
    };
    let aim = Vec3f::new(ai::MELEE_DISTANCE, mins_x, -4.0);
    let damage = 5 + (world.random_u32() % 6) as i32;
    world.fire_hit(key, aim, damage, 400);
}

/// Does nothing in C either ("Unused, but removal is very PITA"), on the
/// strike move nothing selects. C ref: `berserk_strike`.
pub fn berserk_strike(_world: &mut GameWorld, _key: EntityKey) {}

/// Spike or club, evenly. C ref: `berserk_melee`.
pub fn berserk_melee(world: &mut GameWorld, key: EntityKey) {
    if world.random_u32().is_multiple_of(2) {
        set_move(world, key, &BERSERK_MOVE_ATTACK_SPIKE);
    } else {
        set_move(world, key, &BERSERK_MOVE_ATTACK_CLUB);
    }
}

/// C ref: `berserk_pain`.
pub fn berserk_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    damage: i32,
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
    sound(world, key, CHAN_VOICE, SOUND_PAIN, ATTN_NORM);
    if world.game.skill == SKILL_HARDPLUS {
        return; // no pain anims in nightmare
    }
    if damage < 20 || world.random() < 0.5 {
        set_move(world, key, &BERSERK_MOVE_PAIN1);
    } else {
        set_move(world, key, &BERSERK_MOVE_PAIN2);
    }
}

/// The corpse box, then stop. C ref: `berserk_dead`.
pub fn berserk_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// Gib at `gib_health`, else fall over: the long death for 50 damage or
/// more. C ref: `berserk_die`.
pub fn berserk_die(
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
    if damage >= 50 {
        set_move(world, key, &BERSERK_MOVE_DEATH1);
    } else {
        set_move(world, key, &BERSERK_MOVE_DEATH2);
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// The `EntityStorage` half of C `SP_monster_berserk`; `monster_start`
/// (run by `call_spawn`) removes it in deathmatch.
pub fn sp_monster_berserker(
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
        .unwrap_or_else(|| "monster_berserk".to_string());
    ent.mins = MINS;
    ent.maxs = MAXS;
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.game.health = HEALTH;
    ent.game.gib_health = -60;
    ent.game.mass = MASS;
    ent.pain = Some(berserk_pain);
    ent.die = Some(berserk_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(berserk_stand),
        walk: Some(berserk_walk),
        run: Some(berserk_run),
        dodge: None,
        attack: None,
        melee: Some(berserk_melee),
        sight: Some(berserk_sight),
        search: Some(berserk_search),
        currentmove: Some(BERSERK_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::walkmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_berserk`: its sound precaches.
pub fn sp_monster_berserker_finish(
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
    use crate::monster::test_support::{monster_arena, run, sounds};
    use crate::world::{test_world, test_world_recording, MockGameImport};

    type CFrame = (AnimFrameFn, f32, Option<ThinkFn>);
    type CMove = (&'static MonsterMove, i32, i32, Vec<CFrame>, Option<ThinkFn>);

    /// (move, firstframe, lastframe, [(aifunc, dist, thinkfunc)], endfunc) from C.
    #[rustfmt::skip]
    fn c_moves() -> Vec<CMove> {
        vec![
            (&BERSERK_MOVE_STAND, 0, 4, vec![
                (ai::ai_stand as AnimFrameFn, 0.0, Some(berserk_fidget as ThinkFn)),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
                (ai::ai_stand as AnimFrameFn, 0.0, None),
            ], None),
            (&BERSERK_MOVE_STAND_FIDGET, 5, 24, vec![
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
            ], Some(berserk_stand as ThinkFn)),
            (&BERSERK_MOVE_WALK, 25, 35, vec![
                (ai::ai_walk as AnimFrameFn, 9.1, None),
                (ai::ai_walk as AnimFrameFn, 6.3, None),
                (ai::ai_walk as AnimFrameFn, 4.9, None),
                (ai::ai_walk as AnimFrameFn, 6.7, Some(berserk_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 6.0, None),
                (ai::ai_walk as AnimFrameFn, 8.2, None),
                (ai::ai_walk as AnimFrameFn, 7.2, None),
                (ai::ai_walk as AnimFrameFn, 6.1, None),
                (ai::ai_walk as AnimFrameFn, 4.9, Some(berserk_footstep as ThinkFn)),
                (ai::ai_walk as AnimFrameFn, 4.7, None),
                (ai::ai_walk as AnimFrameFn, 4.7, None),
                (ai::ai_walk as AnimFrameFn, 4.8, None),
            ], None),
            (&BERSERK_MOVE_RUN1, 36, 41, vec![
                (ai::ai_run as AnimFrameFn, 21.0, None),
                (ai::ai_run as AnimFrameFn, 11.0, Some(berserk_footstep as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 21.0, None),
                (ai::ai_run as AnimFrameFn, 25.0, None),
                (ai::ai_run as AnimFrameFn, 18.0, Some(berserk_footstep as ThinkFn)),
                (ai::ai_run as AnimFrameFn, 19.0, None),
            ], None),
            (&BERSERK_MOVE_ATTACK_SPIKE, 76, 83, vec![
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(berserk_swing as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(berserk_attack_spike as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
            ], Some(berserk_run as ThinkFn)),
            (&BERSERK_MOVE_ATTACK_CLUB, 84, 95, vec![
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(berserk_footstep as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(berserk_swing as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, Some(berserk_attack_club as ThinkFn)),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
                (ai::ai_charge as AnimFrameFn, 0.0, None),
            ], Some(berserk_run as ThinkFn)),
            (&BERSERK_MOVE_ATTACK_STRIKE, 96, 109, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, Some(berserk_footstep as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 0.0, Some(berserk_swing as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, Some(berserk_strike as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 0.0, Some(berserk_footstep as ThinkFn)),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 9.7, None),
                (ai::ai_move as AnimFrameFn, 13.6, Some(berserk_footstep as ThinkFn)),
            ], Some(berserk_run as ThinkFn)),
            (&BERSERK_MOVE_PAIN1, 199, 202, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(berserk_run as ThinkFn)),
            (&BERSERK_MOVE_PAIN2, 203, 222, vec![
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
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(berserk_run as ThinkFn)),
            (&BERSERK_MOVE_DEATH1, 223, 235, vec![
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
            ], Some(berserk_dead as ThinkFn)),
            (&BERSERK_MOVE_DEATH2, 236, 243, vec![
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
                (ai::ai_move as AnimFrameFn, 0.0, None),
            ], Some(berserk_dead as ThinkFn)),
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
    fn berserker_moves_match_c_frame_by_frame() {
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
    fn berserker_walk_keeps_c_twelfth_frame_that_the_eleven_frame_range_never_plays() {
        let mv = &BERSERK_MOVE_WALK;
        assert_eq!(mv.lastframe - mv.firstframe + 1, 11);
        assert_eq!(mv.frames.len(), 12);
    }

    fn spawned(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        sp_monster_berserker(&mut world.entities, key, &HashMap::new());
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

    #[test]
    fn berserker_spawns_with_its_c_pain_walk_run_and_melee_callbacks() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert!(ent.pain.is_some());
        assert!(mi.walk.is_some() && mi.run.is_some() && mi.melee.is_some());
    }

    #[test]
    fn berserker_spawns_with_the_c_values_and_callbacks() {
        let mut world = test_world();
        let key = spawned(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.classname, "monster_berserk");
        assert_eq!((ent.game.health, ent.game.mass), (240, 250));
        assert_eq!((ent.mins, ent.maxs), (MINS, MAXS));
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert_eq!(ent.solid, Solid::Bbox);
        assert!(std::ptr::fn_addr_eq(
            ent.think.unwrap(),
            ai::walkmonster_start_go as ThinkFn
        ));
        let mi = ent.monsterinfo.as_ref().unwrap();
        assert_eq!(mi.scale, 1.0);
        assert!(mi.attack.is_none() && mi.dodge.is_none() && mi.idle.is_none());
        assert!(mi.sight.is_some() && mi.search.is_some() && mi.stand.is_some());
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND));
    }

    #[test]
    fn berserker_finish_precaches_the_c_sounds_and_the_spawn_row_uses_it() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        let row = crate::spawn::find_spawn("monster_berserk").unwrap();
        assert_eq!(row.2, Some("models/monsters/berserk/tris.md2"));
        row.3.unwrap()(&mut world, key, &HashMap::new());
        assert_eq!(*gi.sounds.borrow(), PRECACHE);
    }

    #[test]
    fn berserker_sight_and_search_play_their_voice_sounds() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        berserk_sight(&mut world, key, key);
        berserk_search(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [
                (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM),
                (CHAN_VOICE, SOUND_SEARCH.to_string(), ATTN_NORM)
            ]
        );
    }

    #[test]
    fn berserker_fidgets_with_its_idle_sound_on_a_low_roll_only() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        seed_random(&mut world, |r| r > 0.15);
        berserk_fidget(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND));

        seed_random(&mut world, |r| r <= 0.15);
        berserk_fidget(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND_FIDGET));
        assert_eq!(
            sounds(&gi),
            [(CHAN_WEAPON, SOUND_IDLE.to_string(), ATTN_IDLE)]
        );
    }

    #[test]
    fn berserker_never_fidgets_with_an_enemy_or_holding_ground() {
        let mut world = test_world();
        let key = spawned(&mut world);
        seed_random(&mut world, |r| r <= 0.15);
        world.entities.get_mut(key).unwrap().game.enemy = Some(key);
        berserk_fidget(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND));

        world.entities.get_mut(key).unwrap().game.enemy = None;
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        berserk_fidget(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND));
    }

    #[test]
    fn berserker_walks_and_runs_unless_holding_ground() {
        let mut world = test_world();
        let key = spawned(&mut world);
        berserk_walk(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_WALK));
        berserk_run(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_RUN1));
        mi_mut(&mut world, key).aiflags = AiFlags::STAND_GROUND;
        berserk_run(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND));
    }

    #[test]
    fn berserker_melee_picks_spike_on_even_randk_and_club_on_odd() {
        let mut world = test_world();
        let key = spawned(&mut world);
        seed_randk(&mut world, |r| r % 2 == 0);
        berserk_melee(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_ATTACK_SPIKE));
        seed_randk(&mut world, |r| r % 2 == 1);
        berserk_melee(&mut world, key);
        assert!(is_move(&world, key, &BERSERK_MOVE_ATTACK_CLUB));
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
    fn berserker_spike_hits_for_15_to_20_and_club_for_5_to_10() {
        for (attack, low) in [
            (berserk_attack_spike as ThinkFn, 15),
            (berserk_attack_club as ThinkFn, 5),
        ] {
            for k in 0..6 {
                let (mut world, gi) = test_world_recording();
                let key = spawned(&mut world);
                let enemy = enemy_in_reach(&mut world, &gi, key);
                seed_randk(&mut world, |r| r % 6 == k);
                attack(&mut world, key);
                let health = world.entities.get(enemy).unwrap().game.health;
                assert_eq!(health, 100 - (low + k as i32));
            }
        }
    }

    #[test]
    fn berserker_swing_plays_the_punch_sound() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        berserk_swing(&mut world, key);
        assert_eq!(
            sounds(&gi),
            [(CHAN_WEAPON, SOUND_PUNCH.to_string(), ATTN_NORM)]
        );
    }

    #[test]
    fn berserker_pain_turns_the_skin_below_half_health_even_while_debounced() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.level.time = 5.0;
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.max_health = 240;
        ent.game.health = 119;
        ent.game.pain_debounce_time = 6.0;
        berserk_pain(&mut world, key, key, 0.0, 10);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.skinnum, 1);
        assert_eq!(ent.game.pain_debounce_time, 6.0);
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND));
    }

    #[test]
    fn berserker_pain_debounces_three_seconds_and_plays_its_sound() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.level.time = 5.0;
        world.entities.get_mut(key).unwrap().game.max_health = 240;
        berserk_pain(&mut world, key, key, 0.0, 10);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.skinnum, 0);
        assert_eq!(ent.game.pain_debounce_time, 8.0);
        assert_eq!(
            sounds(&gi),
            [(CHAN_VOICE, SOUND_PAIN.to_string(), ATTN_NORM)]
        );
        assert!(is_move(&world, key, &BERSERK_MOVE_PAIN1));
    }

    #[test]
    fn berserker_pain_under_20_damage_or_on_a_low_roll_is_short() {
        let mut world = test_world();
        let key = spawned(&mut world);
        seed_random(&mut world, |r| r >= 0.5);
        berserk_pain(&mut world, key, key, 0.0, 19);
        assert!(is_move(&world, key, &BERSERK_MOVE_PAIN1));

        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 0.0;
        seed_random(&mut world, |r| r < 0.5);
        berserk_pain(&mut world, key, key, 0.0, 20);
        assert!(is_move(&world, key, &BERSERK_MOVE_PAIN1));

        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 0.0;
        seed_random(&mut world, |r| r >= 0.5);
        berserk_pain(&mut world, key, key, 0.0, 20);
        assert!(is_move(&world, key, &BERSERK_MOVE_PAIN2));
    }

    #[test]
    fn berserker_pain_in_nightmare_plays_the_sound_without_an_animation() {
        let (mut world, gi) = test_world_recording();
        world.game.skill = 3;
        let key = spawned(&mut world);
        berserk_pain(&mut world, key, key, 0.0, 50);
        assert_eq!(sounds(&gi).len(), 1);
        assert!(is_move(&world, key, &BERSERK_MOVE_STAND));
    }

    #[test]
    fn berserker_at_its_c_gib_health_throws_six_gibs() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -60;
        let before = world.entities.count();
        let die = world.entities.get(key).unwrap().die.unwrap();
        die(&mut world, key, key, key, 100, Vec3f::ZERO);
        assert_eq!(world.entities.count(), before + 6);
    }

    #[test]
    fn berserker_gibbing_plays_udeath_and_leaves_a_dead_head() {
        let (mut world, gi) = test_world_recording();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -60;
        berserk_die(&mut world, key, key, key, 100, Vec3f::ZERO);
        assert_eq!(
            sounds(&gi),
            [(CHAN_VOICE, "misc/udeath.wav".to_string(), ATTN_NORM)]
        );
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.state.effects & EF_GIB, EF_GIB);
    }

    #[test]
    fn berserker_above_gib_health_dies_by_damage_and_only_once() {
        for (damage, death) in [(50, &BERSERK_MOVE_DEATH1), (49, &BERSERK_MOVE_DEATH2)] {
            let (mut world, gi) = test_world_recording();
            let key = spawned(&mut world);
            world.entities.get_mut(key).unwrap().game.health = -59;
            let before = world.entities.count();
            berserk_die(&mut world, key, key, key, damage, Vec3f::ZERO);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(ent.game.deadflag, DeadFlag::Dead);
            assert_eq!(ent.game.takedamage, TakeDamage::Yes);
            assert_eq!(world.entities.count(), before);
            assert!(is_move(&world, key, death));
            assert_eq!(
                sounds(&gi),
                [(CHAN_VOICE, SOUND_DIE.to_string(), ATTN_NORM)]
            );

            berserk_die(&mut world, key, key, key, 10, Vec3f::ZERO);
            assert!(is_move(&world, key, death));
            assert_eq!(sounds(&gi).len(), 1);
        }
    }

    #[test]
    fn berserker_dead_shrinks_to_the_corpse_box_and_stops_thinking() {
        let mut world = test_world();
        let key = spawned(&mut world);
        world.entities.get_mut(key).unwrap().game.nextthink = 9.0;
        berserk_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs, Vec3f::new(16.0, 16.0, -8.0));
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(ent.game.nextthink, 0.0);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
    }

    #[test]
    fn berserker_sights_a_player_runs_at_it_and_swings() {
        let (mut world, gi, key, player) =
            monster_arena(sp_monster_berserker, sp_monster_berserker_finish, 300.0);
        let (mut ran, mut swung) = (false, false);
        run(&mut world, key, 60, |w| {
            ran |= is_move(w, key, &BERSERK_MOVE_RUN1);
            swung |= is_move(w, key, &BERSERK_MOVE_ATTACK_SPIKE)
                || is_move(w, key, &BERSERK_MOVE_ATTACK_CLUB);
        });
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        let played = sounds(&gi.floor.inner);
        assert_eq!(played[0], (CHAN_VOICE, SOUND_SIGHT.to_string(), ATTN_NORM));
        assert!(ran && swung, "ran {ran} swung {swung}");
        assert!(played.iter().any(|s| s.1 == SOUND_PUNCH));
        // FloorImport-based traces reach the player, so the swings land.
        assert!(world.entities.get(player).unwrap().game.health < 100);
    }
}
