//! Icarus (hover): flies, fires blaster bursts, falls and explodes when
//! killed. C ref: `~/Qwasm2/src/game/monster/hover/hover.c`.

use q2_shared::constants::{EF_HYPERBLASTER, MZ2_HOVER_BLASTER_1};
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::misc::{become_explosion1, throw_gib, throw_head, GibType};
use crate::monster::{fire::monster_fire_blaster, flash::monster_flash_source};
use crate::monster::{frame, set_move};
use crate::world::GameWorld;
use std::collections::HashMap;

const SOUND_PAIN1: &str = "hover/hovpain1.wav";
const SOUND_PAIN2: &str = "hover/hovpain2.wav";
const SOUND_DEATH1: &str = "hover/hovdeth1.wav";
const SOUND_DEATH2: &str = "hover/hovdeth2.wav";
const SOUND_SIGHT: &str = "hover/hovsght1.wav";
const SOUND_SEARCH1: &str = "hover/hovsrch1.wav";
const SOUND_SEARCH2: &str = "hover/hovsrch2.wav";
/// Every sound C `SP_monster_hover` indexes; `hovatck1` is precached but
/// never played.
const SOUNDS: [&str; 8] = [
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_DEATH1,
    SOUND_DEATH2,
    SOUND_SIGHT,
    SOUND_SEARCH1,
    SOUND_SEARCH2,
    "hover/hovatck1.wav",
];
/// C `self->s.sound`, the engine hum.
const SOUND_IDLE_LOOP: &str = "hover/hovidle1.wav";

/// C `MODEL_SCALE` (hover.h).
const MODEL_SCALE: f32 = 1.0;

// ---------------------------------------------------------------------------
// Moves. C ref: the `mframe_t`/`mmove_t` tables, frame by frame.
// ---------------------------------------------------------------------------

// C ref: hover.h (`FRAME_*`, model frame numbers).

const FRAME_STAND01: i32 = 0;
const FRAME_STAND30: i32 = 29;
const FRAME_FORWRD01: i32 = 30;
const FRAME_FORWRD35: i32 = 64;
const FRAME_STOP101: i32 = 65;
const FRAME_STOP109: i32 = 73;
const FRAME_STOP201: i32 = 74;
const FRAME_STOP208: i32 = 81;
const FRAME_TAKEOF01: i32 = 82;
const FRAME_TAKEOF30: i32 = 111;
const FRAME_LAND01: i32 = 112;
const FRAME_PAIN101: i32 = 113;
const FRAME_PAIN128: i32 = 140;
const FRAME_PAIN201: i32 = 141;
const FRAME_PAIN212: i32 = 152;
const FRAME_PAIN301: i32 = 153;
const FRAME_PAIN309: i32 = 161;
const FRAME_DEATH101: i32 = 162;
const FRAME_DEATH111: i32 = 172;
const FRAME_BACKWD01: i32 = 173;
const FRAME_BACKWD24: i32 = 196;
const FRAME_ATTAK101: i32 = 197;
const FRAME_ATTAK103: i32 = 199;
const FRAME_ATTAK104: i32 = 200;
const FRAME_ATTAK106: i32 = 202;
const FRAME_ATTAK107: i32 = 203;
const FRAME_ATTAK108: i32 = 204;

static HOVER_FRAMES_STAND: [MonsterFrame; 30] = [frame(ai::ai_stand, 0.0, None); 30];

pub static HOVER_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND01,
    lastframe: FRAME_STAND30,
    frames: &HOVER_FRAMES_STAND,
    endfunc: None,
};

static HOVER_FRAMES_STOP1: [MonsterFrame; 9] = [frame(ai::ai_move, 0.0, None); 9];

pub static HOVER_MOVE_STOP1: MonsterMove = MonsterMove {
    firstframe: FRAME_STOP101,
    lastframe: FRAME_STOP109,
    frames: &HOVER_FRAMES_STOP1,
    endfunc: None,
};

static HOVER_FRAMES_STOP2: [MonsterFrame; 8] = [frame(ai::ai_move, 0.0, None); 8];

pub static HOVER_MOVE_STOP2: MonsterMove = MonsterMove {
    firstframe: FRAME_STOP201,
    lastframe: FRAME_STOP208,
    frames: &HOVER_FRAMES_STOP2,
    endfunc: None,
};

