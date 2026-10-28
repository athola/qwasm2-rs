//! Entity system — `SlotMap`-backed entity storage.
//!
//! Replaces the flat `edict_t` array from the C engine with a safe,
//! generational-index `SlotMap` that gives O(1) insert / remove / lookup.
//! Freeing is soft, as in C G_FreeEdict: a freed entity keeps its slot and
//! key (with `in_use == false`) until its number is reused, so code that
//! must notice a free checks `is_live`, not whether the key resolves.

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

/// One animation frame of a move. C ref: `mframe_t` (local.h).
#[derive(Debug, Clone, Copy)]
pub struct MonsterFrame {
    /// AI movement for this frame (e.g., ai_walk, ai_run, ai_charge).
    pub aifunc: Option<AnimFrameFn>,
    /// Distance passed to `aifunc`, before `monsterinfo.scale`.
    pub dist: f32,
    /// Called after `aifunc` on this frame (fire, sounds, refire checks).
    pub thinkfunc: Option<ThinkFn>,
}

/// Monster animation move definition. C ref: `mmove_t` (local.h).
/// `frames[i]` belongs to frame `firstframe + i`; the tables are statics,
/// like C's, so callbacks stay plain `fn` pointers.
#[derive(Debug, Clone, Default)]
pub struct MonsterMove {
    pub firstframe: i32,
    pub lastframe: i32,
    pub frames: &'static [MonsterFrame],
    /// Called when animation sequence completes.
    pub endfunc: Option<ThinkFn>,
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
    /// Until when other monsters notice this one is angry (C
    /// `edict_t.show_hostile`). C keeps it on every edict, but only
    /// monsters set it; a player reads as C's zero.
    pub show_hostile: f32,
    /// When the monster starts to drown or suffocate (C
    /// `edict_t.air_finished`, which only monster code sets here).
    pub air_finished: f32,

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
    pub inventory: [i32; crate::constants::MAX_ITEMS],
    /// C `max_bullets`..`max_slugs`; Bandolier and Ammo Pack raise them.
    pub ammo_max: crate::items::AmmoMax,
    /// Item indices (C `gitem_t *`): an `EntityKey` would dangle once
    /// `spawn_entities` rebuilds the storage.
    pub weapon: Option<usize>,
    pub last_weapon: Option<usize>,
    /// C `savedFlags`: the FL_GODMODE, FL_NOTARGET and FL_POWER_ARMOR bits
    /// SaveClientData carries to the next level.
    pub saved_flags: crate::constants::EntityFlags,
    /// C `pers.score`: a coop score carried across levels.
    pub score: i32,
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
            inventory: [0; crate::constants::MAX_ITEMS],
            // C InitClientPersistant.
            ammo_max: crate::items::AMMO_MAX_DEFAULT,
            weapon: None,
            last_weapon: None,
            saved_flags: crate::constants::EntityFlags::empty(),
            score: 0,
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
    /// Last pmove result, to tell pmove to snap when something else moved
    /// the player (C `gclient_t.old_pmove`).
    pub old_pmove: PmoveState,
    pub ping: i32,
    /// Until when the HUD shows the last pickup (C `pickup_msg_time`).
    pub pickup_msg_time: f32,
    pub pers: ClientPersistent,
    pub resp: ClientRespawn,
    /// Shots the silencer still muffles (C `silencer_shots`).
    pub silencer_shots: i32,
    /// Animation class playing; a higher one is not interrupted (C
    /// `anim_priority`, `ANIM_*`).
    pub anim_priority: i32,
    /// Frame the playing animation stops on (C `anim_end`).
    pub anim_end: i32,
    /// This and last command's buttons, and the presses since the last
    /// server frame (C `buttons`, `oldbuttons`, `latched_buttons`).
    pub buttons: u8,
    pub oldbuttons: u8,
    pub latched_buttons: u8,
    /// The weapon already thought this frame from a command (C
    /// `weapon_thunk`).
    pub weapon_thunk: bool,
    /// Aim direction (C `v_angle`); weapons fire along it.
    pub v_angle: Vec3f,
    /// Item index to switch to once the current weapon is down (C
    /// `newweapon`).
    pub newweapon: Option<usize>,
    pub weaponstate: WeaponState,
    /// Inventory index of the current weapon's ammo, 0 for none (C
    /// `ammo_index`).
    pub ammo_index: usize,
    /// View kick from the last shot, cleared each server frame (C
    /// `kick_origin`, `kick_angles`).
    pub kick_origin: Vec3f,
    pub kick_angles: Vec3f,
    /// Level frame until which each powerup lasts (C `quad_framenum`,
    /// `invincible_framenum`, `breather_framenum`, `enviro_framenum`).
    pub quad_framenum: i32,
    pub invincible_framenum: i32,
    pub breather_framenum: i32,
    pub enviro_framenum: i32,
    /// Machinegun climb, 0..9 shots (C `machinegun_shots`).
    pub machinegun_shots: i32,
    /// When a cooked hand grenade goes off, 0 when none (C
    /// `grenade_time`), and whether it already went off in the hand.
    pub grenade_time: f32,
    pub grenade_blew_up: bool,
    /// Looping sound of the current weapon (C `weapon_sound`).
    pub weapon_sound: i32,
    /// Fading damage kick on the view (C `v_dmg_pitch`, `v_dmg_roll`,
    /// `v_dmg_time`).
    pub v_dmg_pitch: f32,
    pub v_dmg_roll: f32,
    pub v_dmg_time: f32,
}

