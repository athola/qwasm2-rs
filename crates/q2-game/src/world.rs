//! Game world — central state holder for all game logic.
//!
//! `GameWorld` owns all entities, level state, and engine callbacks.
//! It replaces the C globals `g_edicts[]`, `level`, `game`, `globals`.

use q2_shared::types::*;

use crate::entity::{EntityKey, EntityStorage};
use crate::items::ItemDef;
use crate::spawn::SpawnTable;
use crate::traits::GameImport;

// ---------------------------------------------------------------------------
// LevelLocals — per-level state (reset each map)
// C ref: local.h:795-860
// ---------------------------------------------------------------------------

/// Level-specific state — reset each map load.
#[derive(Debug, Clone, Default)]
pub struct LevelLocals {
    pub framenum: i32,
    pub time: f32,
    pub level_name: String,
    pub mapname: String,
    pub nextmap: String,
    pub intermission_time: f32,
    pub exitintermission: bool,
    pub sight_client_key: Option<EntityKey>,
    pub sight_entity_framenum: i32,
    pub total_secrets: i32,
    pub found_secrets: i32,
    pub total_goals: i32,
    pub found_goals: i32,
    pub total_monsters: i32,
    pub killed_monsters: i32,
    pub current_entity: Option<EntityKey>,
}

// ---------------------------------------------------------------------------
// GameLocals — persistent game state (survives level changes)
// C ref: local.h:861-886
// ---------------------------------------------------------------------------

/// Persistent game state — survives level changes.
#[derive(Debug, Clone)]
pub struct GameLocals {
    pub maxclients: i32,
    pub maxentities: i32,
    pub serverflags: i32,
    pub num_items: i32,
    pub autosaved: bool,
}

impl Default for GameLocals {
    fn default() -> Self {
        Self {
            maxclients: 1,
            maxentities: 1024,
            serverflags: 0,
            num_items: 0,
            autosaved: false,
        }
    }
}

// ---------------------------------------------------------------------------
// GameWorld — central state
// ---------------------------------------------------------------------------

/// Central state for all game logic.
///
/// All game code receives `&mut GameWorld` and operates on entities through it.
/// This replaces the C pattern of global variables (`g_edicts[]`, `level`,
/// `game`, `globals`) with an explicit, borrow-checked state container.
pub struct GameWorld {
    pub entities: EntityStorage,
    pub level: LevelLocals,
    pub game: GameLocals,
    pub items: Vec<ItemDef>,
    pub spawn_table: SpawnTable,
    pub gi: Box<dyn GameImport>,
}

impl GameWorld {
    /// Create a new game world with the given engine callbacks.
    pub fn new(gi: Box<dyn GameImport>, maxentities: usize) -> Self {
        Self {
            entities: EntityStorage::new(maxentities),
            level: LevelLocals::default(),
            game: GameLocals {
                maxentities: maxentities as i32,
                ..GameLocals::default()
            },
            items: Vec::new(),
            spawn_table: SpawnTable::new(),
            gi,
        }
    }

    // -- Entity management ---------------------------------------------------

    /// Spawn a new entity. Returns `None` if storage is full.
    /// C ref: G_Spawn (g_utils.c)
    pub fn spawn(&mut self) -> Option<EntityKey> {
        self.entities.spawn()
    }

    /// Free an entity and remove it from the world.
    /// C ref: G_FreeEdict (g_utils.c)
    pub fn free_entity(&mut self, key: EntityKey) {
        if let Some(ent) = self.entities.get_mut(key) {
            ent.in_use = false;
            ent.game.classname = String::new();
            ent.game.target = String::new();
            ent.game.targetname = String::new();
            ent.think = None;
            ent.touch = None;
            ent.use_fn = None;
            ent.pain = None;
            ent.die = None;
            ent.blocked = None;
        }
        self.entities.free(key);
    }

    // -- Search utilities ----------------------------------------------------

