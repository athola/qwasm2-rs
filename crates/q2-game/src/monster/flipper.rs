//! Flipper (barracuda shark): swims, bites in melee range, no ranged
//! attack. C ref: `~/Qwasm2/src/game/monster/flipper/flipper.c`.

use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{
    AnimFrameFn, EntityKey, EntityStorage, MonsterFrame, MonsterInfo, MonsterMove, ThinkFn,
};
use crate::misc::{throw_gib, throw_head, GibType};
use crate::world::GameWorld;
use std::collections::HashMap;

const SOUND_PAIN1: &str = "flipper/flppain1.wav";
const SOUND_PAIN2: &str = "flipper/flppain2.wav";
const SOUND_DEATH: &str = "flipper/flpdeth1.wav";
const SOUND_CHOMP: &str = "flipper/flpatck1.wav";
const SOUND_SIGHT: &str = "flipper/flpsght1.wav";
/// Every sound C `SP_monster_flipper` indexes; `flpatck2`, `flpidle1` and
/// `flpsrch1` are precached but never played.
const SOUNDS: [&str; 8] = [
    SOUND_PAIN1,
    SOUND_PAIN2,
    SOUND_DEATH,
    SOUND_CHOMP,
    "flipper/flpatck2.wav",
    "flipper/flpidle1.wav",
    "flipper/flpsrch1.wav",
    SOUND_SIGHT,
];

/// C `MODEL_SCALE` (flipper.h).
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

// C ref: flipper.h (`FRAME_*`, model frame numbers).

const FRAME_FLPBIT01: i32 = 0;
const FRAME_FLPBIT20: i32 = 19;
const FRAME_FLPHOR01: i32 = 41;
const FRAME_FLPHOR05: i32 = 45;
const FRAME_FLPHOR24: i32 = 64;
const FRAME_FLPVER01: i32 = 65;
const FRAME_FLPVER06: i32 = 70;
const FRAME_FLPVER29: i32 = 93;
const FRAME_FLPPN101: i32 = 94;
const FRAME_FLPPN105: i32 = 98;
const FRAME_FLPPN201: i32 = 99;
const FRAME_FLPPN205: i32 = 103;
const FRAME_FLPDTH01: i32 = 104;
const FRAME_FLPDTH56: i32 = 159;

static FLIPPER_FRAMES_STAND: [MonsterFrame; 1] = [frame(ai::ai_stand, 0.0, None); 1];

pub static FLIPPER_MOVE_STAND: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPHOR01,
    lastframe: FRAME_FLPHOR01,
    frames: &FLIPPER_FRAMES_STAND,
    endfunc: None,
};

static FLIPPER_FRAMES_RUN: [MonsterFrame; 24] = [frame(ai::ai_run, 24.0, None); 24];

pub static FLIPPER_MOVE_RUN_LOOP: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPVER06,
    lastframe: FRAME_FLPVER29,
    frames: &FLIPPER_FRAMES_RUN,
    endfunc: None,
};

static FLIPPER_FRAMES_RUN_START: [MonsterFrame; 6] = [frame(ai::ai_run, 8.0, None); 6];

pub static FLIPPER_MOVE_RUN_START: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPVER01,
    lastframe: FRAME_FLPVER06,
    frames: &FLIPPER_FRAMES_RUN_START,
    endfunc: Some(flipper_run_loop),
};

static FLIPPER_FRAMES_WALK: [MonsterFrame; 24] = [frame(ai::ai_walk, 4.0, None); 24];

pub static FLIPPER_MOVE_WALK: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPHOR01,
    lastframe: FRAME_FLPHOR24,
    frames: &FLIPPER_FRAMES_WALK,
    endfunc: None,
};

static FLIPPER_FRAMES_START_RUN: [MonsterFrame; 5] = [
    frame(ai::ai_run, 8.0, None),
    frame(ai::ai_run, 8.0, None),
    frame(ai::ai_run, 8.0, None),
    frame(ai::ai_run, 8.0, None),
    frame(ai::ai_run, 8.0, Some(flipper_run)),
];

pub static FLIPPER_MOVE_START_RUN: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPHOR01,
    lastframe: FRAME_FLPHOR05,
    frames: &FLIPPER_FRAMES_START_RUN,
    endfunc: None,
};

