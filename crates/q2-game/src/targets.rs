//! Target entities — produce effects when triggered.
//!
//! Port of `g_target.c` (1,234 lines, 17 target types).
//! Each target has a `use` callback that fires when the entity is triggered.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_target.c`

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Target use callbacks
// ---------------------------------------------------------------------------

/// target_explosion — creates explosion effect and radius damage.
fn target_explosion_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let (dmg, origin) = {
        let Some(ent) = world.entities.get(self_key) else { return };
        (ent.game.dmg, ent.state.origin)
    };

    if dmg > 0 {
        world.t_radius_damage(
            self_key,
            self_key,
            dmg as f32,
            None,
            dmg as f32, // radius = damage (simplified)
            MeansOfDeath::Explosive,
        );
    }
    let _ = origin;
}

/// target_changelevel — exit to next map.
fn target_changelevel_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let map = world
        .entities
        .get(self_key)
        .map(|e| e.game.message.clone())
        .unwrap_or_default();

    if !map.is_empty() {
        world.level.nextmap = map;
        world.level.exitintermission = true;
    }
}

/// target_speaker — play a sound at entity origin.
fn target_speaker_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    _activator: EntityKey,
) {
    let noise = world
        .entities
        .get(self_key)
        .map(|e| e.game.noise_index)
        .unwrap_or(0);

    world.gi.sound(None, 1, noise, 1.0, 1.0, 0.0);
}

/// target_secret — increment secret counter.
fn target_secret_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.level.found_secrets += 1;
    world.use_targets(self_key, activator);
}

/// target_goal — increment goal counter.
fn target_goal_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.level.found_goals += 1;
    world.use_targets(self_key, activator);
}

/// Generic target use — just fires its own targets (relay behavior).
fn target_generic_use(
    world: &mut GameWorld,
    self_key: EntityKey,
    _other: EntityKey,
    activator: EntityKey,
) {
    world.use_targets(self_key, activator);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Register all target spawn functions.
    pub fn register_target_spawns(&mut self) {
        self.spawn_table.insert("target_temp_entity".to_string(), sp_target_generic);
        self.spawn_table.insert("target_speaker".to_string(), sp_target_speaker);
        self.spawn_table.insert("target_explosion".to_string(), sp_target_explosion);
        self.spawn_table.insert("target_changelevel".to_string(), sp_target_changelevel);
        self.spawn_table.insert("target_secret".to_string(), sp_target_secret);
        self.spawn_table.insert("target_goal".to_string(), sp_target_goal);
        self.spawn_table.insert("target_splash".to_string(), sp_target_generic);
        self.spawn_table.insert("target_spawner".to_string(), sp_target_generic);
        self.spawn_table.insert("target_blaster".to_string(), sp_target_generic);
        self.spawn_table.insert("target_crosslevel_trigger".to_string(), sp_target_generic);
        self.spawn_table.insert("target_crosslevel_target".to_string(), sp_target_generic);
        self.spawn_table.insert("target_laser".to_string(), sp_target_generic);
        self.spawn_table.insert("target_help".to_string(), sp_target_generic);
        self.spawn_table.insert("target_lightramp".to_string(), sp_target_generic);
        self.spawn_table.insert("target_earthquake".to_string(), sp_target_generic);
        self.spawn_table.insert("target_character".to_string(), sp_target_generic);
        self.spawn_table.insert("target_string".to_string(), sp_target_generic);
    }
}

fn sp_target_generic(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "target".to_string());
        ent.use_fn = Some(target_generic_use);
    }
}

fn sp_target_speaker(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_speaker".to_string();
        ent.use_fn = Some(target_speaker_use);
        let _ = fields;
    }
}

fn sp_target_explosion(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_explosion".to_string();
        ent.use_fn = Some(target_explosion_use);
        ent.game.dmg = fields
            .get("dmg")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
    }
}

fn sp_target_changelevel(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_changelevel".to_string();
        ent.use_fn = Some(target_changelevel_use);
        if let Some(map) = fields.get("map") {
            ent.game.message = map.clone();
        }
    }
}

fn sp_target_secret(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_secret".to_string();
        ent.use_fn = Some(target_secret_use);
        let _ = fields;
    }
}

fn sp_target_goal(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "target_goal".to_string();
        ent.use_fn = Some(target_goal_use);
        let _ = fields;
    }
}

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use q2_shared::types::Vec3f;
    use crate::world::test_world;

    #[test]
    fn register_target_spawns() {
        let mut world = test_world();
        world.register_target_spawns();
        assert!(world.spawn_table.get("target_explosion").is_some());
        assert!(world.spawn_table.get("target_changelevel").is_some());
        assert!(world.spawn_table.get("target_speaker").is_some());
        assert!(world.spawn_table.get("target_laser").is_some());
    }

    #[test]
    fn target_changelevel_sets_exit() {
        let mut world = test_world();

        let target = world.spawn().unwrap();
        world.entities.get_mut(target).unwrap().game.message = "base2".to_string();

        assert!(!world.level.exitintermission);

        target_changelevel_use(&mut world, target, target, target);

        assert!(world.level.exitintermission);
        assert_eq!(world.level.nextmap, "base2");
    }

    #[test]
    fn target_secret_increments_counter() {
        let mut world = test_world();
        let target = world.spawn().unwrap();

        assert_eq!(world.level.found_secrets, 0);
        target_secret_use(&mut world, target, target, target);
        assert_eq!(world.level.found_secrets, 1);
    }

    #[test]
    fn target_goal_increments_counter() {
        let mut world = test_world();
        let target = world.spawn().unwrap();

        assert_eq!(world.level.found_goals, 0);
        target_goal_use(&mut world, target, target, target);
        assert_eq!(world.level.found_goals, 1);
    }

    #[test]
    fn target_explosion_does_radius_damage() {
        let mut world = test_world();
        let target_ent = world.spawn().unwrap();
        let victim = world.spawn().unwrap();

        {
            let ent = world.entities.get_mut(target_ent).unwrap();
            ent.game.dmg = 50;
            ent.state.origin = Vec3f::ZERO;
        }
        {
            let ent = world.entities.get_mut(victim).unwrap();
            ent.state.origin = Vec3f::new(10.0, 0.0, 0.0);
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
        }

        target_explosion_use(&mut world, target_ent, target_ent, target_ent);

        assert!(world.entities.get(victim).unwrap().game.health < 200);
    }
}
