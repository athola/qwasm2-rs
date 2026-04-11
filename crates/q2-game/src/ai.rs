//! Monster AI — pathfinding, target selection, movement, animation.
//!
//! Port of `g_ai.c` (1,328 lines) and `g_monster.c` (1,086 lines).
//! Provides the base AI functions that all 20 monster types share.
//!
//! # AI state machine
//! Monsters cycle: stand → (spot player) → run → (in range) → attack → pain → die.
//! Each state is a `MonsterMove` with frame ranges and per-frame callbacks.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_ai.c`, `~/Qwasm2/src/game/g_monster.c`

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// AI movement functions — per-frame callbacks used in MonsterMove sequences
// ---------------------------------------------------------------------------

/// Stand in place. C ref: `ai_stand` (g_ai.c).
pub fn ai_stand(world: &mut GameWorld, key: EntityKey, _dist: f32) {
    // Check for enemies periodically.
    let has_enemy = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .is_some();

    if !has_enemy {
        find_target(world, key);
    }
}

/// Walk toward goal. C ref: `ai_walk` (g_ai.c).
pub fn ai_walk(world: &mut GameWorld, key: EntityKey, dist: f32) {
    // Move forward by dist units.
    move_forward(world, key, dist);
}

/// Run toward enemy. C ref: `ai_run` (g_ai.c).
pub fn ai_run(world: &mut GameWorld, key: EntityKey, dist: f32) {
    let has_enemy = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .is_some();

    if !has_enemy {
        // Lost enemy — go back to stand.
        if let Some(ent) = world.entities.get(key) {
            if let Some(ref mi) = ent.monsterinfo {
                if let Some(stand) = mi.stand {
                    stand(world, key);
                    return;
                }
            }
        }
        return;
    }

    move_forward(world, key, dist);
}

/// Charge toward enemy. C ref: `ai_charge` (g_ai.c).
pub fn ai_charge(world: &mut GameWorld, key: EntityKey, dist: f32) {
    // Face enemy and advance.
    face_enemy(world, key);
    move_forward(world, key, dist);
}

/// Turn to face ideal_yaw. C ref: `ai_turn` (g_ai.c).
pub fn ai_turn(world: &mut GameWorld, key: EntityKey, _dist: f32) {
    // Simplified: snap to ideal_yaw.
    let ideal = world
        .entities
        .get(key)
        .map(|e| e.game.ideal_yaw)
        .unwrap_or(0.0);

    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.angles.y = ideal;
    }
}

// ---------------------------------------------------------------------------
// AI utility functions
// ---------------------------------------------------------------------------

/// Move entity forward along its facing direction by `dist` units.
fn move_forward(world: &mut GameWorld, key: EntityKey, dist: f32) {
    if dist <= 0.0 {
        return;
    }

    let yaw = world
        .entities
        .get(key)
        .map(|e| e.state.angles.y.to_radians())
        .unwrap_or(0.0);

    let forward = Vec3f::new(yaw.cos(), yaw.sin(), 0.0);

    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.origin += forward * dist;
    }
}

/// Face the current enemy. C ref: part of `ai_run`.
fn face_enemy(world: &mut GameWorld, key: EntityKey) {
    let enemy_origin = world
        .entities
        .get(key)
        .and_then(|e| e.game.enemy)
        .and_then(|ek| world.entities.get(ek))
        .map(|e| e.state.origin);

    let self_origin = world
        .entities
        .get(key)
        .map(|e| e.state.origin);

    if let (Some(enemy), Some(self_pos)) = (enemy_origin, self_origin) {
        let diff = enemy - self_pos;
        let yaw = diff.y.atan2(diff.x).to_degrees();
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.ideal_yaw = yaw;
            ent.state.angles.y = yaw;
        }
    }
}