static HOVER_FRAMES_TAKEOFF: [MonsterFrame; 30] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -2.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, -1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -9.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
];

pub static HOVER_MOVE_TAKEOFF: MonsterMove = MonsterMove {
    firstframe: FRAME_TAKEOF01,
    lastframe: FRAME_TAKEOF30,
    frames: &HOVER_FRAMES_TAKEOFF,
    endfunc: None,
};

static HOVER_FRAMES_PAIN3: [MonsterFrame; 9] = [frame(ai::ai_move, 0.0, None); 9];

pub static HOVER_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN309,
    frames: &HOVER_FRAMES_PAIN3,
    endfunc: Some(hover_run),
};

static HOVER_FRAMES_PAIN2: [MonsterFrame; 12] = [frame(ai::ai_move, 0.0, None); 12];

pub static HOVER_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN212,
    frames: &HOVER_FRAMES_PAIN2,
    endfunc: Some(hover_run),
};

static HOVER_FRAMES_PAIN1: [MonsterFrame; 28] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, -8.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -6.0, None),
    frame(ai::ai_move, -4.0, None),
    frame(ai::ai_move, -3.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 7.0, None),
    frame(ai::ai_move, 1.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 2.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 4.0, None),
];

pub static HOVER_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN128,
    frames: &HOVER_FRAMES_PAIN1,
    endfunc: Some(hover_run),
};

static HOVER_FRAMES_LAND: [MonsterFrame; 1] = [frame(ai::ai_move, 0.0, None); 1];

pub static HOVER_MOVE_LAND: MonsterMove = MonsterMove {
    firstframe: FRAME_LAND01,
    lastframe: FRAME_LAND01,
    frames: &HOVER_FRAMES_LAND,
    endfunc: None,
};

static HOVER_FRAMES_FORWARD: [MonsterFrame; 35] = [frame(ai::ai_move, 0.0, None); 35];

pub static HOVER_MOVE_FORWARD: MonsterMove = MonsterMove {
    firstframe: FRAME_FORWRD01,
    lastframe: FRAME_FORWRD35,
    frames: &HOVER_FRAMES_FORWARD,
    endfunc: None,
};

static HOVER_FRAMES_WALK: [MonsterFrame; 35] = [frame(ai::ai_walk, 4.0, None); 35];

pub static HOVER_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_FORWRD01,
    lastframe: FRAME_FORWRD35,
    frames: &HOVER_FRAMES_WALK,
    endfunc: None,
};

static HOVER_FRAMES_RUN: [MonsterFrame; 35] = [frame(ai::ai_run, 10.0, None); 35];

pub static HOVER_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_FORWRD01,
    lastframe: FRAME_FORWRD35,
    frames: &HOVER_FRAMES_RUN,
    endfunc: None,
};

static HOVER_FRAMES_DEATH1: [MonsterFrame; 11] = [
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, 0.0, None),
    frame(ai::ai_move, -10.0, None),
    frame(ai::ai_move, 3.0, None),
    frame(ai::ai_move, 5.0, None),
    frame(ai::ai_move, 4.0, None),
    frame(ai::ai_move, 7.0, None),
];

pub static HOVER_MOVE_DEATH1: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH101,
    lastframe: FRAME_DEATH111,
    frames: &HOVER_FRAMES_DEATH1,
    endfunc: Some(hover_dead),
};

static HOVER_FRAMES_BACKWARD: [MonsterFrame; 24] = [frame(ai::ai_move, 0.0, None); 24];

pub static HOVER_MOVE_BACKWARD: MonsterMove = MonsterMove {
    firstframe: FRAME_BACKWD01,
    lastframe: FRAME_BACKWD24,
    frames: &HOVER_FRAMES_BACKWARD,
    endfunc: None,
};

static HOVER_FRAMES_START_ATTACK: [MonsterFrame; 3] = [frame(ai::ai_charge, 1.0, None); 3];

pub static HOVER_MOVE_START_ATTACK: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK103,
    frames: &HOVER_FRAMES_START_ATTACK,
    endfunc: Some(hover_attack),
};

