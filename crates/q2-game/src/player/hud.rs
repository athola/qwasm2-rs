//! Player HUD — stat calculations for health, ammo, armor, weapon icon.
//! C ref: `player/hud.c` (657 lines)

use crate::entity::EntityKey;
use crate::world::GameWorld;

// Stat indices matching Q2 protocol.
const STAT_HEALTH_ICON: usize = 0;
const STAT_HEALTH: usize = 1;
const STAT_AMMO_ICON: usize = 2;
const STAT_AMMO: usize = 3;
const STAT_ARMOR_ICON: usize = 4;
const STAT_ARMOR: usize = 5;
const STAT_FRAGS: usize = 14;

impl GameWorld {
    /// Update player_state_t.stats[] with current HUD values.
    /// C ref: `G_SetStats` (player/hud.c).
    pub fn update_player_stats(&mut self, key: EntityKey) {
        let (health, armor) = {
            let Some(ent) = self.entities.get(key) else { return };
            let Some(ref client) = ent.client else { return };
            (ent.game.health, client.pers.inventory[1]) // armor at slot 1
        };

        if let Some(ent) = self.entities.get_mut(key) {
            if let Some(ref mut client) = ent.client {
                client.ps.stats[STAT_HEALTH] = health as i16;
                client.ps.stats[STAT_ARMOR] = armor as i16;
                client.ps.stats[STAT_FRAGS] = client.resp.score as i16;
                // Icon indices would be set from configstrings in full impl.
                let _ = (STAT_HEALTH_ICON, STAT_AMMO_ICON, STAT_AMMO, STAT_ARMOR_ICON);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    #[test]
    fn stats_reflect_health() {
        let mut world = test_world();
        let key = world.client_connect("\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().game.health = 75;

        world.update_player_stats(key);

        let stats = world.entities.get(key).unwrap()
            .client.as_ref().unwrap().ps.stats;
        assert_eq!(stats[STAT_HEALTH], 75);
    }

    #[test]
    fn stats_reflect_armor() {
        let mut world = test_world();
        let key = world.client_connect("\\name\\P").unwrap();
        if let Some(ref mut client) = world.entities.get_mut(key).unwrap().client {
            client.pers.inventory[1] = 50;
        }

        world.update_player_stats(key);

        let stats = world.entities.get(key).unwrap()
            .client.as_ref().unwrap().ps.stats;
        assert_eq!(stats[STAT_ARMOR], 50);
    }
}