/// Look for visible players to target. C ref: `FindTarget` (g_ai.c).
pub fn find_target(world: &mut GameWorld, key: EntityKey) -> bool {
    // Check sight client (set by AI_SetSightClient each frame).
    let sight_client = world.level.sight_client_key;
    let Some(client_key) = sight_client else {
        return false;
    };

    // Check if client is visible via trace.
    let (self_origin, client_origin) = {
        let self_ent = world.entities.get(key);
        let client_ent = world.entities.get(client_key);
        match (self_ent, client_ent) {
            (Some(s), Some(c)) => (s.state.origin, c.state.origin),
            _ => return false,
        }
    };

    let trace = world.gi.trace(
        self_origin,
        Vec3f::ZERO,
        Vec3f::ZERO,
        client_origin,
        None,
        1, // MASK_SOLID
    );

    if trace.fraction < 1.0 {
        return false; // Blocked.
    }

    // Found target!
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.enemy = Some(client_key);
    }

    // Call sight callback.
    let sight_fn = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .and_then(|mi| mi.sight);
    if let Some(sight) = sight_fn {
        sight(world, key, client_key);
    }

    // Transition to run state.
    let run_fn = world
        .entities
        .get(key)
        .and_then(|e| e.monsterinfo.as_ref())
        .and_then(|mi| mi.run);
    if let Some(run) = run_fn {
        run(world, key);
    }

    true
}

/// Pick a random player for monsters to check this frame.
/// C ref: `AI_SetSightClient` (g_ai.c).
pub fn ai_set_sight_client(world: &mut GameWorld) {
    // Find first player entity.
    let player = world.find_by_classname(None, "player");
    if player.is_none() {
        // Try info_player_start as fallback.
        let start = world.find_by_classname(None, "info_player_start");
        world.level.sight_client_key = start;
    } else {
        world.level.sight_client_key = player;
    }
}

// ---------------------------------------------------------------------------
// Monster animation system
// ---------------------------------------------------------------------------

/// Advance the current animation frame. Called each game tick.
/// C ref: `M_MoveFrame` (g_monster.c).
pub fn monster_move_frame(world: &mut GameWorld, key: EntityKey) {
    let (firstframe, lastframe, frame_fn, dist, endfunc) = {
        let Some(ent) = world.entities.get(key) else { return };
        let Some(ref mi) = ent.monsterinfo else { return };
        let Some(ref cm) = mi.currentmove else { return };
        (cm.firstframe, cm.lastframe, cm.frame_fn, cm.dist, cm.endfunc)
    };

    // Get current frame from entity state.
    let current_frame = world
        .entities
        .get(key)
        .map(|e| e.state.frame)
        .unwrap_or(firstframe);

    // Call per-frame AI function (e.g., ai_stand, ai_walk, ai_run).
    if let Some(ffn) = frame_fn {
        ffn(world, key, dist);
    }

    // Advance frame.
    let next_frame = if current_frame >= lastframe {
        // Animation complete — loop or call endfunc.
        if let Some(ef) = endfunc {
            ef(world, key);
        }
        firstframe
    } else {
        current_frame + 1
    };

    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.frame = next_frame;
    }
}

/// Monster think — advance animation each frame.
/// This is set as the `think` callback on monster entities.
pub fn monster_think(world: &mut GameWorld, key: EntityKey) {
    monster_move_frame(world, key);

    // Schedule next think.
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = world.level.time + FRAMETIME;
    }
}

// ---------------------------------------------------------------------------
// Monster initialization
// ---------------------------------------------------------------------------

