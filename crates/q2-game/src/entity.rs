//! Entity system — `SlotMap`-backed entity storage.
//!
//! Replaces the flat `edict_t` array from the C engine with a safe,
//! generational-index `SlotMap` that gives O(1) insert / remove / lookup
//! and catches use-after-free at the type level.

use q2_shared::types::*;
use slotmap::{new_key_type, SlotMap};

use crate::constants::{AiFlags, AttackState, DeadFlag, EntityFlags, MoveType, TakeDamage};
use crate::world::GameWorld;

new_key_type! {
    /// Generational key for an entity slot.
    pub struct EntityKey;
}

// ---------------------------------------------------------------------------
// Entity callback function types
// ---------------------------------------------------------------------------
// All callbacks receive `&mut GameWorld` plus the entity's own key, replacing
// the C pattern of `void (*think)(edict_t *self)` which accessed globals.
// These are `fn` pointers (not closures) so entities stay Send+Sync and
// callbacks can be serialized by name for save/load.

/// `void (*prethink)(edict_t *self)` — called before physics.
pub type PreThinkFn = fn(&mut GameWorld, EntityKey);

/// `void (*think)(edict_t *self)` — called when `nextthink <= level.time`.
pub type ThinkFn = fn(&mut GameWorld, EntityKey);

/// `void (*blocked)(edict_t *self, edict_t *other)` — push blocked.
pub type BlockedFn = fn(&mut GameWorld, EntityKey, EntityKey);

/// `void (*touch)(edict_t *self, edict_t *other, cplane_t *, csurface_t *)`.
pub type TouchFn = fn(&mut GameWorld, EntityKey, EntityKey, Option<&Plane>, Option<&Surface>);

/// `void (*use)(edict_t *self, edict_t *other, edict_t *activator)`.
pub type UseFn = fn(&mut GameWorld, EntityKey, EntityKey, EntityKey);

/// `void (*pain)(edict_t *self, edict_t *other, float kick, int damage)`.
pub type PainFn = fn(&mut GameWorld, EntityKey, EntityKey, f32, i32);

/// `void (*die)(edict_t *self, edict_t *inflictor, edict_t *attacker, int damage, vec3_t point)`.
pub type DieFn = fn(&mut GameWorld, EntityKey, EntityKey, EntityKey, i32, Vec3f);

// ---------------------------------------------------------------------------
// MoveInfo — platform/door movement data (moveinfo_t)
// C ref: local.h:362-390
// ---------------------------------------------------------------------------

/// End-of-move callback for platforms and doors.
pub type EndMoveFn = fn(&mut GameWorld, EntityKey);

/// Movement controller for func_* entities (doors, platforms, trains).
#[derive(Debug, Clone, Default)]
pub struct MoveInfo {
    // -- fixed data (set at spawn) --
    pub start_origin: Vec3f,
    pub start_angles: Vec3f,
    pub end_origin: Vec3f,
    pub end_angles: Vec3f,

    pub sound_start: i32,
    pub sound_middle: i32,
    pub sound_end: i32,

    pub accel: f32,
    pub speed: f32,
    pub decel: f32,
    pub distance: f32,
    pub wait: f32,

    // -- runtime state --
    pub state: i32,
    pub dir: Vec3f,
    pub current_speed: f32,
    pub move_speed: f32,
    pub next_speed: f32,
    pub remaining_distance: f32,
    pub decel_distance: f32,
    pub endfunc: Option<EndMoveFn>,
}

// ---------------------------------------------------------------------------
// MonsterInfo — AI state (monsterinfo_t)
// C ref: local.h:407-439
// ---------------------------------------------------------------------------

/// A single animation frame callback.
pub type AnimFrameFn = fn(&mut GameWorld, EntityKey, f32);

/// Monster animation move definition (mmove_t).
/// Specifies a range of frames with per-frame callbacks.
#[derive(Debug, Clone)]
pub struct MonsterMove {
    pub firstframe: i32,
    pub lastframe: i32,
    /// Per-frame AI callback (e.g., ai_walk, ai_run, ai_charge).
    pub frame_fn: Option<AnimFrameFn>,
    /// Distance argument passed to frame_fn each frame.
    pub dist: f32,
    /// Called when animation sequence completes.
    pub endfunc: Option<ThinkFn>,
}

impl Default for MonsterMove {
    fn default() -> Self {
        Self {
            firstframe: 0,
            lastframe: 0,
            frame_fn: None,
            dist: 0.0,
            endfunc: None,
        }
    }
}