/// Where the gun is in its cycle (C `weaponstate_t`, same order).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WeaponState {
    #[default]
    Ready,
    Activating,
    Dropping,
    Firing,
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
    /// Map "model" key (C `ent->model`): `*n` for brush entities.
    pub model: String,
    pub spawnflags: i32,
    pub timestamp: f32,

    // -- targeting ---------------------------------------------------------
    pub target: String,
    pub targetname: String,
    pub killtarget: String,
    /// Fired instead of `target` when a monster dies (C `ent->deathtarget`).
    pub deathtarget: String,
    pub message: String,
    pub team: String,
    /// Map to change to (C `ent->map`, target_changelevel).
    pub map: String,
    pub pathtarget: String,
    pub combattarget: String,
    /// The entity `target` resolved to, for those that hold on to it (C
    /// `target_ent`): the path_corner a func_train drives to.
    pub target_ent: Option<EntityKey>,

    // -- entity references -------------------------------------------------
    pub ground_entity: Option<EntityKey>,
    /// The ground entity's `linkcount` when it was stood on (C
    /// `groundentity_linkcount`), to notice it moving.
    pub groundentity_linkcount: i32,
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
    /// Health at or below which a monster's die callback gibs instead of
    /// playing a death. C ref: `edict_t.gib_health` (local.h).
    pub gib_health: i32,
    pub deadflag: DeadFlag,
    pub takedamage: TakeDamage,
    pub dmg: i32,
    pub radius_dmg: i32,
    pub dmg_radius: f32,
    pub mass: i32,

    // -- movement ----------------------------------------------------------
    pub speed: f32,
    pub accel: f32,
    pub decel: f32,
    pub ideal_yaw: f32,
    pub yaw_speed: f32,
    pub move_origin: Vec3f,
    pub move_angles: Vec3f,
    /// Unit direction a mover opens along (C `movedir`, G_SetMovedir).
    pub movedir: Vec3f,

    // -- timing ------------------------------------------------------------
    pub pain_debounce_time: f32,
    pub damage_debounce_time: f32,
    pub fly_sound_debounce_time: f32,
    pub last_move_time: f32,
    pub nextthink: f32,
    pub touch_debounce_time: f32,
    /// Until when power armor shows it took a hit (C `powerarmor_time`,
    /// set by `CheckPowerArmor`, read by `M_SetEffects`).
    pub powerarmor_time: f32,

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
    /// Eye height above the origin (C `edict_t.viewheight`).
    pub viewheight: f32,
    /// How lit the entity is for monster sight: a player's from its
    /// usercmd, 128 for a monster that just saw one (C `light_level`).
    pub light_level: i32,
    /// The player's own noise markers, made on first use (C `mynoise`
    /// for PNOISE_SELF/WEAPON, `mynoise2` for PNOISE_IMPACT).
    pub mynoise: Option<EntityKey>,
    pub mynoise2: Option<EntityKey>,
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
    /// The areas the server put the entity in at its last link, 0 for
    /// none (C `areanum`/`areanum2`, written by SV_LinkEdict); a door may
    /// straddle two.
    pub areanum: i32,
    pub areanum2: i32,

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

    /// Slot in `GameWorld::clients` (entity number - 1), only `Some` for
    /// player entities: C `ent->client` pointing into `game.clients`.
    pub client: Option<usize>,

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
            areanum: 0,
            areanum2: 0,
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

/// `SlotMap`-backed entity storage with C edict numbering.
///
/// Every live entity also has a wire number (`state.number`, C's index into
/// `g_edicts`): 0 is the world and `1..=max_clients` are the client slots,
/// which `spawn` never hands out. Generational keys mean a key held across a
/// slot's reuse goes stale instead of aliasing the new entity.
pub struct EntityStorage {
    entities: SlotMap<EntityKey, Entity>,
    max_entities: usize,
    /// Entity number -> key (C `g_edicts[n]`).
    numbers: Vec<Option<EntityKey>>,
    max_clients: usize,
    /// `level.time`, for G_Spawn's "freed more than 0.5 s ago" reuse rule.
    level_time: f32,
}

