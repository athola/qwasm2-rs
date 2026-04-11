//! Soldier monster — 3 variants (light/normal/ss).
//!
//! Template monster implementation. All other monsters follow this pattern.
//! C ref: `~/Qwasm2/src/game/monster/soldier/`

use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage, MonsterInfo, MonsterMove};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Animation frame ranges (matching C animation tables)
// ---------------------------------------------------------------------------

const FRAME_STAND_FIRST: i32 = 0;
const FRAME_STAND_LAST: i32 = 29;

const FRAME_WALK_FIRST: i32 = 73;
const FRAME_WALK_LAST: i32 = 96;

const FRAME_RUN_FIRST: i32 = 97;
const FRAME_RUN_LAST: i32 = 104;

pub const FRAME_ATTACK_FIRST: i32 = 105;
pub const FRAME_ATTACK_LAST: i32 = 112;

const FRAME_PAIN_FIRST: i32 = 60;
const FRAME_PAIN_LAST: i32 = 63;

const FRAME_DEATH_FIRST: i32 = 178;
const FRAME_DEATH_LAST: i32 = 213;

// ---------------------------------------------------------------------------
// Soldier health values per variant
// ---------------------------------------------------------------------------

const SOLDIER_LIGHT_HEALTH: i32 = 20;
const SOLDIER_HEALTH: i32 = 30;
const SOLDIER_SS_HEALTH: i32 = 40;

// ---------------------------------------------------------------------------
// State callbacks
// ---------------------------------------------------------------------------

fn soldier_stand(world: &mut GameWorld, key: EntityKey) {
    let cm = MonsterMove {
        firstframe: FRAME_STAND_FIRST,
        lastframe: FRAME_STAND_LAST,
        frame_fn: Some(ai::ai_stand),
        dist: 0.0,
        endfunc: Some(soldier_stand), // loop
    };
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.monsterinfo {
            mi.currentmove = Some(cm);
        }
    }
}

fn soldier_walk(world: &mut GameWorld, key: EntityKey) {
    let cm = MonsterMove {
        firstframe: FRAME_WALK_FIRST,
        lastframe: FRAME_WALK_LAST,
        frame_fn: Some(ai::ai_walk),
        dist: 4.0,
        endfunc: Some(soldier_walk),
    };
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.monsterinfo {
            mi.currentmove = Some(cm);
        }
    }
}

fn soldier_run(world: &mut GameWorld, key: EntityKey) {
    let cm = MonsterMove {
        firstframe: FRAME_RUN_FIRST,
        lastframe: FRAME_RUN_LAST,
        frame_fn: Some(ai::ai_run),
        dist: 10.0,
        endfunc: Some(soldier_run),
    };
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.monsterinfo {
            mi.currentmove = Some(cm);
        }
    }
}

fn soldier_pain(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _kick: f32,
    _damage: i32,
) {
    let cm = MonsterMove {
        firstframe: FRAME_PAIN_FIRST,
        lastframe: FRAME_PAIN_LAST,
        frame_fn: None,
        dist: 0.0,
        endfunc: Some(soldier_run), // return to run after pain
    };
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.monsterinfo {
            mi.currentmove = Some(cm);
        }
    }
}

fn soldier_die(
    world: &mut GameWorld,
    key: EntityKey,
    _inflictor: EntityKey,
    _attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let cm = MonsterMove {
        firstframe: FRAME_DEATH_FIRST,
        lastframe: FRAME_DEATH_LAST,
        frame_fn: None,
        dist: 0.0,
        endfunc: None, // stay dead
    };
    if let Some(ent) = world.entities.get_mut(key) {
        if let Some(ref mut mi) = ent.monsterinfo {
            mi.currentmove = Some(cm);
        }
        ent.game.deadflag = DeadFlag::Dead;
        ent.svflags |= SvFlags::DEADMONSTER.bits();
        ent.solid = Solid::Not;
        ent.game.takedamage = TakeDamage::No;
    }
}

