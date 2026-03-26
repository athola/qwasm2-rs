//! Entity system — `SlotMap`-backed entity storage.
//!
//! Replaces the flat `edict_t` array from the C engine with a safe,
//! generational-index `SlotMap` that gives O(1) insert / remove / lookup
//! and catches use-after-free at the type level.

use q2_shared::types::*;
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    /// Generational key for an entity slot.
    pub struct EntityKey;
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
    pub movetype: i32,
    pub flags: i32,
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

    // -- combat / health ---------------------------------------------------
    pub health: i32,
    pub max_health: i32,
    pub deadflag: i32,
    pub takedamage: i32,
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
#[derive(Debug, Clone)]
pub struct Entity {
    /// Networked state (sent to clients).
    pub state: EntityState,
    pub in_use: bool,
    pub solid: Solid,
    pub svflags: u32,

    pub mins: Vec3f,
    pub maxs: Vec3f,
    pub absmin: Vec3f,
    pub absmax: Vec3f,
    pub size: Vec3f,

    pub clipmask: i32,
    pub owner: Option<EntityKey>,

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
            mins: Vec3f::ZERO,
            maxs: Vec3f::ZERO,
            absmin: Vec3f::ZERO,
            absmax: Vec3f::ZERO,
            size: Vec3f::ZERO,
            clipmask: 0,
            owner: None,
            client: None,
            game: GameEntityData::default(),
        }
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
        let mut ent = Entity::default();
        ent.in_use = true;
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
}
