//! Trigger entities — activate effects when touched or targeted.
//!
//! Port of `g_trigger.c`. Not ported: the coop branch of `trigger_key`.
//! `trigger_elevator` lives in `func.rs`, beside the train, as in `g_func.c`.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_trigger.c`

use q2_shared::constants::{SVF_DEADMONSTER, SVF_MONSTER, SVF_NOCLIENT};
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;

/// trigger_push spawnflag: remove the trigger after one push.
const PUSH_ONCE: i32 = 1;

// trigger_multiple / trigger_once spawnflags.
const MULTI_MONSTER: i32 = 1;
const MULTI_NOT_PLAYER: i32 = 2;
const MULTI_TRIGGERED: i32 = 4;

// trigger_hurt spawnflags.
const HURT_START_OFF: i32 = 1;
const HURT_TOGGLE: i32 = 2;
const HURT_SILENT: i32 = 4;
const HURT_NO_PROTECTION: i32 = 8;
const HURT_SLOW: i32 = 16;

/// trigger_counter spawnflag: no "n more to go" messages.
const COUNTER_NOMESSAGE: i32 = 1;

/// C `AngleVectors` forward vector.
fn angles_forward(angles: Vec3f) -> Vec3f {
    let (sp, cp) = angles.x.to_radians().sin_cos();
    let (sy, cy) = angles.y.to_radians().sin_cos();
    Vec3f::new(cp * cy, cp * sy, -sp)
}

// ---------------------------------------------------------------------------
// trigger_multiple / trigger_once
// ---------------------------------------------------------------------------

/// The wait is over: the trigger can fire again. C ref: `multi_wait`.
fn multi_wait(world: &mut GameWorld, key: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.nextthink = 0.0;
    }
}

/// Remove the entity on its next think (C `think = G_FreeEdict`).
fn free_think(world: &mut GameWorld, key: EntityKey) {
    world.free_entity(key);
}

/// Fire the targets unless still waiting; then wait, or remove the trigger
/// next frame when `wait` is not positive (trigger_once).
/// C ref: `multi_trigger`.
fn multi_trigger(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    if ent.game.nextthink != 0.0 {
        return; // Already been triggered.
    }
    let activator = ent.game.activator.unwrap_or(key);
    world.use_targets(key, activator);

    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(key).filter(|e| e.in_use) else {
        return;
    };
    if ent.game.wait > 0.0 {
        ent.think = Some(multi_wait);
        ent.game.nextthink = time + ent.game.wait;
    } else {
        // Can't remove self here: this is a touch function called while
        // looping through area links.
        ent.touch = None;
        ent.game.nextthink = time + FRAMETIME;
        ent.think = Some(free_think);
    }
}

/// C ref: `Use_Multi`.
fn use_multi(world: &mut GameWorld, key: EntityKey, _other: EntityKey, activator: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.activator = Some(activator);
    }
    multi_trigger(world, key);
}

/// Players (unless flagged off) and flagged monsters fire the trigger,
/// only when facing along `movedir` if one is set.
/// C ref: `Touch_Multi`.
fn touch_multi(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let (Some(ent), Some(other)) = (world.entities.get(self_key), world.entities.get(other_key))
    else {
        return;
    };
    let flags = ent.game.spawnflags;
    if other.client.is_some() {
        if flags & MULTI_NOT_PLAYER != 0 {
            return;
        }
    } else if other.svflags & SVF_MONSTER != 0 {
        if flags & MULTI_MONSTER == 0 {
            return;
        }
    } else {
        return;
    }
    if ent.game.movedir != Vec3f::ZERO
        && angles_forward(other.state.angles).dot(ent.game.movedir) < 0.0
    {
        return;
    }
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.activator = Some(other_key);
    }
    multi_trigger(world, self_key);
}

/// A TRIGGERED trigger_multiple becomes touchable when used.
/// C ref: `trigger_enable`.
fn trigger_enable(world: &mut GameWorld, key: EntityKey, _other: EntityKey, _activator: EntityKey) {
    if let Some(ent) = world.entities.get_mut(key) {
        ent.solid = Solid::Trigger;
        ent.use_fn = Some(use_multi);
    }
    world.link_entity(key);
}

/// trigger_relay / generic relay use. C ref: `trigger_relay_use`.
fn trigger_relay_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.use_targets(self_key, activator);
}

// ---------------------------------------------------------------------------
// trigger_key
// ---------------------------------------------------------------------------

