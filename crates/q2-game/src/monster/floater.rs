//! Technician (floater): flies, fires blaster bursts, claws and zaps up
//! close, explodes when killed. C ref: `~/Qwasm2/src/game/monster/float/float.c`.

use q2_shared::constants::{EF_HYPERBLASTER, MZ2_FLOAT_BLASTER_1};
use q2_shared::protocol::SvcOp;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove};
use crate::misc::become_explosion1;
use crate::monster::{
    fire::monster_fire_blaster, flash::monster_flash_source, flash::project_source,
};
use crate::monster::{frame, set_move, sound};
use crate::world::GameWorld;
use std::collections::HashMap;

/// C `SPLASH_SPARKS`, the `TE_SPLASH` colour (shared.h).
const SPLASH_SPARKS: i32 = 1;

const SOUND_ATTACK2: &str = "floater/fltatck2.wav";
const SOUND_ATTACK3: &str = "floater/fltatck3.wav";
const SOUND_DEATH1: &str = "floater/fltdeth1.wav";
const SOUND_IDLE: &str = "floater/fltidle1.wav";
const SOUND_PAIN1: &str = "floater/fltpain1.wav";
const SOUND_PAIN2: &str = "floater/fltpain2.wav";
const SOUND_SIGHT: &str = "floater/fltsght1.wav";
/// Every sound C `SP_monster_floater` indexes; `fltatck1` is precached
/// but never played.
const SOUNDS: [&str; 8] = [
    SOUND_ATTACK2,
    SOUND_ATTACK3,
    SOUND_DEATH1,
    SOUND_IDLE,
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_SIGHT,
    "floater/fltatck1.wav",
];
/// C `self->s.sound`.
const SOUND_IDLE_LOOP: &str = "floater/fltsrch1.wav";

/// C `MODEL_SCALE` (float.h).
const MODEL_SCALE: f32 = 1.0;

// ---------------------------------------------------------------------------
// Moves. C ref: the `mframe_t`/`mmove_t` tables, frame by frame.
// ---------------------------------------------------------------------------

// C ref: float.h (`FRAME_*`, model frame numbers).

const FRAME_ACTVAT01: i32 = 0;
const FRAME_ACTVAT31: i32 = 30;
const FRAME_ATTAK101: i32 = 31;
const FRAME_ATTAK104: i32 = 34;
const FRAME_ATTAK107: i32 = 37;
const FRAME_ATTAK114: i32 = 44;
const FRAME_ATTAK201: i32 = 45;
const FRAME_ATTAK225: i32 = 69;
const FRAME_ATTAK301: i32 = 70;
const FRAME_ATTAK334: i32 = 103;
const FRAME_DEATH01: i32 = 104;
const FRAME_DEATH13: i32 = 116;
const FRAME_PAIN101: i32 = 117;
const FRAME_PAIN107: i32 = 123;
const FRAME_PAIN201: i32 = 124;
const FRAME_PAIN208: i32 = 131;
const FRAME_PAIN301: i32 = 132;
const FRAME_PAIN312: i32 = 143;
const FRAME_STAND101: i32 = 144;
const FRAME_STAND152: i32 = 195;
const FRAME_STAND201: i32 = 196;
const FRAME_STAND252: i32 = 247;

static FLOATER_FRAMES_STAND1: [MonsterFrame; 52] = [frame(ai::ai_stand, 0.0, None); 52];

pub static FLOATER_MOVE_STAND1: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND101,
    lastframe: FRAME_STAND152,
    frames: &FLOATER_FRAMES_STAND1,
    endfunc: None,
};

static FLOATER_FRAMES_STAND2: [MonsterFrame; 52] = [frame(ai::ai_stand, 0.0, None); 52];

pub static FLOATER_MOVE_STAND2: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND201,
    lastframe: FRAME_STAND252,
    frames: &FLOATER_FRAMES_STAND2,
    endfunc: None,
};