fn soldier_sight(world: &mut GameWorld, key: EntityKey, _other: EntityKey) {
    // Play sight sound (simplified — just transition to run).
    soldier_run(world, key);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

fn spawn_soldier(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
    health: i32,
    skin: i32,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "monster_soldier".to_string());
        ent.game.health = health;
        ent.game.max_health = health;
        ent.state.skinnum = skin;

        // Monster bounding box.
        ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 32.0);

        // Set up MonsterInfo with callbacks.
        ent.monsterinfo = Some(Box::new(MonsterInfo {
            stand: Some(soldier_stand),
            walk: Some(soldier_walk),
            run: Some(soldier_run),
            sight: Some(soldier_sight),
            currentmove: Some(MonsterMove {
                firstframe: FRAME_STAND_FIRST,
                lastframe: FRAME_STAND_LAST,
                frame_fn: Some(ai::ai_stand),
                dist: 0.0,
                endfunc: Some(soldier_stand),
            }),
            ..Default::default()
        }));

        ent.pain = Some(soldier_pain);
        ent.die = Some(soldier_die);
    }
}

pub fn sp_monster_soldier_light(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_soldier(storage, key, fields, SOLDIER_LIGHT_HEALTH, 0);
    // Call monster_start after — needs GameWorld, done in world.rs.
}

pub fn sp_monster_soldier(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_soldier(storage, key, fields, SOLDIER_HEALTH, 2);
}

pub fn sp_monster_soldier_ss(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    spawn_soldier(storage, key, fields, SOLDIER_SS_HEALTH, 4);
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    #[test]
    fn soldier_spawns_with_correct_health() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        let fields = HashMap::new();

        sp_monster_soldier(&mut world.entities, key, &fields);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.health, SOLDIER_HEALTH);
        assert!(ent.monsterinfo.is_some());
        assert!(ent.pain.is_some());
        assert!(ent.die.is_some());
    }

    #[test]
    fn soldier_variants_have_different_health() {
        let mut world = test_world();
        let empty = HashMap::new();

        let light = world.spawn().unwrap();
        sp_monster_soldier_light(&mut world.entities, light, &empty);

        let normal = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, normal, &empty);

        let ss = world.spawn().unwrap();
        sp_monster_soldier_ss(&mut world.entities, ss, &empty);

        assert_eq!(world.entities.get(light).unwrap().game.health, 20);
        assert_eq!(world.entities.get(normal).unwrap().game.health, 30);
        assert_eq!(world.entities.get(ss).unwrap().game.health, 40);
    }

    #[test]
    fn soldier_stand_sets_animation() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        soldier_stand(&mut world, key);

        let cm = world
            .entities
            .get(key)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap()
            .currentmove
            .as_ref()
            .unwrap();
        assert_eq!(cm.firstframe, FRAME_STAND_FIRST);
        assert_eq!(cm.lastframe, FRAME_STAND_LAST);
    }

    #[test]
    fn soldier_run_sets_higher_speed() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        soldier_run(&mut world, key);

        let cm = world
            .entities
            .get(key)
            .unwrap()
            .monsterinfo
            .as_ref()
            .unwrap()
            .currentmove
            .as_ref()
            .unwrap();
        assert_eq!(cm.dist, 10.0); // run is faster than walk (4.0)
    }

    #[test]
    fn soldier_die_sets_dead_state() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        let attacker = world.spawn().unwrap();
        soldier_die(&mut world, key, attacker, attacker, 100, Vec3f::ZERO);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.deadflag, DeadFlag::Dead);
        assert!(ent.svflags & SvFlags::DEADMONSTER.bits() != 0);
        assert_eq!(ent.solid, Solid::Not);
    }

    #[test]
    fn soldier_full_lifecycle() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        sp_monster_soldier(&mut world.entities, key, &HashMap::new());

        // Initialize.
        ai::monster_start(&mut world, key);
        assert!(world.entities.get(key).unwrap().think.is_some());

        // Run one AI frame (think → monster_move_frame → advance animation).
        ai::monster_think(&mut world, key);
        assert_eq!(world.entities.get(key).unwrap().state.frame, 1);

        // Run several more frames.
        for _ in 0..5 {
            ai::monster_think(&mut world, key);
        }
        assert_eq!(world.entities.get(key).unwrap().state.frame, 6);
    }
}