static FLIPPER_FRAMES_PAIN2: [MonsterFrame; 5] = [frame(ai::ai_move, 0.0, None); 5];

pub static FLIPPER_MOVE_PAIN2: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPPN101,
    lastframe: FRAME_FLPPN105,
    frames: &FLIPPER_FRAMES_PAIN2,
    endfunc: Some(flipper_run),
};

static FLIPPER_FRAMES_PAIN1: [MonsterFrame; 5] = [frame(ai::ai_move, 0.0, None); 5];

pub static FLIPPER_MOVE_PAIN1: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPPN201,
    lastframe: FRAME_FLPPN205,
    frames: &FLIPPER_FRAMES_PAIN1,
    endfunc: Some(flipper_run),
};

static FLIPPER_FRAMES_ATTACK: [MonsterFrame; 20] = [
    frame(ai::ai_charge, 0.0, Some(flipper_preattack)),
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
    frame(ai::ai_charge, 0.0, Some(flipper_bite)),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, None),
    frame(ai::ai_charge, 0.0, Some(flipper_bite)),
    frame(ai::ai_charge, 0.0, None),
];

pub static FLIPPER_MOVE_ATTACK: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPBIT01,
    lastframe: FRAME_FLPBIT20,
    frames: &FLIPPER_FRAMES_ATTACK,
    endfunc: Some(flipper_run),
};

static FLIPPER_FRAMES_DEATH: [MonsterFrame; 56] = [frame(ai::ai_move, 0.0, None); 56];

pub static FLIPPER_MOVE_DEATH: MonsterMove = MonsterMove {
    firstframe: FRAME_FLPDTH01,
    lastframe: FRAME_FLPDTH56,
    frames: &FLIPPER_FRAMES_DEATH,
    endfunc: Some(flipper_dead),
};

// ---------------------------------------------------------------------------
// Callbacks. C ref: flipper.c.
// ---------------------------------------------------------------------------

fn sound(world: &mut GameWorld, key: EntityKey, channel: i32, name: &str) {
    let index = world.gi.sound_index(name);
    world
        .gi
        .sound(world.number_of(key), channel, index, 1.0, ATTN_NORM, 0.0);
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

/// C ref: `flipper_stand`.
pub fn flipper_stand(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLIPPER_MOVE_STAND);
}

/// C ref: `flipper_run_loop`.
pub fn flipper_run_loop(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLIPPER_MOVE_RUN_LOOP);
}

/// C ref: `flipper_run`.
pub fn flipper_run(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLIPPER_MOVE_RUN_START);
}

/// C ref: `flipper_walk`.
pub fn flipper_walk(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLIPPER_MOVE_WALK);
}

/// C ref: `flipper_start_run`.
pub fn flipper_start_run(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLIPPER_MOVE_START_RUN);
}

/// C ref: `flipper_bite`.
pub fn flipper_bite(world: &mut GameWorld, key: EntityKey) {
    world.fire_hit(key, Vec3f::new(ai::MELEE_DISTANCE, 0.0, 0.0), 5, 0);
}

/// C ref: `flipper_preattack`.
pub fn flipper_preattack(world: &mut GameWorld, key: EntityKey) {
    sound(world, key, CHAN_WEAPON, SOUND_CHOMP);
}

/// C ref: `flipper_melee`.
pub fn flipper_melee(world: &mut GameWorld, key: EntityKey) {
    set_move(world, key, &FLIPPER_MOVE_ATTACK);
}

/// Hurt skin below half health; pain1 or pain2 at most every 3 s, none
/// in nightmare. C ref: `flipper_pain`.
pub fn flipper_pain(
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
    if world.random_u32().wrapping_add(1).is_multiple_of(2) {
        sound(world, key, CHAN_VOICE, SOUND_PAIN1);
        set_move(world, key, &FLIPPER_MOVE_PAIN1);
    } else {
        sound(world, key, CHAN_VOICE, SOUND_PAIN2);
        set_move(world, key, &FLIPPER_MOVE_PAIN2);
    }
}

/// The corpse's box floor drops to where a trace 8 units down stops, so
/// it stays out of solids, then it falls. C ref: `flipper_dead`.
pub fn flipper_dead(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (origin, mins, maxs, clipmask) = (ent.state.origin, ent.mins, ent.maxs, ent.clipmask);
    let below = Vec3f::new(origin.x, origin.y, origin.z - 8.0);
    let tr = world
        .gi
        .trace(origin, mins, maxs, below, world.number_of(key), clipmask);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.mins.z = tr.endpos.z - origin.z;
    }
    ai::monster_dead(world, key);
}