static FLOATER_FRAMES_ACTIVATE: [MonsterFrame; 30] = [frame(ai::ai_move, 0.0, None); 30];

pub static FLOATER_MOVE_ACTIVATE: MonsterMove = MonsterMove {
    firstframe: FRAME_ACTVAT01,
    lastframe: FRAME_ACTVAT31,
    frames: &FLOATER_FRAMES_ACTIVATE,
    endfunc: None,
};

static FLOATER_FRAMES_ATTACK1: [MonsterFrame; 14] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(floater_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(floater_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(floater_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(floater_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(floater_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(floater_fire_blaster)),
    frame(ai::ai_charge, 0.0, Some(floater_fire_blaster)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

pub static FLOATER_MOVE_ATTACK1: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK101,
    lastframe: FRAME_ATTAK114,
    frames: &FLOATER_FRAMES_ATTACK1,
    endfunc: Some(floater_run),
};

static FLOATER_FRAMES_ATTACK2: [MonsterFrame; 25] = [
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
    frame(ai::ai_charge, 0.0, Some(floater_wham)),
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
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

pub static FLOATER_MOVE_ATTACK2: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK201,
    lastframe: FRAME_ATTAK225,
    frames: &FLOATER_FRAMES_ATTACK2,
    endfunc: Some(floater_run),
};

static FLOATER_FRAMES_ATTACK3: [MonsterFrame; 34] = [
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(floater_zap)),
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
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
];

pub static FLOATER_MOVE_ATTACK3: MonsterMove = MonsterMove {
    firstframe: FRAME_ATTAK301,
    lastframe: FRAME_ATTAK334,
    frames: &FLOATER_FRAMES_ATTACK3,
    endfunc: Some(floater_run),
};

static FLOATER_FRAMES_DEATH: [MonsterFrame; 13] = [frame(ai::ai_move, 0.0, None); 13];

pub static FLOATER_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_DEATH01,
    lastframe: FRAME_DEATH13,
    frames: &FLOATER_FRAMES_DEATH,
    endfunc: Some(floater_dead),
};

static FLOATER_FRAMES_PAIN1: [MonsterFrame; 7] = [frame(ai::ai_move, 0.0, None); 7];

pub static FLOATER_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN101,
    lastframe: FRAME_PAIN107,
    frames: &FLOATER_FRAMES_PAIN1,
    endfunc: Some(floater_run),
};

static FLOATER_FRAMES_PAIN2: [MonsterFrame; 8] = [frame(ai::ai_move, 0.0, None); 8];

pub static FLOATER_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN201,
    lastframe: FRAME_PAIN208,
    frames: &FLOATER_FRAMES_PAIN2,
    endfunc: Some(floater_run),
};

static FLOATER_FRAMES_PAIN3: [MonsterFrame; 12] = [frame(ai::ai_move, 0.0, None); 12];

pub static FLOATER_MOVE_PAIN3: MonsterMove = MonsterMove {
    firstframe: FRAME_PAIN301,
    lastframe: FRAME_PAIN312,
    frames: &FLOATER_FRAMES_PAIN3,
    endfunc: Some(floater_run),
};

static FLOATER_FRAMES_WALK: [MonsterFrame; 52] = [frame(ai::ai_walk, 5.0, None); 52];

pub static FLOATER_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND101,
    lastframe: FRAME_STAND152,
    frames: &FLOATER_FRAMES_WALK,
    endfunc: None,
};

static FLOATER_FRAMES_RUN: [MonsterFrame; 52] = [frame(ai::ai_run, 13.0, None); 52];

pub static FLOATER_MOVE_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_STAND101,
    lastframe: FRAME_STAND152,
    frames: &FLOATER_FRAMES_RUN,
    endfunc: None,
};

// ---------------------------------------------------------------------------
// Callbacks. C ref: float.c. C sets `self->gib_health = -80`, but
// `floater_die` explodes whatever the health, so nothing reads it.
// ---------------------------------------------------------------------------

