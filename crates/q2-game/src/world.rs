//! Game world — central state holder for all game logic.
//!
//! `GameWorld` owns all entities, level state, and engine callbacks.
//! It replaces the C globals `g_edicts[]`, `level`, `game`, `globals`.

use q2_shared::types::*;

use crate::constants::{
    SPAWNFLAG_NOT_COOP, SPAWNFLAG_NOT_DEATHMATCH, SPAWNFLAG_NOT_EASY, SPAWNFLAG_NOT_HARD,
    SPAWNFLAG_NOT_MEDIUM,
};
use crate::entity::{ClientData, Entity, EntityKey, EntityStorage};
use crate::items::ItemDef;
use crate::spawn::SpawnTable;
#[cfg(test)]
use crate::traits::ModelInfo;
use crate::traits::{EntityLink, GameImport};
#[cfg(test)]
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
    /// Map the pending exit goes to (C `level.changemap`).
    pub changemap: String,
    pub intermission_time: f32,
    pub exitintermission: bool,
    /// Where intermission puts everyone and which way they look.
    pub intermission_origin: Vec3f,
    pub intermission_angle: Vec3f,
    pub sight_client_key: Option<EntityKey>,
    /// A monster that just saw a player, which other monsters notice
    /// (C `level.sight_entity`, set by `FoundTarget`).
    pub sight_entity: Option<EntityKey>,
    pub sight_entity_framenum: i32,
    /// The latest player noise, heard by monsters (C `level.sound_entity`,
    /// set by `PlayerNoise`).
    pub sound_entity: Option<EntityKey>,
    pub sound_entity_framenum: i32,
    /// The latest impact noise, heard only by monsters without an enemy
    /// (C `level.sound2_entity`).
    pub sound2_entity: Option<EntityKey>,
    pub sound2_entity_framenum: i32,
    pub total_secrets: i32,
    pub found_secrets: i32,
    pub total_goals: i32,
    pub found_goals: i32,
    pub total_monsters: i32,
    pub killed_monsters: i32,
    pub current_entity: Option<EntityKey>,
    /// Image index of `i_health` (C `level.pic_health`).
    pub pic_health: i32,
    /// State of `GameWorld::random_u32`.
    pub rand_state: u32,
    /// Gibs and debris thrown this frame, capped at `MAX_GIBS` and
    /// `MAX_DEBRIS` so a big explosion cannot exhaust the edicts (C
    /// `gibsthisframe`/`debristhisframe`, g_misc.c).
    pub gibs_this_frame: i32,
    pub debris_this_frame: i32,
    /// Body queue slot the next corpse goes in (C `level.body_que`).
    pub body_que: i32,
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
    /// C `dmflags` cvar (`DF_*` bits); read by deathmatch spawn selection.
    pub dmflags: i32,
    pub num_items: i32,
    /// C `skill` cvar, clamped to 0..=3 (easy, medium, hard, hard+).
    pub skill: i32,
    /// C `deathmatch` and `coop` cvars (default 0): the browser plays
    /// single player, so both stay false there.
    pub deathmatch: bool,
    pub coop: bool,
    /// C `fraglimit` and `timelimit` (minutes) cvars, 0 for none, and
    /// `sv_maplist`, the deathmatch rotation.
    pub fraglimit: i32,
    pub timelimit: f32,
    pub maplist: String,
}