static HOVER_FRAMES_ATTACK1: [MonsterFrame; 3] = [
    frame(ai::ai_charge, -10.0, Some(hover_fire_blaster)),
    frame(ai::ai_charge, -10.0, Some(hover_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(hover_reattack)),
];

pub static HOVER_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK104,
    lastframe: FRAME_ATTAK106,
    frames: &HOVER_FRAMES_ATTACK1,
    endfunc: None,
};

static HOVER_FRAMES_END_ATTACK: [MonsterFrame; 2] = [frame(ai::ai_charge, 1.0, None); 2];

pub static HOVER_MOVE_END_ATTACK: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK107,
    lastframe: FRAME_ATTAK108,
    frames: &HOVER_FRAMES_END_ATTACK,
    endfunc: Some(hover_run),
};

// ---------------------------------------------------------------------------
// Callbacks. C ref: hover.c.
// ---------------------------------------------------------------------------

/// Every hover sound plays on `CHAN_VOICE` at `ATTN_NORM`.
fn sound(world: &GameWorld, key: EntityKey, name: &str) {
    crate::monster::sound(world, key, CHAN_VOICE, name, ATTN_NORM);
}

/// C ref: `hover_sight`.
pub fn hover_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, SOUND_SIGHT);
}

/// C ref: `hover_search`.
pub fn hover_search(world: &mut GameWorld, key: EntityKey) {
    let name = if world.random() < 0.5 {
        SOUND_SEARCH1
    } else {
        SOUND_SEARCH2
    };
    sound(world, key, name);
}

/// Keep firing at a live enemy in sight six times in ten, else stop.
/// C ref: `hover_reattack`.
pub fn hover_reattack(world: &mut GameWorld, key: EntityKey) {
    let enemy = world.entities.get(key).and_then(|e| e.game.enemy);
    if let Some(enemy) = enemy {
        let alive = world.entities.get(enemy).is_some_and(|e| e.game.health > 0);
        if alive && ai::visible(world, key, enemy) && world.random() <= 0.6 {
            set_move(world, key, &HOVER_MOVE_ATTACK1);
            return;
        }
    }
    set_move(world, key, &HOVER_MOVE_END_ATTACK);
}

/// A bolt at the enemy's eye, the burst's first frame a hyperblaster one.
/// C ref: `hover_fire_blaster`.
pub fn hover_fire_blaster(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let effect = if ent.state.frame == FRAME_ATTAK104 {
        EF_HYPERBLASTER
    } else {
        0
    };
    let Some(enemy) = ent.game.enemy.and_then(|e| world.entities.get(e)) else {
        return;
    };
    let mut end = enemy.state.origin;
    end.z += enemy.game.viewheight;
    let Some((start, _, _)) = monster_flash_source(world, key, MZ2_HOVER_BLASTER_1) else {
        return;
    };
    monster_fire_blaster(
        world,
        key,
        start,
        end - start,
        1,
        1000,
        MZ2_HOVER_BLASTER_1,
        effect,
    );
}

/// C ref: `hover_stand`.
pub fn hover_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &HOVER_MOVE_STAND);
}

/// C ref: `hover_run`.
pub fn hover_run(world: &mut GameWorld, key: EntityKey) {
    let stand_ground = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    let mv = if stand_ground {
        &HOVER_MOVE_STAND
    } else {
        &HOVER_MOVE_RUN
    };
    set_move(world, key, mv);
}

/// C ref: `hover_walk`.
pub fn hover_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &HOVER_MOVE_WALK);
}

/// C ref: `hover_start_attack`.
pub fn hover_start_attack(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &HOVER_MOVE_START_ATTACK);
}

/// C ref: `hover_attack`.
pub fn hover_attack(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &HOVER_MOVE_ATTACK1);
}

/// Hurt skin below half health; at most every 3 s (never in nightmare) a
/// short pain for up to 25 damage, the long one above. C ref: `hover_pain`.
pub fn hover_pain(
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
    if world.game.skill == 3 {
        return; // No pain anims in nightmare.
    }
    let (name, mv) = if damage > 25 {
        (SOUND_PAIN1, &HOVER_MOVE_PAIN1)
    } else if world.random() < 0.5 {
        (SOUND_PAIN1, &HOVER_MOVE_PAIN3)
    } else {
        (SOUND_PAIN2, &HOVER_MOVE_PAIN2)
    };
    sound(world, key, name);
    set_move(world, key, mv);
}