/// C ref: `floater_sight`.
pub fn floater_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT, ATTN_NORM);
}

/// C ref: `floater_idle`.
pub fn floater_idle(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_IDLE, ATTN_IDLE);
}

/// A bolt at the enemy's eye; frames attak104 and attak107 fire
/// hyperblaster ones. C ref: `floater_fire_blaster`.
pub fn floater_fire_blaster(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let effect = if ent.state.frame == FRAME_ATTAK104 || ent.state.frame == FRAME_ATTAK107 {
        EF_HYPERBLASTER
    } else {
        0
    };
    let Some(enemy) = ent.game.enemy.and_then(|e| world.entities.get(e)) else {
        return;
    };
    let mut end = enemy.state.origin;
    end.z += enemy.game.viewheight;
    let Some((start, _, _)) = monster_flash_source(world, key, MZ2_FLOAT_BLASTER_1) else {
        return;
    };
    monster_fire_blaster(
        world,
        key,
        start,
        end - start,
        1,
        1000,
        MZ2_FLOAT_BLASTER_1,
        effect,
    );
}

/// C ref: `floater_stand`.
pub fn floater_stand(world: &mut GameWorld, key: EntityKey) {
    let mv = if world.random() <= 0.5 {
        &FLOATER_MOVE_STAND1
    } else {
        &FLOATER_MOVE_STAND2
    };
    set_move(world, key, mv);
}

/// C ref: `floater_run`.
pub fn floater_run(world: &mut GameWorld, key: EntityKey) {
    let stand_ground = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::STAND_GROUND));
    let mv = if stand_ground {
        &FLOATER_MOVE_STAND1
    } else {
        &FLOATER_MOVE_RUN
    };
    set_move(world, key, mv);
}

/// C ref: `floater_walk`.
pub fn floater_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLOATER_MOVE_WALK);
}

/// Claw hit for 5..=10 with a pull (negative kick). C ref: `floater_wham`.
pub fn floater_wham(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_ATTACK3, ATTN_NORM);
    let damage = 5 + (world.random_u32() % 6) as i32;
    world.fire_hit(key, Vec3f::new(ai::MELEE_DISTANCE, 0.0, 0.0), damage, -50);
}

/// Sparks from the claw; a visible enemy in front in melee range takes
/// 5..=10 energy damage. C ref: `floater_zap`.
pub fn floater_zap(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let Some(enemy) = ent.game.enemy else {
        return;
    };
    let Some(enemy_origin) = world.entities.get(enemy).map(|e| e.state.origin) else {
        return;
    };
    let dir = enemy_origin - ent.state.origin;
    let (forward, right, _) = crate::physics::angle_vectors(ent.state.angles);
    let origin = project_source(ent.state.origin, [18.5, -0.9, 10.0], forward, right);

    sound(world, key, CHAN_WEAPON, SOUND_ATTACK2, ATTN_NORM);

    world.gi.write_byte(SvcOp::TempEntity as i32);
    world.gi.write_byte(TE_SPLASH);
    world.gi.write_byte(32);
    world.gi.write_position(origin);
    world.gi.write_dir(dir);
    world.gi.write_byte(SPLASH_SPARKS);
    world.gi.multicast(origin, Multicast::PVS);

    if ai::range(world, key, enemy) == Range::Melee
        && ai::infront(world, key, enemy)
        && ai::visible(world, key, enemy)
    {
        let damage = 5 + (world.random_u32() % 6) as i32;
        world.t_damage(
            enemy,
            key,
            key,
            dir,
            enemy_origin,
            Vec3f::ZERO,
            damage,
            -10,
            DamageFlags::ENERGY,
            MeansOfDeath::Unknown,
        );
    }
}

/// C ref: `floater_attack`.
pub fn floater_attack(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLOATER_MOVE_ATTACK1);
}