/// Fire the targets once the activator has the key, consuming it.
/// C ref: `trigger_key_use` (single player).
fn trigger_key_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let Some(index) = world.entities.get(self_key).and_then(|e| e.game.item) else {
        return;
    };
    let Some(number) = world
        .entities
        .get(activator)
        .filter(|a| a.client.is_some())
        .and_then(|a| usize::try_from(a.state.number).ok())
    else {
        return;
    };
    let has_key = world
        .client(activator)
        .is_some_and(|c| c.pers.inventory[index] != 0);

    if !has_key {
        let time = world.level.time;
        let Some(ent) = world.entities.get_mut(self_key) else {
            return;
        };
        if time < ent.game.touch_debounce_time {
            return;
        }
        ent.game.touch_debounce_time = time + 5.0;
        let name = world.items.get(index).map_or("", |i| i.pickup_name);
        world
            .gi
            .centerprintf(Some(number), &format!("You need the {name}"));
        let sound = world.gi.sound_index("misc/keytry.wav");
        world
            .gi
            .sound(Some(number), CHAN_AUTO, sound, 1.0, ATTN_NORM, 0.0);
        return;
    }

    let sound = world.gi.sound_index("misc/keyuse.wav");
    world
        .gi
        .sound(Some(number), CHAN_AUTO, sound, 1.0, ATTN_NORM, 0.0);
    if let Some(client) = world.client_mut(activator) {
        client.pers.inventory[index] -= 1;
    }
    world.use_targets(self_key, activator);
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.use_fn = None;
    }
}

// ---------------------------------------------------------------------------
// trigger_counter / trigger_push / trigger_hurt / trigger_gravity /
// trigger_monsterjump
// ---------------------------------------------------------------------------

/// Count uses down to zero, then fire like a trigger_once.
/// C ref: `trigger_counter_use`.
fn trigger_counter_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    if ent.game.count == 0 {
        return;
    }
    ent.game.count -= 1;
    let (count, quiet) = (ent.game.count, ent.game.spawnflags & COUNTER_NOMESSAGE != 0);
    let number = world
        .entities
        .get(activator)
        .and_then(|a| usize::try_from(a.state.number).ok());

    if !quiet {
        let message = if count != 0 {
            format!("{count} more to go...")
        } else {
            "Sequence completed!".to_string()
        };
        world.gi.centerprintf(number, &message);
        let sound = world.gi.sound_index("misc/talk1.wav");
        world
            .gi
            .sound(number, CHAN_AUTO, sound, 1.0, ATTN_NORM, 0.0);
    }
    if count != 0 {
        return;
    }
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.activator = Some(activator);
    }
    multi_trigger(world, self_key);
}

/// Launch grenades and living entities along `movedir` at `speed * 10`.
/// C ref: `trigger_push_touch`.
fn trigger_push_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let Some(ent) = world.entities.get(self_key) else {
        return;
    };
    let (push, once) = (
        ent.game.movedir * (ent.game.speed * 10.0),
        ent.game.spawnflags & PUSH_ONCE != 0,
    );
    let time = world.level.time;
    let mut wind = None;
    if let Some(other) = world.entities.get_mut(other_key) {
        if other.game.classname == "grenade" {
            other.velocity = push;
        } else if other.game.health > 0 {
            other.velocity = push;
            // C also copies the velocity to client->oldvelocity so the
            // push causes no falling damage; this port has no oldvelocity.
            if other.client.is_some() && other.game.fly_sound_debounce_time < time {
                other.game.fly_sound_debounce_time = time + 1.5;
                wind = usize::try_from(other.state.number).ok();
            }
        }
    }
    if let Some(number) = wind {
        let sound = world.gi.sound_index("misc/windfly.wav");
        world
            .gi
            .sound(Some(number), CHAN_AUTO, sound, 1.0, ATTN_NORM, 0.0);
    }
    if once {
        world.free_entity(self_key);
    }
}