/// The falling wreck explodes when it lands or at its `timestamp`.
/// C ref: `hover_deadthink`.
pub fn hover_deadthink(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.game.ground_entity.is_none() && time < ent.game.timestamp {
        ent.game.nextthink = time + FRAMETIME;
        return;
    }
    become_explosion1(world, key);
}

/// End of the death move: drop as a small box and explode within 15 s.
/// Unlike most `*_dead`, C leaves `SVF_DEADMONSTER` off. C ref: `hover_dead`.
pub fn hover_dead(world: &mut GameWorld, key: EntityKey) {
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    ent.game.movetype = MoveType::Toss;
    ent.think = Some(hover_deadthink);
    ent.game.nextthink = time + FRAMETIME;
    ent.game.timestamp = time + 15.0;
    world.link_entity(key);
}

/// Gib at `gib_health` or below, else play one of two deaths once.
/// C ref: `hover_die`.
pub fn hover_die(
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
        sound(world, key, "misc/udeath.wav");
        for _ in 0..2 {
            throw_gib(
                world,
                key,
                "models/objects/gibs/bone/tris.md2",
                damage,
                GibType::Organic,
            );
        }
        for _ in 0..2 {
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
            "models/objects/gibs/sm_meat/tris.md2",
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
    let name = if world.random() < 0.5 {
        SOUND_DEATH1
    } else {
        SOUND_DEATH2
    };
    sound(world, key, name);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    set_move(world, key, &HOVER_MOVE_DEATH1);
}

// ---------------------------------------------------------------------------
// Spawn. C ref: `SP_monster_hover`, split: the model is the
// `spawn::SPAWNS` row's and the sounds are indexed by `finish_hover`.
// ---------------------------------------------------------------------------

pub fn sp_monster_hover(
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
        .unwrap_or_else(|| "monster_hover".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-24.0, -24.0, -24.0);
    ent.maxs = Vec3f::new(24.0, 24.0, 32.0);
    ent.game.health = 240;
    ent.game.gib_health = -100;
    ent.game.mass = 150;
    ent.pain = Some(hover_pain);
    ent.die = Some(hover_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(hover_stand),
        walk: Some(hover_walk),
        run: Some(hover_run),
        attack: Some(hover_start_attack),
        sight: Some(hover_sight),
        search: Some(hover_search),
        currentmove: Some(HOVER_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::flymonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_hover`: precache its sounds and
/// start the engine hum.
pub fn finish_hover(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    for name in SOUNDS {
        world.gi.sound_index(name);
    }
    let hum = world.gi.sound_index(SOUND_IDLE_LOOP);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.sound = hum;
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ThinkFn;
    use crate::monster::test_support::*;
    use crate::world::{test_world, test_world_recording};

    const NAMES: &[(ThinkFn, &str)] = &[
        (hover_run, "hover_run"),
        (hover_dead, "hover_dead"),
        (hover_attack, "hover_attack"),
        (hover_fire_blaster, "hover_fire_blaster"),
        (hover_reattack, "hover_reattack"),
    ];

    #[test]
    fn hover_moves_match_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&HOVER_MOVE_STAND, 0, 29, "-", "stand:0:-*30"),
            (&HOVER_MOVE_STOP1, 65, 73, "-", "move:0:-*9"),
            (&HOVER_MOVE_STOP2, 74, 81, "-", "move:0:-*8"),
            (&HOVER_MOVE_TAKEOFF, 82, 111, "-", "move:0:- move:-2:- move:5:- move:-1:- move:1:- move:0:-*2 move:-1:-*3 move:0:- move:2:-*2 move:1:-*2 move:-6:- move:-9:- move:1:- move:0:- move:2:-*2 move:1:-*3 move:2:- move:0:- move:2:- move:3:- move:2:- move:0:-"),
            (&HOVER_MOVE_PAIN3, 153, 161, "hover_run", "move:0:-*9"),
            (&HOVER_MOVE_PAIN2, 141, 152, "hover_run", "move:0:-*12"),
            (&HOVER_MOVE_PAIN1, 113, 140, "hover_run", "move:0:-*2 move:2:- move:-8:- move:-4:- move:-6:- move:-4:- move:-3:- move:1:- move:0:-*3 move:3:- move:1:- move:0:- move:2:- move:3:- move:2:- move:7:- move:1:- move:0:-*2 move:2:- move:0:-*2 move:5:- move:3:- move:4:-"),
            (&HOVER_MOVE_LAND, 112, 112, "-", "move:0:-"),
            (&HOVER_MOVE_FORWARD, 30, 64, "-", "move:0:-*35"),
            (&HOVER_MOVE_WALK, 30, 64, "-", "walk:4:-*35"),
            (&HOVER_MOVE_RUN, 30, 64, "-", "run:10:-*35"),
            (&HOVER_MOVE_DEATH1, 162, 172, "hover_dead", "move:0:-*6 move:-10:- move:3:- move:5:- move:4:- move:7:-"),
            (&HOVER_MOVE_BACKWARD, 173, 196, "-", "move:0:-*24"),
            (&HOVER_MOVE_START_ATTACK, 197, 199, "hover_attack", "charge:1:-*3"),
            (&HOVER_MOVE_ATTACK1, 200, 202, "-", "charge:-10:hover_fire_blaster*2 charge:0:hover_reattack"),
            (&HOVER_MOVE_END_ATTACK, 203, 204, "hover_run", "charge:1:-*2"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    fn hover(world: &mut GameWorld) -> EntityKey {
        spawn_monster(world, "monster_hover", Vec3f::new(0.0, 0.0, 64.0))
    }

    #[test]
    fn hover_spawns_with_the_c_values_and_flies() {
        let (mut world, gi) = test_world_recording();
        let key = hover(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.health, ent.game.max_health, ent.game.mass),
            (240, 240, 150)
        );
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-24.0, -24.0, -24.0),
                Vec3f::new(24.0, 24.0, 32.0)
            )
        );
        assert!(ent.game.flags.contains(EntityFlags::FLY));
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::flymonster_start_go as ThinkFn)));
        assert_eq!(ent.state.sound, world.gi.sound_index(SOUND_IDLE_LOOP));
        let mi = mi(&world, key);
        assert_eq!(mi.scale, 1.0);
        assert!(mi
            .attack
            .is_some_and(|f| std::ptr::fn_addr_eq(f, hover_start_attack as ThinkFn)));
        assert!(mi.melee.is_none() && mi.idle.is_none());
        assert!(mi.search.is_some() && mi.sight.is_some() && mi.walk.is_some() && mi.run.is_some());
        assert!(playing(&world, key, &HOVER_MOVE_STAND));
        for s in SOUNDS {
            assert!(gi.sounds.borrow().iter().any(|n| n == s), "{s} precached");
        }
    }

    #[test]
    fn hover_run_stands_its_ground_or_runs() {
        let mut world = test_world();
        let key = hover(&mut world);
        hover_run(&mut world, key);
        assert!(playing(&world, key, &HOVER_MOVE_RUN));
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        hover_run(&mut world, key);
        assert!(playing(&world, key, &HOVER_MOVE_STAND));
        hover_walk(&mut world, key);
        assert!(playing(&world, key, &HOVER_MOVE_WALK));
        hover_start_attack(&mut world, key);
        assert!(playing(&world, key, &HOVER_MOVE_START_ATTACK));
        hover_attack(&mut world, key);
        assert!(playing(&world, key, &HOVER_MOVE_ATTACK1));
        hover_stand(&mut world, key);
        assert!(playing(&world, key, &HOVER_MOVE_STAND));
    }

    #[test]
    fn hover_search_plays_either_search_sound() {
        let (mut world, gi) = test_world_recording();
        let key = hover(&mut world);
        for _ in 0..100 {
            hover_search(&mut world, key);
        }
        let played = sounds_of(&world, &gi, key);
        for name in [SOUND_SEARCH1, SOUND_SEARCH2] {
            assert!(played
                .iter()
                .any(|s| s == &(CHAN_VOICE, name.to_string(), ATTN_NORM)));
        }
        hover_sight(&mut world, key, key);
        assert_eq!(sounds_of(&world, &gi, key).pop().unwrap().1, SOUND_SIGHT);
    }

    #[test]
    fn hover_reattack_fires_again_at_a_live_visible_enemy_six_times_in_ten() {
        let mut world = test_world();
        let key = hover(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        let mut again = 0;
        for _ in 0..1000 {
            hover_reattack(&mut world, key);
            if playing(&world, key, &HOVER_MOVE_ATTACK1) {
                again += 1;
            } else {
                assert!(playing(&world, key, &HOVER_MOVE_END_ATTACK));
            }
        }
        assert!((550..650).contains(&again), "{again}");

        world.entities.get_mut(player).unwrap().game.health = 0;
        for _ in 0..20 {
            hover_reattack(&mut world, key);
            assert!(playing(&world, key, &HOVER_MOVE_END_ATTACK));
        }
    }

    #[test]
    fn hover_fire_blaster_shoots_a_bolt_hyper_only_on_attak104() {
        let mut world = test_world();
        let key = hover(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        for (frame, effect) in [(FRAME_ATTAK104, EF_HYPERBLASTER), (FRAME_ATTAK104 + 1, 0)] {
            world.entities.get_mut(key).unwrap().state.frame = frame;
            hover_fire_blaster(&mut world, key);
            let bolt = world.find_by_classname(None, "bolt").unwrap();
            let ent = world.entities.get(bolt).unwrap();
            assert_eq!(ent.state.effects & EF_HYPERBLASTER, effect);
            assert_eq!(ent.game.dmg, 1);
            // Aimed at the player's eye.
            assert!(ent.velocity.x > 0.0 && (ent.velocity.length() - 1000.0).abs() < 0.5);
            world.free_entity(bolt);
        }
    }

    #[test]
    fn hover_reattack_ends_the_attack_when_the_enemy_is_out_of_sight() {
        let (mut world, gi) = test_world_recording();
        let key = hover(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        seed(&mut world, |w| w.random() <= 0.6);
        *gi.next_trace.borrow_mut() = Some(Trace {
            fraction: 0.5,
            ..Trace::default()
        });
        hover_reattack(&mut world, key);
        assert!(playing(&world, key, &HOVER_MOVE_END_ATTACK));
    }

    #[test]
    fn hover_fire_blaster_shoots_from_the_c_muzzle_with_its_flash() {
        let (mut world, gi) = crate::misc::wire::wire_world();
        world.entities.spawn_numbered(0).unwrap();
        let key = hover(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        hover_fire_blaster(&mut world, key);
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        // C flash offset (32.5, -0.8, 10) from (0, 0, 64) facing +x.
        let at = world.entities.get(bolt).unwrap().state.origin;
        assert!((at - Vec3f::new(32.5, 0.8, 74.0)).length() < 1e-3, "{at}");
        let wire = gi.wire.borrow();
        assert_eq!(
            wire[wire.len() - 2],
            crate::misc::wire::Wire::Byte(MZ2_HOVER_BLASTER_1)
        );
    }

    fn pain(world: &mut GameWorld, key: EntityKey, damage: i32) {
        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 0.0;
        hover_pain(world, key, key, 0.0, damage);
    }

    #[test]
    fn hover_pain_up_to_25_damage_plays_pain3_or_pain2_and_more_plays_pain1() {
        let (mut world, gi) = test_world_recording();
        let key = hover(&mut world);
        let (mut three, mut two) = (0, 0);
        for _ in 0..100 {
            pain(&mut world, key, 25);
            let sound = sounds_of(&world, &gi, key).pop().unwrap().1;
            if playing(&world, key, &HOVER_MOVE_PAIN3) {
                assert_eq!(sound, SOUND_PAIN1);
                three += 1;
            } else {
                assert!(playing(&world, key, &HOVER_MOVE_PAIN2));
                assert_eq!(sound, SOUND_PAIN2);
                two += 1;
            }
        }
        assert!(three > 25 && two > 25, "{three} {two}");
        pain(&mut world, key, 26);
        assert!(playing(&world, key, &HOVER_MOVE_PAIN1));
        assert_eq!(sounds_of(&world, &gi, key).pop().unwrap().1, SOUND_PAIN1);
    }

    #[test]
    fn hover_pain_debounces_skins_and_skips_animations_in_nightmare() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 2.0;
        let key = hover(&mut world);
        world.entities.get_mut(key).unwrap().game.health = 119;
        hover_pain(&mut world, key, key, 0.0, 10);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.state.skinnum, ent.game.pain_debounce_time), (1, 5.0));
        let calls = gi.sound_calls.borrow().len();
        hover_stand(&mut world, key);
        hover_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(gi.sound_calls.borrow().len(), calls, "debounced");
        world.game.skill = 3;
        world.level.time = 6.0;
        hover_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            9.0
        );
        assert_eq!(gi.sound_calls.borrow().len(), calls, "nightmare");
        assert!(playing(&world, key, &HOVER_MOVE_STAND));
    }

    #[test]
    fn hover_die_at_gib_health_throws_gibs_and_a_head() {
        let (mut world, gi) = test_world_recording();
        let key = hover(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -100;
        let before = world.entities.iter().count();
        hover_die(&mut world, key, key, key, 120, Vec3f::ZERO);
        assert_eq!(world.entities.iter().count(), before + 4);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.svflags & SvFlags::MONSTER.bits(), 0, "now a head");
        assert_eq!(sounds_of(&world, &gi, key)[0].1, "misc/udeath.wav");
    }

    #[test]
    fn hover_die_above_gib_health_plays_one_of_two_deaths_once() {
        let (mut world, gi) = test_world_recording();
        let mut seen = Vec::new();
        for _ in 0..40 {
            gi.sound_calls.borrow_mut().clear();
            let key = hover(&mut world);
            world.entities.get_mut(key).unwrap().game.health = -99;
            hover_die(&mut world, key, key, key, 120, Vec3f::ZERO);
            let ent = world.entities.get(key).unwrap();
            assert_eq!(
                (ent.game.deadflag, ent.game.takedamage),
                (DeadFlag::Dead, TakeDamage::Yes)
            );
            assert!(playing(&world, key, &HOVER_MOVE_DEATH1));
            let played = sounds_of(&world, &gi, key);
            assert_eq!(played.len(), 1);
            seen.push(played[0].1.clone());
            hover_stand(&mut world, key);
            hover_die(&mut world, key, key, key, 120, Vec3f::ZERO);
            assert!(playing(&world, key, &HOVER_MOVE_STAND), "already dead");
            world.free_entity(key);
        }
        assert!(seen.iter().any(|s| s == SOUND_DEATH1) && seen.iter().any(|s| s == SOUND_DEATH2));
    }

    #[test]
    fn hover_dead_falls_and_explodes_on_landing_or_after_15_seconds() {
        let mut world = test_world();
        world.level.time = 10.0;
        let key = hover(&mut world);
        hover_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.mins, ent.maxs),
            (
                Vec3f::new(-16.0, -16.0, -24.0),
                Vec3f::new(16.0, 16.0, -8.0)
            )
        );
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_eq!(
            ent.svflags & SvFlags::DEADMONSTER.bits(),
            0,
            "C leaves it off"
        );
        assert_eq!((ent.game.nextthink, ent.game.timestamp), (10.1, 25.0));
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, hover_deadthink as ThinkFn)));

        // Airborne before the timeout: think again next frame.
        world.level.time = 20.0;
        hover_deadthink(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().game.nextthink, 20.1);
        // Landed: explode.
        let ground = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().game.ground_entity = Some(ground);
        hover_deadthink(&mut world, key);
        assert!(!world.entities.is_live(key));
        // Still falling at the timeout: explode too.
        let key = hover(&mut world);
        hover_dead(&mut world, key);
        world.level.time = 35.0;
        hover_deadthink(&mut world, key);
        assert!(!world.entities.is_live(key));
    }

    #[test]
    fn hover_sights_a_player_and_shoots_blaster_bolts() {
        let (mut world, gi) = hit_world(false);
        let key = hover(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(400.0, 0.0, 24.0));
        let fired = run_until(&mut world, 60, |w| {
            w.find_by_classname(None, "bolt").is_some()
        });
        assert!(fired.is_some(), "attacked");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        let played = sounds_of(&world, &gi.floor.inner, key);
        assert!(played.iter().any(|s| s.1 == SOUND_SIGHT));
        let bolt = world.find_by_classname(None, "bolt").unwrap();
        assert_eq!(world.entities.get(bolt).unwrap().owner, Some(key));
    }
}