/// C G_FindFreeEdict policies.
#[derive(Clone, Copy, PartialEq)]
enum FreePolicy {
    Default,
    Desperate,
}

impl EntityStorage {
    /// Create storage with room for up to `max_entities` entity numbers,
    /// including the world (number 0).
    pub fn new(max_entities: usize) -> Self {
        Self {
            entities: SlotMap::with_capacity_and_key(max_entities),
            max_entities,
            numbers: vec![None; max_entities],
            max_clients: 0,
            level_time: 0.0,
        }
    }

    /// Reserve numbers `1..=max_clients` for client entities.
    pub fn set_max_clients(&mut self, max_clients: usize) {
        self.max_clients = max_clients;
    }

    /// Current `level.time`, consulted when reusing freed slots.
    pub fn set_level_time(&mut self, time: f32) {
        self.level_time = time;
    }

    /// Spawn a new entity in the first free number above the client slots
    /// (C G_Spawn). Returns `None` if every number is taken.
    pub fn spawn(&mut self) -> Option<EntityKey> {
        let first = self.max_clients + 1;
        let number = self
            .find_free(first, FreePolicy::Default)
            .or_else(|| (first..self.max_entities).find(|&n| self.numbers[n].is_none()))
            .or_else(|| self.find_free(first, FreePolicy::Desperate))?;
        self.init_at(number)
    }

    /// Spawn into a specific number: 0 for the world, `1..=max_clients` for
    /// a client. `None` if the number is out of range or still in use.
    pub fn spawn_numbered(&mut self, number: usize) -> Option<EntityKey> {
        if number >= self.max_entities {
            return None;
        }
        if let Some(key) = self.numbers[number] {
            if self.entities.get(key).is_some_and(|e| e.in_use) {
                return None;
            }
        }
        self.init_at(number)
    }

    /// First soft-freed number at or above `first` that `policy` allows.
    fn find_free(&self, first: usize, policy: FreePolicy) -> Option<usize> {
        (first..self.max_entities).find(|&n| {
            let Some(ent) = self.numbers[n].and_then(|k| self.entities.get(k)) else {
                return false;
            };
            !ent.in_use
                && (policy == FreePolicy::Desperate
                    || ent.game.freetime < 2.0
                    || self.level_time - ent.game.freetime > 0.5)
        })
    }

    /// C G_InitEdict on number `n`, replacing any freed entity there.
    fn init_at(&mut self, number: usize) -> Option<EntityKey> {
        if let Some(old) = self.numbers[number].take() {
            self.entities.remove(old);
        }
        let ent = Entity {
            in_use: true,
            state: EntityState {
                number: number as i32,
                ..Default::default()
            },
            game: GameEntityData {
                gravity: 1.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let key = self.entities.insert(ent);
        self.numbers[number] = Some(key);
        Some(key)
    }

    /// C G_FreeEdict: clear the entity but keep its number until G_Spawn
    /// reuses it, so clients don't see a freed entity morph into a new one.
    pub fn soft_free(&mut self, key: EntityKey) {
        let level_time = self.level_time;
        if let Some(ent) = self.entities.get_mut(key) {
            let number = ent.state.number;
            *ent = Entity {
                state: EntityState {
                    number,
                    ..Default::default()
                },
                ..Default::default()
            };
            ent.game.classname = "freed".to_string();
            ent.game.freetime = level_time;
        }
    }

    /// Remove an entity outright and release its number.
    pub fn free(&mut self, key: EntityKey) {
        if let Some(ent) = self.entities.remove(key) {
            if let Some(slot) = usize::try_from(ent.state.number)
                .ok()
                .and_then(|n| self.numbers.get_mut(n))
            {
                if *slot == Some(key) {
                    *slot = None;
                }
            }
        }
    }

    /// The key holding entity number `n` (in use or soft-freed), if any.
    pub fn key_for_number(&self, n: usize) -> Option<EntityKey> {
        self.numbers.get(n).copied().flatten()
    }

    /// Keys of occupied numbers from `first` upward, in entity-number order
    /// (C walks `g_edicts`). Includes soft-freed entities.
    pub fn keys_from(&self, first: usize) -> impl Iterator<Item = EntityKey> + '_ {
        self.numbers.iter().skip(first).filter_map(|k| *k)
    }

    /// Immutable lookup by key.
    pub fn get(&self, key: EntityKey) -> Option<&Entity> {
        self.entities.get(key)
    }

    /// Mutable lookup by key.
    pub fn get_mut(&mut self, key: EntityKey) -> Option<&mut Entity> {
        self.entities.get_mut(key)
    }

    /// Number of occupied slots, including soft-freed entities whose number
    /// has not been reused yet.
    pub fn count(&self) -> usize {
        self.entities.len()
    }

    /// `true` if `key` names an entity that is in use (C `ent->inuse`).
    pub fn is_live(&self, key: EntityKey) -> bool {
        self.entities.get(key).is_some_and(|e| e.in_use)
    }

    /// Iterate over every occupied slot (in use or soft-freed), in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityKey, &Entity)> {
        self.entities.iter()
    }

