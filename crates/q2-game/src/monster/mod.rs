//! Monster implementations — 20 enemy types.
//!
//! Each monster defines animation frame tables, AI state callbacks,
//! and spawn functions. The soldier is implemented as the template;
//! remaining monsters follow the same pattern.

pub mod soldier;
pub mod infantry;
pub mod gunner;
pub mod gladiator;
pub mod berserker;
pub mod brain;
pub mod chick;
pub mod flipper;
pub mod floater;
pub mod flyer;
pub mod hover;
pub mod medic;
pub mod mutant;
pub mod parasite;
pub mod insane;
pub mod tank;
pub mod supertank;
pub mod boss2;
pub mod boss3;
pub mod turret;

use crate::world::GameWorld;

impl GameWorld {
    /// Register all monster spawn functions.
    pub fn register_monster_spawns(&mut self) {
        // Soldier (3 variants).
        self.spawn_table.insert("monster_soldier_light".to_string(), soldier::sp_monster_soldier_light);
        self.spawn_table.insert("monster_soldier".to_string(), soldier::sp_monster_soldier);
        self.spawn_table.insert("monster_soldier_ss".to_string(), soldier::sp_monster_soldier_ss);

        // Infantry.
        self.spawn_table.insert("monster_infantry".to_string(), infantry::sp_monster_infantry);

        // Gunner.
        self.spawn_table.insert("monster_gunner".to_string(), gunner::sp_monster_gunner);

        // Gladiator.
        self.spawn_table.insert("monster_gladiator".to_string(), gladiator::sp_monster_gladiator);

        // Berserker.
        self.spawn_table.insert("monster_berserker".to_string(), berserker::sp_monster_berserker);

        // Brain.
        self.spawn_table.insert("monster_brain".to_string(), brain::sp_monster_brain);

        // Chick (Iron Maiden).
        self.spawn_table.insert("monster_chick".to_string(), chick::sp_monster_chick);

        // Flipper (Barracuda Shark).
        self.spawn_table.insert("monster_flipper".to_string(), flipper::sp_monster_flipper);

        // Floater.
        self.spawn_table.insert("monster_floater".to_string(), floater::sp_monster_floater);

        // Flyer.
        self.spawn_table.insert("monster_flyer".to_string(), flyer::sp_monster_flyer);

        // Hover (Icarus).
        self.spawn_table.insert("monster_hover".to_string(), hover::sp_monster_hover);

        // Medic.
        self.spawn_table.insert("monster_medic".to_string(), medic::sp_monster_medic);

        // Mutant.
        self.spawn_table.insert("monster_mutant".to_string(), mutant::sp_monster_mutant);

        // Parasite.
        self.spawn_table.insert("monster_parasite".to_string(), parasite::sp_monster_parasite);

        // Insane.
        self.spawn_table.insert("monster_insane".to_string(), insane::sp_monster_insane);

        // Tank (2 variants).
        self.spawn_table.insert("monster_tank".to_string(), tank::sp_monster_tank);
        self.spawn_table.insert("monster_tank_commander".to_string(), tank::sp_monster_tank_commander);

        // Supertank.
        self.spawn_table.insert("monster_supertank".to_string(), supertank::sp_monster_supertank);

        // Boss2 (Hornet).
        self.spawn_table.insert("monster_boss2".to_string(), boss2::sp_monster_boss2);

        // Boss3 — Jorg and Makron.
        self.spawn_table.insert("monster_boss3_stand".to_string(), boss3::sp_monster_boss3_stand);
        self.spawn_table.insert("monster_makron".to_string(), boss3::sp_monster_makron);
        self.spawn_table.insert("monster_jorg".to_string(), boss3::sp_monster_jorg);

        // Turrets.
        self.spawn_table.insert("turret_breach".to_string(), turret::sp_turret_breach);
        self.spawn_table.insert("turret_base".to_string(), turret::sp_turret_base);
        self.spawn_table.insert("turret_driver".to_string(), turret::sp_turret_driver);
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