/// C ref: `flipper_sight`.
pub fn flipper_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    sound(world, key, CHAN_VOICE, SOUND_SIGHT);
}

/// Gib at `gib_health` or below, else play the death once.
/// C ref: `flipper_die`.
pub fn flipper_die(
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
        sound(world, key, CHAN_VOICE, "misc/udeath.wav");
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
    sound(world, key, CHAN_VOICE, SOUND_DEATH);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.deadflag = DeadFlag::Dead;
        ent.game.takedamage = TakeDamage::Yes;
    }
    set_move(world, key, &FLIPPER_MOVE_DEATH);
}

// ---------------------------------------------------------------------------
// Spawn. C ref: `SP_monster_flipper`, split: the model is the
// `spawn::SPAWNS` row's and the sounds are indexed by `finish_flipper`.
// ---------------------------------------------------------------------------

pub fn sp_monster_flipper(
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
        .unwrap_or_else(|| "monster_flipper".to_string());
    ent.game.movetype = MoveType::Step;
    ent.solid = Solid::Bbox;
    ent.mins = Vec3f::new(-16.0, -16.0, 0.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
    ent.game.health = 50;
    ent.game.gib_health = -30;
    ent.game.mass = 100;
    ent.pain = Some(flipper_pain);
    ent.die = Some(flipper_die);
    ent.monsterinfo = Some(Box::new(MonsterInfo {
        stand: Some(flipper_stand),
        walk: Some(flipper_walk),
        run: Some(flipper_start_run),
        melee: Some(flipper_melee),
        sight: Some(flipper_sight),
        currentmove: Some(FLIPPER_MOVE_STAND.clone()),
        scale: MODEL_SCALE,
        ..Default::default()
    }));
    ai::swimmonster_start(ent);
}

/// The `GameWorld` half of C `SP_monster_flipper`: precache its sounds.
pub fn finish_flipper(world: &mut GameWorld, _key: EntityKey, _fields: &HashMap<String, String>) {
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
        (flipper_stand, "flipper_stand"),
        (flipper_run_loop, "flipper_run_loop"),
        (flipper_run, "flipper_run"),
        (flipper_walk, "flipper_walk"),
        (flipper_start_run, "flipper_start_run"),
        (flipper_bite, "flipper_bite"),
        (flipper_preattack, "flipper_preattack"),
        (flipper_melee, "flipper_melee"),
        (flipper_dead, "flipper_dead"),
    ];

    #[test]
    fn flipper_moves_match_c() {
        #[rustfmt::skip]
        let c: &[CMoveRow] = &[
            (&FLIPPER_MOVE_STAND, 41, 41, "-", "stand:0:-"),
            (&FLIPPER_MOVE_RUN_LOOP, 70, 93, "-", "run:24:-*24"),
            (&FLIPPER_MOVE_RUN_START, 65, 70, "flipper_run_loop", "run:8:-*6"),
            (&FLIPPER_MOVE_WALK, 41, 64, "-", "walk:4:-*24"),
            (&FLIPPER_MOVE_START_RUN, 41, 45, "-", "run:8:-*4 run:8:flipper_run"),
            (&FLIPPER_MOVE_PAIN2, 94, 98, "flipper_run", "move:0:-*5"),
            (&FLIPPER_MOVE_PAIN1, 99, 103, "flipper_run", "move:0:-*5"),
            (&FLIPPER_MOVE_ATTACK, 0, 19, "flipper_run", "charge:0:flipper_preattack charge:0:-*12 charge:0:flipper_bite charge:0:-*4 charge:0:flipper_bite charge:0:-"),
            (&FLIPPER_MOVE_DEATH, 104, 159, "flipper_dead", "move:0:-*56"),
        ];
        assert_moves_match_c(c, NAMES);
    }

    fn flipper(world: &mut GameWorld) -> EntityKey {
        spawn_monster(world, "monster_flipper", Vec3f::new(0.0, 0.0, 0.0))
    }

    #[test]
    fn flipper_spawns_with_the_c_values_and_swims() {
        let (mut world, gi) = test_world_recording();
        let key = flipper(&mut world);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(
            (ent.game.health, ent.game.max_health, ent.game.mass),
            (50, 50, 100)
        );
        assert_eq!(
            (ent.mins, ent.maxs),
            (Vec3f::new(-16.0, -16.0, 0.0), Vec3f::new(16.0, 16.0, 32.0))
        );
        assert!(ent.game.flags.contains(EntityFlags::SWIM));
        assert!(ent
            .think
            .is_some_and(|t| std::ptr::fn_addr_eq(t, ai::swimmonster_start_go as ThinkFn)));
        let mi = mi(&world, key);
        assert_eq!(mi.scale, 1.0);
        assert!(mi.attack.is_none(), "C flipper has no ranged attack");
        assert!(mi
            .melee
            .is_some_and(|f| std::ptr::fn_addr_eq(f, flipper_melee as ThinkFn)));
        assert!(mi
            .run
            .is_some_and(|f| std::ptr::fn_addr_eq(f, flipper_start_run as ThinkFn)));
        assert!(mi.walk.is_some() && mi.stand.is_some() && mi.sight.is_some());
        assert!(playing(&world, key, &FLIPPER_MOVE_STAND));
        for s in SOUNDS {
            assert!(gi.sounds.borrow().iter().any(|n| n == s), "{s} precached");
        }
    }

    #[test]
    fn flipper_run_starts_run_start_then_loops() {
        let mut world = test_world();
        let key = flipper(&mut world);
        flipper_start_run(&mut world, key);
        assert!(playing(&world, key, &FLIPPER_MOVE_START_RUN));
        flipper_run(&mut world, key);
        assert!(playing(&world, key, &FLIPPER_MOVE_RUN_START));
        flipper_run_loop(&mut world, key);
        assert!(playing(&world, key, &FLIPPER_MOVE_RUN_LOOP));
        flipper_walk(&mut world, key);
        assert!(playing(&world, key, &FLIPPER_MOVE_WALK));
        flipper_melee(&mut world, key);
        assert!(playing(&world, key, &FLIPPER_MOVE_ATTACK));
        flipper_stand(&mut world, key);
        assert!(playing(&world, key, &FLIPPER_MOVE_STAND));
    }

    #[test]
    fn flipper_preattack_chomps_and_bite_hits_in_melee_range() {
        let (mut world, gi) = test_world_recording();
        let key = flipper(&mut world);
        let player = player_at(&mut world, Vec3f::new(40.0, 0.0, 0.0));
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);
        flipper_preattack(&mut world, key);
        assert_eq!(
            sounds_of(&world, &gi, key),
            [(CHAN_WEAPON, "flipper/flpatck1.wav".into(), ATTN_NORM)]
        );
        next_trace_hits(&world, &gi, player);
        flipper_bite(&mut world, key);
        assert_eq!(world.entities.get(player).unwrap().game.health, 95);
    }

    fn pain(world: &mut GameWorld, key: EntityKey, health: i32) {
        world.entities.get_mut(key).unwrap().game.health = health;
        flipper_pain(world, key, key, 0.0, 10);
    }

    #[test]
    fn flipper_pain_below_half_health_shows_the_hurt_skin() {
        let mut world = test_world();
        let key = flipper(&mut world);
        pain(&mut world, key, 25);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 0);
        world.entities.get_mut(key).unwrap().game.pain_debounce_time = 100.0;
        pain(&mut world, key, 24);
        assert_eq!(world.entities.get(key).unwrap().state.skinnum, 1);
    }

    #[test]
    fn flipper_pain_debounces_three_seconds_and_picks_pain1_or_pain2() {
        let (mut world, gi) = test_world_recording();
        world.level.time = 5.0;
        let key = flipper(&mut world);
        let (mut one, mut two) = (0, 0);
        for _ in 0..200 {
            world.entities.get_mut(key).unwrap().game.pain_debounce_time = 0.0;
            pain(&mut world, key, 40);
            assert_eq!(
                world.entities.get(key).unwrap().game.pain_debounce_time,
                8.0
            );
            let sound = sounds_of(&world, &gi, key).pop().unwrap().1;
            if playing(&world, key, &FLIPPER_MOVE_PAIN1) {
                assert_eq!(sound, "flipper/flppain1.wav");
                one += 1;
            } else {
                assert!(playing(&world, key, &FLIPPER_MOVE_PAIN2));
                assert_eq!(sound, "flipper/flppain2.wav");
                two += 1;
            }
        }
        assert!(one > 50 && two > 50, "{one} {two}");

        // Within the debounce: no new sound or move.
        let calls = gi.sound_calls.borrow().len();
        flipper_stand(&mut world, key);
        pain(&mut world, key, 40);
        assert_eq!(gi.sound_calls.borrow().len(), calls);
        assert!(playing(&world, key, &FLIPPER_MOVE_STAND));
    }

    #[test]
    fn flipper_pain_in_nightmare_debounces_without_an_animation() {
        let (mut world, gi) = test_world_recording();
        world.game.skill = 3;
        let key = flipper(&mut world);
        let calls = gi.sound_calls.borrow().len();
        pain(&mut world, key, 40);
        assert_eq!(
            world.entities.get(key).unwrap().game.pain_debounce_time,
            3.0
        );
        assert_eq!(gi.sound_calls.borrow().len(), calls);
        assert!(playing(&world, key, &FLIPPER_MOVE_STAND));
    }

    #[test]
    fn flipper_die_at_gib_health_throws_gibs_and_a_head() {
        let (mut world, gi) = test_world_recording();
        let key = flipper(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -30;
        let before = world.entities.iter().count();
        flipper_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        // 2 bones + 2 meat; the flipper itself became the head.
        assert_eq!(world.entities.iter().count(), before + 4);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_ne!(
            ent.svflags & SvFlags::MONSTER.bits(),
            SvFlags::MONSTER.bits()
        );
        assert_eq!(sounds_of(&world, &gi, key)[0].1, "misc/udeath.wav");
    }

    #[test]
    fn flipper_die_above_gib_health_plays_the_death_once() {
        let (mut world, gi) = test_world_recording();
        let key = flipper(&mut world);
        world.entities.get_mut(key).unwrap().game.health = -29;
        flipper_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert!(playing(&world, key, &FLIPPER_MOVE_DEATH));
        assert_eq!(
            sounds_of(&world, &gi, key),
            [(CHAN_VOICE, "flipper/flpdeth1.wav".into(), ATTN_NORM)]
        );

        // Already dead: nothing more.
        flipper_stand(&mut world, key);
        flipper_die(&mut world, key, key, key, 50, Vec3f::ZERO);
        assert!(playing(&world, key, &FLIPPER_MOVE_STAND));
        assert_eq!(sounds_of(&world, &gi, key).len(), 1);
    }

    #[test]
    fn flipper_dead_lowers_the_box_floor_to_the_trace_and_falls() {
        let (mut world, gi) = test_world_recording();
        let key = flipper(&mut world);
        world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 100.0);
        next_trace_hits(&world, &gi, key);
        gi.next_trace.borrow_mut().as_mut().unwrap().endpos = Vec3f::new(0.0, 0.0, 96.0);
        flipper_dead(&mut world, key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.mins.z, -4.0);
        assert_eq!(ent.game.movetype, MoveType::Toss);
        assert_ne!(ent.svflags & SvFlags::DEADMONSTER.bits(), 0);
        assert_eq!(ent.game.nextthink, 0.0);
        assert_eq!(*gi.trace_masks.borrow().last().unwrap(), ent.clipmask);
    }

    #[test]
    fn flipper_sights_a_player_and_bites_it() {
        let (mut world, gi) = hit_world(true);
        let key = flipper(&mut world);
        let player = hit_player(&mut world, &gi, Vec3f::new(120.0, 0.0, 0.0));
        let attacking = run_until(&mut world, 20, |w| playing(w, key, &FLIPPER_MOVE_ATTACK));
        assert!(attacking.is_some(), "sighted and went for melee");
        assert_eq!(world.entities.get(key).unwrap().game.enemy, Some(player));
        assert!(sounds_of(&world, &gi.floor.inner, key)
            .iter()
            .any(|s| s.1 == "flipper/flpsght1.wav"));
        // It swam in to bite range; the first bite (frame 14) hits.
        assert!(world.entities.get(key).unwrap().state.origin.x > 0.0);
        run_until(&mut world, 20, |w| {
            w.entities.get(key).unwrap().state.frame == FRAME_FLPBIT01 + 13
        });
        assert_eq!(world.entities.get(player).unwrap().game.health, 95);
    }
}
