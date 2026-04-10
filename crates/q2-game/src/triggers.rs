//! Trigger entities — activate effects when touched or targeted.
//!
//! Port of `g_trigger.c` (863 lines, 11 trigger types).
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_trigger.c`

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;

// ---------------------------------------------------------------------------
// Trigger callbacks
// ---------------------------------------------------------------------------

/// Generic trigger touch — fires targets then applies wait time.
/// C ref: `multi_trigger` / `trigger_multiple` touch handler.
fn trigger_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    // Check wait (debounce).
    let (touch_debounce, wait) = {
        let Some(ent) = world.entities.get(self_key) else { return };
        (ent.game.touch_debounce_time, ent.game.wait)
    };

    if touch_debounce > world.level.time {
        return;
    }

    // Set debounce time.
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.touch_debounce_time = if wait > 0.0 {
            world.level.time + wait
        } else {
            world.level.time + 0.2 // minimum 200ms debounce
        };
    }

    // Fire targets.
    world.use_targets(self_key, other_key);
}

/// Trigger_once touch — fires targets then removes itself.
fn trigger_once_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    plane: Option<&Plane>,
    surface: Option<&Surface>,
) {
    trigger_touch(world, self_key, other_key, plane, surface);
    // Remove self after firing.
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.touch = None;
    }
    world.free_entity(self_key);
}

/// Trigger_relay use — pass-through activation to targets.
fn trigger_relay_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.use_targets(self_key, activator);
}

/// Trigger_push touch — launches touching entities.
fn trigger_push_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let speed = world
        .entities
        .get(self_key)
        .map(|e| e.game.speed)
        .unwrap_or(1000.0);

    // Compute push direction from entity's movedir.
    let movedir = world
        .entities
        .get(self_key)
        .map(|e| e.game.move_origin) // using move_origin to store movedir
        .unwrap_or(Vec3f::new(0.0, 0.0, 1.0));

    if let Some(ent) = world.entities.get_mut(other_key) {
        ent.velocity = movedir * speed;
        // Clear ground entity — entity is now airborne.
        ent.game.ground_entity = None;
    }
}

/// Trigger_hurt touch — apply damage to touching entities.
fn trigger_hurt_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let (dmg, touch_debounce) = {
        let Some(ent) = world.entities.get(self_key) else { return };
        (ent.game.dmg, ent.game.touch_debounce_time)
    };

    if touch_debounce > world.level.time {
        return;
    }

    // Set 1-second debounce.
    if let Some(ent) = world.entities.get_mut(self_key) {
        ent.game.touch_debounce_time = world.level.time + 1.0;
    }

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
        DamageFlags::NO_PROTECTION,
        MeansOfDeath::TriggerHurt,
    );
}

/// Trigger_gravity touch — set per-entity gravity.
fn trigger_gravity_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    let gravity = world
        .entities
        .get(self_key)
        .map(|e| e.game.gravity)
        .unwrap_or(1.0);

    if let Some(ent) = world.entities.get_mut(other_key) {
        ent.game.gravity = gravity;
    }
}

/// Trigger_counter use — count activations, fire targets when count reached.
fn trigger_counter_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    let count = {
        let Some(ent) = world.entities.get_mut(self_key) else { return };
        ent.game.count -= 1;
        ent.game.count
    };

    if count > 0 {
        return; // Not yet — need more activations.
    }

    world.use_targets(self_key, activator);
    world.free_entity(self_key);
}

/// Trigger_always use — fires immediately on spawn.
fn trigger_always_think(world: &mut GameWorld, self_key: EntityKey) {
    world.use_targets(self_key, self_key);
    world.free_entity(self_key);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Register all trigger spawn functions in the spawn table.
    pub fn register_trigger_spawns(&mut self) {
        self.spawn_table.insert("trigger_multiple".to_string(), sp_trigger_multiple);
        self.spawn_table.insert("trigger_once".to_string(), sp_trigger_once);
        self.spawn_table.insert("trigger_relay".to_string(), sp_trigger_relay);
        self.spawn_table.insert("trigger_push".to_string(), sp_trigger_push);
        self.spawn_table.insert("trigger_hurt".to_string(), sp_trigger_hurt);
        self.spawn_table.insert("trigger_gravity".to_string(), sp_trigger_gravity);
        self.spawn_table.insert("trigger_counter".to_string(), sp_trigger_counter);
        self.spawn_table.insert("trigger_always".to_string(), sp_trigger_always);
        self.spawn_table.insert("trigger_key".to_string(), sp_trigger_key);
        self.spawn_table.insert("trigger_elevator".to_string(), sp_trigger_elevator);
        self.spawn_table.insert("trigger_monsterjump".to_string(), sp_trigger_monsterjump);
    }
}

// All spawn functions follow the SpawnFn signature from spawn.rs.
use crate::entity::EntityStorage;
use std::collections::HashMap;

fn sp_trigger_multiple(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_multiple".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        ent.touch = Some(trigger_touch);
        ent.game.wait = fields
            .get("wait")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.2);
    }
}

fn sp_trigger_once(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_once".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        ent.touch = Some(trigger_once_touch);
        let _ = fields;
    }
}

fn sp_trigger_relay(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_relay".to_string();
        ent.use_fn = Some(trigger_relay_use);
        let _ = fields;
    }
}

fn sp_trigger_push(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_push".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        ent.touch = Some(trigger_push_touch);
        ent.game.speed = fields
            .get("speed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000.0);
    }
}

fn sp_trigger_hurt(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_hurt".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        ent.touch = Some(trigger_hurt_touch);
        ent.game.dmg = fields
            .get("dmg")
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
    }
}

fn sp_trigger_gravity(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_gravity".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        ent.touch = Some(trigger_gravity_touch);
        ent.game.gravity = fields
            .get("gravity")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0);
    }
}

fn sp_trigger_counter(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_counter".to_string();
        ent.use_fn = Some(trigger_counter_use);
        ent.game.count = fields
            .get("count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(2);
    }
}

fn sp_trigger_always(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_always".to_string();
        ent.think = Some(trigger_always_think);
        ent.game.nextthink = 0.3; // Fire shortly after spawn.
        let _ = fields;
    }
}

fn sp_trigger_key(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_key".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        // Key checking requires item system integration.
        // For now, just set basic fields.
        let _ = fields;
    }
}

fn sp_trigger_elevator(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_elevator".to_string();
        ent.use_fn = Some(trigger_relay_use); // Simplified: acts as relay.
        let _ = fields;
    }
}

fn sp_trigger_monsterjump(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "trigger_monsterjump".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        ent.touch = Some(trigger_push_touch); // Similar to push.
        ent.game.speed = fields
            .get("speed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(200.0);
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    #[test]
    fn register_trigger_spawns() {
        let mut world = test_world();
        world.register_trigger_spawns();
        assert!(world.spawn_table.get("trigger_multiple").is_some());
        assert!(world.spawn_table.get("trigger_once").is_some());
        assert!(world.spawn_table.get("trigger_relay").is_some());
        assert!(world.spawn_table.get("trigger_push").is_some());
        assert!(world.spawn_table.get("trigger_hurt").is_some());
        assert!(world.spawn_table.get("trigger_gravity").is_some());
        assert!(world.spawn_table.get("trigger_counter").is_some());
        assert!(world.spawn_table.get("trigger_always").is_some());
    }

    #[test]
    fn trigger_multiple_fires_with_debounce() {
        use std::sync::atomic::{AtomicI32, Ordering};
        static COUNT: AtomicI32 = AtomicI32::new(0);

        fn test_use(_w: &mut GameWorld, _s: EntityKey, _o: EntityKey, _a: EntityKey) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }

        COUNT.store(0, Ordering::Relaxed);

        let mut world = test_world();
        world.level.time = 1.0;

        // Create trigger with target.
        let trigger = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(trigger).unwrap();
            ent.game.classname = "trigger_multiple".to_string();
            ent.game.wait = 2.0;
            ent.game.target = "my_target".to_string();
        }

        // Create target entity.
        let target = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.targetname = "my_target".to_string();
            ent.use_fn = Some(test_use);
        }

        let player = world.spawn().unwrap();

        // First touch should fire.
        trigger_touch(&mut world, trigger, player, None, None);
        assert_eq!(COUNT.load(Ordering::Relaxed), 1);

        // Second touch within wait should NOT fire.
        trigger_touch(&mut world, trigger, player, None, None);
        assert_eq!(COUNT.load(Ordering::Relaxed), 1);

        // After wait expires, should fire again.
        world.level.time = 4.0;
        trigger_touch(&mut world, trigger, player, None, None);
        assert_eq!(COUNT.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn trigger_once_fires_and_removes() {
        let mut world = test_world();
        world.level.time = 1.0;

        let trigger = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(trigger).unwrap();
            ent.game.classname = "trigger_once".to_string();
        }
        let player = world.spawn().unwrap();

        trigger_once_touch(&mut world, trigger, player, None, None);

        // Trigger should be freed.
        assert!(world.entities.get(trigger).is_none());
    }

    #[test]
    fn trigger_push_sets_velocity() {
        let mut world = test_world();
        let trigger = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(trigger).unwrap();
            ent.game.speed = 800.0;
            ent.game.move_origin = Vec3f::new(0.0, 0.0, 1.0); // push up
        }

        let player = world.spawn().unwrap();
        world.entities.get_mut(player).unwrap().velocity = Vec3f::ZERO;

        trigger_push_touch(&mut world, trigger, player, None, None);

        let v = world.entities.get(player).unwrap().velocity;
        assert!((v.z - 800.0).abs() < 0.01);
    }

    #[test]
    fn trigger_hurt_damages_on_touch() {
        let mut world = test_world();
        world.level.time = 1.0;

        let trigger = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(trigger).unwrap();
            ent.game.dmg = 10;
        }

        let player = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(player).unwrap();
            ent.game.health = 100;
            ent.game.takedamage = TakeDamage::Yes;
        }

        trigger_hurt_touch(&mut world, trigger, player, None, None);

        assert_eq!(world.entities.get(player).unwrap().game.health, 90);
    }

    #[test]
    fn trigger_gravity_sets_entity_gravity() {
        let mut world = test_world();

        let trigger = world.spawn().unwrap();
        world.entities.get_mut(trigger).unwrap().game.gravity = 0.25;

        let player = world.spawn().unwrap();
        world.entities.get_mut(player).unwrap().game.gravity = 1.0;

        trigger_gravity_touch(&mut world, trigger, player, None, None);

        assert_eq!(world.entities.get(player).unwrap().game.gravity, 0.25);
    }

    #[test]
    fn trigger_counter_fires_after_n_activations() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static FIRED: AtomicBool = AtomicBool::new(false);

        fn test_use(_w: &mut GameWorld, _s: EntityKey, _o: EntityKey, _a: EntityKey) {
            FIRED.store(true, Ordering::Relaxed);
        }

        FIRED.store(false, Ordering::Relaxed);

        let mut world = test_world();

        let counter = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(counter).unwrap();
            ent.game.count = 3;
            ent.game.target = "tgt".to_string();
        }

        let target = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(target).unwrap();
            ent.game.targetname = "tgt".to_string();
            ent.use_fn = Some(test_use);
        }

        let activator = world.spawn().unwrap();

        // First two activations don't fire.
        trigger_counter_use(&mut world, counter, activator, activator);
        assert!(!FIRED.load(Ordering::Relaxed));

        trigger_counter_use(&mut world, counter, activator, activator);
        assert!(!FIRED.load(Ordering::Relaxed));

        // Third activation fires.
        trigger_counter_use(&mut world, counter, activator, activator);
        assert!(FIRED.load(Ordering::Relaxed));
    }
}
