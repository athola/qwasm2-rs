//! Game world — central state holder for all game logic.
//!
//! `GameWorld` owns all entities, level state, and engine callbacks.
//! It replaces the C globals `g_edicts[]`, `level`, `game`, `globals`.

use q2_shared::types::*;

use crate::entity::{EntityKey, EntityStorage};
use crate::items::ItemDef;
use crate::spawn::SpawnTable;
use crate::traits::{EntityLink, GameImport, ModelInfo};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
    /// Target name of the start to use (C `game.spawnpoint`), from the
    /// previous level's exit; empty for a fresh map.
    pub spawnpoint: String,
    pub maxclients: i32,
    pub maxentities: i32,
    pub serverflags: i32,
    pub num_items: i32,
    pub autosaved: bool,
}

impl Default for GameLocals {
    fn default() -> Self {
        Self {
            spawnpoint: String::new(),
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
    /// Engine callbacks. `Rc` so a pmove trace closure can hold its own
    /// handle while game code borrows `self` mutably.
    pub gi: Rc<dyn GameImport>,
}

impl GameWorld {
    /// Create a new game world with the given engine callbacks.
    pub fn new(gi: Rc<dyn GameImport>, maxentities: usize) -> Self {
        let game = GameLocals {
            maxentities: maxentities as i32,
            ..GameLocals::default()
        };
        let mut entities = EntityStorage::new(maxentities);
        entities.set_max_clients(game.maxclients.max(0) as usize);
        Self {
            entities,
            level: LevelLocals::default(),
            game,
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

    /// Free an entity: unlink it and keep the slot as a freed edict, so its
    /// number is not reused for another 0.5 s. Client slots are never freed.
    /// C ref: G_FreeEdict (g_utils.c)
    pub fn free_entity(&mut self, key: EntityKey) {
        let Some(number) = self.entities.get(key).map(|e| e.state.number) else {
            return;
        };
        if let Ok(n) = usize::try_from(number) {
            self.gi.unlink_entity(n);
        }
        if number <= self.game.maxclients {
            return;
        }
        self.entities.set_level_time(self.level.time);
        self.entities.soft_free(key);
    }

    /// Recompute the link fields and hand the entity to the server.
    /// C ref: SV_LinkEdict (sv_world.c), whose edict-side half lives here
    /// because the game owns the entities.
    pub fn link_entity(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        if ent.state.number == 0 || !ent.in_use {
            return; // C: the world and free edicts are never linked
        }
        ent.size = ent.maxs - ent.mins;

        // Encode the size into entity_state for client prediction.
        ent.state.solid = if ent.solid == Solid::Bbox
            && ent.svflags & q2_shared::constants::SVF_DEADMONSTER == 0
        {
            let i = ((ent.maxs.x as i32) / 8).clamp(1, 31);
            let j = ((-ent.mins.z as i32) / 8).clamp(1, 31);
            let k = (((ent.maxs.z + 32.0) as i32) / 8).clamp(1, 63);
            (k << 10) | (j << 5) | i
        } else if ent.solid == Solid::Bsp {
            31 // a solid_bbox will never create this value
        } else {
            0
        };

        let angles = ent.state.angles;
        if ent.solid == Solid::Bsp && (angles.x != 0.0 || angles.y != 0.0 || angles.z != 0.0) {
            // Expand for rotation.
            let max = [ent.mins, ent.maxs]
                .iter()
                .flat_map(|v| [v.x.abs(), v.y.abs(), v.z.abs()])
                .fold(0.0f32, f32::max);
            ent.absmin = ent.state.origin - Vec3f::splat(max);
            ent.absmax = ent.state.origin + Vec3f::splat(max);
        } else {
            ent.absmin = ent.state.origin + ent.mins;
            ent.absmax = ent.state.origin + ent.maxs;
        }
        // Movement is clipped an epsilon away from an edge, so check boxes
        // that don't quite touch.
        ent.absmin -= Vec3f::ONE;
        ent.absmax += Vec3f::ONE;
        ent.linkcount += 1;

        let owner = ent.owner;
        let link = EntityLink {
            number: ent.state.number as usize,
            origin: ent.state.origin,
            angles,
            mins: ent.mins,
            maxs: ent.maxs,
            absmin: ent.absmin,
            absmax: ent.absmax,
            solid: ent.solid,
            modelindex: ent.state.modelindex,
            owner: None,
            svflags: ent.svflags,
        };
        let owner = owner
            .and_then(|k| self.entities.get(k))
            .and_then(|o| usize::try_from(o.state.number).ok());
        self.gi.link_entity(&EntityLink { owner, ..link });
    }

    /// C `gi.setmodel`: modelindex from the server, and for an inline model
    /// its bounds, then link.
    pub fn set_model(&mut self, key: EntityKey, name: &str) {
        let Some(number) = self.number_of(key) else {
            return;
        };
        let info = self.gi.set_model(number, name);
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        ent.state.modelindex = info.index;
        if let Some((mins, maxs)) = info.bounds {
            ent.mins = mins;
            ent.maxs = maxs;
            self.link_entity(key);
        }
    }

    /// Call the touch function of every trigger `key`'s box overlaps.
    /// C ref: `G_TouchTriggers` (g_utils.c).
    pub fn touch_triggers(&mut self, key: EntityKey) {
        let Some(ent) = self.entities.get(key) else {
            return;
        };
        // Dead things don't activate triggers.
        let alive_kind =
            ent.client.is_some() || ent.svflags & q2_shared::constants::SVF_MONSTER != 0;
        if alive_kind && ent.game.health <= 0 {
            return;
        }
        let hits = self.gi.box_edicts(
            ent.absmin,
            ent.absmax,
            q2_shared::constants::MAX_EDICTS,
            AREA_TRIGGERS,
        );
        for number in hits {
            // A trigger earlier in the list may have freed this one.
            let Some(hit) = self.entities.key_for_number(number) else {
                continue;
            };
            let touch = self
                .entities
                .get(hit)
                .filter(|e| e.in_use)
                .and_then(|e| e.touch);
            if let Some(touch) = touch {
                touch(self, hit, key, None, None);
            }
        }
    }

    /// Entity number of `key`, as passed to GameImport calls.
    pub fn number_of(&self, key: EntityKey) -> Option<usize> {
        self.entities
            .get(key)
            .and_then(|e| usize::try_from(e.state.number).ok())
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

/// C `AREA_TRIGGERS` for `GameImport::box_edicts`.
pub const AREA_TRIGGERS: i32 = 2;

/// `GameImport` for testing game logic without a real server.
///
/// Calls return safe defaults; links, unlinks and trace pass-entities are
/// recorded so tests can check what the game told the server.
#[derive(Default)]
pub struct MockGameImport {
    pub links: RefCell<Vec<EntityLink>>,
    pub unlinks: RefCell<Vec<usize>>,
    pub trace_pass_ents: RefCell<Vec<Option<usize>>>,
    pub configstrings: RefCell<Vec<(i32, String)>>,
    /// Returned by the next `trace` call instead of an unobstructed trace.
    pub next_trace: RefCell<Option<Trace>>,
    /// Inline-model bounds `set_model` reports, by name (`*n`).
    pub inline_bounds: RefCell<HashMap<String, (Vec3f, Vec3f)>>,
    /// Model names in registration order; index = position + 1.
    pub models: RefCell<Vec<String>>,
    /// Entity numbers `box_edicts` reports.
    pub box_edicts: RefCell<Vec<usize>>,
}

impl GameImport for MockGameImport {
    fn bprintf(&self, _: i32, _: &str) {}
    fn dprintf(&self, _: &str) {}
    fn cprintf(&self, _: Option<usize>, _: i32, _: &str) {}
    fn centerprintf(&self, _: Option<usize>, _: &str) {}
    fn sound(&self, _: Option<usize>, _: i32, _: i32, _: f32, _: f32, _: f32) {}
    fn model_index(&self, _: &str) -> i32 {
        0
    }
    fn sound_index(&self, _: &str) -> i32 {
        0
    }
    fn image_index(&self, _: &str) -> i32 {
        0
    }
    fn set_model(&self, _: usize, name: &str) -> ModelInfo {
        if name.is_empty() {
            return ModelInfo::default();
        }
        if let Some(n) = name.strip_prefix('*').and_then(|n| n.parse::<i32>().ok()) {
            return ModelInfo {
                index: n + 1,
                bounds: self.inline_bounds.borrow().get(name).copied(),
            };
        }
        let mut models = self.models.borrow_mut();
        let pos = models.iter().position(|m| m == name).unwrap_or_else(|| {
            models.push(name.to_string());
            models.len() - 1
        });
        ModelInfo {
            index: pos as i32 + 1,
            bounds: None,
        }
    }

    fn trace(
        &self,
        _start: Vec3f,
        _mins: Vec3f,
        _maxs: Vec3f,
        end: Vec3f,
        pass_ent: Option<usize>,
        _content_mask: i32,
    ) -> Trace {
        self.trace_pass_ents.borrow_mut().push(pass_ent);
        if let Some(trace) = self.next_trace.borrow_mut().take() {
            return trace;
        }
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

    fn point_contents(&self, _: Vec3f) -> i32 {
        0
    }
    fn in_pvs(&self, _: Vec3f, _: Vec3f) -> bool {
        true
    }
    fn in_phs(&self, _: Vec3f, _: Vec3f) -> bool {
        true
    }
    fn link_entity(&self, ent: &EntityLink) {
        self.links.borrow_mut().push(*ent);
    }
    fn unlink_entity(&self, number: usize) {
        self.unlinks.borrow_mut().push(number);
    }
    fn box_edicts(&self, _: Vec3f, _: Vec3f, max_count: usize, _: i32) -> Vec<usize> {
        let mut hits = self.box_edicts.borrow().clone();
        hits.truncate(max_count);
        hits
    }

    fn configstring(&self, num: i32, string: &str) {
        self.configstrings
            .borrow_mut()
            .push((num, string.to_string()));
    }
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

    fn argc(&self) -> i32 {
        0
    }
    fn argv(&self, _: i32) -> String {
        String::new()
    }
    fn args(&self) -> String {
        String::new()
    }
    fn add_command_string(&self, _: &str) {}
}

// ---------------------------------------------------------------------------
// Game Main Loop — G_RunFrame, InitGame
// C ref: g_main.c
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Initialize the game — register all spawn functions, set up items.
    /// C ref: `InitGame` (g_main.c / savegame.c).
    pub fn init_game(&mut self) {
        self.init_items();
        self.register_trigger_spawns();
        self.register_target_spawns();
        self.register_func_spawns();
        self.register_misc_spawns();
        self.register_monster_spawns();
    }

    /// Run one game frame — the main game tick.
    /// C ref: `G_RunFrame` (g_main.c:447-514).
    pub fn run_frame(&mut self) {
        self.level.framenum += 1;
        self.level.time = self.level.framenum as f32 * crate::constants::FRAMETIME;
        self.entities.set_level_time(self.level.time);

        // Pick a player for monsters to target this frame.
        crate::ai::ai_set_sight_client(self);

        // Check exit intermission.
        if self.level.exitintermission {
            return;
        }

        // Collect all entity keys (avoids borrow issues during iteration).
        let keys: Vec<crate::entity::EntityKey> = self.entities.iter().map(|(k, _)| k).collect();

        for key in keys {
            let Some(ent) = self.entities.get(key) else {
                continue;
            };
            if !ent.in_use {
                continue;
            }

            // Save old origin for lerp.
            let origin = ent.state.origin;
            if let Some(ent) = self.entities.get_mut(key) {
                ent.state.old_origin = origin;
            }

            // Run entity physics + think. Move_Calc starts a move this
            // frame only when called from the mover's own think.
            self.level.current_entity = Some(key);
            self.run_entity(key);
        }

        // Build the player state each client is sent this frame.
        self.client_end_server_frames();
    }

    /// Parse an entity string and spawn all entities.
    /// C ref: `SpawnEntities` (g_spawn.c).
    pub fn spawn_entities(&mut self, mapname: &str, entstring: &str, spawnpoint: &str) {
        self.game.spawnpoint = spawnpoint.to_string();
        // C SpawnEntities: a new map starts from zeroed edicts and level.
        self.entities = EntityStorage::new(self.game.maxentities.max(0) as usize);
        self.entities
            .set_max_clients(self.game.maxclients.max(0) as usize);
        self.level = LevelLocals {
            mapname: mapname.to_string(),
            ..LevelLocals::default()
        };
        // C SP_worldspawn sets this from the maxclients cvar (g_spawn.c).
        self.gi.configstring(
            q2_shared::constants::CS_MAXCLIENTS as i32,
            &self.game.maxclients.to_string(),
        );

        // C ED_ParseEdict errors are ERR_DROP: spawn nothing from a bad string.
        let parsed = match crate::spawn::parse_entity_string(entstring) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::error!("spawn_entities: {e}");
                return;
            }
        };

        for fields in &parsed {
            let classname = match fields.get("classname") {
                Some(cn) => cn.clone(),
                None => continue,
            };

            // C ED_CallSpawn: items first (health has its own spawns), then
            // the spawn table.
            let item = match crate::items::health_spawn(&classname) {
                Some(_) => self.find_item_by_name("Health"),
                None => self.find_item(&classname),
            };
            if let Some(item) = item {
                let Some(key) = self.entities.spawn() else {
                    break; // Storage full.
                };
                if let Some(ent) = self.entities.get_mut(key) {
                    crate::spawn::apply_spawn_fields(ent, fields);
                    if let Some((model, count)) = crate::items::health_spawn(&classname) {
                        ent.game.model = model.to_string();
                        ent.game.count = count;
                    }
                }
                self.spawn_item(key, item);
                continue;
            }

            let spawn_fn = self.spawn_table.get(&classname).copied();

            if let Some(sfn) = spawn_fn {
                // C SpawnEntities: the first entity (worldspawn) is g_edicts[0].
                let slot = if classname == "worldspawn" {
                    self.entities.spawn_numbered(0)
                } else {
                    self.entities.spawn()
                };
                let Some(key) = slot else {
                    break; // Storage full.
                };

                if let Some(ent) = self.entities.get_mut(key) {
                    crate::spawn::apply_spawn_fields(ent, fields);
                }

                // Every C brush-entity SP_* calls gi.setmodel(ent, ent->model)
                // before sizing itself, and every SP_* links; done once here
                // because spawn functions only see EntityStorage.
                if let Some(model) = fields.get("model").filter(|m| m.starts_with('*')) {
                    self.set_model(key, model);
                }
                sfn(&mut self.entities, key, fields);
                self.link_entity(key);

                // Run monster_start for monster entities.
                let is_monster = self
                    .entities
                    .get(key)
                    .map(|e| e.monsterinfo.is_some())
                    .unwrap_or(false);
                if is_monster {
                    crate::ai::monster_start(self, key);
                }
            }
            // Unknown classnames are silently skipped (logged in full impl).
        }

        // Entities a spawn function created itself (a plat's trigger); C
        // links them inside the SP_* function.
        let unlinked: Vec<EntityKey> = self
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.linkcount == 0)
            .map(|(k, _)| k)
            .collect();
        for key in unlinked {
            self.link_entity(key);
        }
    }
}

/// Helper to create a `GameWorld` for testing.
pub fn test_world() -> GameWorld {
    test_world_recording().0
}

/// A test `GameWorld` plus a handle on its recording import.
pub fn test_world_recording() -> (GameWorld, Rc<MockGameImport>) {
    let gi = Rc::new(MockGameImport::default());
    (GameWorld::new(gi.clone(), 1024), gi)
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
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().expect("should spawn");
        assert_eq!(world.entities.count(), 1);
        assert!(world.entities.get(key).unwrap().in_use);

        // C G_FreeEdict: unlink, then keep the slot as a freed edict so its
        // number is not reused for another 0.5 s.
        world.level.time = 3.0;
        world.free_entity(key);
        let ent = world.entities.get(key).unwrap();
        assert!(!ent.in_use);
        assert_eq!(ent.game.classname, "freed");
        assert_eq!(ent.game.freetime, 3.0);
        assert_eq!(*gi.unlinks.borrow(), [ent.state.number as usize]);
    }

    #[test]
    fn free_entity_never_frees_a_client_slot() {
        let (mut world, gi) = test_world_recording();
        world.set_max_clients(4);
        let client = world.entities.spawn_numbered(2).unwrap();
        world.free_entity(client);
        assert!(world.entities.get(client).unwrap().in_use);
        assert_eq!(*gi.unlinks.borrow(), [2]);
    }

    #[test]
    fn link_entity_sends_number_bounds_and_encodes_bbox_solid() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.origin = Vec3f::new(10.0, 20.0, 30.0);
            ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
            ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
            ent.solid = Solid::Bbox;
        }
        world.link_entity(key);

        let ent = world.entities.get(key).unwrap();
        // C SV_LinkEdict: i = 16/8, j = 24/8, k = (32+32)/8.
        assert_eq!(ent.state.solid, (8 << 10) | (3 << 5) | 2);
        assert_eq!(ent.absmin, Vec3f::new(-7.0, 3.0, 5.0));
        assert_eq!(ent.absmax, Vec3f::new(27.0, 37.0, 63.0));
        assert_eq!(ent.size, Vec3f::new(32.0, 32.0, 56.0));
        let links = gi.links.borrow();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].number, ent.state.number as usize);
        assert_eq!(links[0].absmin, ent.absmin);
        assert_eq!(links[0].solid, Solid::Bbox);
    }

    #[test]
    fn link_entity_expands_rotated_bmodels_and_skips_world_and_freed() {
        let (mut world, gi) = test_world_recording();
        let door = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(door).unwrap();
            ent.mins = Vec3f::new(-8.0, -64.0, 0.0);
            ent.maxs = Vec3f::new(8.0, 0.0, 96.0);
            ent.state.angles = Vec3f::new(0.0, 90.0, 0.0);
            ent.solid = Solid::Bsp;
        }
        world.link_entity(door);
        let ent = world.entities.get(door).unwrap();
        assert_eq!(ent.state.solid, 31);
        // Rotated BSP: a cube of the largest extent (96) around the origin.
        assert_eq!(ent.absmin, Vec3f::new(-97.0, -97.0, -97.0));
        assert_eq!(ent.absmax, Vec3f::new(97.0, 97.0, 97.0));

        let dead = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(dead).unwrap();
            ent.solid = Solid::Bbox;
            ent.svflags = q2_shared::constants::SVF_DEADMONSTER;
        }
        world.link_entity(dead);
        assert_eq!(world.entities.get(dead).unwrap().state.solid, 0);

        let world_ent = world.entities.spawn_numbered(0).unwrap();
        world.link_entity(world_ent);
        let freed = world.spawn().unwrap();
        world.free_entity(freed);
        world.link_entity(freed);
        let numbers: Vec<usize> = gi.links.borrow().iter().map(|l| l.number).collect();
        assert_eq!(numbers.len(), 2, "world and freed entities are not linked");
    }

    #[test]
    fn spawn_entities_fills_generic_fields_like_ed_parse_edict() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            concat!(
                "{ \"classname\" \"worldspawn\" }\n",
                "{ \"classname\" \"info_player_start\" \"targetname\" \"from_base2\" ",
                "\"target\" \"t1\" \"spawnflags\" \"3\" \"wait\" \"2.5\" ",
                "\"angle\" \"90\" \"health\" \"40x\" \"move_origin\" \"1 2 3\" }",
            ),
            "",
        );
        let key = world
            .find_by_targetname(None, "from_base2")
            .expect("targetname parsed");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.target, "t1");
        assert_eq!(ent.game.spawnflags, 3);
        assert_eq!(ent.game.wait, 2.5);
        assert_eq!(ent.state.angles, Vec3f::new(0.0, 90.0, 0.0)); // F_ANGLEHACK
        assert_eq!(ent.game.health, 40); // strtol stops at the 'x'
        assert_eq!(ent.game.move_origin, Vec3f::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn set_model_takes_inline_bounds_and_links_like_pf_setmodel() {
        let (mut world, gi) = test_world_recording();
        gi.inline_bounds.borrow_mut().insert(
            "*1".into(),
            (Vec3f::new(-8.0, -8.0, 0.0), Vec3f::new(8.0, 8.0, 64.0)),
        );
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().solid = Solid::Bsp;
        world.set_model(key, "*1");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.modelindex, 2, "*1 is model index 2");
        assert_eq!(ent.mins, Vec3f::new(-8.0, -8.0, 0.0));
        assert_eq!(ent.maxs, Vec3f::new(8.0, 8.0, 64.0));
        let link = *gi.links.borrow().last().expect("linked");
        assert_eq!(link.modelindex, 2);
        assert_eq!(link.absmax, Vec3f::new(9.0, 9.0, 65.0));
    }

    #[test]
    fn set_model_keeps_bounds_for_non_inline_models() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        world.entities.get_mut(key).unwrap().maxs = Vec3f::splat(16.0);
        world.set_model(key, "models/items/armor/body/tris.md2");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.maxs, Vec3f::splat(16.0));
        assert_eq!(
            ent.state.modelindex, 1,
            "mock registers the first model at 1"
        );
        assert!(gi.links.borrow().is_empty(), "C links only inline models");
    }

    #[test]
    fn spawn_entities_gives_brush_entities_their_model_and_links_them() {
        let (mut world, gi) = test_world_recording();
        gi.inline_bounds.borrow_mut().insert(
            "*1".into(),
            (Vec3f::new(0.0, 0.0, 0.0), Vec3f::new(32.0, 8.0, 96.0)),
        );
        world.init_game();
        world.spawn_entities(
            "base1",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"func_wall\" \"model\" \"*1\" }",
            "",
        );
        let key = world.find_by_classname(None, "func_wall").expect("spawned");
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.game.model, "*1");
        assert_eq!(ent.size, Vec3f::new(32.0, 8.0, 96.0));
        let number = ent.state.number as usize;
        assert!(
            gi.links
                .borrow()
                .iter()
                .any(|l| l.number == number && l.solid == Solid::Bsp),
            "brush entity linked with its bounds"
        );
    }

    #[test]
    fn spawn_entities_links_point_entities_after_their_spawn_function() {
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities(
            "base1",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"trigger_relay\" \"targetname\" \"r\" }",
            "",
        );
        let key = world.find_by_targetname(None, "r").unwrap();
        let number = world.number_of(key).unwrap();
        assert!(gi.links.borrow().iter().any(|l| l.number == number));
    }

    fn touched(
        world: &mut GameWorld,
        trigger: EntityKey,
        _other: EntityKey,
        _: Option<&Plane>,
        _: Option<&Surface>,
    ) {
        world.entities.get_mut(trigger).unwrap().game.count += 1;
    }

    /// A live client and a trigger `box_edicts` reports overlapping it.
    fn client_in_trigger() -> (GameWorld, EntityKey, EntityKey) {
        let (mut world, gi) = test_world_recording();
        let player = world.spawn().unwrap();
        let ent = world.entities.get_mut(player).unwrap();
        ent.client = Some(crate::entity::ClientData::default());
        ent.game.health = 100;
        let trigger = world.spawn().unwrap();
        world.entities.get_mut(trigger).unwrap().touch = Some(touched);
        gi.box_edicts
            .borrow_mut()
            .push(world.number_of(trigger).unwrap());
        (world, player, trigger)
    }

    #[test]
    fn touch_triggers_calls_each_overlapping_trigger() {
        let (mut world, player, trigger) = client_in_trigger();
        world.touch_triggers(player);
        assert_eq!(world.entities.get(trigger).unwrap().game.count, 1);
    }

    #[test]
    fn touch_triggers_ignores_dead_clients() {
        let (mut world, player, trigger) = client_in_trigger();
        world.entities.get_mut(player).unwrap().game.health = 0;
        world.touch_triggers(player);
        assert_eq!(world.entities.get(trigger).unwrap().game.count, 0);
    }

    #[test]
    fn touch_triggers_skips_freed_triggers() {
        let (mut world, player, trigger) = client_in_trigger();
        world.free_entity(trigger);
        world.touch_triggers(player);
        assert_eq!(world.entities.get(trigger).map_or(0, |e| e.game.count), 0);
    }

    #[test]
    fn spawn_entities_puts_worldspawn_at_number_zero() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"info_player_start\" \"origin\" \"0 0 0\" }",
            "",
        );
        let key = world.entities.key_for_number(0).expect("world at 0");
        assert_eq!(
            world.entities.get(key).unwrap().game.classname,
            "worldspawn"
        );
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
        assert!(!world.entities.get(victim).is_some_and(|e| e.in_use));
    }

    #[test]
    fn use_targets_fires_use_fn() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static CALLED: AtomicBool = AtomicBool::new(false);

        fn test_use(
            _world: &mut GameWorld,
            _self_key: EntityKey,
            _other: EntityKey,
            _activator: EntityKey,
        ) {
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
        let gi = MockGameImport::default();
        let start = Vec3f::new(0.0, 0.0, 0.0);
        let end = Vec3f::new(100.0, 0.0, 0.0);
        let trace = gi.trace(start, Vec3f::ZERO, Vec3f::ZERO, end, None, 0);
        assert_eq!(trace.fraction, 1.0);
        assert_eq!(trace.endpos, end);
        assert!(!trace.allsolid);
    }

    // -- Game Main Loop tests --

    #[test]
    fn init_game_registers_all_spawns() {
        let mut world = test_world();
        world.init_game();

        assert!(!world.items.is_empty());
        assert!(world.spawn_table.get("trigger_multiple").is_some());
        assert!(world.spawn_table.get("target_explosion").is_some());
        assert!(world.spawn_table.get("func_door").is_some());
        assert!(world.spawn_table.get("misc_teleporter").is_some());
        assert!(world.spawn_table.get("monster_soldier").is_some());
    }

    #[test]
    fn run_frame_advances_time() {
        let mut world = test_world();
        assert_eq!(world.level.framenum, 0);
        assert_eq!(world.level.time, 0.0);

        world.run_frame();
        assert_eq!(world.level.framenum, 1);
        assert!((world.level.time - 0.1).abs() < 0.001);

        world.run_frame();
        assert_eq!(world.level.framenum, 2);
        assert!((world.level.time - 0.2).abs() < 0.001);
    }

    #[test]
    fn run_frame_processes_entities() {
        let mut world = test_world();

        // Noclip entity with velocity should move.
        let key = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.movetype = crate::constants::MoveType::Noclip;
            ent.velocity = Vec3f::new(100.0, 0.0, 0.0);
        }

        world.run_frame();

        let origin = world.entities.get(key).unwrap().state.origin;
        assert!(origin.x > 0.0);
    }

    #[test]
    fn spawn_entities_from_string() {
        let mut world = test_world();
        world.init_game();

        let entstring = r#"
        {
        "classname" "info_player_start"
        "origin" "100 200 300"
        }
        {
        "classname" "monster_soldier"
        "origin" "500 0 0"
        }
        "#;

        world.spawn_entities("test", entstring, "");

        // Should have 2 entities.
        assert_eq!(world.entities.count(), 2);

        // Player start.
        let ps = world.find_by_classname(None, "info_player_start").unwrap();
        let pos = world.entities.get(ps).unwrap().state.origin;
        assert!((pos.x - 100.0).abs() < 0.01);

        // Soldier — should have been initialized with monster_start.
        let sol = world.find_by_classname(None, "monster_soldier").unwrap();
        let ent = world.entities.get(sol).unwrap();
        assert_eq!(ent.game.health, 30);
        assert!(ent.monsterinfo.is_some());
        assert!(ent.think.is_some()); // monster_think was set by monster_start
    }

    /// **CP-2 Checkpoint**: Spawn player + monster_soldier, soldier enters
    /// stand state, run 10 AI frames without panic.
    #[test]
    fn cp2_spawn_player_and_soldier_run_frames() {
        let mut world = test_world();
        world.init_game();

        let entstring = r#"
        {
        "classname" "info_player_start"
        "origin" "0 0 0"
        }
        {
        "classname" "monster_soldier"
        "origin" "200 0 0"
        }
        "#;

        world.spawn_entities("base1", entstring, "");
        assert_eq!(world.entities.count(), 2);

        // Run 10 game frames — no panics.
        for _ in 0..10 {
            world.run_frame();
        }

        // Soldier should still be alive with advancing animation.
        let sol = world.find_by_classname(None, "monster_soldier").unwrap();
        let ent = world.entities.get(sol).unwrap();
        assert_eq!(ent.game.health, 30);
        assert!(ent.state.frame > 0); // Animation advanced.
        assert!(ent.game.nextthink > 0.0);

        // Level time should have advanced 10 frames.
        assert_eq!(world.level.framenum, 10);
        assert!((world.level.time - 1.0).abs() < 0.01);
    }

    // ===================================================================
    // Cross-module integration tests
    // ===================================================================

    /// Rocket hits soldier → direct damage + splash to bystander.
    #[test]
    fn integration_rocket_kills_soldier() {
        let mut world = test_world();
        world.init_game();

        // Spawn soldier.
        let soldier = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(soldier).unwrap();
            ent.game.classname = "monster_soldier".to_string();
            ent.game.health = 30;
            ent.game.max_health = 30;
            ent.game.takedamage = crate::constants::TakeDamage::Yes;
            ent.state.origin = Vec3f::new(100.0, 0.0, 0.0);
        }

        // Spawn bystander nearby.
        let bystander = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(bystander).unwrap();
            ent.game.health = 200;
            ent.game.takedamage = crate::constants::TakeDamage::Yes;
            ent.state.origin = Vec3f::new(120.0, 0.0, 0.0);
        }

        // Simulate rocket impact on soldier.
        let attacker = world.spawn().unwrap();
        world.t_damage(
            soldier,
            attacker,
            attacker,
            Vec3f::new(1.0, 0.0, 0.0),
            Vec3f::new(100.0, 0.0, 0.0),
            Vec3f::ZERO,
            100,
            0,
            crate::constants::DamageFlags::empty(),
            crate::constants::MeansOfDeath::Rocket,
        );

        // Soldier should be dead (30hp - 100dmg).
        assert!(world.entities.get(soldier).unwrap().game.health <= 0);

        // Radius damage to bystander.
        world.t_radius_damage(
            attacker,
            attacker,
            120.0,
            Some(soldier),
            150.0,
            crate::constants::MeansOfDeath::RocketSplash,
        );

        // Bystander should have taken splash damage.
        assert!(world.entities.get(bystander).unwrap().game.health < 200);
    }

    /// Trigger chain: trigger_relay → target_secret → counter increments.
    #[test]
    fn integration_trigger_chain() {
        use std::sync::atomic::{AtomicI32, Ordering};
        static ACTIVATIONS: AtomicI32 = AtomicI32::new(0);

        fn count_use(_w: &mut GameWorld, _s: EntityKey, _o: EntityKey, _a: EntityKey) {
            ACTIVATIONS.fetch_add(1, Ordering::Relaxed);
        }

        ACTIVATIONS.store(0, Ordering::Relaxed);

        let mut world = test_world();

        // relay → target_a (counter)
        let relay = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(relay).unwrap();
            ent.game.classname = "trigger_relay".to_string();
            ent.game.target = "counter".to_string();
        }

        let counter = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(counter).unwrap();
            ent.game.classname = "target".to_string();
            ent.game.targetname = "counter".to_string();
            ent.use_fn = Some(count_use);
        }

        // Fire relay.
        world.use_targets(relay, relay);

        assert_eq!(ACTIVATIONS.load(Ordering::Relaxed), 1);

        // Fire again.
        world.use_targets(relay, relay);
        assert_eq!(ACTIVATIONS.load(Ordering::Relaxed), 2);
    }

    /// Player connects, picks up health, takes damage, verifies HUD stats.
    #[test]
    fn integration_player_lifecycle() {
        let mut world = test_world();
        world.init_game();

        // Create spawn point.
        let spawn = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(spawn).unwrap();
            ent.game.classname = "info_player_start".to_string();
            ent.state.origin = Vec3f::new(0.0, 0.0, 0.0);
        }

        // Connect and begin.
        let player = world.connect_client(1, "\\name\\TestPlayer").unwrap();
        world.begin_client(player);

        assert_eq!(world.entities.get(player).unwrap().game.health, 100);

        // Take damage.
        let attacker = world.spawn().unwrap();
        world.t_damage(
            player,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            25,
            0,
            crate::constants::DamageFlags::empty(),
            crate::constants::MeansOfDeath::Blaster,
        );

        assert_eq!(world.entities.get(player).unwrap().game.health, 75);

        // Update HUD stats.
        world.update_player_stats(player);

        let stats = world
            .entities
            .get(player)
            .unwrap()
            .client
            .as_ref()
            .unwrap()
            .ps
            .stats;
        assert_eq!(stats[1], 75); // STAT_HEALTH
    }

    /// Spawn all entity types from a complex entity string, run frames.
    #[test]
    fn integration_complex_entity_string() {
        let mut world = test_world();
        world.init_game();

        let entstring = r#"
        {
        "classname" "info_player_start"
        "origin" "0 0 0"
        }
        {
        "classname" "monster_soldier_light"
        "origin" "200 0 0"
        }
        {
        "classname" "monster_tank"
        "origin" "500 0 0"
        }
        {
        "classname" "monster_gladiator"
        "origin" "800 0 0"
        }
        {
        "classname" "light"
        "origin" "0 0 128"
        }
        "#;

        world.spawn_entities("test", entstring, "");

        // Should have 5 entities (light might be freed by sp_light).
        assert!(world.entities.count() >= 4);

        // Run 5 frames — no panics.
        for _ in 0..5 {
            world.run_frame();
        }

        // Verify monsters are alive with correct health.
        let sol = world.find_by_classname(None, "monster_soldier_light");
        assert!(sol.is_some());
        assert_eq!(world.entities.get(sol.unwrap()).unwrap().game.health, 20);

        let tank = world.find_by_classname(None, "monster_tank");
        assert!(tank.is_some());
        assert_eq!(world.entities.get(tank.unwrap()).unwrap().game.health, 750);
    }

    /// Weapon fire creates projectile, projectile moves via physics.
    #[test]
    fn integration_weapon_projectile_lifecycle() {
        let mut world = test_world();
        world.level.time = 1.0;

        let owner = world.spawn().unwrap();

        // Fire a rocket.
        world.fire_rocket(
            owner,
            Vec3f::ZERO,
            Vec3f::new(1.0, 0.0, 0.0),
            100,
            650,
            150.0,
            120,
        );

        let rocket = world.find_by_classname(None, "rocket").unwrap();
        let initial_x = world.entities.get(rocket).unwrap().state.origin.x;

        // Run entity physics — rocket should move.
        world.run_entity(rocket);

        let final_x = world.entities.get(rocket).unwrap().state.origin.x;
        assert!(final_x > initial_x, "rocket should have moved forward");
    }

    /// Monster takes damage, pain fires, monster dies.
    #[test]
    fn integration_monster_damage_to_death() {
        let mut world = test_world();
        world.init_game();
        world.level.time = 1.0;

        let entstring = r#"
        {
        "classname" "monster_soldier"
        "origin" "100 0 0"
        }
        "#;
        world.spawn_entities("test", entstring, "");

        let sol = world.find_by_classname(None, "monster_soldier").unwrap();
        assert_eq!(world.entities.get(sol).unwrap().game.health, 30);

        let attacker = world.spawn().unwrap();

        // Hit for 15 — should survive.
        world.t_damage(
            sol,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            15,
            0,
            crate::constants::DamageFlags::empty(),
            crate::constants::MeansOfDeath::Shotgun,
        );
        assert_eq!(world.entities.get(sol).unwrap().game.health, 15);

        // Hit for 20 — should die.
        world.t_damage(
            sol,
            attacker,
            attacker,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::ZERO,
            20,
            0,
            crate::constants::DamageFlags::empty(),
            crate::constants::MeansOfDeath::Shotgun,
        );
        assert!(world.entities.get(sol).unwrap().game.health <= 0);
        assert_eq!(
            world.entities.get(sol).unwrap().game.deadflag,
            crate::constants::DeadFlag::Dead
        );
    }
}