/// Common monster initialization. C ref: `monster_start` (g_monster.c).
/// Sets default fields shared by all monsters.
pub fn monster_start(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.svflags |= SvFlags::MONSTER.bits();
        ent.game.movetype = MoveType::Step;
        ent.solid = Solid::Bbox;
        ent.clipmask = 0xFFFF; // MASK_MONSTERSOLID
        ent.game.takedamage = TakeDamage::Yes;

        // Default mass.
        if ent.game.mass == 0 {
            ent.game.mass = 200;
        }

        // Set think to monster animation driver.
        ent.think = Some(monster_think);
        ent.game.nextthink = world.level.time + FRAMETIME;

        // Call stand state if available.
        let stand = ent.monsterinfo.as_ref().and_then(|mi| mi.stand);
        if let Some(stand_fn) = stand {
            // Can't call through ent — need to release the borrow first.
            // The stand_fn variable captures the fn pointer by value (Copy).
            let _ = ent; // release borrow
            stand_fn(world, key);
        }
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::MonsterMove;
    use crate::world::test_world;

    #[test]
    fn ai_stand_without_enemy_does_not_panic() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        ai_stand(&mut world, key, 0.0);
    }

    #[test]
    fn ai_walk_moves_forward() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().state.angles.y = 0.0; // facing +X

        let before = world.entities.get(key).unwrap().state.origin.x;
        ai_walk(&mut world, key, 10.0);
        let after = world.entities.get(key).unwrap().state.origin.x;

        assert!(after > before);
        assert!((after - before - 10.0).abs() < 0.01);
    }

    #[test]
    fn ai_run_without_enemy_returns_to_stand() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static STOOD: AtomicBool = AtomicBool::new(false);

        fn test_stand(_world: &mut GameWorld, _key: EntityKey) {
            STOOD.store(true, Ordering::Relaxed);
        }

        STOOD.store(false, Ordering::Relaxed);

        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                stand: Some(test_stand),
                ..Default::default()
            }));
        }

        ai_run(&mut world, key, 10.0);

        assert!(STOOD.load(Ordering::Relaxed));
    }

    #[test]
    fn monster_move_frame_advances() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.frame = 0;
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 5,
                    frame_fn: None,
                    dist: 0.0,
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.frame, 1);
    }

    #[test]
    fn monster_move_frame_loops() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.frame = 5; // at lastframe
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 5,
                    frame_fn: None,
                    dist: 0.0,
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_move_frame(&mut world, key);

        assert_eq!(world.entities.get(key).unwrap().state.frame, 0); // looped
    }

    #[test]
    fn monster_move_frame_calls_frame_fn() {
        use std::sync::atomic::{AtomicI32, Ordering};
        static CALLS: AtomicI32 = AtomicI32::new(0);

        fn test_frame(_world: &mut GameWorld, _key: EntityKey, _dist: f32) {
            CALLS.fetch_add(1, Ordering::Relaxed);
        }

        CALLS.store(0, Ordering::Relaxed);

        let mut world = test_world();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.frame = 0;
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 3,
                    frame_fn: Some(test_frame),
                    dist: 8.0,
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_move_frame(&mut world, key);

        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn monster_start_sets_defaults() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.monsterinfo = Some(Box::default());
        }

        monster_start(&mut world, key);

        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.movetype, MoveType::Step);
        assert_eq!(ent.game.takedamage, TakeDamage::Yes);
        assert!(ent.svflags & SvFlags::MONSTER.bits() != 0);
        assert_eq!(ent.game.mass, 200);
        assert!(ent.think.is_some());
    }

    #[test]
    fn monster_think_schedules_next() {
        let mut world = test_world();
        world.level.time = 1.0;
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.monsterinfo = Some(Box::new(crate::entity::MonsterInfo {
                currentmove: Some(MonsterMove {
                    firstframe: 0,
                    lastframe: 3,
                    frame_fn: None,
                    dist: 0.0,
                    endfunc: None,
                }),
                ..Default::default()
            }));
        }

        monster_think(&mut world, key);

        let nt = world.entities.get(key).unwrap().game.nextthink;
        assert!((nt - 1.1).abs() < 0.01); // level.time + FRAMETIME
    }

    #[test]
    fn ai_set_sight_client_finds_player() {
        let mut world = test_world();
        let player = world.spawn().unwrap();
        world.entities.get_mut(player).unwrap().game.classname = "player".to_string();

        ai_set_sight_client(&mut world);

        assert_eq!(world.level.sight_client_key, Some(player));
    }

    #[test]
    fn find_target_sets_enemy() {
        let mut world = test_world();

        // Create player as sight client.
        let player = world.spawn().unwrap();
        world.entities.get_mut(player).unwrap().game.classname = "player".to_string();
        world.entities.get_mut(player).unwrap().state.origin = Vec3f::new(100.0, 0.0, 0.0);
        world.level.sight_client_key = Some(player);

        // Create monster.
        let monster = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.state.origin = Vec3f::ZERO;
            ent.monsterinfo = Some(Box::default());
        }

        // MockGameImport trace returns fraction=1.0 (clear LOS).
        let found = find_target(&mut world, monster);

        assert!(found);
        assert_eq!(
            world.entities.get(monster).unwrap().game.enemy,
            Some(player)
        );
    }

    #[test]
    fn face_enemy_rotates_toward_target() {
        let mut world = test_world();

        let monster = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().state.origin = Vec3f::ZERO;

        let enemy = world.spawn().unwrap();
        world.entities.get_mut(enemy).unwrap().state.origin = Vec3f::new(0.0, 100.0, 0.0);

        world.entities.get_mut(monster).unwrap().game.enemy = Some(enemy);

        face_enemy(&mut world, monster);

        let yaw = world.entities.get(monster).unwrap().state.angles.y;
        // Enemy is at +Y → yaw should be ~90 degrees.
        assert!((yaw - 90.0).abs() < 0.1);
    }
}
