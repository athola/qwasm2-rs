//! Monster implementations — 20 enemy types.
//!
//! Each monster defines animation frame tables, AI state callbacks,
//! and spawn functions. The soldier is implemented as the template;
//! remaining monsters follow the same pattern.

pub mod soldier;

use crate::entity::{EntityKey, EntityStorage};
use crate::world::GameWorld;
use std::collections::HashMap;

impl GameWorld {
    /// Register all monster spawn functions.
    pub fn register_monster_spawns(&mut self) {
        // Soldier (3 variants).
        self.spawn_table.insert("monster_soldier_light".to_string(), soldier::sp_monster_soldier_light);
        self.spawn_table.insert("monster_soldier".to_string(), soldier::sp_monster_soldier);
        self.spawn_table.insert("monster_soldier_ss".to_string(), soldier::sp_monster_soldier_ss);

        // Remaining monsters — register as generic stubs for now.
        // Each will be fleshed out in TASK-014.
        let stub_monsters = [
            "monster_infantry", "monster_gunner", "monster_gladiator",
            "monster_berserker", "monster_brain", "monster_chick",
            "monster_flipper", "monster_floater", "monster_flyer",
            "monster_hover", "monster_medic", "monster_mutant",
            "monster_parasite", "monster_insane", "monster_tank",
            "monster_tank_commander", "monster_supertank",
            "monster_boss2", "monster_boss3_stand",
            "monster_makron", "monster_jorg",
        ];
        for name in stub_monsters {
            self.spawn_table.insert(name.to_string(), sp_monster_stub);
        }

        // Turrets.
        self.spawn_table.insert("turret_breach".to_string(), sp_monster_stub);
        self.spawn_table.insert("turret_base".to_string(), sp_monster_stub);
        self.spawn_table.insert("turret_driver".to_string(), sp_monster_stub);
    }
}

/// Stub spawn for unimplemented monsters — sets basic monster defaults.
fn sp_monster_stub(
    storage: &mut EntityStorage,
    key: EntityKey,
    fields: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = fields
            .get("classname")
            .cloned()
            .unwrap_or_else(|| "monster".to_string());
        ent.game.health = 100;
    }
}

#[cfg(test)]
mod tests {
    use crate::world::test_world;

    #[test]
    fn register_monster_spawns() {
        let mut world = test_world();
        world.register_monster_spawns();
        assert!(world.spawn_table.get("monster_soldier").is_some());
        assert!(world.spawn_table.get("monster_soldier_light").is_some());
        assert!(world.spawn_table.get("monster_infantry").is_some());
        assert!(world.spawn_table.get("monster_tank").is_some());
        assert!(world.spawn_table.get("monster_boss2").is_some());
    }
}
