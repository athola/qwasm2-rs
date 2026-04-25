//! Miscellaneous entities — explobox, teleporters, gibs, path_corners, lights.
//!
//! Port of `g_misc.c` (2,726 lines). Contains utility entities that don't
//! fit into trigger/target/func categories.
//!
//! # C source reference
//! `~/Qwasm2/src/game/g_misc.c`

use q2_shared::types::*;

use crate::constants::*;
use crate::entity::{EntityKey, EntityStorage};
use crate::world::GameWorld;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Callbacks
// ---------------------------------------------------------------------------

/// Teleporter touch — move entity to destination.
fn teleporter_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    // Find the destination entity.
    let dest_key = {
        let target = world
            .entities
            .get(self_key)
            .map(|e| e.game.target.clone())
            .unwrap_or_default();
        if target.is_empty() {
            return;
        }
        world.find_by_targetname(None, &target)
    };

    let Some(dest) = dest_key else { return };
    let dest_origin = world
        .entities
        .get(dest)
        .map(|e| e.state.origin)
        .unwrap_or_default();

    // Teleport the touching entity.
    if let Some(ent) = world.entities.get_mut(other_key) {
        ent.state.origin = dest_origin;
        ent.velocity = Vec3f::ZERO;
        ent.game.ground_entity = None;
    }
}

/// Path corner reached — move to next waypoint.
fn path_corner_touch(
    world: &mut GameWorld,
    self_key: EntityKey,
    other_key: EntityKey,
    _plane: Option<&Plane>,
    _surface: Option<&Surface>,
) {
    // Set the touching entity's target to our target (next waypoint).
    let next_target = world
        .entities
        .get(self_key)
        .map(|e| e.game.target.clone())
        .unwrap_or_default();

    if let Some(ent) = world.entities.get_mut(other_key) {
        ent.game.target = next_target;
    }

    // Fire our own targets.
    world.use_targets(self_key, other_key);
}

/// Explobox die — explode on death.
fn explobox_die(
    world: &mut GameWorld,
    self_key: EntityKey,
    _inflictor: EntityKey,
    attacker: EntityKey,
    _damage: i32,
    _point: Vec3f,
) {
    let (dmg, origin) = {
        let Some(ent) = world.entities.get(self_key) else { return };
        (ent.game.dmg, ent.state.origin)
    };
    let _ = origin;

    // Radius damage.
    world.t_radius_damage(
        self_key,
        attacker,
        dmg as f32,
        None,
        dmg as f32,
        MeansOfDeath::Barrel,
    );

    world.free_entity(self_key);
}

// ---------------------------------------------------------------------------
// Spawn functions
// ---------------------------------------------------------------------------

impl GameWorld {
    /// Register all misc spawn functions.
    pub fn register_misc_spawns(&mut self) {
        self.spawn_table.insert("misc_explobox".to_string(), sp_misc_explobox);
        self.spawn_table.insert("misc_banner".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_satellite_dish".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_gib_arm".to_string(), sp_misc_gib);
        self.spawn_table.insert("misc_gib_leg".to_string(), sp_misc_gib);
        self.spawn_table.insert("misc_gib_head".to_string(), sp_misc_gib);
        self.spawn_table.insert("misc_insane".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_deadsoldier".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_viper".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_viper_bomb".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_bigviper".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_strogg_ship".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_teleporter".to_string(), sp_misc_teleporter);
        self.spawn_table.insert("misc_teleporter_dest".to_string(), sp_misc_teleporter_dest);
        self.spawn_table.insert("misc_blackhole".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_eastertank".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_easterchick".to_string(), sp_misc_generic);
        self.spawn_table.insert("misc_easterchick2".to_string(), sp_misc_generic);

        // Non-misc entities from g_misc.c
        self.spawn_table.insert("path_corner".to_string(), sp_path_corner);
        self.spawn_table.insert("point_combat".to_string(), sp_point_combat);
        self.spawn_table.insert("info_null".to_string(), sp_info_null);
        self.spawn_table.insert("func_group".to_string(), sp_info_null);
        self.spawn_table.insert("info_notnull".to_string(), sp_info_notnull);
        self.spawn_table.insert("viewthing".to_string(), sp_misc_generic);
        self.spawn_table.insert("light_mine1".to_string(), sp_light_mine);
        self.spawn_table.insert("light_mine2".to_string(), sp_light_mine);
        self.spawn_table.insert("monster_commander_body".to_string(), sp_misc_generic);

        // target_character and target_string live in g_misc.c
        self.spawn_table.insert("target_character".to_string(), sp_misc_generic);
        self.spawn_table.insert("target_string".to_string(), sp_misc_generic);
    }
}