/// Damage a touching damageable entity every frame (every second when
/// SLOW). C ref: `hurt_touch`.
fn trigger_hurt_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    if world
        .entities
        .get(other_key)
        .is_none_or(|o| o.game.takedamage == TakeDamage::No)
    {
        return;
    }
    let time = world.level.time;
    let Some(ent) = world.entities.get_mut(self_key) else {
        return;
    };
    if ent.game.timestamp > time {
        return;
    }
    let flags = ent.game.spawnflags;
    ent.game.timestamp = time
        + if flags & HURT_SLOW != 0 {
            1.0
        } else {
            FRAMETIME
        };
    let dmg = ent.game.dmg;

    if flags & HURT_SILENT == 0 && world.level.framenum % 10 == 0 {
        let sound = world.gi.sound_index("world/electro.wav");
        world.gi.sound(
            world.number_of(other_key),
            CHAN_AUTO,
            sound,
            1.0,
            ATTN_NORM,
            0.0,
        );
    }
    let dflags = if flags & HURT_NO_PROTECTION != 0 {
        DamageFlags::NO_PROTECTION
    } else {
        DamageFlags::empty()
    };
    let origin = world
        .entities
        .get(other_key)
        .map(|e| e.state.origin)
        .unwrap_or_default();
    world.t_damage(
        other_key,
        self_key,
        self_key,
        Vec3f::ZERO,
        origin,
        Vec3f::ZERO,
        dmg,
        dmg,
        dflags,
        MeansOfDeath::TriggerHurt,
    );
}

/// Toggle a START_OFF/TOGGLE trigger_hurt; turning on hurts whatever is
/// already inside. C ref: `hurt_use`.
fn trigger_hurt_use(
    world: &mut GameWorld,
    key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    if ent.solid == Solid::Not {
        ent.solid = Solid::Trigger;
        let (absmin, absmax) = (ent.absmin, ent.absmax);
        let inside = world.gi.box_edicts(
            absmin,
            absmax,
            q2_shared::constants::MAX_EDICTS,
            crate::traits::AREA_SOLID,
        );
        for number in inside {
            if let Some(victim) = world.entities.key_for_number(number) {
                trigger_hurt_touch(world, key, victim, None, None);
            }
        }
    } else {
        ent.solid = Solid::Not;
    }
    world.link_entity(key);
    if let Some(ent) = world.entities.get_mut(key) {
        if ent.game.spawnflags & HURT_TOGGLE == 0 {
            ent.use_fn = None;
        }
    }
}

/// C ref: `trigger_gravity_touch`.
fn trigger_gravity_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let Some(gravity) = world.entities.get(self_key).map(|e| e.game.gravity) else {
        return;
    };
    if let Some(ent) = world.entities.get_mut(other_key) {
        ent.game.gravity = gravity;
    }
}

/// Make walking monsters jump along `movedir` (height in `movedir.z`).
/// C ref: `trigger_monsterjump_touch`.
fn trigger_monsterjump_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let Some((movedir, speed)) = world
        .entities
        .get(self_key)
        .map(|e| (e.game.movedir, e.game.speed))
    else {
        return;
    };
    let Some(other) = world.entities.get_mut(other_key) else {
        return;
    };
    if other
        .game
        .flags
        .intersects(EntityFlags::FLY | EntityFlags::SWIM)
        || other.svflags & SVF_DEADMONSTER != 0
        || other.svflags & SVF_MONSTER == 0
    {
        return;
    }
    // Set XY even if not on ground, so the jump will clear lips.
    other.velocity.x = movedir.x * speed;
    other.velocity.y = movedir.y * speed;
    if other.game.ground_entity.is_none() {
        return;
    }
    other.game.ground_entity = None;
    other.velocity.z = movedir.z;
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// The parts of trigger spawn functions that need the item table or
    /// `GameImport` (spawn functions only see `EntityStorage`).
    pub(crate) fn finish_trigger_spawn(
        &mut self,
        key: EntityKey,
        classname: &str,
        fields: &HashMap<String, String>,
    ) {
        match classname {
            // C SP_trigger_multiple's `sounds` key.
            "trigger_multiple" | "trigger_once" => {
                let sound = match fields.get("sounds").map(|s| crate::spawn::strtol(s)) {
                    Some(1) => "misc/secret.wav",
                    Some(2) => "misc/talk.wav",
                    Some(3) => "misc/trigger1.wav",
                    _ => return,
                };
                let index = self.gi.sound_index(sound);
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.game.noise_index = index;
                }
            }
            // C SP_trigger_always: G_UseTargets with at least 0.2 s delay
            // so the targets are present.
            "trigger_always" => self.use_targets(key, key),
            // C SP_trigger_key.
            "trigger_key" => {
                let origin = self.entities.get(key).map(|e| e.state.origin);
                let Some(item_name) = fields.get("item") else {
                    self.gi
                        .dprintf(&format!("no key item for trigger_key at {origin:?}\n"));
                    return;
                };
                let Some(item) = self.find_item(item_name) else {
                    self.gi.dprintf(&format!(
                        "item {item_name} not found for trigger_key at {origin:?}\n"
                    ));
                    return;
                };
                let Some(ent) = self.entities.get_mut(key) else {
                    return;
                };
                ent.game.item = Some(item);
                if ent.game.target.is_empty() {
                    self.gi
                        .dprintf(&format!("trigger_key at {origin:?} has no target\n"));
                    return;
                }
                ent.use_fn = Some(trigger_key_use);
                self.gi.sound_index("misc/keytry.wav");
                self.gi.sound_index("misc/keyuse.wav");
            }
            _ => {}
        }
    }
}