    /// Find the next entity with a matching classname.
    /// C ref: G_Find (g_utils.c) — simplified to classname search only.
    pub fn find_by_classname(
        &self,
        start_after: Option<EntityKey>,
        classname: &str,
    ) -> Option<EntityKey> {
        let mut past_start = start_after.is_none();

        for (key, ent) in self.entities.iter() {
            if !past_start {
                if Some(key) == start_after {
                    past_start = true;
                }
                continue;
            }
            if ent.in_use && ent.game.classname == classname {
                return Some(key);
            }
        }
        None
    }

    /// Find the next entity with a matching targetname.
    pub fn find_by_targetname(
        &self,
        start_after: Option<EntityKey>,
        targetname: &str,
    ) -> Option<EntityKey> {
        let mut past_start = start_after.is_none();

        for (key, ent) in self.entities.iter() {
            if !past_start {
                if Some(key) == start_after {
                    past_start = true;
                }
                continue;
            }
            if ent.in_use && ent.game.targetname == targetname {
                return Some(key);
            }
        }
        None
    }

    /// Pick a random target entity matching `targetname`.
    /// C ref: G_PickTarget (g_utils.c)
    pub fn pick_target(&self, targetname: &str) -> Option<EntityKey> {
        if targetname.is_empty() {
            return None;
        }

        // Collect all matching targets.
        let mut choices = Vec::new();
        let mut search = None;
        while let Some(key) = self.find_by_targetname(search, targetname) {
            choices.push(key);
            search = Some(key);
        }

        if choices.is_empty() {
            return None;
        }

        // Deterministic selection based on level time for reproducibility.
        let idx = (self.level.time * 10.0) as usize % choices.len();
        Some(choices[idx])
    }

    /// Activate all entities targeted by `ent`'s `target` and free its
    /// `killtarget` entities.
    /// C ref: G_UseTargets (g_utils.c)
    pub fn use_targets(&mut self, ent_key: EntityKey, activator_key: EntityKey) {
        // Read target and killtarget strings from the entity first.
        let (target, killtarget) = {
            let Some(ent) = self.entities.get(ent_key) else {
                return;
            };
            (ent.game.target.clone(), ent.game.killtarget.clone())
        };

        // Kill targets first.
        if !killtarget.is_empty() {
            let mut kill_keys = Vec::new();
            let mut search = None;
            while let Some(key) = self.find_by_targetname(search, &killtarget) {
                kill_keys.push(key);
                search = Some(key);
            }
            for key in kill_keys {
                self.free_entity(key);
            }
        }

        // Activate targets.
        if !target.is_empty() {
            let mut target_keys = Vec::new();
            let mut search = None;
            while let Some(key) = self.find_by_targetname(search, &target) {
                target_keys.push(key);
                search = Some(key);
            }
            for key in target_keys {
                // Call the target's use_fn callback.
                if let Some(use_fn) = self.entities.get(key).and_then(|e| e.use_fn) {
                    use_fn(self, key, ent_key, activator_key);
                }
            }
        }
    }

    /// Compute the movement direction from `angles` field (for triggers/targets).
    /// C ref: G_SetMovedir (g_utils.c)
    pub fn set_movedir(angles: Vec3f) -> Vec3f {
        // Special angle values from the map editor
        if angles.y == -1.0 {
            return Vec3f::new(0.0, 0.0, 1.0); // up
        }
        if angles.y == -2.0 {
            return Vec3f::new(0.0, 0.0, -1.0); // down
        }

        let yaw = angles.y.to_radians();
        let pitch = angles.x.to_radians();
        Vec3f::new(
            yaw.cos() * pitch.cos(),
            yaw.sin() * pitch.cos(),
            -pitch.sin(),
        )
    }
}

// ---------------------------------------------------------------------------
// MockGameImport — for unit testing game code without a real server
// ---------------------------------------------------------------------------

/// No-op `GameImport` implementation for testing.
///
/// All methods do nothing or return safe defaults. This allows testing
/// game logic in isolation without a real server.
pub struct MockGameImport;