/// Zap or claw, evenly. C ref: `floater_melee`.
pub fn floater_melee(world: &mut GameWorld, key: EntityKey) {
    let mv = if world.random() < 0.5 {
        &FLOATER_MOVE_ATTACK3
    } else {
        &FLOATER_MOVE_ATTACK2
    };
    set_move(world, key, mv);
}

/// Hurt skin below half health; pain1 one time in three, else pain2 (C
/// never picks pain3), at most every 3 s, none in nightmare.
/// C ref: `floater_pain`.
pub fn floater_pain(
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
    if world.random_u32().wrapping_add(1).is_multiple_of(3) {
        sound(world, key, CHAN_VOICE, SOUND_PAIN1, ATTN_NORM);
        set_move(world, key, &FLOATER_MOVE_PAIN1);
    } else {
        sound(world, key, CHAN_VOICE, SOUND_PAIN2, ATTN_NORM);
        set_move(world, key, &FLOATER_MOVE_PAIN2);
    }
}

/// End of the (unplayed) death move. C ref: `floater_dead`.
pub fn floater_dead(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, -8.0);
    }
    ai::monster_dead(world, key);
}

/// C ref: `floater_die`.
pub fn floater_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    sound(world, key, CHAN_VOICE, SOUND_DEATH1, ATTN_NORM);
    become_explosion1(world, key);
}

// ---------------------------------------------------------------------------
// Spawn. C ref: `SP_monster_floater`, split: the model is the
// `spawn::SPAWNS` row's; `finish_floater` indexes the sounds and picks
// the stand move.
// ---------------------------------------------------------------------------