// All spawn functions follow the SpawnFn signature from spawn.rs.
use crate::entity::{Entity, EntityStorage};
use std::collections::HashMap;

/// C ref: `InitTrigger`: an invisible, non-moving touch volume (its model
/// and bounds come from spawn_entities' set_model).
fn init_trigger(ent: &mut Entity) {
    if ent.state.angles != Vec3f::ZERO {
        ent.game.movedir = GameWorld::set_movedir(ent.state.angles);
        ent.state.angles = Vec3f::ZERO;
    }
    ent.solid = Solid::Trigger;
    ent.game.movetype = MoveType::None;
    ent.svflags = SVF_NOCLIENT;
}

/// C ref: `SP_trigger_multiple`.
pub fn sp_trigger_multiple(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    if ent.game.wait == 0.0 {
        ent.game.wait = 0.2;
    }
    ent.touch = Some(touch_multi);
    ent.game.movetype = MoveType::None;
    ent.svflags |= SVF_NOCLIENT;
    if ent.game.spawnflags & MULTI_TRIGGERED != 0 {
        ent.solid = Solid::Not;
        ent.use_fn = Some(trigger_enable);
    } else {
        ent.solid = Solid::Trigger;
        ent.use_fn = Some(use_multi);
    }
    if ent.state.angles != Vec3f::ZERO {
        ent.game.movedir = GameWorld::set_movedir(ent.state.angles);
        ent.state.angles = Vec3f::ZERO;
    }
}

/// C ref: `SP_trigger_once`: a trigger_multiple with `wait -1`.
pub fn sp_trigger_once(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    // Old maps put TRIGGERED on bit 1 instead of bit 4.
    if ent.game.spawnflags & 1 != 0 {
        ent.game.spawnflags = (ent.game.spawnflags & !1) | MULTI_TRIGGERED;
    }
    ent.game.wait = -1.0;
    sp_trigger_multiple(storage, key, fields);
}

pub fn sp_trigger_relay(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.use_fn = Some(trigger_relay_use);
    }
}

/// C ref: `SP_trigger_push`.
pub fn sp_trigger_push(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        init_trigger(ent);
        ent.touch = Some(trigger_push_touch);
        if ent.game.speed == 0.0 {
            ent.game.speed = 1000.0;
        }
    }
}

/// C ref: `SP_trigger_hurt`.
pub fn sp_trigger_hurt(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        init_trigger(ent);
        ent.touch = Some(trigger_hurt_touch);
        if ent.game.dmg == 0 {
            ent.game.dmg = 5;
        }
        ent.solid = if ent.game.spawnflags & HURT_START_OFF != 0 {
            Solid::Not
        } else {
            Solid::Trigger
        };
        if ent.game.spawnflags & HURT_TOGGLE != 0 {
            ent.use_fn = Some(trigger_hurt_use);
        }
    }
}

/// C ref: `SP_trigger_gravity`. C parses the key with `strtol`, so only
/// whole-number gravity values take effect.
pub fn sp_trigger_gravity(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let gravity = fields.get("gravity").map_or(0, |g| crate::spawn::strtol(g));
    let Some(ent) = storage.get_mut(key) else {
        return;
    };
    if fields.get("gravity").is_none_or(|g| g.is_empty()) {
        tracing::debug!(
            "trigger_gravity without gravity set at {:?}",
            ent.state.origin
        );
        storage.soft_free(key);
        return;
    }
    init_trigger(ent);
    ent.game.gravity = gravity as f32;
    ent.touch = Some(trigger_gravity_touch);
}

/// C ref: `SP_trigger_counter`.
pub fn sp_trigger_counter(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.wait = -1.0;
        if ent.game.count == 0 {
            ent.game.count = 2;
        }
        ent.use_fn = Some(trigger_counter_use);
    }
}

/// C ref: `SP_trigger_always`; `finish_trigger_spawn` fires the targets.
pub fn sp_trigger_always(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        if ent.game.delay < 0.2 {
            ent.game.delay = 0.2;
        }
    }
}