    /// Mutably iterate over every occupied slot (in use or soft-freed).
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
        // Number 0 is the world, which G_Spawn never hands out, so a
        // three-slot table holds two spawned entities.
        let mut storage = EntityStorage::new(3);
        let _k1 = storage.spawn().unwrap();
        let _k2 = storage.spawn().unwrap();
        assert!(storage.spawn().is_none(), "should return None when full");
    }

    fn number_of(storage: &EntityStorage, key: EntityKey) -> i32 {
        storage.get(key).unwrap().state.number
    }

    #[test]
    fn spawn_numbers_start_above_reserved_client_slots_like_g_spawn() {
        let mut storage = EntityStorage::new(16);
        storage.set_max_clients(4);
        let a = storage.spawn().unwrap();
        let b = storage.spawn().unwrap();
        assert_eq!((number_of(&storage, a), number_of(&storage, b)), (5, 6));
        assert_eq!(storage.key_for_number(5), Some(a));
        assert_eq!(storage.key_for_number(6), Some(b));
        assert_eq!(storage.key_for_number(1), None);
    }

    #[test]
    fn spawn_numbered_places_world_and_clients_in_their_slots() {
        let mut storage = EntityStorage::new(16);
        storage.set_max_clients(4);
        let world = storage.spawn_numbered(0).unwrap();
        let client = storage.spawn_numbered(3).unwrap();
        assert_eq!(number_of(&storage, world), 0);
        assert_eq!(number_of(&storage, client), 3);
        assert!(storage.get(client).unwrap().in_use);
        // An in-use slot is not handed out twice; out of range is refused.
        assert!(storage.spawn_numbered(3).is_none());
        assert!(storage.spawn_numbered(16).is_none());
    }

    #[test]
    fn soft_freed_slot_is_reused_only_after_half_a_second_like_g_spawn() {
        let mut storage = EntityStorage::new(3);
        let old = storage.spawn().unwrap();
        storage.set_level_time(5.0);
        storage.soft_free(old);
        assert!(!storage.get(old).unwrap().in_use);

        // Freed 0.4 s ago (and after the first 2 s): number 1 stays retired,
        // so the next spawn takes number 2.
        storage.set_level_time(5.4);
        let fresh = storage.spawn().unwrap();
        assert_eq!(number_of(&storage, fresh), 2);

        // Table full now; the desperate policy reuses number 1 at once and
        // the old key goes stale instead of aliasing the new entity.
        let reused = storage.spawn().unwrap();
        assert_eq!(number_of(&storage, reused), 1);
        assert!(storage.get(old).is_none());
        assert_eq!(storage.key_for_number(1), Some(reused));
    }

    #[test]
    fn slot_freed_in_first_two_seconds_or_long_ago_is_reused_first() {
        let mut storage = EntityStorage::new(8);
        let early = storage.spawn().unwrap();
        storage.set_level_time(1.0);
        storage.soft_free(early); // freetime 1.0 < 2
        let again = storage.spawn().unwrap();
        assert_eq!(number_of(&storage, again), 1);

        storage.set_level_time(10.0);
        storage.soft_free(again);
        storage.set_level_time(10.6);
        let later = storage.spawn().unwrap();
        assert_eq!(number_of(&storage, later), 1);
    }

    #[test]
    fn free_releases_the_number_for_reuse() {
        let mut storage = EntityStorage::new(8);
        let key = storage.spawn().unwrap();
        storage.free(key);
        assert_eq!(storage.key_for_number(1), None);
        let next = storage.spawn().unwrap();
        assert_eq!(number_of(&storage, next), 1);
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
            frames: &[MonsterFrame {
                aifunc: None,
                dist: 8.0,
                thinkfunc: None,
            }],
            endfunc: None,
        };
        let mi = MonsterInfo {
            currentmove: Some(mm),
            ..MonsterInfo::default()
        };
        let cm = mi.currentmove.as_ref().unwrap();
        assert_eq!(cm.firstframe, 0);
        assert_eq!(cm.lastframe, 10);
        assert_eq!(cm.frames[0].dist, 8.0);
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