pub fn sp_monster_floater(
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
        .unwrap_or_else(|| "monster_floater".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-24.0, -24.0, -24.0);
    ent.maxs = Vec3f::new(24.0, 24.0, 32.0);
    ent.game.health = 200;
    ent.game.gib_health = -80;
    ent.game.mass = 300;
    ent.pain = Some(floater_pain);
    ent.die = Some(floater_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(floater_stand),
        walk: Some(floater_walk),
        run: Some(floater_run),
        attack: Some(floater_attack),
        melee: Some(floater_melee),
        sight: Some(floater_sight),
        idle: Some(floater_idle),
        currentmove: Some(FLOATER_MOVE_STAND1.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::flymonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_floater`: precache its sounds,
/// start the idle loop and stand in stand1 or stand2 evenly.
pub fn finish_floater(world: &mut GameWorld, key: EntityKey, _fields: &HashMap<String, String>) {
    for name in SOUNDS {
        world.gi.sound_index(name);
    }
    let idle = world.gi.sound_index(SOUND_IDLE_LOOP);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.sound = idle;
    }
    floater_stand(world, key);
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
        (floater_run, "floater_run"),
        (floater_dead, "floater_dead"),
        (floater_fire_blaster, "floater_fire_blaster"),
        (floater_wham, "floater_wham"),
        (floater_zap, "floater_zap"),
    ];

    #[test]
    fn floater_moves_match_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&FLOATER_MOVE_STAND1, 144, 195, "-", "stand:0:-*52"),
            (&FLOATER_MOVE_STAND2, 196, 247, "-", "stand:0:-*52"),
            (&FLOATER_MOVE_ACTIVATE, 0, 30, "-", "move:0:-*30"),
            (&FLOATER_MOVE_ATTACK1, 31, 44, "floater_run", "charge:0:-*3 charge:0:floater_fire_blaster*7 charge:0:-*4"),
            (&FLOATER_MOVE_ATTACK2, 45, 69, "floater_run", "charge:0:-*11 charge:0:floater_wham charge:0:-*13"),
            (&FLOATER_MOVE_ATTACK3, 70, 103, "floater_run", "charge:0:-*8 charge:0:floater_zap charge:0:-*25"),
            (&FLOATER_MOVE_DEATH, 104, 116, "floater_dead", "move:0:-*13"),
            (&FLOATER_MOVE_PAIN1, 117, 123, "floater_run", "move:0:-*7"),
            (&FLOATER_MOVE_PAIN2, 124, 131, "floater_run", "move:0:-*8"),
            (&FLOATER_MOVE_PAIN3, 132, 143, "floater_run", "move:0:-*12"),
            (&FLOATER_MOVE_WALK, 144, 195, "-", "walk:5:-*52"),
            (&FLOATER_MOVE_RUN, 144, 195, "-", "run:13:-*52"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    #[test]
    fn floater_activate_keeps_cs_30_frames_for_its_31_frame_range() {
        // C's table is one frame short; nothing plays the move.
        assert_eq!(FLOATER_MOVE_ACTIVATE.frames.len(), 30);
        assert_eq!(
            FLOATER_MOVE_ACTIVATE.lastframe - FLOATER_MOVE_ACTIVATE.firstframe,
            30
        );
    }

    fn floater(world: &mut GameWorld) -> EntityKey {
        spawn_monster(world, "monster_floater", Vec3f::new(0.0, 0.0, 64.0))
    }

    #[test]
    fn floater_spawns_with_the_c_values_and_flies() {
        let (mut world, gi) = test_world_recording();
        let key = floater(&mut world);
        let hum = world.gi.sound_index(SOUND_IDLE_LOOP);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.health, ent.game.max_health, ent.game.mass),
            (200, 200, 300)
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
        assert_eq!(ent.state.sound, hum);
        let mi = mi(&world, key);
        assert_eq!(mi.scale, 1.0);
        assert!(mi
            .attack
            .is_some_and(|f| std::ptr::fn_addr_eq(f, floater_attack as ThinkFn)));
        assert!(mi
            .melee
            .is_some_and(|f| std::ptr::fn_addr_eq(f, floater_melee as ThinkFn)));
        assert!(mi.idle.is_some() && mi.sight.is_some() && mi.search.is_none());
        for s in SOUNDS {
            assert!(gi.sounds.borrow().iter().any(|n| n == s), "{s} precached");
        }
    }

    #[test]
    fn floater_spawns_and_stands_in_stand1_or_stand2_evenly() {
        let mut world = test_world();
        let (mut one, mut two) = (0, 0);
        for _ in 0..200 {
            let key = floater(&mut world);
            if playing(&world, key, &FLOATER_MOVE_STAND1) {
                one += 1;
            } else {
                assert!(playing(&world, key, &FLOATER_MOVE_STAND2));
                two += 1;
            }
            floater_stand(&mut world, key);
            world.free_entity(key);
        }
        assert!(one > 60 && two > 60, "{one} {two}");
    }

    #[test]
    fn floater_run_walk_attack_and_melee_pick_the_c_moves() {
        let mut world = test_world();
        let key = floater(&mut world);
        floater_run(&mut world, key);
        assert!(playing(&world, key, &FLOATER_MOVE_RUN));
        mi_mut(&mut world, key).aiflags |= AiFlags::STAND_GROUND;
        floater_run(&mut world, key);
        assert!(playing(&world, key, &FLOATER_MOVE_STAND1));
        floater_walk(&mut world, key);
        assert!(playing(&world, key, &FLOATER_MOVE_WALK));
        floater_attack(&mut world, key);
        assert!(playing(&world, key, &FLOATER_MOVE_ATTACK1));
        let (mut zap, mut wham) = (0, 0);
        for _ in 0..200 {
            floater_melee(&mut world, key);
            if playing(&world, key, &FLOATER_MOVE_ATTACK3) {
                zap += 1;
            } else {
                assert!(playing(&world, key, &FLOATER_MOVE_ATTACK2));
                wham += 1;
            }
        }
        assert!(zap > 60 && wham > 60, "{zap} {wham}");
    }

    #[test]
    fn floater_sight_and_idle_sounds() {
        let (mut world, gi) = test_world_recording();
        let key = floater(&mut world);
        floater_sight(&mut world, key, key);
        floater_idle(&mut world, key);
        assert_eq!(
            sounds_of(&world, &gi, key),
            [
                (CHAN_VOICE, SOUND_SIGHT.into(), ATTN_NORM),
                (CHAN_VOICE, SOUND_IDLE.into(), ATTN_IDLE)
            ]
        );
    }

    #[test]
    fn floater_fire_blaster_shoots_hyper_bolts_on_attak104_and_attak107() {
        let mut world = test_world();
        let key = floater(&mut world);
        let player = player_at(&mut world, Vec3f::new(300.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        for frame in FRAME_ATTAK101 + 3..=FRAME_ATTAK101 + 9 {
            world.entities.get_mut(key).unwrap().state.frame = frame;
            floater_fire_blaster(&mut world, key);
            let bolt = world.find_by_classname(None, "bolt").unwrap();
            let ent = world.entities.get(bolt).unwrap();
            let hyper = frame == FRAME_ATTAK104 || frame == FRAME_ATTAK107;
            assert_eq!(
                ent.state.effects & EF_HYPERBLASTER != 0,
                hyper,
                "frame {frame}"
            );
            assert_eq!(ent.game.dmg, 1);
            world.free_entity(bolt);
        }
    }

    #[test]
    fn floater_wham_hits_for_5_to_10_and_pulls_in() {
        let (mut world, gi) = hit_world(false);
        let key = floater(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(50.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        let mut hits = std::collections::BTreeSet::new();
        for _ in 0..100 {
            world.entities.get_mut(player).unwrap().game.health = 100;
            world.entities.get_mut(player).unwrap().velocity = Vec3f::ZERO;
            floater_wham(&mut world, key);
            hits.insert(100 - world.entities.get(player).unwrap().game.health);
            // C kick -50: fire_hit's knockback pulls the player in.
            let pull = world.entities.get(player).unwrap().velocity;
            assert!(
                (pull.length() - 50.0).abs() < 0.01 && pull.x < 0.0,
                "{pull}"
            );
        }
        assert_eq!(hits, (5..=10).collect());
        let played = sounds_of(&world, &gi.floor.inner, key);
        assert_eq!(played[0], (CHAN_WEAPON, SOUND_ATTACK3.into(), ATTN_NORM));
    }

    #[test]
    fn floater_zap_sparks_and_shocks_a_visible_enemy_in_front_in_melee_range() {
        let (mut world, gi) = hit_world(false);
        let key = floater(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(50.0, 0.0, 64.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        floater_zap(&mut world, key);
        let health = world.entities.get(player).unwrap().game.health;
        assert!((90..=95).contains(&health), "{health}");
        assert_eq!(
            sounds_of(&world, &gi.floor.inner, key)[0],
            (CHAN_WEAPON, SOUND_ATTACK2.into(), ATTN_NORM)
        );
        // C: svc_temp_entity, TE_SPLASH, 32 sparks at the claw, along dir.
        let origin = Vec3f::new(18.5, 0.9, 74.0);
        assert_eq!(
            *gi.wire.borrow(),
            [
                format!("byte {}", q2_shared::protocol::SvcOp::TempEntity as i32),
                "byte 10".to_string(),
                "byte 32".to_string(),
                format!("position {origin:?}"),
                format!("dir {:?}", Vec3f::new(50.0, 0.0, 0.0)),
                "byte 1".to_string(),
                format!("multicast {origin:?} PVS"),
            ]
        );

        // Out of melee range: sparks only.
        gi.wire.borrow_mut().clear();
        world.entities.get_mut(player).unwrap().state.origin.x = 200.0;
        floater_zap(&mut world, key);
        assert_eq!(world.entities.get(player).unwrap().game.health, health);
        assert_eq!(gi.wire.borrow().len(), 7);
        // Behind it: no shock either.
        world.entities.get_mut(player).unwrap().state.origin.x = -50.0;
        floater_zap(&mut world, key);
        assert_eq!(world.entities.get(player).unwrap().game.health, health);
    }

    fn pain(world: &mut GameWorld, key: EntityKey) {
        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 0.0;
        floater_pain(world, key, key, 0.0, 10);
    }

    #[test]
    fn floater_pain_picks_pain1_or_pain2_never_pain3() {
        let (mut world, gi) = test_world_recording();
        let key = floater(&mut world);
        let (mut one, mut two) = (0, 0);
        for _ in 0..300 {
            pain(&mut world, key);
            let sound = sounds_of(&world, &gi, key).pop().unwrap().1;
            if playing(&world, key, &FLOATER_MOVE_PAIN1) {
                assert_eq!(sound, SOUND_PAIN1);
                one += 1;
            } else {
                assert!(playing(&world, key, &FLOATER_MOVE_PAIN2));
                assert_eq!(sound, SOUND_PAIN2);
                two += 1;
            }
        }
        // C: n = (randk() + 1) % 3, pain1 only for n == 0.
        assert!((60..140).contains(&one) && two > 150, "{one} {two}");
    }

    #[test]
    fn floater_pain_debounces_skins_and_skips_animations_in_nightmare() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 2.0;
        let key = floater(&mut world);
        world.entities.get_mut(key).unwrap().game.health = 99;
        floater_pain(&mut world, key, key, 0.0, 10);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.state.skinnum, ent.game.pain_debounce_time), (1, 5.0));
        let calls = gi.sound_calls.borrow().len();
        floater_stand(&mut world, key);
        let stand = mi(&world, key).currentmove.clone().unwrap();
        floater_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(gi.sound_calls.borrow().len(), calls, "debounced");
        world.game.skill = 3;
        world.level.time = 6.0;
        floater_pain(&mut world, key, key, 0.0, 10);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            9.0
        );
        assert_eq!(gi.sound_calls.borrow().len(), calls, "nightmare");
        assert!(playing(&world, key, &stand));
    }

    #[test]
    fn floater_die_explodes_with_the_death_sound() {
        let (mut world, gi) = test_world_recording();
        let key = floater(&mut world);
        let number = world.number_of(key);
        floater_die(&mut world, key, key, key, 300, Vec3f::ZERO);
        assert!(!world.entities.is_live(key));
        let index = world.gi.sound_index(SOUND_DEATH1);
        assert!(gi
            .sound_calls
            .borrow()
            .iter()
            .any(|c| (c.0, c.1, c.2) == (number, CHAN_VOICE, index)));
    }

    #[test]
    fn floater_dead_shrinks_the_box_and_falls() {
        let mut world = test_world();
        let key = floater(&mut world);
        floater_dead(&mut world, key);
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
    fn floater_sights_a_player_and_shoots_blaster_bolts() {
        let (mut world, gi) = hit_world(false);
        // Hard skill doubles the missile chance, so it fires before it
        // closes to melee range.
        world.game.skill = 2;
        let key = floater(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(400.0, 0.0, 24.0));
        let fired = run_until(&mut world, 80, |w| {
            w.find_by_classname(None, "bolt").is_some()
        });
        assert!(fired.is_some(), "attacked");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(sounds_of(&world, &gi.floor.inner, key)
            .iter()
            .any(|s| s.1 == SOUND_SIGHT));
        assert!(playing(&world, key, &FLOATER_MOVE_ATTACK1));
    }

    #[test]
    fn floater_next_to_a_player_goes_for_melee_and_hurts_it() {
        let (mut world, gi) = hit_world(false);
        let key = floater(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(60.0, 0.0, 64.0));
        let hurt = run_until(&mut world, 80, |w| {
            w.entities.get(player).unwrap().game.health < 100
        });
        assert!(hurt.is_some(), "meleed");
        assert!(
            playing(&world, key, &FLOATER_MOVE_ATTACK2)
                || playing(&world, key, &FLOATER_MOVE_ATTACK3)
        );
    }
}