/// C ref: `SP_trigger_key`; `finish_trigger_spawn` resolves the item.
pub fn sp_trigger_key(
    _storage: &mut EntityStorage,
    _key: EntityKey,
    _fields: &HashMap<String, String>,
) {
}

/// C ref: `SP_trigger_monsterjump`.
pub fn sp_trigger_monsterjump(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    let height = fields.get("height").map_or(0, |h| crate::spawn::strtol(h));
    if let Some(ent) = storage.get_mut(key) {
        if ent.game.speed == 0.0 {
            ent.game.speed = 200.0;
        }
        if ent.state.angles.y == 0.0 {
            ent.state.angles.y = 360.0;
        }
        init_trigger(ent);
        ent.touch = Some(trigger_monsterjump_touch);
        ent.game.movedir.z = if height == 0 { 200.0 } else { height as f32 };
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod init_trigger_tests {
    use crate::world::test_world_recording;
    use q2_shared::constants::SVF_NOCLIENT;
    use q2_shared::types::{Solid, Vec3f};

    #[test]
    fn trigger_brushes_are_never_sent_to_clients() {
        let (mut world, gi) = test_world_recording();
        gi.inline_bounds
            .borrow_mut()
            .insert("*1".into(), (Vec3f::ZERO, Vec3f::splat(32.0)));
        world.init_game();
        world.spawn_entities(
            "t",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"trigger_multiple\" \"model\" \"*1\" \"angle\" \"90\" }",
            "",
        );
        let key = world.find_by_classname(None, "trigger_multiple").unwrap();
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.svflags & SVF_NOCLIENT, SVF_NOCLIENT);
        assert_eq!(ent.solid, Solid::Trigger);
        assert_eq!(ent.state.angles, Vec3f::ZERO);
        assert!((ent.game.movedir.y - 1.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ClientData;
    use crate::world::{test_world, test_world_recording};

    fn spawn_player(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn_test_client(ClientData::default());
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.health = 100;
        key
    }

    fn bump(world: &mut GameWorld, key: EntityKey, _o: EntityKey, _a: EntityKey) {
        world.entities.get_mut(key).unwrap().game.count += 1;
    }

    /// A target named `t` that counts its uses.
    fn spawn_counter_target(world: &mut GameWorld) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.targetname = "t".into();
        ent.use_fn = Some(bump);
        key
    }

    fn uses(world: &GameWorld, key: EntityKey) -> i32 {
        world.entities.get(key).unwrap().game.count
    }

    fn spawn_map(world: &mut GameWorld, ents: &str) {
        world.init_game();
        world.spawn_entities(
            "t",
            &format!("{{ \"classname\" \"worldspawn\" }}\n{ents}"),
            "",
        );
    }

    #[test]
    fn register_trigger_spawns() {
        let world = test_world();
        assert!(world.spawn_table.get("trigger_multiple").is_some());
        assert!(world.spawn_table.get("trigger_once").is_some());
        assert!(world.spawn_table.get("trigger_relay").is_some());
        assert!(world.spawn_table.get("trigger_push").is_some());
        assert!(world.spawn_table.get("trigger_hurt").is_some());
        assert!(world.spawn_table.get("trigger_gravity").is_some());
        assert!(world.spawn_table.get("trigger_counter").is_some());
        assert!(world.spawn_table.get("trigger_always").is_some());
        assert!(world.spawn_table.get("trigger_elevator").is_some());
    }

    #[test]
    fn trigger_multiple_fires_once_per_wait() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_multiple" "target" "t" "wait" "2" }"#,
        );
        let trigger = world.find_by_classname(None, "trigger_multiple").unwrap();
        let player = spawn_player(&mut world);
        let target = spawn_counter_target(&mut world);

        touch_multi(&mut world, trigger, player, None, None);
        assert_eq!(uses(&world, target), 1);
        touch_multi(&mut world, trigger, player, None, None);
        assert_eq!(uses(&world, target), 1, "still waiting");
        for _ in 0..20 {
            world.run_frame();
        }
        touch_multi(&mut world, trigger, player, None, None);
        assert_eq!(uses(&world, target), 2);
    }

    #[test]
    fn trigger_multiple_ignores_non_clients_and_unflagged_monsters() {
        let mut world = test_world();
        let target = spawn_counter_target(&mut world);
        let trigger = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(trigger).unwrap();
            e.game.target = "t".into();
            e.game.wait = 0.2;
        }
        let item = world.spawn().unwrap();
        let monster = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().svflags |= SVF_MONSTER;
        touch_multi(&mut world, trigger, item, None, None);
        touch_multi(&mut world, trigger, monster, None, None);
        assert_eq!(uses(&world, target), 0);

        world.entities.get_mut(trigger).unwrap().game.spawnflags = MULTI_MONSTER;
        touch_multi(&mut world, trigger, monster, None, None);
        assert_eq!(uses(&world, target), 1);
    }

    #[test]
    fn trigger_multiple_with_wait_minus_one_fires_once_then_removes_itself() {
        let mut world = test_world();
        let target = spawn_counter_target(&mut world);
        let trigger = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(trigger).unwrap();
            e.game.target = "t".into();
            e.game.wait = -1.0;
        }
        let player = spawn_player(&mut world);
        touch_multi(&mut world, trigger, player, None, None);
        touch_multi(&mut world, trigger, player, None, None);
        world.run_frame();
        assert_eq!(uses(&world, target), 1);
        assert!(!world.entities.is_live(trigger));
    }

    #[test]
    fn trigger_facing_movedir_only_fires_for_players_facing_along_it() {
        let mut world = test_world();
        let target = spawn_counter_target(&mut world);
        let trigger = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(trigger).unwrap();
            e.game.target = "t".into();
            e.game.wait = 0.2;
            e.game.movedir = Vec3f::new(1.0, 0.0, 0.0);
        }
        let player = spawn_player(&mut world);
        world.entities.get_mut(player).unwrap().state.angles.y = 180.0;
        touch_multi(&mut world, trigger, player, None, None);
        assert_eq!(uses(&world, target), 0);
    }

    #[test]
    fn triggered_trigger_once_waits_for_its_use() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_once" "spawnflags" "4" }"#,
        );
        let trigger = world.find_by_classname(None, "trigger_once").unwrap();
        assert_eq!(world.entities.get(trigger).unwrap().solid, Solid::Not);
        let use_fn = world.entities.get(trigger).unwrap().use_fn.unwrap();
        use_fn(&mut world, trigger, trigger, trigger);
        assert_eq!(world.entities.get(trigger).unwrap().solid, Solid::Trigger);
    }

    #[test]
    fn trigger_push_launches_living_things_at_ten_times_speed() {
        let mut world = test_world();
        let trigger = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(trigger).unwrap();
            ent.game.speed = 80.0;
            ent.game.movedir = Vec3f::new(0.0, 0.0, 1.0); // push up
        }
        let player = spawn_player(&mut world);
        let corpse = world.spawn().unwrap();

        trigger_push_touch(&mut world, trigger, player, None, None);
        trigger_push_touch(&mut world, trigger, corpse, None, None);

        // C: VectorScale(movedir, speed * 10, velocity).
        assert_eq!(world.entities.get(player).unwrap().velocity.z, 800.0);
        assert_eq!(world.entities.get(corpse).unwrap().velocity, Vec3f::ZERO);
    }

    #[test]
    fn push_once_trigger_removes_itself() {
        let mut world = test_world();
        let trigger = world.spawn().unwrap();
        world.entities.get_mut(trigger).unwrap().game.spawnflags = PUSH_ONCE;
        let player = spawn_player(&mut world);
        trigger_push_touch(&mut world, trigger, player, None, None);
        assert!(!world.entities.is_live(trigger));
    }

    #[test]
    fn trigger_hurt_damages_every_frame_and_respects_armor_by_default() {
        let mut world = test_world();
        world.level.time = 1.0;
        let trigger = world.spawn().unwrap();
        world.entities.get_mut(trigger).unwrap().game.dmg = 10;

        let victim = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(victim).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
        }

        trigger_hurt_touch(&mut world, trigger, victim, None, None);
        trigger_hurt_touch(&mut world, trigger, victim, None, None);
        assert_eq!(world.entities.get(victim).unwrap().game.health, 90);
        world.level.time += FRAMETIME;
        trigger_hurt_touch(&mut world, trigger, victim, None, None);
        assert_eq!(world.entities.get(victim).unwrap().game.health, 80);
        assert_eq!(
            world.entities.get(trigger).unwrap().game.spawnflags & HURT_NO_PROTECTION,
            0
        );
    }

    #[test]
    fn slow_trigger_hurt_damages_once_a_second() {
        let mut world = test_world();
        let trigger = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(trigger).unwrap();
            e.game.dmg = 10;
            e.game.spawnflags = HURT_SLOW;
        }
        let victim = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(victim).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
        }
        trigger_hurt_touch(&mut world, trigger, victim, None, None);
        world.level.time += 0.5;
        trigger_hurt_touch(&mut world, trigger, victim, None, None);
        assert_eq!(world.entities.get(victim).unwrap().game.health, 90);
    }

    #[test]
    fn start_off_trigger_hurt_is_switched_on_by_use() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_hurt" "spawnflags" "3" "targetname" "h" }"#,
        );
        let hurt = world.find_by_targetname(None, "h").unwrap();
        assert_eq!(world.entities.get(hurt).unwrap().solid, Solid::Not);
        trigger_hurt_use(&mut world, hurt, hurt, hurt);
        assert_eq!(world.entities.get(hurt).unwrap().solid, Solid::Trigger);
        trigger_hurt_use(&mut world, hurt, hurt, hurt);
        assert_eq!(world.entities.get(hurt).unwrap().solid, Solid::Not);
    }

    #[test]
    fn trigger_gravity_sets_entity_gravity() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_gravity" "gravity" "2" }"#,
        );
        let trigger = world.find_by_classname(None, "trigger_gravity").unwrap();
        let player = spawn_player(&mut world);
        trigger_gravity_touch(&mut world, trigger, player, None, None);
        assert_eq!(world.entities.get(player).unwrap().game.gravity, 2.0);
    }

    #[test]
    fn trigger_counter_fires_after_n_activations() {
        let (mut world, _gi) = test_world_recording();
        let target = spawn_counter_target(&mut world);
        let counter = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(counter).unwrap();
            ent.game.count = 3;
            ent.game.wait = -1.0;
            ent.game.target = "t".to_string();
        }
        let activator = spawn_player(&mut world);

        trigger_counter_use(&mut world, counter, activator, activator);
        trigger_counter_use(&mut world, counter, activator, activator);
        assert_eq!(uses(&world, target), 0);
        trigger_counter_use(&mut world, counter, activator, activator);
        assert_eq!(uses(&world, target), 1);
        trigger_counter_use(&mut world, counter, activator, activator);
        assert_eq!(uses(&world, target), 1);
    }

    #[test]
    fn trigger_always_fires_its_targets_after_its_delay() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_always" "target" "t" }"#,
        );
        let target = spawn_counter_target(&mut world);
        world.run_frame();
        assert_eq!(uses(&world, target), 0);
        world.run_frame();
        assert_eq!(uses(&world, target), 1);
    }

    #[test]
    fn trigger_key_needs_the_key_and_consumes_it() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_key" "item" "key_blue_key" "target" "t" }"#,
        );
        let target = spawn_counter_target(&mut world);
        let trigger = world.find_by_classname(None, "trigger_key").unwrap();
        let player = spawn_player(&mut world);
        let use_fn = world.entities.get(trigger).unwrap().use_fn.unwrap();

        use_fn(&mut world, trigger, player, player);
        assert_eq!(uses(&world, target), 0);

        let index = world.find_item("key_blue_key").unwrap();
        world.client_mut(player).unwrap().pers.inventory[index] = 1;
        use_fn(&mut world, trigger, player, player);
        assert_eq!(uses(&world, target), 1);
        let inv = world.client(player).unwrap().pers.inventory[index];
        assert_eq!(inv, 0);
        assert!(world.entities.get(trigger).unwrap().use_fn.is_none());
    }

    #[test]
    fn trigger_monsterjump_only_launches_grounded_walking_monsters() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_monsterjump" "speed" "100" "height" "300" }"#,
        );
        let trigger = world
            .find_by_classname(None, "trigger_monsterjump")
            .unwrap();
        let player = spawn_player(&mut world);
        let monster = world.spawn().unwrap();
        {
            let m = world.entities.get_mut(monster).unwrap();
            m.svflags |= SVF_MONSTER;
            m.game.ground_entity = Some(trigger);
        }
        trigger_monsterjump_touch(&mut world, trigger, player, None, None);
        trigger_monsterjump_touch(&mut world, trigger, monster, None, None);
        assert_eq!(world.entities.get(player).unwrap().velocity, Vec3f::ZERO);
        let v = world.entities.get(monster).unwrap().velocity;
        assert!((v.x - 100.0).abs() < 1e-3, "{v:?}");
        assert_eq!(v.z, 300.0);
    }

    #[test]
    fn trigger_spawns_without_keys_take_the_c_defaults() {
        // C SP_trigger_multiple wait 0.2, SP_trigger_push speed 1000,
        // SP_trigger_hurt dmg 5, SP_trigger_counter count 2,
        // SP_trigger_monsterjump speed 200 and height 200.
        let mut world = test_world();
        spawn_map(
            &mut world,
            concat!(
                "{ \"classname\" \"trigger_multiple\" }\n",
                "{ \"classname\" \"trigger_push\" }\n",
                "{ \"classname\" \"trigger_hurt\" }\n",
                "{ \"classname\" \"trigger_counter\" }\n",
                "{ \"classname\" \"trigger_monsterjump\" }\n",
            ),
        );
        let game = |class: &str| {
            let key = world.find_by_classname(None, class).expect(class);
            world.entities.get(key).unwrap().game.clone()
        };
        assert_eq!(game("trigger_multiple").wait, 0.2);
        assert_eq!(game("trigger_push").speed, 1000.0);
        assert_eq!(game("trigger_hurt").dmg, 5);
        assert_eq!(game("trigger_counter").count, 2);
        let jump = game("trigger_monsterjump");
        assert_eq!(jump.speed, 200.0);
        assert_eq!(jump.movedir.z, 200.0);
    }

    #[test]
    fn not_player_trigger_multiple_ignores_clients() {
        let mut world = test_world();
        let target = spawn_counter_target(&mut world);
        let trigger = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(trigger).unwrap();
            e.game.target = "t".into();
            e.game.wait = 0.2;
            e.game.spawnflags = MULTI_NOT_PLAYER;
        }
        let player = spawn_player(&mut world);
        touch_multi(&mut world, trigger, player, None, None);
        assert_eq!(uses(&world, target), 0);
    }

    /// A trigger_hurt doing 10 a touch, and a god-mode player.
    fn hurt_and_god(world: &mut GameWorld, spawnflags: i32) -> (EntityKey, EntityKey) {
        let trigger = world.spawn().unwrap();
        {
            let e = world.entities.get_mut(trigger).unwrap();
            e.game.dmg = 10;
            e.game.spawnflags = spawnflags;
        }
        let player = spawn_player(world);
        {
            let e = world.entities.get_mut(player).unwrap();
            e.game.takedamage = TakeDamage::Yes;
            e.game.flags |= EntityFlags::GODMODE;
        }
        (trigger, player)
    }

    #[test]
    fn no_protection_trigger_hurt_damages_through_god_mode() {
        let mut world = test_world();
        let (plain, god) = hurt_and_god(&mut world, 0);
        trigger_hurt_touch(&mut world, plain, god, None, None);
        assert_eq!(world.entities.get(god).unwrap().game.health, 100);

        let mut world = test_world();
        let (unprotected, god) = hurt_and_god(&mut world, HURT_NO_PROTECTION);
        trigger_hurt_touch(&mut world, unprotected, god, None, None);
        assert_eq!(world.entities.get(god).unwrap().game.health, 90);
    }

    #[test]
    fn toggle_trigger_hurt_stays_usable_and_hurts_what_it_is_switched_on_over() {
        // C hurt_use: switching on touches everything already inside
        // (gi.BoxEdicts), and only a TOGGLE hurt keeps its use.
        let (mut world, gi) = test_world_recording();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_hurt" "spawnflags" "3" "dmg" "10" "targetname" "h" }"#,
        );
        let hurt = world.find_by_targetname(None, "h").unwrap();
        let player = spawn_player(&mut world);
        world.entities.get_mut(player).unwrap().game.takedamage = TakeDamage::Yes;
        *gi.box_edicts.borrow_mut() = vec![1];
        let use_fn = world.entities.get(hurt).unwrap().use_fn.expect("hurt_use");
        use_fn(&mut world, hurt, hurt, hurt);
        assert_eq!(world.entities.get(player).unwrap().game.health, 90);
        assert!(world.entities.get(hurt).unwrap().use_fn.is_some());
    }

    #[test]
    fn trigger_monsterjump_gives_an_airborne_monster_only_its_forward_speed() {
        let mut world = test_world();
        spawn_map(
            &mut world,
            r#"{ "classname" "trigger_monsterjump" "speed" "100" "height" "300" }"#,
        );
        let trigger = world
            .find_by_classname(None, "trigger_monsterjump")
            .unwrap();
        let monster = world.spawn().unwrap();
        world.entities.get_mut(monster).unwrap().svflags |= SVF_MONSTER;
        trigger_monsterjump_touch(&mut world, trigger, monster, None, None);
        let v = world.entities.get(monster).unwrap().velocity;
        assert!((v.x - 100.0).abs() < 1e-3, "{v:?}");
        assert_eq!(v.z, 0.0);
    }
}