/// Monster AI callback types.
pub type MonsterStandFn = fn(&mut GameWorld, EntityKey);
pub type MonsterDodgeFn = fn(&mut GameWorld, EntityKey, EntityKey, f32);
pub type MonsterSightFn = fn(&mut GameWorld, EntityKey, EntityKey);
pub type MonsterCheckAttackFn = fn(&mut GameWorld, EntityKey) -> bool;

/// Monster AI state. C ref: local.h:407-439 (monsterinfo_t).
#[derive(Debug, Clone, Default)]
pub struct MonsterInfo {
    pub currentmove: Option<MonsterMove>,
    pub aiflags: AiFlags,
    pub nextframe: i32,
    pub scale: f32,

    // -- AI state callbacks --
    pub stand: Option<MonsterStandFn>,
    pub idle: Option<ThinkFn>,
    pub search: Option<ThinkFn>,
    pub walk: Option<ThinkFn>,
    pub run: Option<ThinkFn>,
    pub dodge: Option<MonsterDodgeFn>,
    pub attack: Option<ThinkFn>,
    pub melee: Option<ThinkFn>,
    pub sight: Option<MonsterSightFn>,
    pub checkattack: Option<MonsterCheckAttackFn>,

    // -- AI runtime state --
    pub pausetime: f32,
    pub attack_finished: f32,
    pub saved_goal: Vec3f,
    pub search_time: f32,
    pub trail_time: f32,
    pub last_sighting: Vec3f,
    pub attack_state: AttackState,
    pub lefty: bool,
    pub idle_time: f32,
    pub linkcount: i32,

    pub power_armor_type: i32,
    pub power_armor_power: i32,
}

// ---------------------------------------------------------------------------
// Client-specific data (only present on player entities)
// ---------------------------------------------------------------------------

/// Persistent client data — survives level changes.
#[derive(Debug, Clone)]
pub struct ClientPersistent {
    pub userinfo: String,
    pub netname: String,
    pub connected: bool,
    pub health: i32,
    pub max_health: i32,
    pub selected_item: i32,
    pub inventory: [i32; 256], // MAX_ITEMS
    pub weapon: Option<EntityKey>,
    pub last_weapon: Option<EntityKey>,
}

impl Default for ClientPersistent {
    fn default() -> Self {
        Self {
            userinfo: String::new(),
            netname: String::new(),
            connected: false,
            health: 0,
            max_health: 0,
            selected_item: 0,
            inventory: [0; 256],
            weapon: None,
            last_weapon: None,
        }
    }
}

/// Per-level client data — reset on each map load.
#[derive(Debug, Clone, Default)]
pub struct ClientRespawn {
    pub coop_respawn: ClientPersistent,
    pub enter_frame: i32,
    pub score: i32,
    pub cmd_angles: Vec3f,
    pub spectator: bool,
}

/// Full client-side state (mirrors the game-visible part of `gclient_t`).
#[derive(Debug, Clone, Default)]
pub struct ClientData {
    pub ps: PlayerState,
    pub ping: i32,
    pub pers: ClientPersistent,
    pub resp: ClientRespawn,
}

// ---------------------------------------------------------------------------
// Game-specific entity data  (fields from `local.h`'s extended edict_t)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct GameEntityData {
    pub movetype: MoveType,
    pub flags: EntityFlags,
    pub freetime: f32,
    pub classname: String,
    pub spawnflags: i32,
    pub timestamp: f32,

    // -- targeting ---------------------------------------------------------
    pub target: String,
    pub targetname: String,
    pub killtarget: String,
    pub message: String,
    pub team: String,
    pub pathtarget: String,
    pub combattarget: String,

    // -- entity references -------------------------------------------------
    pub ground_entity: Option<EntityKey>,
    pub enemy: Option<EntityKey>,
    pub oldenemy: Option<EntityKey>,
    pub activator: Option<EntityKey>,
    pub movetarget: Option<EntityKey>,
    pub goalentity: Option<EntityKey>,
    pub chain: Option<EntityKey>,
    pub teamchain: Option<EntityKey>,
    pub teammaster: Option<EntityKey>,

    // -- combat / health ---------------------------------------------------
    pub health: i32,
    pub max_health: i32,
    pub deadflag: DeadFlag,
    pub takedamage: TakeDamage,
    pub dmg: i32,
    pub mass: i32,

    // -- movement ----------------------------------------------------------
    pub speed: f32,
    pub accel: f32,
    pub decel: f32,
    pub ideal_yaw: f32,
    pub yaw_speed: f32,
    pub move_origin: Vec3f,
    pub move_angles: Vec3f,

    // -- timing ------------------------------------------------------------
    pub pain_debounce_time: f32,
    pub damage_debounce_time: f32,
    pub fly_sound_debounce_time: f32,
    pub last_move_time: f32,
    pub nextthink: f32,
    pub touch_debounce_time: f32,

    // -- sound -------------------------------------------------------------
    pub noise_index: i32,
    pub noise_index2: i32,
    pub volume: f32,
    pub attenuation: f32,

    // -- misc --------------------------------------------------------------
    pub wait: f32,
    pub delay: f32,
    pub random: f32,
    pub count: i32,
    pub style: i32,
    pub item: Option<usize>, // index into the item table
    pub gravity: f32,        // per-entity gravity multiplier
    pub watertype: i32,
    pub waterlevel: i32,
}