impl Default for GameLocals {
    fn default() -> Self {
        Self {
            spawnpoint: String::new(),
            maxclients: 1,
            maxentities: 1024,
            serverflags: 0,
            dmflags: 0,
            num_items: 0,
            skill: 1, // C cvar default
            deathmatch: false,
            coop: false,
            fraglimit: 0,
            timelimit: 0.0,
            maplist: String::new(),
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
    /// C `game.clients`, by slot (entity number - 1), for the whole game:
    /// a client entity's `Entity::client` names its slot, and the record
    /// outlives the edicts `spawn_entities` rebuilds.
    pub clients: Vec<ClientData>,
    pub items: Vec<ItemDef>,
    /// What the last `t_damage` was from, for obituaries (C global
    /// `meansOfDeath`).
    pub means_of_death: crate::constants::MeansOfDeath,
    /// Frames left on a player-dropped quad being picked up, for
    /// `use_quad` (C `quad_drop_timeout_hack`); 0 for a fresh one.
    pub quad_drop_timeout_hack: i32,
    /// Which of the three pain animations a hit starts next (C
    /// `P_DamageFeedback`'s static counter).
    pub pain_anim_cycle: i32,
    pub spawn_table: SpawnTable,
    /// Breadcrumbs monsters follow after losing sight (C trail.c statics).
    pub trail: crate::player::trail::PlayerTrail,
    /// What `ai_checkattack` learned about the enemy this frame (C g_ai.c
    /// statics `enemy_vis`, `enemy_infront`, `enemy_range`, `enemy_yaw`).
    pub enemy_check: crate::ai::EnemyCheck,
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
        let maxclients = game.maxclients.max(0) as usize;
        entities.set_max_clients(maxclients);
        Self {
            entities,
            level: LevelLocals::default(),
            game,
            clients: vec![ClientData::default(); maxclients],
            items: Vec::new(),
            means_of_death: crate::constants::MeansOfDeath::Unknown,
            quad_drop_timeout_hack: 0,
            pain_anim_cycle: 0,
            spawn_table: SpawnTable::new(),
            trail: Default::default(),
            enemy_check: Default::default(),
            gi,
        }
    }

    // -- Entity management ---------------------------------------------------

    /// The client record `key` points at (C `ent->client`).
    pub fn client(&self, key: EntityKey) -> Option<&ClientData> {
        self.clients.get(self.entities.get(key)?.client?)
    }

    /// Mutable form of [`Self::client`].
    pub fn client_mut(&mut self, key: EntityKey) -> Option<&mut ClientData> {
        let slot = self.entities.get(key)?.client?;
        self.clients.get_mut(slot)
    }

    /// An entity and its client record at once, for code that reads one
    /// while writing the other as C does through `ent` and `ent->client`.
    pub fn entity_and_client_mut(
        &mut self,
        key: EntityKey,
    ) -> Option<(&mut Entity, &mut ClientData)> {
        let Self {
            entities, clients, ..
        } = self;
        let ent = entities.get_mut(key)?;
        let client = clients.get_mut(ent.client?)?;
        Some((ent, client))
    }

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
        // C G_FreeEdict: client edicts, and in coop or deathmatch the body
        // queue after them, are never freed. C always has `bodyque` edicts
        // there in those modes; checking the classname keeps entities
        // spawned without a worldspawn (unit tests) freeable.
        let body_queue_end = self.game.maxclients + crate::constants::BODY_QUEUE_SIZE as i32;
        let body_queue = (self.game.deathmatch || self.game.coop)
            && number <= body_queue_end
            && self
                .entities
                .get(key)
                .is_some_and(|e| e.game.classname == "bodyque");
        if number <= self.game.maxclients || body_queue {
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
        let (areanum, areanum2) = self.gi.entity_areas(link.number);
        if let Some(ent) = self.entities.get_mut(key) {
            ent.areanum = areanum;
            ent.areanum2 = areanum2;
        }
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

    /// Next in-use entity after `start_after` (in entity-number order) whose
    /// `field` matches `value` case-insensitively. An empty field never
    /// matches (C skips NULL strings).
    /// C ref: G_Find (g_utils.c)
    fn find_by(
        &self,
        start_after: Option<EntityKey>,
        value: &str,
        field: fn(&crate::entity::Entity) -> &str,
    ) -> Option<EntityKey> {
        let first = match start_after {
            Some(key) => self.number_of(key)? + 1,
            None => 0,
        };
        self.entities.keys_from(first).find(|&key| {
            self.entities.get(key).is_some_and(|ent| {
                let s = field(ent);
                ent.in_use && !s.is_empty() && s.eq_ignore_ascii_case(value)
            })
        })
    }

    /// Find the next entity with a matching classname.
    /// C ref: G_Find with FOFS(classname).
    pub fn find_by_classname(
        &self,
        start_after: Option<EntityKey>,
        classname: &str,
    ) -> Option<EntityKey> {
        self.find_by(start_after, classname, |e| &e.game.classname)
    }

    /// Find the next entity with a matching targetname.
    /// C ref: G_Find with FOFS(targetname).
    pub fn find_by_targetname(
        &self,
        start_after: Option<EntityKey>,
        targetname: &str,
    ) -> Option<EntityKey> {
        self.find_by(start_after, targetname, |e| &e.game.targetname)
    }

    /// The next in-use, non-`SOLID_NOT` entity after `start_after` whose box
    /// center is within `radius` of `origin`. C ref: `findradius`
    /// (g_utils.c).
    pub fn find_radius(
        &self,
        start_after: Option<EntityKey>,
        origin: Vec3f,
        radius: f32,
    ) -> Option<EntityKey> {
        let first = match start_after {
            Some(key) => self.number_of(key)? + 1,
            None => 0,
        };
        self.entities.keys_from(first).find(|&key| {
            self.entities.get(key).is_some_and(|ent| {
                ent.in_use
                    && ent.solid != Solid::Not
                    && (origin - (ent.state.origin + (ent.mins + ent.maxs) * 0.5)).length()
                        <= radius
            })
        })
    }

    /// Next value of the game's pseudo-random sequence. A deterministic
    /// stand-in for C `randk()`, advanced on every call.
    pub fn random_u32(&mut self) -> u32 {
        // Numerical Recipes LCG.
        self.level.rand_state = self
            .level
            .rand_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.level.rand_state >> 8
    }

    /// C `random()`: uniform in [0, 1].
    pub fn random(&mut self) -> f32 {
        (self.random_u32() & 0x7fff) as f32 / 0x7fff as f32
    }

    /// C `crandom()`: uniform in [-1, 1].
    pub fn crandom(&mut self) -> f32 {
        let random = (self.random_u32() & 0x7fff) as f32 / 0x7fff as f32;
        2.0 * (random - 0.5)
    }

    /// Telefrag every entity whose box overlaps `key`'s. Returns `false` if
    /// one survived (C callers mostly ignore it).
    /// C ref: `KillBox` (g_utils.c).
    pub fn kill_box(&mut self, key: EntityKey) -> bool {
        use crate::constants::{DamageFlags, MeansOfDeath};
        loop {
            let Some(ent) = self.entities.get(key) else {
                return true;
            };
            let (origin, mins, maxs) = (ent.state.origin, ent.mins, ent.maxs);
            let tr = self.gi.trace(
                origin,
                mins,
                maxs,
                origin,
                None,
                q2_shared::constants::MASK_PLAYERSOLID,
            );
            // The world (0) is never telefragged; C's trace.ent is NULL
            // when nothing is hit.
            let Some(victim) = tr
                .ent_index
                .filter(|&n| n != 0)
                .and_then(|n| self.entities.key_for_number(n))
                .filter(|&v| self.entities.is_live(v))
            else {
                return true;
            };
            // Nail it.
            self.t_damage(
                victim,
                key,
                key,
                Vec3f::ZERO,
                origin,
                Vec3f::ZERO,
                100_000,
                0,
                DamageFlags::NO_PROTECTION,
                MeansOfDeath::Telefrag,
            );
            // If we didn't kill it, fail.
            if self
                .entities
                .get(victim)
                .is_some_and(|v| v.in_use && v.solid != Solid::Not)
            {
                return false;
            }
        }
    }

    /// Pick a random target entity among the first eight matching
    /// `targetname`.
    /// C ref: G_PickTarget (g_utils.c)
    pub fn pick_target(&mut self, targetname: &str) -> Option<EntityKey> {
        const MAXCHOICES: usize = 8;
        if targetname.is_empty() {
            return None;
        }

        let mut choices = Vec::with_capacity(MAXCHOICES);
        let mut search = None;
        while let Some(key) = self.find_by_targetname(search, targetname) {
            choices.push(key);
            search = Some(key);
            if choices.len() == MAXCHOICES {
                break;
            }
        }

        if choices.is_empty() {
            self.gi
                .dprintf(&format!("G_PickTarget: target {targetname} not found\n"));
            return None;
        }
        let idx = self.random_u32() as usize % choices.len();
        Some(choices[idx])
    }

    /// Fire `ent`'s targets: after `delay` via a DelayedUse entity,
    /// otherwise centerprint its message to the activator, free its
    /// `killtarget` entities and call the `use` of every `target` entity.
    /// C ref: G_UseTargets (g_utils.c)
    pub fn use_targets(&mut self, ent_key: EntityKey, activator_key: EntityKey) {
        let Some(ent) = self.entities.get(ent_key) else {
            return;
        };
        let (delay, message, target, killtarget, noise_index, classname) = (
            ent.game.delay,
            ent.game.message.clone(),
            ent.game.target.clone(),
            ent.game.killtarget.clone(),
            ent.game.noise_index,
            ent.game.classname.clone(),
        );

        // Check for a delay: create a temp object to fire later.
        if delay != 0.0 {
            let Some(t) = self.spawn() else {
                return;
            };
            let time = self.level.time;
            if let Some(t) = self.entities.get_mut(t) {
                t.game.classname = "DelayedUse".to_string();
                t.game.nextthink = time + delay;
                t.think = Some(think_delay);
                t.game.activator = Some(activator_key);
                t.game.message = message;
                t.game.target = target;
                t.game.killtarget = killtarget;
            }
            return;
        }

        // Print the message.
        let activator = self
            .entities
            .get(activator_key)
            .filter(|a| a.svflags & q2_shared::constants::SVF_MONSTER == 0)
            .map(|a| a.state.number);
        if let (Some(number), false) = (activator, message.is_empty()) {
            let number = usize::try_from(number).ok();
            self.gi.centerprintf(number, &message);
            let sound = if noise_index != 0 {
                noise_index
            } else {
                self.gi.sound_index("misc/talk1.wav")
            };
            // CHAN_AUTO, ATTN_NORM.
            self.gi.sound(number, 0, sound, 1.0, 1.0, 0.0);
        }

        // Kill killtargets.
        if !killtarget.is_empty() {
            let mut search = None;
            while let Some(t) = self.find_by_targetname(search, &killtarget) {
                search = Some(t);
                let t_class = self
                    .entities
                    .get(t)
                    .map(|e| e.game.classname.clone())
                    .unwrap_or_default();
                if t_class.eq_ignore_ascii_case("target_secret") {
                    self.level.total_secrets -= 1;
                } else if t_class.eq_ignore_ascii_case("target_goal") {
                    self.level.total_goals -= 1;
                    if self.level.found_goals >= self.level.total_goals {
                        self.gi
                            .configstring(q2_shared::constants::CS_CDTRACK as i32, "0");
                    }
                }
                self.free_entity(t);
                if !self.entities.is_live(ent_key) {
                    self.gi
                        .dprintf("entity was removed while using killtargets\n");
                    return;
                }
            }
        }

        // Fire targets.
        if !target.is_empty() {
            let mut search = None;
            while let Some(t) = self.find_by_targetname(search, &target) {
                search = Some(t);
                let t_class = self.entities.get(t).map(|e| e.game.classname.as_str());
                // Doors fire area portals in a specific way.
                if t_class.is_some_and(|c| c.eq_ignore_ascii_case("func_areaportal"))
                    && (classname.eq_ignore_ascii_case("func_door")
                        || classname.eq_ignore_ascii_case("func_door_rotating"))
                {
                    continue;
                }
                if t == ent_key {
                    self.gi.dprintf("WARNING: Entity used itself.\n");
                } else if let Some(use_fn) = self.entities.get(t).and_then(|e| e.use_fn) {
                    use_fn(self, t, ent_key, activator_key);
                }
                if !self.entities.is_live(ent_key) {
                    self.gi.dprintf("entity was removed while using targets\n");
                    return;
                }
            }
        }
    }

    /// Compute the movement direction from `angles` field (for triggers/targets).
    /// C G_SetMovedir also clears `angles`; callers do that themselves.
    /// C ref: G_SetMovedir (g_utils.c)
    pub fn set_movedir(angles: Vec3f) -> Vec3f {
        // Special angle vectors from the map editor (C VEC_UP / VEC_DOWN).
        if angles == Vec3f::new(0.0, -1.0, 0.0) {
            return Vec3f::new(0.0, 0.0, 1.0); // up
        }
        if angles == Vec3f::new(0.0, -2.0, 0.0) {
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

/// C `Think_Delay`: fire the delayed targets, then remove the temp entity.
fn think_delay(world: &mut GameWorld, key: EntityKey) {
    let activator = world.entities.get(key).and_then(|e| e.game.activator);
    match activator {
        Some(activator) => world.use_targets(key, activator),
        None => world.gi.dprintf("Think_Delay with no activator\n"),
    }
    world.free_entity(key);
}

// ---------------------------------------------------------------------------
// MockGameImport — for unit testing game code without a real server
// ---------------------------------------------------------------------------

/// C SP_worldspawn's light styles 0-11: a letter per 0.1 s, 'a' dark, 'm'
/// normal, 'z' double bright.
const LIGHT_STYLES: [&str; 12] = [
    "m",
    "mmnmmommommnonmmonqnmmo",
    "abcdefghijklmnopqrstuvwxyzyxwvutsrqponmlkjihgfedcba",
    "mmmmmaaaaammmmmaaaaaabcdefgabcdefg",
    "mamamamamama",
    "jklmnopqrstuvwxyzyxwvutsrqponmlkj",
    "nmonqnmomnmomomno",
    "mmmaaaabcdefgmmmmaaaammmaamm",
    "mmmaaammmaaammmabcdefaaaammmmabcdefmmmaaaa",
    "aaaaaaaazzzzzzzz",
    "mmamammmmammamamaaamammma",
    "abcdefghijklmnopqrrqponmlkjihgfedcba",
];

/// `light` spawnflag.
const LIGHT_START_OFF: i32 = 1;

/// C ref: `light_use`.
fn light_use(world: &mut GameWorld, key: EntityKey, _other: EntityKey, _activator: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    ent.game.spawnflags ^= LIGHT_START_OFF;
    let on = ent.game.spawnflags & LIGHT_START_OFF == 0;
    let style = ent.game.style;
    world.set_light_style(style, on);
}

/// Sounds C SP_worldspawn precaches before the sexed weapon models, in
/// its order: lava and slime, pickups, gibs and the player's sexed sounds.
const WORLDSPAWN_SOUNDS: [&str; 24] = [
    "player/fry.wav",
    "player/lava1.wav",
    "player/lava2.wav",
    "misc/pc_up.wav",
    "misc/talk1.wav",
    "misc/udeath.wav",
    "items/respawn1.wav",
    "*death1.wav",
    "*death2.wav",
    "*death3.wav",
    "*death4.wav",
    "*fall1.wav",
    "*fall2.wav",
    "*gurp1.wav",
    "*gurp2.wav",
    "*jump1.wav",
    "*pain25_1.wav",
    "*pain25_2.wav",
    "*pain50_1.wav",
    "*pain50_2.wav",
    "*pain75_1.wav",
    "*pain75_2.wav",
    "*pain100_1.wav",
    "*pain100_2.wav",
];

/// View weapons on other players (C SP_worldspawn; the order must match
/// the client's sexed weapon model numbers).
const SEXED_WEAPON_MODELS: [&str; 11] = [
    "#w_blaster.md2",
    "#w_shotgun.md2",
    "#w_sshotgun.md2",
    "#w_machinegun.md2",
    "#w_chaingun.md2",
    "#a_grenades.md2",
    "#w_glauncher.md2",
    "#w_rlauncher.md2",
    "#w_hyperblaster.md2",
    "#w_railgun.md2",
    "#w_bfg.md2",
];

/// Sounds C SP_worldspawn precaches after the sexed weapon models.
const WORLDSPAWN_LATE_SOUNDS: [&str; 15] = [
    "player/gasp1.wav",
    "player/gasp2.wav",
    "player/watr_in.wav",
    "player/watr_out.wav",
    "player/watr_un.wav",
    "player/u_breath1.wav",
    "player/u_breath2.wav",
    "items/pkup.wav",
    "world/land.wav",
    "misc/h2ohit1.wav",
    "items/damage.wav",
    "items/protect.wav",
    "items/protect4.wav",
    "weapons/noammo.wav",
    "infantry/inflies1.wav",
];

/// Gib models C SP_worldspawn precaches, so throwing one never registers
/// a model mid-game.
const GIB_MODELS: [&str; 7] = [
    "models/objects/gibs/sm_meat/tris.md2",
    "models/objects/gibs/arm/tris.md2",
    "models/objects/gibs/bone/tris.md2",
    "models/objects/gibs/bone2/tris.md2",
    "models/objects/gibs/chest/tris.md2",
    "models/objects/gibs/skull/tris.md2",
    "models/objects/gibs/head2/tris.md2",
];

/// The single-player status bar layout program (C g_spawn.c
/// `single_statusbar`), run by the client's SCR_ExecuteLayoutString.
pub const SINGLE_STATUSBAR: &str = concat!(
    "yb\t-24 ",
    // health
    "xv\t0 ",
    "hnum ",
    "xv\t50 ",
    "pic 0 ",
    // ammo
    "if 2 ",
    "\txv\t100 ",
    "\tanum ",
    "\txv\t150 ",
    "\tpic 2 ",
    "endif ",
    // armor
    "if 4 ",
    "\txv\t200 ",
    "\trnum ",
    "\txv\t250 ",
    "\tpic 4 ",
    "endif ",
    // selected item
    "if 6 ",
    "\txv\t296 ",
    "\tpic 6 ",
    "endif ",
    "yb\t-50 ",
    // picked up item
    "if 7 ",
    "\txv\t0 ",
    "\tpic 7 ",
    "\txv\t26 ",
    "\tyb\t-42 ",
    "\tstat_string 8 ",
    "\tyb\t-50 ",
    "endif ",
    // timer
    "if 9 ",
    "\txv\t262 ",
    "\tnum\t2\t10 ",
    "\txv\t296 ",
    "\tpic\t9 ",
    "endif ",
    // help / weapon icon
    "if 11 ",
    "\txv\t148 ",
    "\tpic\t11 ",
    "endif ",
);

/// The deathmatch status bar layout program (C g_spawn.c `dm_statusbar`):
/// the single-player bar plus frags, the spectator notice and the chase
/// camera target.
pub const DM_STATUSBAR: &str = concat!(
    "yb\t-24 ",
    // health
    "xv\t0 ",
    "hnum ",
    "xv\t50 ",
    "pic 0 ",
    // ammo
    "if 2 ",
    "\txv\t100 ",
    "\tanum ",
    "\txv\t150 ",
    "\tpic 2 ",
    "endif ",
    // armor
    "if 4 ",
    "\txv\t200 ",
    "\trnum ",
    "\txv\t250 ",
    "\tpic 4 ",
    "endif ",
    // selected item
    "if 6 ",
    "\txv\t296 ",
    "\tpic 6 ",
    "endif ",
    "yb\t-50 ",
    // picked up item
    "if 7 ",
    "\txv\t0 ",
    "\tpic 7 ",
    "\txv\t26 ",
    "\tyb\t-42 ",
    "\tstat_string 8 ",
    "\tyb\t-50 ",
    "endif ",
    // timer
    "if 9 ",
    "\txv\t246 ",
    "\tnum\t2\t10 ",
    "\txv\t296 ",
    "\tpic\t9 ",
    "endif ",
    // help / weapon icon
    "if 11 ",
    "\txv\t148 ",
    "\tpic\t11 ",
    "endif ",
    // frags
    "xr\t-50 ",
    "yt 2 ",
    "num 3 14 ",
    // spectator
    "if 17 ",
    "xv 0 ",
    "yb -58 ",
    "string2 \"SPECTATOR MODE\" ",
    "endif ",
    // chase camera
    "if 16 ",
    "xv 0 ",
    "yb -68 ",
    "string \"Chasing\" ",
    "xv 64 ",
    "stat_string 16 ",
    "endif ",
);

use crate::traits::AREA_TRIGGERS;

/// `GameImport` for testing game logic without a real server.
///
/// Calls return safe defaults; links, unlinks and trace pass-entities are
/// recorded so tests can check what the game told the server.
/// A recorded `GameImport::sound` call, its arguments in order.
#[cfg(test)]
pub type SoundCall = (Option<usize>, i32, i32, f32, f32, f32);

#[cfg(test)]
#[derive(Default)]
pub struct MockGameImport {
    /// What `point_contents` answers everywhere.
    pub contents: std::cell::Cell<i32>,
    pub links: RefCell<Vec<EntityLink>>,
    pub unlinks: RefCell<Vec<usize>>,
    pub trace_pass_ents: RefCell<Vec<Option<usize>>>,
    /// Content mask of every `trace` call, in order.
    pub trace_masks: RefCell<Vec<i32>>,
    pub configstrings: RefCell<Vec<(i32, String)>>,
    /// Returned by the next `trace` call instead of an unobstructed trace.
    pub next_trace: RefCell<Option<Trace>>,
    /// Returned by later `trace` calls, in order, once `next_trace` is used.
    pub queued_traces: RefCell<std::collections::VecDeque<Trace>>,
    /// Every `add_command_string` text, in order.
    pub commands: RefCell<Vec<String>>,
    /// Inline-model bounds `set_model` reports, by name (`*n`).
    pub inline_bounds: RefCell<HashMap<String, (Vec3f, Vec3f)>>,
    /// Model names in registration order; index = position + 1.
    pub models: RefCell<Vec<String>>,
    /// Entity numbers `box_edicts` reports.
    pub box_edicts: RefCell<Vec<usize>>,
    /// Image names in registration order; index = position + 1.
    pub images: RefCell<Vec<String>>,
    /// Sound names in registration order; index = position + 1.
    pub sounds: RefCell<Vec<String>>,
    /// Every `dprintf` text, in order.
    pub dprints: RefCell<Vec<String>>,
    /// Every `positioned_sound` call as (origin, entity, channel, sound).
    pub positioned_sounds: RefCell<Vec<(Vec3f, usize, i32, i32)>>,
    /// Every `sound` call (entity, channel, index, volume, attenuation,
    /// time offset), in order.
    pub sound_calls: RefCell<Vec<SoundCall>>,
    /// Every `centerprintf` call, in order.
    pub centerprints: RefCell<Vec<(Option<usize>, String)>>,
    /// Every `cprintf` call (entity, text), in order.
    pub cprints: RefCell<Vec<(Option<usize>, String)>>,
    /// The client command `argv`/`args` read, already tokenized.
    pub command: RefCell<Vec<String>>,
    /// Every `set_area_portal_state` call, in order.
    pub portal_states: RefCell<Vec<(i32, bool)>>,
    /// Every `bprintf` call as (print level, text).
    pub bprints: RefCell<Vec<(i32, String)>>,
    /// Every `write_byte` and `write_short` value, in order.
    pub writes: RefCell<Vec<i32>>,
    /// Every `multicast` call.
    pub multicasts: RefCell<Vec<(Vec3f, Multicast)>>,
    /// `entity_areas` by entity number; unlisted numbers are in no area.
    pub areas: RefCell<HashMap<usize, (i32, i32)>>,
    /// Area pairs `areas_connected` reports as cut off (either order).
    pub disconnected_areas: RefCell<Vec<(i32, i32)>>,
}

#[cfg(test)]
impl GameImport for MockGameImport {
    fn bprintf(&self, level: i32, msg: &str) {
        self.bprints.borrow_mut().push((level, msg.to_string()));
    }
    fn dprintf(&self, msg: &str) {
        self.dprints.borrow_mut().push(msg.to_string());
    }
    fn cprintf(&self, ent: Option<usize>, _: i32, msg: &str) {
        self.cprints.borrow_mut().push((ent, msg.to_string()));
    }
    fn centerprintf(&self, ent: Option<usize>, msg: &str) {
        self.centerprints.borrow_mut().push((ent, msg.to_string()));
    }
    fn sound(&self, ent: Option<usize>, chan: i32, index: i32, vol: f32, attn: f32, ofs: f32) {
        let call = (ent, chan, index, vol, attn, ofs);
        self.sound_calls.borrow_mut().push(call);
    }
    fn set_area_portal_state(&self, portal_num: i32, open: bool) {
        self.portal_states.borrow_mut().push((portal_num, open));
    }
    fn areas_connected(&self, a: i32, b: i32) -> bool {
        let cut = self.disconnected_areas.borrow();
        !cut.contains(&(a, b)) && !cut.contains(&(b, a))
    }
    fn entity_areas(&self, number: usize) -> (i32, i32) {
        self.areas.borrow().get(&number).copied().unwrap_or((0, 0))
    }
    fn positioned_sound(&self, o: Vec3f, e: usize, c: i32, s: i32, _: f32, _: f32, _: f32) {
        self.positioned_sounds.borrow_mut().push((o, e, c, s));
    }
    /// Agrees with `set_model`'s index for the same name.
    fn model_index(&self, name: &str) -> i32 {
        self.set_model(0, name).index
    }
    fn sound_index(&self, name: &str) -> i32 {
        let mut sounds = self.sounds.borrow_mut();
        let pos = sounds.iter().position(|s| s == name).unwrap_or_else(|| {
            sounds.push(name.to_string());
            sounds.len() - 1
        });
        pos as i32 + 1
    }
    fn image_index(&self, name: &str) -> i32 {
        let mut images = self.images.borrow_mut();
        let pos = images.iter().position(|i| i == name).unwrap_or_else(|| {
            images.push(name.to_string());
            images.len() - 1
        });
        pos as i32 + 1
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
        content_mask: i32,
    ) -> Trace {
        self.trace_pass_ents.borrow_mut().push(pass_ent);
        self.trace_masks.borrow_mut().push(content_mask);
        if let Some(trace) = self.next_trace.borrow_mut().take() {
            return trace;
        }
        if let Some(trace) = self.queued_traces.borrow_mut().pop_front() {
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
        self.contents.get()
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
    fn write_byte(&self, c: i32) {
        self.writes.borrow_mut().push(c);
    }
    fn write_short(&self, c: i32) {
        self.writes.borrow_mut().push(c);
    }
    fn write_long(&self, _: i32) {}
    fn write_float(&self, _: f32) {}
    fn write_string(&self, _: &str) {}
    fn write_position(&self, _: Vec3f) {}
    fn write_dir(&self, _: Vec3f) {}
    fn write_angle(&self, _: f32) {}
    fn multicast(&self, origin: Vec3f, to: Multicast) {
        self.multicasts.borrow_mut().push((origin, to));
    }
    fn unicast(&self, _: usize, _: bool) {}

    fn argc(&self) -> i32 {
        self.command.borrow().len() as i32
    }
    fn argv(&self, n: i32) -> String {
        let command = self.command.borrow();
        usize::try_from(n)
            .ok()
            .and_then(|n| command.get(n).cloned())
            .unwrap_or_default()
    }
    fn args(&self) -> String {
        self.command.borrow().get(1..).unwrap_or_default().join(" ")
    }
    fn add_command_string(&self, text: &str) {
        self.commands.borrow_mut().push(text.to_string());
    }
}

// ---------------------------------------------------------------------------
// Game Main Loop — G_RunFrame, InitGame
// C ref: g_main.c
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Initialize the game: set up items (spawn functions are the static
    /// `spawn::SPAWNS`). C ref: `InitGame` (g_main.c / savegame.c).
    pub fn init_game(&mut self) {
        self.init_items();
    }

    /// The body queue, configstrings, images, sounds and models C
    /// SP_worldspawn sets up. `sv_gravity` and `snd_fry` are not ported.
    pub(crate) fn worldspawn_configstrings(
        &mut self,
        _key: EntityKey,
        fields: &HashMap<String, String>,
    ) {
        use q2_shared::constants::{
            CS_CDTRACK, CS_ITEMS, CS_NAME, CS_SKY, CS_SKYAXIS, CS_SKYROTATE, CS_STATUSBAR,
        };
        self.init_body_que();
        // SetItemNames.
        for (i, item) in self.items.iter().enumerate() {
            self.gi
                .configstring((CS_ITEMS + i) as i32, item.pickup_name);
        }
        self.level.nextmap = fields.get("nextmap").cloned().unwrap_or_default();
        match fields.get("message").filter(|m| !m.is_empty()) {
            Some(message) => {
                self.gi.configstring(CS_NAME as i32, message);
                self.level.level_name = message.clone();
            }
            None => self.level.level_name = self.level.mapname.clone(),
        }
        let sky = fields.get("sky").filter(|s| !s.is_empty());
        self.gi
            .configstring(CS_SKY as i32, sky.map_or("unit1_", String::as_str));
        let skyrotate = fields
            .get("skyrotate")
            .map_or(0.0, |v| q2_common::cvar::parse_value(v));
        self.gi
            .configstring(CS_SKYROTATE as i32, &format!("{skyrotate:.6}"));
        let axis = fields
            .get("skyaxis")
            .map_or(Vec3f::ZERO, |v| crate::spawn::scan_vector(v));
        self.gi.configstring(
            CS_SKYAXIS as i32,
            &format!("{:.6} {:.6} {:.6}", axis.x, axis.y, axis.z),
        );
        let sounds = fields.get("sounds").map_or(0, |v| crate::spawn::strtol(v));
        self.gi.configstring(CS_CDTRACK as i32, &sounds.to_string());
        let statusbar = if self.game.deathmatch {
            DM_STATUSBAR
        } else {
            SINGLE_STATUSBAR
        };
        self.gi.configstring(CS_STATUSBAR as i32, statusbar);
        self.gi.image_index("i_help");
        self.level.pic_health = self.gi.image_index("i_health");
        self.gi.image_index("help");
        self.gi.image_index("field_3");
        for (i, sound) in WORLDSPAWN_SOUNDS.into_iter().enumerate() {
            self.gi.sound_index(sound);
            if i == 0 {
                // After snd_fry, as C does.
                if let Some(blaster) = self.find_item_by_name("Blaster") {
                    self.precache_item(blaster);
                }
            }
        }
        // Only coop and deathmatch show other players' weapons.
        if self.game.coop || self.game.deathmatch {
            for model in SEXED_WEAPON_MODELS {
                self.gi.model_index(model);
            }
        }
        for sound in WORLDSPAWN_LATE_SOUNDS {
            self.gi.sound_index(sound);
        }
        for model in GIB_MODELS {
            self.gi.model_index(model);
        }

        // Light styles: 0 normal, then C's animated presets; 63 is testing.
        for (style, pattern) in LIGHT_STYLES.iter().enumerate() {
            self.gi
                .configstring((q2_shared::constants::CS_LIGHTS + style) as i32, pattern);
        }
        self.gi
            .configstring((q2_shared::constants::CS_LIGHTS + 63) as i32, "a");
    }

    /// C SP_light: untargeted lights are freed; switchable styles (32 and
    /// up) start on or off and toggle when used.
    pub(crate) fn spawn_light(&mut self, key: EntityKey, _fields: &HashMap<String, String>) {
        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        if ent.game.targetname.is_empty() {
            self.free_entity(key);
            return;
        }
        if ent.game.style >= 32 {
            ent.use_fn = Some(light_use);
            let on = ent.game.spawnflags & LIGHT_START_OFF == 0;
            let style = ent.game.style;
            self.set_light_style(style, on);
        }
    }

    fn set_light_style(&self, style: i32, on: bool) {
        let index = q2_shared::constants::CS_LIGHTS as i32 + style;
        self.gi.configstring(index, if on { "m" } else { "a" });
    }

    /// Run one game frame — the main game tick.
    /// C ref: `G_RunFrame` (g_main.c:447-514).
    pub fn run_frame(&mut self) {
        self.level.framenum += 1;
        self.level.time = self.level.framenum as f32 * crate::constants::FRAMETIME;
        self.entities.set_level_time(self.level.time);
        self.level.gibs_this_frame = 0;
        self.level.debris_this_frame = 0;

        // Pick a player for monsters to target this frame.
        crate::ai::ai_set_sight_client(self);

        if self.level.exitintermission {
            self.exit_level();
            return;
        }

        // Walk entity numbers, re-reading the table each step so entities
        // spawned during the frame run in it too (C loops to the growing
        // `globals.num_edicts`).
        let maxclients = self.game.maxclients.max(0) as usize;
        for number in 0..self.game.maxentities.max(0) as usize {
            let Some(key) = self.entities.key_for_number(number) else {
                continue;
            };
            if !self.entities.is_live(key) {
                continue;
            }
            // Move_Calc starts a move this frame only when called from the
            // mover's own think.
            self.level.current_entity = Some(key);

            let Some(ent) = self.entities.get(key) else {
                continue;
            };
            let (ground, ground_linkcount) =
                (ent.game.ground_entity, ent.game.groundentity_linkcount);
            let ground_moved = ground.is_some_and(|g| {
                self.entities.get(g).map(|g| g.linkcount) != Some(ground_linkcount)
            });
            let Some(ent) = self.entities.get_mut(key) else {
                continue;
            };
            // Save old origin for lerp.
            ent.state.old_origin = ent.state.origin;

            // If the ground entity moved, make sure we are still on it.
            if ground_moved {
                ent.game.ground_entity = None;
                let swims_or_flies = ent.game.flags.intersects(
                    crate::constants::EntityFlags::SWIM | crate::constants::EntityFlags::FLY,
                );
                if !swims_or_flies && ent.svflags & q2_shared::constants::SVF_MONSTER != 0 {
                    self.m_check_ground(key);
                }
            }

            if (1..=maxclients).contains(&number) {
                self.client_begin_server_frame(key);
                continue;
            }
            self.run_entity(key);
        }

        // See if it is time to end a deathmatch.
        self.check_dm_rules();

        // Build the player state each client is sent this frame.
        self.client_end_server_frames();
    }

    /// Run the spawn function for `classname` on the already-filled `key`.
    /// C ref: `ED_CallSpawn` (g_spawn.c).
    pub(crate) fn call_spawn(
        &mut self,
        key: EntityKey,
        classname: &str,
        fields: &HashMap<String, String>,
    ) {
        // Items first (health has its own spawns), then the spawn table.
        let item = match crate::items::health_spawn(classname) {
            Some(_) => self.find_item_by_name("Health"),
            None => self.find_item(classname),
        };
        if let Some(item) = item {
            if let (Some(ent), Some((model, count))) = (
                self.entities.get_mut(key),
                crate::items::health_spawn(classname),
            ) {
                ent.game.model = model.to_string();
                ent.game.count = count;
            }
            self.spawn_item(key, item);
            return;
        }

        let Some(&(_, sfn, model, finish)) = crate::spawn::find_spawn(classname) else {
            self.gi
                .dprintf(&format!("{classname} doesn't have a spawn function\n"));
            return;
        };

        // Every C brush-entity SP_* calls gi.setmodel(ent, ent->model)
        // before sizing itself, and every SP_* links; done once here
        // because spawn functions only see EntityStorage.
        if let Some(model) = fields.get("model").filter(|m| m.starts_with('*')) {
            self.set_model(key, model);
        }
        sfn(&mut self.entities, key, fields);
        if !self.entities.is_live(key) {
            return; // The spawn function removed it (info_null, bad keys).
        }
        if let Some(finish) = finish {
            finish(self, key, fields);
            if !self.entities.is_live(key) {
                return; // C SP_light frees untargeted lights.
            }
        }
        if let Some(model) = model {
            self.set_model(key, model);
        }

        let is_monster = self
            .entities
            .get(key)
            .is_some_and(|e| e.monsterinfo.is_some());
        if is_monster {
            if !crate::ai::monster_start(self, key) {
                return; // Deathmatch freed the monster.
            }
            self.monster_start_item(key, classname, fields);
        }
        // C monster_start / SP_* link the entity after setting it up.
        self.link_entity(key);
    }

    /// The item a monster drops on death, from its `item` key. C ref:
    /// `monster_start` (g_monster.c), which reads `st.item`.
    fn monster_start_item(
        &mut self,
        key: EntityKey,
        classname: &str,
        fields: &HashMap<String, String>,
    ) {
        let Some(name) = fields.get("item") else {
            return;
        };
        match self.find_item(name) {
            Some(item) => {
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.game.item = Some(item);
                }
            }
            None => {
                let origin = self
                    .entities
                    .get(key)
                    .map_or(Vec3f::ZERO, |e| e.state.origin);
                // C vtos: "(%i %i %i)".
                self.gi.dprintf(&format!(
                    "{classname} at ({} {} {}) has bad item: {name}\n",
                    origin.x as i32, origin.y as i32, origin.z as i32
                ));
            }
        }
    }

    /// Chain entities that share a `team` key: the first becomes the
    /// teammaster, the others FL_TEAMSLAVE members linked by `teamchain`.
    /// C ref: `G_FindTeams` (g_spawn.c).
    fn find_teams(&mut self) {
        let keys: Vec<EntityKey> = self.entities.keys_from(1).collect();
        let (mut teams, mut members) = (0, 0);
        for (i, &e) in keys.iter().enumerate() {
            let Some(ent) = self.entities.get(e) else {
                continue;
            };
            if !ent.in_use
                || ent.game.team.is_empty()
                || ent
                    .game
                    .flags
                    .contains(crate::constants::EntityFlags::TEAMSLAVE)
            {
                continue;
            }
            let team = ent.game.team.clone();
            let mut chain = e;
            if let Some(ent) = self.entities.get_mut(e) {
                ent.game.teammaster = Some(e);
            }
            teams += 1;
            members += 1;
            for &e2 in &keys[i + 1..] {
                let Some(ent2) = self.entities.get_mut(e2) else {
                    continue;
                };
                if !ent2.in_use
                    || ent2
                        .game
                        .flags
                        .contains(crate::constants::EntityFlags::TEAMSLAVE)
                    || ent2.game.team != team
                {
                    continue;
                }
                members += 1;
                ent2.game.teammaster = Some(e);
                ent2.game.flags |= crate::constants::EntityFlags::TEAMSLAVE;
                if let Some(c) = self.entities.get_mut(chain) {
                    c.game.teamchain = Some(e2);
                }
                chain = e2;
            }
        }
        self.gi
            .dprintf(&format!("{teams} teams with {members} entities.\n"));
    }

    /// Ask the server to load `level.changemap` and resume normal frames.
    /// C ref: `ExitLevel` (g_main.c).
    fn exit_level(&mut self) {
        let command = format!("gamemap \"{}\"\n", self.level.changemap);
        self.gi.add_command_string(&command);
        self.level.changemap.clear();
        self.level.exitintermission = false;
        self.level.intermission_time = 0.0;
        self.client_end_server_frames();

        // Clear some things before going to the next level.
        for number in 1..=self.game.maxclients.max(0) as usize {
            let Some(ent) = self
                .entities
                .key_for_number(number)
                .and_then(|k| self.entities.get_mut(k))
                .filter(|e| e.in_use)
            else {
                continue;
            };
            ent.game.health = ent.game.health.min(ent.game.max_health);
        }
    }

    /// Parse an entity string and spawn all entities.
    /// C ref: `SpawnEntities` (g_spawn.c).
    pub fn spawn_entities(&mut self, mapname: &str, entstring: &str, spawnpoint: &str) {
        self.game.spawnpoint = spawnpoint.to_string();
        self.save_client_data();
        // `self.clients` is untouched: C re-points `g_edicts[i + 1].client`
        // at `game.clients + i` here, and ClientBegin does it for each new
        // body.
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

        let mut inhibit = 0;
        for fields in &parsed {
            let classname = match fields.get("classname") {
                Some(cn) => cn.clone(),
                None => continue,
            };

            // C SpawnEntities: the first entity (worldspawn) is g_edicts[0].
            let slot = if classname == "worldspawn" {
                self.entities.spawn_numbered(0)
            } else {
                self.entities.spawn()
            };
            let Some(key) = slot else {
                break; // Storage full.
            };
            let Some(ent) = self.entities.get_mut(key) else {
                continue;
            };
            crate::spawn::apply_spawn_fields(ent, fields);

            // Yet another map hack.
            if mapname.eq_ignore_ascii_case("command")
                && classname.eq_ignore_ascii_case("trigger_once")
                && ent.game.model.eq_ignore_ascii_case("*27")
            {
                ent.game.spawnflags &= !SPAWNFLAG_NOT_HARD;
            }

            // Remove things (except the world) from other skill levels or
            // deathmatch. Coop filters like single player: C has its
            // SPAWNFLAG_NOT_COOP test commented out.
            if classname != "worldspawn" {
                let skill_flag = match self.game.skill {
                    0 => SPAWNFLAG_NOT_EASY,
                    1 => SPAWNFLAG_NOT_MEDIUM,
                    _ => SPAWNFLAG_NOT_HARD,
                };
                let removed = if self.game.deathmatch {
                    SPAWNFLAG_NOT_DEATHMATCH
                } else {
                    skill_flag
                };
                if ent.game.spawnflags & removed != 0 {
                    self.free_entity(key);
                    inhibit += 1;
                    continue;
                }
                ent.game.spawnflags &= !(SPAWNFLAG_NOT_EASY
                    | SPAWNFLAG_NOT_MEDIUM
                    | SPAWNFLAG_NOT_HARD
                    | SPAWNFLAG_NOT_COOP
                    | SPAWNFLAG_NOT_DEATHMATCH);
            }

            self.call_spawn(key, &classname, fields);
        }
        self.gi.dprintf(&format!("{inhibit} entities inhibited.\n"));
        self.find_teams();
        self.trail.init();

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
#[cfg(test)]
pub fn test_world() -> GameWorld {
    test_world_recording().0
}

/// A test `GameWorld` plus a handle on its recording import.
#[cfg(test)]
pub fn test_world_recording() -> (GameWorld, Rc<MockGameImport>) {
    let gi = Rc::new(MockGameImport::default());
    (GameWorld::new(gi.clone(), 1024), gi)
}

#[cfg(test)]
impl GameWorld {
    /// Spawn a fake player in the first free client number, holding
    /// `client` in that number's `game.clients` slot. Panics when every
    /// client number is taken: C has no client record for other edicts.
    /// The player stands in a lit room (`light_level` 128), so monsters'
    /// dark check (`FindTarget`'s `light_level <= 5`) does not hide it.
    pub(crate) fn spawn_test_client(&mut self, client: ClientData) -> EntityKey {
        let number = (1..=self.clients.len())
            .find(|&n| {
                self.entities
                    .key_for_number(n)
                    .is_none_or(|k| !self.entities.is_live(k))
            })
            .expect("a free client number");
        let key = self.entities.spawn_numbered(number).unwrap();
        self.clients[number - 1] = client;
        let ent = self.entities.get_mut(key).unwrap();
        ent.client = Some(number - 1);
        ent.game.light_level = 128;
        key
    }
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
    fn random_stays_within_zero_and_one() {
        let mut world = test_world();
        for _ in 0..10_000 {
            let r = world.random();
            assert!((0.0..=1.0).contains(&r), "{r}");
        }
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
    fn link_entity_stores_the_areas_the_server_found_like_sv_linkedict() {
        let (mut world, gi) = test_world_recording();
        let key = world.spawn().unwrap();
        let number = world.number_of(key).unwrap();
        gi.areas.borrow_mut().insert(number, (2, 3));
        world.link_entity(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.areanum, ent.areanum2), (2, 3));

        gi.areas.borrow_mut().insert(number, (1, 0));
        world.link_entity(key);
        let ent = world.entities.get(key).unwrap();
        assert_eq!((ent.areanum, ent.areanum2), (1, 0));
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

    #[test]
    fn spawn_entities_links_monsters_with_the_box_monster_start_gives_them() {
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities(
            "base1",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"monster_soldier\" }",
            "",
        );
        let key = world.find_by_classname(None, "monster_soldier").unwrap();
        let number = world.number_of(key).unwrap();
        let last = gi
            .links
            .borrow()
            .iter()
            .rev()
            .find(|l| l.number == number)
            .cloned()
            .expect("monster linked");
        assert_eq!(last.solid, Solid::Bbox);
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
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        let ent = world.entities.get_mut(player).unwrap();
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
    fn monsters_and_misc_models_get_their_c_model_index_at_spawn() {
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities(
            "base1",
            concat!(
                "{ \"classname\" \"worldspawn\" }\n",
                "{ \"classname\" \"monster_soldier\" }\n",
                "{ \"classname\" \"misc_explobox\" }\n",
                "{ \"classname\" \"info_notnull\" }",
            ),
            "",
        );
        let model_of = |classname: &str| {
            let key = world.find_by_classname(None, classname).unwrap();
            let index = world.entities.get(key).unwrap().state.modelindex;
            usize::try_from(index - 1)
                .ok()
                .map(|i| gi.models.borrow()[i].clone())
        };
        assert_eq!(
            model_of("monster_soldier").as_deref(),
            Some("models/monsters/soldier/tris.md2")
        );
        assert_eq!(
            model_of("misc_explobox").as_deref(),
            Some("models/objects/barrels/tris.md2")
        );
        assert_eq!(
            model_of("info_notnull"),
            None,
            "point entities draw nothing"
        );
    }

    fn configstring(gi: &MockGameImport, index: usize) -> Option<String> {
        gi.configstrings
            .borrow()
            .iter()
            .rev()
            .find(|(i, _)| *i == index as i32)
            .map(|(_, v)| v.clone())
    }

    #[test]
    fn worldspawn_writes_the_statusbar_sky_name_and_item_names() {
        use q2_shared::constants::{CS_ITEMS, CS_NAME, CS_SKY, CS_STATUSBAR};
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities(
            "base1",
            "{ \"classname\" \"worldspawn\" \"sky\" \"unit3_\" \"message\" \"Outer Base\" }",
            "",
        );
        let bar = configstring(&gi, CS_STATUSBAR).expect("statusbar");
        assert!(bar.starts_with("yb\t-24 ") && bar.contains("hnum"), "{bar}");
        assert_eq!(configstring(&gi, CS_SKY).as_deref(), Some("unit3_"));
        assert_eq!(configstring(&gi, CS_NAME).as_deref(), Some("Outer Base"));
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        assert_eq!(
            configstring(&gi, CS_ITEMS + shotgun).as_deref(),
            Some("Shotgun")
        );
    }

    fn world_in_mode(
        deathmatch: bool,
        coop: bool,
        entstring: &str,
    ) -> (GameWorld, Rc<MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.set_max_clients(2);
        world.game.deathmatch = deathmatch;
        world.game.coop = coop;
        world.spawn_entities("base1", entstring, "");
        (world, gi)
    }

    #[test]
    fn worldspawn_precaches_the_blaster_after_the_lava_fry_sound() {
        let (_, gi) = world_in_mode(false, false, "{ \"classname\" \"worldspawn\" }");
        let sounds = gi.sounds.borrow();
        let fry = sounds.iter().position(|s| s == "player/fry.wav").unwrap();
        assert_eq!(
            sounds[fry + 1],
            "misc/w_pkup.wav",
            "the blaster's pickup sound"
        );
        assert!(sounds.iter().any(|s| s == "weapons/blastf1a.wav"));
        assert!(gi
            .models
            .borrow()
            .iter()
            .any(|m| m == "models/weapons/v_blast/tris.md2"));
    }

    #[test]
    fn deathmatch_worldspawn_sends_the_deathmatch_status_bar() {
        use q2_shared::constants::CS_STATUSBAR;
        let (_, gi) = world_in_mode(true, false, "{ \"classname\" \"worldspawn\" }");
        let bar = configstring(&gi, CS_STATUSBAR).unwrap();
        assert_eq!(bar, DM_STATUSBAR);
        assert!(bar.contains("num 3 14 ") && bar.contains("\"SPECTATOR MODE\""));
        let (_, gi) = world_in_mode(false, true, "{ \"classname\" \"worldspawn\" }");
        assert_eq!(configstring(&gi, CS_STATUSBAR).unwrap(), SINGLE_STATUSBAR);
    }

    #[test]
    fn worldspawn_precaches_player_sounds_and_gib_models_like_c() {
        let (_, gi) = world_in_mode(false, false, "{ \"classname\" \"worldspawn\" }");
        let sounds = gi.sounds.borrow();
        for sound in [
            "player/fry.wav",
            "player/lava1.wav",
            "misc/udeath.wav",
            "*death1.wav",
            "*pain100_2.wav",
            "player/u_breath2.wav",
            "weapons/noammo.wav",
            "infantry/inflies1.wav",
        ] {
            assert!(sounds.iter().any(|s| s == sound), "{sound}");
        }
        let models = gi.models.borrow();
        for model in [
            "models/objects/gibs/sm_meat/tris.md2",
            "models/objects/gibs/arm/tris.md2",
            "models/objects/gibs/head2/tris.md2",
        ] {
            assert!(models.iter().any(|m| m == model), "{model}");
        }
        // The sexed view weapons load only in coop and deathmatch.
        assert!(!models.iter().any(|m| m.starts_with('#')));
        drop((sounds, models));
        for (deathmatch, coop) in [(true, false), (false, true)] {
            let (_, gi) = world_in_mode(deathmatch, coop, "{ \"classname\" \"worldspawn\" }");
            let models = gi.models.borrow();
            let sexed: Vec<_> = models.iter().filter(|m| m.starts_with('#')).collect();
            assert_eq!(sexed.len(), 11);
            assert_eq!(
                (sexed[0].as_str(), sexed[10].as_str()),
                ("#w_blaster.md2", "#w_bfg.md2")
            );
        }
    }

    #[test]
    fn coop_and_deathmatch_reserve_eight_body_queue_edicts_after_the_clients() {
        // C InitBodyQue, called by SP_worldspawn before any other entity.
        let map = "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"info_notnull\" }";
        for (deathmatch, coop) in [(true, false), (false, true)] {
            let (world, _) = world_in_mode(deathmatch, coop, map);
            for number in 3..=10 {
                let key = world.entities.key_for_number(number).unwrap();
                assert_eq!(world.entities.get(key).unwrap().game.classname, "bodyque");
            }
            let key = world.entities.key_for_number(11).unwrap();
            assert_eq!(
                world.entities.get(key).unwrap().game.classname,
                "info_notnull"
            );
            assert_eq!(world.level.body_que, 0);
        }
        let (world, _) = world_in_mode(false, false, map);
        assert!(world.find_by_classname(None, "bodyque").is_none());
    }

    #[test]
    fn body_queue_edicts_are_never_freed_in_coop_or_deathmatch() {
        // C G_FreeEdict returns for numbers up to maxclients + BODY_QUEUE_SIZE.
        let (mut world, _) = world_in_mode(true, false, "{ \"classname\" \"worldspawn\" }");
        let last = world.entities.key_for_number(10).unwrap();
        world.free_entity(last);
        assert!(world.entities.get(last).unwrap().in_use);
        let other = world.spawn().unwrap();
        world.free_entity(other);
        assert!(!world.entities.get(other).is_some_and(|e| e.in_use));
    }

    #[test]
    fn info_player_coop_stays_only_in_coop() {
        // C SP_info_player_coop frees itself when !coop.
        let map = "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"info_player_coop\" }";
        let (world, _) = world_in_mode(false, true, map);
        assert!(world.find_by_classname(None, "info_player_coop").is_some());
        for (deathmatch, coop) in [(false, false), (true, false)] {
            let (world, _) = world_in_mode(deathmatch, coop, map);
            assert!(world.find_by_classname(None, "info_player_coop").is_none());
        }
    }

    #[test]
    fn worldspawn_publishes_the_standard_light_styles() {
        use q2_shared::constants::CS_LIGHTS;
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities("base1", "{ \"classname\" \"worldspawn\" }", "");
        assert_eq!(configstring(&gi, CS_LIGHTS).as_deref(), Some("m"));
        assert_eq!(
            configstring(&gi, CS_LIGHTS + 1).as_deref(),
            Some("mmnmmommommnonmmonqnmmo")
        );
        assert_eq!(
            configstring(&gi, CS_LIGHTS + 11).as_deref(),
            Some("abcdefghijklmnopqrrqponmlkjihgfedcba")
        );
        assert_eq!(configstring(&gi, CS_LIGHTS + 63).as_deref(), Some("a"));
    }

    #[test]
    fn switchable_light_publishes_its_style_and_toggles_when_used() {
        use q2_shared::constants::CS_LIGHTS;
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities(
            "base1",
            concat!(
                "{ \"classname\" \"worldspawn\" }\n",
                "{ \"classname\" \"light\" \"targetname\" \"l\" \"style\" \"32\" \"spawnflags\" \"1\" }\n",
                "{ \"classname\" \"light\" \"style\" \"33\" }",
            ),
            "",
        );
        assert_eq!(
            configstring(&gi, CS_LIGHTS + 32).as_deref(),
            Some("a"),
            "START_OFF"
        );
        let light = world.find_by_targetname(None, "l").unwrap();
        let use_fn = world
            .entities
            .get(light)
            .unwrap()
            .use_fn
            .expect("light_use");
        use_fn(&mut world, light, light, light);
        assert_eq!(configstring(&gi, CS_LIGHTS + 32).as_deref(), Some("m"));
        let untargeted = world
            .entities
            .iter()
            .filter(|(_, e)| e.in_use && e.game.classname == "light")
            .count();
        assert_eq!(untargeted, 1, "C frees lights nothing targets");
    }

    #[test]
    fn worldspawn_without_a_sky_key_uses_unit1() {
        use q2_shared::constants::CS_SKY;
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities("base1", "{ \"classname\" \"worldspawn\" }", "");
        assert_eq!(configstring(&gi, CS_SKY).as_deref(), Some("unit1_"));
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
        let mut world = test_world();
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
        fn test_use(
            world: &mut GameWorld,
            key: EntityKey,
            _other: EntityKey,
            _activator: EntityKey,
        ) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

        let mut world = test_world();

        let activator = world.spawn().unwrap();
        world.entities.get_mut(activator).unwrap().game.target = "my_target".into();

        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.targetname = "my_target".into();
        world.entities.get_mut(target).unwrap().use_fn = Some(test_use);

        world.use_targets(activator, activator);

        assert!(world.entities.get(target).unwrap().game.count != 0);
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
        // monster_start sets walkmonster_start_go, whose first think sets
        // monster_think.
        assert!(ent.think.is_some());
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
            ent.solid = Solid::Bbox; // findradius skips SOLID_NOT
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
        fn count_use(world: &mut GameWorld, key: EntityKey, _o: EntityKey, _a: EntityKey) {
            world.entities.get_mut(key).unwrap().game.count += 1;
        }

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

        assert_eq!(world.entities.get(counter).unwrap().game.count, 1);

        // Fire again.
        world.use_targets(relay, relay);
        assert_eq!(world.entities.get(counter).unwrap().game.count, 2);
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

        let stats = world.client(player).unwrap().ps.stats;
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
    // -- G_UseTargets / G_Find / G_RunFrame parity --

    fn relay(world: &mut GameWorld, key: EntityKey, _other: EntityKey, activator: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 1;
        world.use_targets(key, activator);
    }

    fn bump(world: &mut GameWorld, key: EntityKey, _other: EntityKey, _activator: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 1;
    }

    fn free_user(world: &mut GameWorld, _key: EntityKey, other: EntityKey, _activator: EntityKey) {
        world.free_entity(other);
    }

    #[test]
    fn entity_targeting_its_own_targetname_does_not_use_itself() {
        let mut world = test_world();
        let ent = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(ent).unwrap();
            e.game.target = "a".into();
            e.game.targetname = "a".into();
            e.use_fn = Some(relay);
        }
        relay(&mut world, ent, ent, ent);
        assert_eq!(world.entities.get(ent).unwrap().game.count, 1);
    }

    #[test]
    fn use_targets_stops_when_a_target_removes_the_user() {
        let mut world = test_world();
        let user = world.spawn().unwrap();
        world.entities.get_mut(user).unwrap().game.target = "t".into();
        let first = world.spawn().unwrap();
        let second = world.spawn().unwrap();
        for (k, f) in [(first, free_user as crate::entity::UseFn), (second, bump)] {
            let e = world.entities.get_mut(k).unwrap();
            e.game.targetname = "t".into();
            e.use_fn = Some(f);
        }
        world.use_targets(user, user);
        assert_eq!(world.entities.get(second).unwrap().game.count, 0);
    }

    #[test]
    fn delayed_use_fires_the_targets_after_the_delay() {
        let mut world = test_world();
        let user = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(user).unwrap();
            e.game.target = "t".into();
            e.game.delay = 0.3;
        }
        let target = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(target).unwrap();
            e.game.targetname = "t".into();
            e.use_fn = Some(bump);
        }
        world.use_targets(user, user);
        assert_eq!(world.entities.get(target).unwrap().game.count, 0);
        for _ in 0..2 {
            world.run_frame();
        }
        assert_eq!(world.entities.get(target).unwrap().game.count, 0);
        world.run_frame();
        assert_eq!(world.entities.get(target).unwrap().game.count, 1);
        assert!(world.find_by_classname(None, "DelayedUse").is_none());
    }

    #[test]
    fn use_targets_centerprints_the_message_to_the_activator() {
        #[derive(Default)]
        struct Printer {
            inner: MockGameImport,
            printed: RefCell<Vec<(Option<usize>, String)>>,
        }
        impl GameImport for Printer {
            fn centerprintf(&self, ent: Option<usize>, msg: &str) {
                self.printed.borrow_mut().push((ent, msg.to_string()));
            }
            fn sound(&self, _: Option<usize>, _: i32, _: i32, _: f32, _: f32, _: f32) {}
            fn positioned_sound(&self, _: Vec3f, _: usize, _: i32, _: i32, _: f32, _: f32, _: f32) {
            }
            fn bprintf(&self, a: i32, b: &str) {
                self.inner.bprintf(a, b)
            }
            fn dprintf(&self, a: &str) {
                self.inner.dprintf(a)
            }
            fn cprintf(&self, a: Option<usize>, b: i32, c: &str) {
                self.inner.cprintf(a, b, c)
            }
            fn model_index(&self, a: &str) -> i32 {
                self.inner.model_index(a)
            }
            fn sound_index(&self, a: &str) -> i32 {
                self.inner.sound_index(a)
            }
            fn image_index(&self, a: &str) -> i32 {
                self.inner.image_index(a)
            }
            fn set_model(&self, a: usize, b: &str) -> ModelInfo {
                self.inner.set_model(a, b)
            }
            fn trace(
                &self,
                a: Vec3f,
                b: Vec3f,
                c: Vec3f,
                d: Vec3f,
                e: Option<usize>,
                f: i32,
            ) -> Trace {
                self.inner.trace(a, b, c, d, e, f)
            }
            fn point_contents(&self, a: Vec3f) -> i32 {
                self.inner.point_contents(a)
            }
            fn in_pvs(&self, a: Vec3f, b: Vec3f) -> bool {
                self.inner.in_pvs(a, b)
            }
            fn in_phs(&self, a: Vec3f, b: Vec3f) -> bool {
                self.inner.in_phs(a, b)
            }
            fn link_entity(&self, a: &EntityLink) {
                self.inner.link_entity(a)
            }
            fn unlink_entity(&self, a: usize) {
                self.inner.unlink_entity(a)
            }
            fn box_edicts(&self, a: Vec3f, b: Vec3f, c: usize, d: i32) -> Vec<usize> {
                self.inner.box_edicts(a, b, c, d)
            }
            fn configstring(&self, a: i32, b: &str) {
                self.inner.configstring(a, b)
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
        let gi = Rc::new(Printer::default());
        let mut world = GameWorld::new(gi.clone(), 64);
        let player = world.entities.spawn_numbered(1).unwrap();
        let user = world.spawn().unwrap();
        world.entities.get_mut(user).unwrap().game.message = "You need the red key".into();
        world.use_targets(user, player);
        assert_eq!(
            *gi.printed.borrow(),
            [(Some(1), "You need the red key".to_string())]
        );
    }

    #[test]
    fn find_by_targetname_ignores_case_and_never_matches_empty_names() {
        let mut world = test_world();
        let blank = world.spawn().unwrap();
        let named = world.spawn().unwrap();
        world.entities.get_mut(named).unwrap().game.targetname = "Door1".into();
        assert_eq!(world.find_by_targetname(None, "door1"), Some(named));
        assert_eq!(world.find_by_targetname(None, ""), None);
        let _ = blank;
    }

    #[test]
    fn find_walks_entities_in_number_order_after_slot_reuse() {
        let mut world = test_world();
        let a = world.spawn().unwrap();
        let b = world.spawn().unwrap();
        world.free_entity(a);
        world.level.time = 10.0;
        world.entities.set_level_time(10.0);
        let c = world.spawn().unwrap(); // reuses a's number, a later slot
        assert!(world.number_of(c) < world.number_of(b));
        for k in [b, c] {
            world.entities.get_mut(k).unwrap().game.classname = "x".into();
        }
        assert_eq!(world.find_by_classname(None, "x"), Some(c));
        assert_eq!(world.find_by_classname(Some(c), "x"), Some(b));
    }

    #[test]
    fn pick_target_varies_between_calls_in_one_frame() {
        let mut world = test_world();
        for _ in 0..8 {
            let k = world.spawn().unwrap();
            world.entities.get_mut(k).unwrap().game.targetname = "t".into();
        }
        let picks: std::collections::HashSet<_> =
            (0..32).filter_map(|_| world.pick_target("t")).collect();
        assert!(picks.len() > 1);
    }

    #[test]
    fn set_movedir_treats_only_the_exact_editor_vectors_as_up_and_down() {
        let tilted = GameWorld::set_movedir(Vec3f::new(30.0, -1.0, 0.0));
        assert!(tilted.z < 0.0, "pitch 30 points down: {tilted:?}");
    }

    #[test]
    fn entity_spawned_during_the_frame_runs_in_the_same_frame() {
        fn spawn_child(world: &mut GameWorld, _key: EntityKey) {
            let child = world.spawn().unwrap();
            let e = world.entities.get_mut(child).unwrap();
            e.game.classname = "child".into();
            e.game.movetype = crate::constants::MoveType::Noclip;
            e.velocity = Vec3f::new(100.0, 0.0, 0.0);
        }
        let mut world = test_world();
        let parent = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(parent).unwrap();
            e.think = Some(spawn_child);
            e.game.nextthink = 0.1;
        }
        world.run_frame();
        let child = world.find_by_classname(None, "child").unwrap();
        assert_eq!(world.entities.get(child).unwrap().state.origin.x, 10.0);
    }

    #[test]
    fn monster_whose_ground_entity_moved_rechecks_the_ground() {
        let mut world = test_world();
        let plat = world.spawn().unwrap();
        let monster = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(monster).unwrap();
            e.svflags |= q2_shared::constants::SVF_MONSTER;
            e.game.movetype = crate::constants::MoveType::Step;
            e.game.ground_entity = Some(plat);
            e.game.groundentity_linkcount = 0;
        }
        world.link_entity(plat); // linkcount 1: the plat moved.
        world.run_frame();
        // The mock world has no floor, so the monster starts falling.
        let e = world.entities.get(monster).unwrap();
        assert!(e.game.ground_entity.is_none());
        assert!(e.velocity.z < 0.0);
    }
    #[test]
    fn entities_flagged_not_for_the_current_skill_are_removed() {
        let mut world = test_world();
        world.init_game();
        world.game.skill = 1;
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" }
               { "classname" "info_notnull" "targetname" "medium_off" "spawnflags" "512" }
               { "classname" "info_notnull" "targetname" "easy_off" "spawnflags" "257" }"#,
            "",
        );
        assert!(world.find_by_targetname(None, "medium_off").is_none());
        let kept = world.find_by_targetname(None, "easy_off").unwrap();
        // The skill/deathmatch bits are cleared on what stays.
        assert_eq!(world.entities.get(kept).unwrap().game.spawnflags, 1);
    }

    /// Spawn flags 0x800 (not in deathmatch), 0x1000 (not in coop) and
    /// 0x400 (not on hard), each on its own info_notnull.
    const MODE_FLAGGED: &str = r#"{ "classname" "worldspawn" }
        { "classname" "info_notnull" "targetname" "not_dm" "spawnflags" "2048" }
        { "classname" "info_notnull" "targetname" "not_coop" "spawnflags" "4096" }
        { "classname" "info_notnull" "targetname" "not_hard" "spawnflags" "1024" }"#;

    #[test]
    fn deathmatch_removes_not_deathmatch_entities_and_ignores_skill_flags() {
        let mut world = test_world();
        world.init_game();
        world.game.deathmatch = true;
        world.game.skill = 2;
        world.spawn_entities("q2dm1", MODE_FLAGGED, "");
        assert!(world.find_by_targetname(None, "not_dm").is_none());
        let kept = world.find_by_targetname(None, "not_hard").unwrap();
        assert!(world.find_by_targetname(None, "not_coop").is_some());
        assert_eq!(world.entities.get(kept).unwrap().game.spawnflags, 0);
    }

    #[test]
    fn coop_filters_by_skill_and_keeps_not_coop_entities_like_c() {
        // C G_SpawnEntities has the SPAWNFLAG_NOT_COOP test commented out.
        let mut world = test_world();
        world.init_game();
        world.game.coop = true;
        world.game.skill = 2;
        world.spawn_entities("base1", MODE_FLAGGED, "");
        assert!(world.find_by_targetname(None, "not_hard").is_none());
        assert!(world.find_by_targetname(None, "not_coop").is_some());
        assert!(world.find_by_targetname(None, "not_dm").is_some());
    }

    #[test]
    fn unknown_classname_is_reported() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "monster_nonexistent" }"#,
            "",
        );
        let ent = world.find_by_classname(None, "monster_nonexistent");
        assert!(ent.is_some(), "C keeps the edict, without a spawn function");
    }

    #[test]
    fn info_null_is_soft_freed_and_its_number_reused_at_level_start() {
        let mut world = test_world();
        world.init_game();
        world.spawn_entities(
            "base1",
            r#"{ "classname" "worldspawn" } { "classname" "info_null" }
               { "classname" "info_notnull" "targetname" "after" }"#,
            "",
        );
        let after = world.find_by_targetname(None, "after").unwrap();
        // C G_Spawn reuses edicts freed before level time 2.
        let first = world.game.maxclients as usize + 1;
        assert_eq!(world.number_of(after), Some(first));
    }

    #[test]
    fn exit_level_clamps_client_health_to_max_health_before_the_map_change() {
        let mut world = test_world();
        world.set_max_clients(2);
        let over = world.entities.spawn_numbered(1).unwrap();
        let under = world.entities.spawn_numbered(2).unwrap();
        for (key, health) in [(over, 250), (under, 40)] {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.health = health;
            ent.game.max_health = 100;
        }
        world.level.changemap = "base2".to_string();
        world.level.exitintermission = true;
        world.run_frame();
        assert_eq!(world.entities.get(over).unwrap().game.health, 100);
        assert_eq!(world.entities.get(under).unwrap().game.health, 40);
    }

    fn spawn_soldier_with_item(item: &str) -> (GameWorld, EntityKey) {
        spawn_soldier_with_item_recording(item).0
    }

    fn spawn_soldier_with_item_recording(
        item: &str,
    ) -> ((GameWorld, EntityKey), Rc<MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.spawn_entities(
            "t",
            &format!(
                "{{ \"classname\" \"worldspawn\" }}\n\
                 {{ \"classname\" \"monster_soldier\" \"item\" \"{item}\" }}"
            ),
            "",
        );
        let key = world.find_by_classname(None, "monster_soldier").unwrap();
        ((world, key), gi)
    }

    #[test]
    fn monster_spawned_with_an_item_key_carries_that_item() {
        // C monster_start: self->item = FindItemByClassname(st.item).
        let (world, key) = spawn_soldier_with_item("item_armor_shard");
        let shard = world.find_item("item_armor_shard");
        assert!(shard.is_some());
        assert_eq!(world.entities.get(key).unwrap().game.item, shard);
    }

    #[test]
    fn monster_with_a_bad_item_key_spawns_without_an_item() {
        // C monster_start prints "has bad item" and leaves self->item NULL.
        let ((world, key), gi) = spawn_soldier_with_item_recording("item_no_such_thing");
        assert_eq!(world.entities.get(key).unwrap().game.item, None);
        assert!(gi
            .dprints
            .borrow()
            .iter()
            .any(|m| m == "monster_soldier at (0 0 0) has bad item: item_no_such_thing\n"));
    }

    #[test]
    fn deathmatch_monster_is_freed_without_reading_its_item_or_linking() {
        // C monster_start frees the edict in deathmatch and returns before
        // st.item and gi.linkentity.
        let (mut world, gi) = test_world_recording();
        world.init_game();
        world.game.deathmatch = true;
        let key = world.spawn().unwrap();
        let fields = HashMap::from([("item".to_string(), "item_no_such_thing".to_string())]);
        gi.links.borrow_mut().clear();
        world.call_spawn(key, "monster_soldier", &fields);
        assert!(!world.entities.get(key).is_some_and(|e| e.in_use));
        assert!(gi.dprints.borrow().iter().all(|m| !m.contains("bad item")));
        assert!(gi.links.borrow().is_empty());
    }

    fn radius_target(world: &mut GameWorld, origin: Vec3f, solid: Solid) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin = origin;
        ent.mins = Vec3f::new(-16.0, -16.0, 0.0);
        ent.maxs = Vec3f::new(16.0, 16.0, 64.0);
        ent.solid = solid;
        key
    }

    #[test]
    fn find_radius_measures_to_the_box_center_and_walks_in_number_order() {
        // C findradius: distance from org to origin + (mins + maxs) / 2.
        let mut world = test_world();
        let near = radius_target(&mut world, Vec3f::new(100.0, 0.0, -32.0), Solid::Bbox);
        let far = radius_target(&mut world, Vec3f::new(101.0, 0.0, -32.0), Solid::Bbox);
        let next = radius_target(&mut world, Vec3f::new(0.0, 50.0, -32.0), Solid::Trigger);
        assert_eq!(world.find_radius(None, Vec3f::ZERO, 100.0), Some(near));
        assert_eq!(
            world.find_radius(Some(near), Vec3f::ZERO, 100.0),
            Some(next)
        );
        assert_eq!(world.find_radius(Some(next), Vec3f::ZERO, 100.0), None);
        assert_eq!(world.find_radius(Some(near), Vec3f::ZERO, 101.0), Some(far));
    }

    #[test]
    fn find_radius_skips_free_and_non_solid_entities() {
        let mut world = test_world();
        radius_target(&mut world, Vec3f::ZERO, Solid::Not);
        let freed = radius_target(&mut world, Vec3f::ZERO, Solid::Bbox);
        world.free_entity(freed);
        assert_eq!(world.find_radius(None, Vec3f::ZERO, 1000.0), None);
    }
}