impl GameImport for MockGameImport {
    fn bprintf(&self, _: i32, _: &str) {}
    fn dprintf(&self, _: &str) {}
    fn cprintf(&self, _: Option<usize>, _: i32, _: &str) {}
    fn centerprintf(&self, _: Option<usize>, _: &str) {}
    fn sound(&self, _: Option<usize>, _: i32, _: i32, _: f32, _: f32, _: f32) {}
    fn model_index(&self, _: &str) -> i32 { 0 }
    fn sound_index(&self, _: &str) -> i32 { 0 }
    fn image_index(&self, _: &str) -> i32 { 0 }
    fn set_model(&self, _: usize, _: &str) {}

    fn trace(
        &self,
        _start: Vec3f,
        _mins: Vec3f,
        _maxs: Vec3f,
        end: Vec3f,
        _pass_ent: Option<usize>,
        _content_mask: i32,
    ) -> Trace {
        // Default trace: no collision, endpoint reached.
        Trace {
            fraction: 1.0,
            endpos: end,
            allsolid: false,
            startsolid: false,
            plane: Plane::default(),
            surface: None,
            contents: 0,
            ent_index: None,
        }
    }

    fn point_contents(&self, _: Vec3f) -> i32 { 0 }
    fn in_pvs(&self, _: Vec3f, _: Vec3f) -> bool { true }
    fn in_phs(&self, _: Vec3f, _: Vec3f) -> bool { true }
    fn link_entity(&self, _: usize) {}
    fn unlink_entity(&self, _: usize) {}
    fn box_edicts(&self, _: Vec3f, _: Vec3f, _: usize, _: i32) -> Vec<usize> {
        Vec::new()
    }

    fn configstring(&self, _: i32, _: &str) {}
    fn write_byte(&self, _: i32) {}
    fn write_short(&self, _: i32) {}
    fn write_long(&self, _: i32) {}
    fn write_float(&self, _: f32) {}
    fn write_string(&self, _: &str) {}
    fn write_position(&self, _: Vec3f) {}
    fn write_dir(&self, _: Vec3f) {}
    fn write_angle(&self, _: f32) {}
    fn multicast(&self, _: Vec3f, _: Multicast) {}
    fn unicast(&self, _: usize, _: bool) {}

    fn argc(&self) -> i32 { 0 }
    fn argv(&self, _: i32) -> String { String::new() }
    fn args(&self) -> String { String::new() }
    fn add_command_string(&self, _: &str) {}
}