fn sp_misc_explobox(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "misc_explobox".to_string();
        ent.solid = Solid::Bbox;
        ent.game.movetype = MoveType::Step;
        ent.game.health = fields
            .get("health")
            .and_then(|v| v.parse().ok())
            .unwrap_or(80);
        ent.game.dmg = fields
            .get("dmg")
            .and_then(|v| v.parse().ok())
            .unwrap_or(150);
        ent.game.takedamage = TakeDamage::Yes;
        ent.die = Some(explobox_die);
        ent.game.mass = 400;
    }
}

fn sp_misc_teleporter(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "misc_teleporter".to_string();
        ent.solid = Solid::Trigger;
        ent.game.movetype = MoveType::None;
        ent.touch = Some(teleporter_touch);
        if let Some(t) = fields.get("target") {
            ent.game.target = t.clone();
        }
    }
}

fn sp_misc_teleporter_dest(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "misc_teleporter_dest".to_string();
        if let Some(tn) = fields.get("targetname") {
            ent.game.targetname = tn.clone();
        }
    }
}

fn sp_path_corner(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "path_corner".to_string();
        ent.touch = Some(path_corner_touch);
        if let Some(t) = fields.get("target") {
            ent.game.target = t.clone();
        }
        if let Some(tn) = fields.get("targetname") {
            ent.game.targetname = tn.clone();
        }
        ent.game.wait = fields
            .get("wait")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
    }
}

fn sp_point_combat(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "point_combat".to_string();
        if let Some(tn) = fields.get("targetname") {
            ent.game.targetname = tn.clone();
        }
    }
}

fn sp_info_null(
    storage: &mut EntityStorage,
    key: EntityKey,
    _fields: &HashMap<String, String>,
) {
    // info_null and func_group are freed immediately — they're map editor hints.
    storage.free(key);
}

fn sp_info_notnull(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "info_notnull".to_string();
        if let Some(tn) = fields.get("targetname") {
            ent.game.targetname = tn.clone();
        }
    }
}

fn sp_misc_generic(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "misc".to_string());
    }
}

fn sp_misc_gib(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "misc_gib".to_string());
        ent.game.movetype = MoveType::Toss;
        ent.solid = Solid::Not;
    }
}

fn sp_light_mine(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "light_mine".to_string());
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
    fn register_misc_spawns() {
        let mut world = test_world();
        world.register_misc_spawns();
        assert!(world.spawn_table.get("misc_explobox").is_some());
        assert!(world.spawn_table.get("misc_teleporter").is_some());
        assert!(world.spawn_table.get("path_corner").is_some());
        assert!(world.spawn_table.get("info_null").is_some());
    }

    #[test]
    fn teleporter_moves_entity() {
        let mut world = test_world();

        // Create teleporter.
        let tele = world.spawn().unwrap();
        world.entities.get_mut(tele).unwrap().game.target = "dest1".to_string();

        // Create destination.
        let dest = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(dest).unwrap();
            ent.game.targetname = "dest1".to_string();
            ent.state.origin = Vec3f::new(1000.0, 2000.0, 3000.0);
        }

        // Create player at origin.
        let player = world.spawn().unwrap();
        world.entities.get_mut(player).unwrap().state.origin = Vec3f::ZERO;

        teleporter_touch(&mut world, tele, player, None, None);

        let pos = world.entities.get(player).unwrap().state.origin;
        assert_eq!(pos, Vec3f::new(1000.0, 2000.0, 3000.0));
    }

    #[test]
    fn explobox_dies_with_radius_damage() {
        let mut world = test_world();

        let box_ent = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(box_ent).unwrap();
            ent.game.dmg = 150;
            ent.state.origin = Vec3f::ZERO;
        }

        let victim = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(victim).unwrap();
            ent.state.origin = Vec3f::new(20.0, 0.0, 0.0);
            ent.game.health = 200;
            ent.game.takedamage = TakeDamage::Yes;
        }

        let attacker = world.spawn().unwrap();

        explobox_die(&mut world, box_ent, attacker, attacker, 100, Vec3f::ZERO);

        // Victim should have taken splash damage.
        assert!(world.entities.get(victim).unwrap().game.health < 200);
        // Box should be freed.
        assert!(world.entities.get(box_ent).is_none());
    }

    #[test]
    fn info_null_frees_immediately() {
        let mut world = test_world();
        let key = world.spawn().unwrap();
        assert_eq!(world.entities.count(), 1);

        sp_info_null(&mut world.entities, key, &HashMap::new());

        assert_eq!(world.entities.count(), 0);
    }

    #[test]
    fn path_corner_sets_next_target() {
        let mut world = test_world();

        let corner = world.spawn().unwrap();
        world.entities.get_mut(corner).unwrap().game.target = "corner2".to_string();

        let train = world.spawn().unwrap();
        world.entities.get_mut(train).unwrap().game.target = "corner1".to_string();

        path_corner_touch(&mut world, corner, train, None, None);

        // Train's target should now be corner2.
        assert_eq!(
            world.entities.get(train).unwrap().game.target,
            "corner2"
        );
    }
}