// ---------------------------------------------------------------------------
// The complete entity
// ---------------------------------------------------------------------------

/// A single entity in the game world.
///
/// This struct merges the C `edict_t` (server-visible) and game-specific
/// fields into a single Rust struct. Callback fields use `fn` pointers
/// (not closures) for serializability and Send+Sync.
#[derive(Clone)]
pub struct Entity {
    /// Networked state (sent to clients).
    pub state: EntityState,
    pub in_use: bool,
    pub solid: Solid,
    pub svflags: u32,
    pub linkcount: i32,

    pub mins: Vec3f,
    pub maxs: Vec3f,
    pub absmin: Vec3f,
    pub absmax: Vec3f,
    pub size: Vec3f,

    pub clipmask: i32,
    pub owner: Option<EntityKey>,

    // -- physics --
    pub velocity: Vec3f,
    pub avelocity: Vec3f,

    // -- entity callbacks (C: function pointers on edict_t) --
    pub prethink: Option<PreThinkFn>,
    pub think: Option<ThinkFn>,
    pub blocked: Option<BlockedFn>,
    pub touch: Option<TouchFn>,
    pub use_fn: Option<UseFn>,
    pub pain: Option<PainFn>,
    pub die: Option<DieFn>,

    // -- movement controller for func_* entities --
    pub moveinfo: Option<Box<MoveInfo>>,

    // -- monster AI state --
    pub monsterinfo: Option<Box<MonsterInfo>>,

    /// Only `Some` for player entities.
    pub client: Option<ClientData>,

    /// Game-specific data from `local.h`.
    pub game: GameEntityData,
}

impl Default for Entity {
    fn default() -> Self {
        Entity {
            state: EntityState::default(),
            in_use: false,
            solid: Solid::Not,
            svflags: 0,
            linkcount: 0,
            mins: Vec3f::ZERO,
            maxs: Vec3f::ZERO,
            absmin: Vec3f::ZERO,
            absmax: Vec3f::ZERO,
            size: Vec3f::ZERO,
            clipmask: 0,
            owner: None,
            velocity: Vec3f::ZERO,
            avelocity: Vec3f::ZERO,
            prethink: None,
            think: None,
            blocked: None,
            touch: None,
            use_fn: None,
            pain: None,
            die: None,
            moveinfo: None,
            monsterinfo: None,
            client: None,
            game: GameEntityData::default(),
        }
    }
}

// Entity can't derive Debug because of fn pointer fields,
// so implement it manually.
impl std::fmt::Debug for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entity")
            .field("in_use", &self.in_use)
            .field("solid", &self.solid)
            .field("classname", &self.game.classname)
            .field("health", &self.game.health)
            .field("movetype", &self.game.movetype)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Entity storage
// ---------------------------------------------------------------------------

/// `SlotMap`-backed entity storage.
///
/// Provides O(1) insert, remove, and lookup with generational indices so
/// stale keys are detected automatically.
pub struct EntityStorage {
    entities: SlotMap<EntityKey, Entity>,
    max_entities: usize,
}

impl EntityStorage {
    /// Create storage with room for up to `max_entities` entities.
    pub fn new(max_entities: usize) -> Self {
        Self {
            entities: SlotMap::with_capacity_and_key(max_entities),
            max_entities,
        }
    }

    /// Spawn a new default entity. Returns `None` if the storage is full.
    pub fn spawn(&mut self) -> Option<EntityKey> {
        if self.entities.len() >= self.max_entities {
            return None;
        }
        let ent = Entity {
            in_use: true,
            ..Default::default()
        };
        Some(self.entities.insert(ent))
    }

    /// Remove an entity and free its slot for reuse.
    pub fn free(&mut self, key: EntityKey) {
        self.entities.remove(key);
    }

    /// Immutable lookup by key.
    pub fn get(&self, key: EntityKey) -> Option<&Entity> {
        self.entities.get(key)
    }

    /// Mutable lookup by key.
    pub fn get_mut(&mut self, key: EntityKey) -> Option<&mut Entity> {
        self.entities.get_mut(key)
    }

    /// Number of entities currently alive.
    pub fn count(&self) -> usize {
        self.entities.len()
    }

