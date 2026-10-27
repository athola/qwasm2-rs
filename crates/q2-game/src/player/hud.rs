//! Player HUD — stat calculations for health, ammo, armor, weapon icon.
//! C ref: `player/hud.c` (657 lines)

use crate::entity::EntityKey;
use crate::world::GameWorld;

use q2_shared::constants::{
    STAT_ARMOR, STAT_ARMOR_ICON, STAT_FRAGS, STAT_HEALTH, STAT_HEALTH_ICON, STAT_LAYOUTS,
    STAT_PICKUP_ICON, STAT_PICKUP_STRING, STAT_SELECTED_ICON, STAT_SELECTED_ITEM,
};

impl GameWorld {
    /// The armor item the player wears, if any (C `ArmorIndex`).
    fn armor_index(&self, key: EntityKey) -> Option<usize> {
        let inventory = &self.entities.get(key)?.client.as_ref()?.pers.inventory;
        ["item_armor_jacket", "item_armor_combat", "item_armor_body"]
            .iter()
            .filter_map(|c| self.find_item(c))
            .find(|&i| inventory[i] > 0)
    }

    /// Update player_state_t.stats[] with current HUD values.
    /// C ref: `G_SetStats` (player/hud.c). Ammo, power armor, powerup
    /// timers and the help icon wait on the weapon and powerup code (#16).
    pub fn update_player_stats(&mut self, key: EntityKey) {
        let armor = self.armor_index(key).map(|i| {
            let icon = self.items[i].icon;
            (self.gi.image_index(icon), i)
        });
        let selected = self
            .entities
            .get(key)
            .and_then(|e| e.client.as_ref())
            .map_or(-1, |c| c.pers.selected_item);
        let selected_icon = usize::try_from(selected)
            .ok()
            .filter(|&i| i > 0)
            .and_then(|i| self.items.get(i))
            .map_or(0, |it| self.gi.image_index(it.icon));
        let (time, pic_health) = (self.level.time, self.level.pic_health);

        let Some(ent) = self.entities.get_mut(key) else {
            return;
        };
        let health = ent.game.health;
        let Some(client) = ent.client.as_mut() else {
            return;
        };
        let stats = &mut client.ps.stats;
        stats[STAT_HEALTH_ICON] = pic_health as i16;
        stats[STAT_HEALTH] = health.max(-99) as i16;
        match armor {
            Some((icon, index)) => {
                stats[STAT_ARMOR_ICON] = icon as i16;
                stats[STAT_ARMOR] = client.pers.inventory[index] as i16;
            }
            None => {
                stats[STAT_ARMOR_ICON] = 0;
                stats[STAT_ARMOR] = 0;
            }
        }
        if time > client.pickup_msg_time {
            stats[STAT_PICKUP_ICON] = 0;
            stats[STAT_PICKUP_STRING] = 0;
        }
        stats[STAT_SELECTED_ICON] = selected_icon as i16;
        stats[STAT_SELECTED_ITEM] = selected as i16;
        stats[STAT_LAYOUTS] = 0;
        stats[STAT_FRAGS] = client.resp.score as i16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    use q2_shared::constants::{CS_ITEMS, STAT_PICKUP_ICON, STAT_PICKUP_STRING};

    fn stats(world: &GameWorld, key: EntityKey) -> [i16; 32] {
        world
            .entities
            .get(key)
            .unwrap()
            .client
            .as_ref()
            .unwrap()
            .ps
            .stats
    }

    #[test]
    fn health_icon_is_i_health_and_health_bottoms_out_at_minus_99() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_game();
        world.spawn_entities("t", "{ \"classname\" \"worldspawn\" }", "");
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().game.health = -150;
        world.update_player_stats(key);
        let s = stats(&world, key);
        assert_eq!(s[STAT_HEALTH], -99);
        let icon = s[STAT_HEALTH_ICON] as usize;
        assert_eq!(gi.images.borrow()[icon - 1], "i_health");
    }

    #[test]
    fn armor_shows_the_worn_armor_icon_and_count() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_game();
        let jacket = world.find_item("item_armor_jacket").unwrap();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world
            .entities
            .get_mut(key)
            .unwrap()
            .client
            .as_mut()
            .unwrap()
            .pers
            .inventory[jacket] = 25;
        world.update_player_stats(key);
        let s = stats(&world, key);
        assert_eq!(s[STAT_ARMOR], 25);
        assert_eq!(
            gi.images.borrow()[s[STAT_ARMOR_ICON] as usize - 1],
            "i_jacketarmor"
        );
    }

    #[test]
    fn picking_up_an_item_shows_its_icon_and_name_for_three_seconds() {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_game();
        world.spawn_entities(
            "t",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"item_armor_shard\" }",
            "",
        );
        let item = world.find_by_classname(None, "item_armor_shard").unwrap();
        world.run_frame();
        world.run_frame();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().game.health = 100;
        crate::items::touch_item(&mut world, item, key, None, None);
        world.update_player_stats(key);
        let s = stats(&world, key);
        let shard = world.find_item("item_armor_shard").unwrap();
        assert_eq!(s[STAT_PICKUP_STRING] as usize, CS_ITEMS + shard);
        assert_eq!(
            gi.images.borrow()[s[STAT_PICKUP_ICON] as usize - 1],
            "i_jacketarmor"
        );
        for _ in 0..31 {
            world.run_frame();
        }
        world.update_player_stats(key);
        assert_eq!(stats(&world, key)[STAT_PICKUP_ICON], 0, "cleared after 3 s");
    }

    #[test]
    fn stats_reflect_health() {
        let mut world = test_world();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.entities.get_mut(key).unwrap().game.health = 75;

        world.update_player_stats(key);

        let stats = world
            .entities
            .get(key)
            .unwrap()
            .client
            .as_ref()
            .unwrap()
            .ps
            .stats;
        assert_eq!(stats[STAT_HEALTH], 75);
    }

    #[test]
    fn stats_reflect_armor() {
        let mut world = test_world();
        world.init_items();
        let body = world.find_item("item_armor_body").unwrap();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        if let Some(ref mut client) = world.entities.get_mut(key).unwrap().client {
            client.pers.inventory[body] = 50;
        }

        world.update_player_stats(key);

        let stats = world
            .entities
            .get(key)
            .unwrap()
            .client
            .as_ref()
            .unwrap()
            .ps
            .stats;
        assert_eq!(stats[STAT_ARMOR], 50);
    }
}