/// Helper to create a `GameWorld` for testing.
pub fn test_world() -> GameWorld {
    GameWorld::new(Box::new(MockGameImport), 1024)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameworld_creates_with_empty_state() {
        let world = test_world();
        assert_eq!(world.entities.count(), 0);
        assert_eq!(world.level.framenum, 0);
        assert_eq!(world.level.time, 0.0);
        assert_eq!(world.game.maxentities, 1024);
    }

    #[test]
    fn spawn_and_free_entity() {
        let mut world = test_world();
        let key = world.spawn().expect("should spawn");
        assert_eq!(world.entities.count(), 1);
        assert!(world.entities.get(key).unwrap().in_use);

        world.free_entity(key);
        assert_eq!(world.entities.count(), 0);
    }

    #[test]
    fn find_by_classname() {
        let mut world = test_world();

        let k1 = world.spawn().unwrap();
        world.entities.get_mut(k1).unwrap().game.classname = "light".into();

        let k2 = world.spawn().unwrap();
        world.entities.get_mut(k2).unwrap().game.classname = "info_player_start".into();

        let k3 = world.spawn().unwrap();
        world.entities.get_mut(k3).unwrap().game.classname = "light".into();

        // Find first light
        let found = world.find_by_classname(None, "light");
        assert_eq!(found, Some(k1));

        // Find second light (after k1)
        let found2 = world.find_by_classname(Some(k1), "light");
        assert_eq!(found2, Some(k3));

        // No more lights after k3
        let found3 = world.find_by_classname(Some(k3), "light");
        assert!(found3.is_none());
    }

    #[test]
    fn find_by_classname_skips_non_in_use() {
        let mut world = test_world();

        let k1 = world.spawn().unwrap();
        world.entities.get_mut(k1).unwrap().game.classname = "test".into();
        world.entities.get_mut(k1).unwrap().in_use = false;

        let found = world.find_by_classname(None, "test");
        assert!(found.is_none());
    }

    #[test]
    fn find_by_targetname() {
        let mut world = test_world();

        let k1 = world.spawn().unwrap();
        world.entities.get_mut(k1).unwrap().game.targetname = "door1".into();

        let found = world.find_by_targetname(None, "door1");
        assert_eq!(found, Some(k1));

        let not_found = world.find_by_targetname(None, "door2");
        assert!(not_found.is_none());
    }

    #[test]
    fn pick_target_returns_none_for_empty() {
        let world = test_world();
        assert!(world.pick_target("").is_none());
        assert!(world.pick_target("nonexistent").is_none());
    }

    #[test]
    fn pick_target_finds_matching() {
        let mut world = test_world();

        let k1 = world.spawn().unwrap();
        world.entities.get_mut(k1).unwrap().game.targetname = "t1".into();

        let found = world.pick_target("t1");
        assert_eq!(found, Some(k1));
    }

    #[test]
    fn use_targets_kills_killtargets() {
        let mut world = test_world();

        // Entity with a killtarget
        let ent = world.spawn().unwrap();
        world.entities.get_mut(ent).unwrap().game.killtarget = "victim".into();

        // The victim entity
        let victim = world.spawn().unwrap();
        world.entities.get_mut(victim).unwrap().game.targetname = "victim".into();

        assert_eq!(world.entities.count(), 2);

        world.use_targets(ent, ent);

        // Victim should be freed
        assert!(world.entities.get(victim).is_none());
    }

    #[test]
    fn use_targets_fires_use_fn() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static CALLED: AtomicBool = AtomicBool::new(false);

        fn test_use(_world: &mut GameWorld, _self_key: EntityKey, _other: EntityKey, _activator: EntityKey) {
            CALLED.store(true, Ordering::Relaxed);
        }

        CALLED.store(false, Ordering::Relaxed);

        let mut world = test_world();

        let activator = world.spawn().unwrap();
        world.entities.get_mut(activator).unwrap().game.target = "my_target".into();

        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.targetname = "my_target".into();
        world.entities.get_mut(target).unwrap().use_fn = Some(test_use);

        world.use_targets(activator, activator);

        assert!(CALLED.load(Ordering::Relaxed));
    }

    #[test]
    fn set_movedir_special_angles() {
        // -1 = straight up
        let dir = GameWorld::set_movedir(Vec3f::new(0.0, -1.0, 0.0));
        assert_eq!(dir, Vec3f::new(0.0, 0.0, 1.0));

        // -2 = straight down
        let dir = GameWorld::set_movedir(Vec3f::new(0.0, -2.0, 0.0));
        assert_eq!(dir, Vec3f::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn set_movedir_forward() {
        // yaw=0, pitch=0 → forward along +X
        let dir = GameWorld::set_movedir(Vec3f::new(0.0, 0.0, 0.0));
        assert!((dir.x - 1.0).abs() < 0.001);
        assert!(dir.y.abs() < 0.001);
        assert!(dir.z.abs() < 0.001);
    }

    #[test]
    fn mock_game_import_trace_passes_through() {
        let gi = MockGameImport;
        let start = Vec3f::new(0.0, 0.0, 0.0);
        let end = Vec3f::new(100.0, 0.0, 0.0);
        let trace = gi.trace(start, Vec3f::ZERO, Vec3f::ZERO, end, None, 0);
        assert_eq!(trace.fraction, 1.0);
        assert_eq!(trace.endpos, end);
        assert!(!trace.allsolid);
    }
}