    /// Iterate over all live entities.
    pub fn iter(&self) -> impl Iterator<Item = (EntityKey, &Entity)> {
        self.entities.iter()
    }

    /// Mutably iterate over all live entities.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (EntityKey, &mut Entity)> {
        self.entities.iter_mut()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_create_and_lookup() {
        let mut storage = EntityStorage::new(64);
        let key = storage.spawn().expect("should spawn");
        let ent = storage.get(key).expect("should exist");
        assert!(ent.in_use);
        assert_eq!(ent.solid, Solid::Not);
        assert_eq!(storage.count(), 1);
    }

    #[test]
    fn entity_free_and_reuse() {
        let mut storage = EntityStorage::new(64);

        let k1 = storage.spawn().unwrap();
        assert_eq!(storage.count(), 1);

        storage.free(k1);
        assert_eq!(storage.count(), 0);
        // Old key is now invalid.
        assert!(storage.get(k1).is_none());

        // Slot should be reusable.
        let k2 = storage.spawn().unwrap();
        assert_eq!(storage.count(), 1);
        assert!(storage.get(k2).is_some());
    }

    #[test]
    fn entity_max_limit() {
        let mut storage = EntityStorage::new(2);
        let _k1 = storage.spawn().unwrap();
        let _k2 = storage.spawn().unwrap();
        assert!(storage.spawn().is_none(), "should return None when full");
    }

    // -- Callback tests --

    #[test]
    fn entity_default_has_no_callbacks() {
        let ent = Entity::default();
        assert!(ent.think.is_none());
        assert!(ent.touch.is_none());
        assert!(ent.use_fn.is_none());
        assert!(ent.pain.is_none());
        assert!(ent.die.is_none());
        assert!(ent.blocked.is_none());
        assert!(ent.prethink.is_none());
    }

    #[test]
    fn entity_default_has_no_moveinfo() {
        let ent = Entity::default();
        assert!(ent.moveinfo.is_none());
    }

    #[test]
    fn entity_default_has_no_monsterinfo() {
        let ent = Entity::default();
        assert!(ent.monsterinfo.is_none());
    }

    #[test]
    fn entity_default_velocity_is_zero() {
        let ent = Entity::default();
        assert_eq!(ent.velocity, Vec3f::ZERO);
        assert_eq!(ent.avelocity, Vec3f::ZERO);
    }

    #[test]
    fn entity_uses_enum_types_for_game_data() {
        let ent = Entity::default();
        assert_eq!(ent.game.movetype, MoveType::None);
        assert_eq!(ent.game.flags, EntityFlags::empty());
        assert_eq!(ent.game.deadflag, DeadFlag::No);
        assert_eq!(ent.game.takedamage, TakeDamage::No);
    }

    // -- MoveInfo tests --

    #[test]
    fn moveinfo_default_is_zeroed() {
        let mi = MoveInfo::default();
        assert_eq!(mi.speed, 0.0);
        assert_eq!(mi.accel, 0.0);
        assert_eq!(mi.decel, 0.0);
        assert_eq!(mi.state, 0);
        assert!(mi.endfunc.is_none());
    }

    // -- MonsterInfo tests --

    #[test]
    fn monsterinfo_default_has_no_callbacks() {
        let mi = MonsterInfo::default();
        assert!(mi.stand.is_none());
        assert!(mi.walk.is_none());
        assert!(mi.run.is_none());
        assert!(mi.attack.is_none());
        assert!(mi.melee.is_none());
        assert!(mi.sight.is_none());
        assert!(mi.checkattack.is_none());
        assert_eq!(mi.aiflags, AiFlags::empty());
        assert_eq!(mi.attack_state, AttackState::default());
    }

    #[test]
    fn monsterinfo_with_currentmove() {
        let mm = MonsterMove {
            firstframe: 0,
            lastframe: 10,
            frame_fn: None,
            dist: 8.0,
            endfunc: None,
        };
        let mi = MonsterInfo {
            currentmove: Some(mm),
            ..MonsterInfo::default()
        };
        let cm = mi.currentmove.as_ref().unwrap();
        assert_eq!(cm.firstframe, 0);
        assert_eq!(cm.lastframe, 10);
        assert_eq!(cm.dist, 8.0);
    }

    // -- LevelLocals / GameLocals tests --

    #[test]
    fn level_locals_default() {
        let ll = crate::world::LevelLocals::default();
        assert_eq!(ll.framenum, 0);
        assert_eq!(ll.time, 0.0);
        assert!(!ll.exitintermission);
    }

    #[test]
    fn game_locals_default() {
        let gl = crate::world::GameLocals::default();
        assert_eq!(gl.maxclients, 1);
        assert_eq!(gl.maxentities, 1024);
    }
}
