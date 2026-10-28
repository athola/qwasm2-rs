//! Player HUD — stat calculations for health, ammo, armor, weapon icon.
//! C ref: `player/hud.c` (657 lines)

use crate::constants::{EntityFlags, ItemFlags, ATTN_NORM, CHAN_ITEM, POWER_ARMOR_NONE};
use crate::entity::EntityKey;
use crate::world::GameWorld;
use q2_shared::types::{PmType, Solid};

use q2_shared::constants::{
    STAT_AMMO, STAT_AMMO_ICON, STAT_ARMOR, STAT_ARMOR_ICON, STAT_FRAGS, STAT_HEALTH,
    STAT_HEALTH_ICON, STAT_LAYOUTS, STAT_PICKUP_ICON, STAT_PICKUP_STRING, STAT_SELECTED_ICON,
    STAT_SELECTED_ITEM, STAT_TIMER, STAT_TIMER_ICON,
};

impl GameWorld {
    /// The armor item the player wears, if any (C `ArmorIndex`).
    pub(crate) fn armor_index(&self, key: EntityKey) -> Option<usize> {
        let inventory = &self.client(key)?.pers.inventory;
        ["item_armor_jacket", "item_armor_combat", "item_armor_body"]
            .iter()
            .filter_map(|c| self.find_item(c))
            .find(|&i| inventory[i] > 0)
    }

    /// Update player_state_t.stats[] with current HUD values.
    /// C ref: `G_SetStats` (player/hud.c); the help icon is not ported.
    pub fn update_player_stats(&mut self, key: EntityKey) {
        // Power armor that ran out of cells switches off.
        let mut power_armor = self.power_armor_type(key);
        let cells = self
            .find_item_by_name("Cells")
            .and_then(|i| self.client(key).map(|c| c.pers.inventory[i]))
            .unwrap_or(0);
        if power_armor != POWER_ARMOR_NONE && cells == 0 {
            if let Some(ent) = self.entities.get_mut(key) {
                ent.game.flags.remove(EntityFlags::POWER_ARMOR);
            }
            let power2 = self.gi.sound_index("misc/power2.wav");
            self.gi
                .sound(self.number_of(key), CHAN_ITEM, power2, 1.0, ATTN_NORM, 0.0);
            power_armor = POWER_ARMOR_NONE;
        }
        let ammo = self.client(key).map_or(0, |c| c.ammo_index);
        let ammo_icon = (ammo != 0)
            .then(|| self.items.get(ammo))
            .flatten()
            .map_or(0, |it| self.gi.image_index(it.icon));
        let framenum = self.level.framenum;
        let timer = self.client(key).and_then(|c| {
            [
                (c.quad_framenum, "p_quad"),
                (c.invincible_framenum, "p_invulnerability"),
                (c.enviro_framenum, "p_envirosuit"),
                (c.breather_framenum, "p_rebreather"),
            ]
            .into_iter()
            .find(|&(until, _)| until > framenum)
        });
        let timer = timer.map(|(until, icon)| (self.gi.image_index(icon), (until - framenum) / 10));
        let power_icon = self.gi.image_index("i_powershield");
        let armor = self.armor_index(key).map(|i| {
            let icon = self.items[i].icon;
            (self.gi.image_index(icon), i)
        });
        let selected = self.client(key).map_or(-1, |c| c.pers.selected_item);
        let selected_icon = usize::try_from(selected)
            .ok()
            .filter(|&i| i > 0)
            .and_then(|i| self.items.get(i))
            .map_or(0, |it| self.gi.image_index(it.icon));
        let (time, pic_health) = (self.level.time, self.level.pic_health);

        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        let health = ent.game.health;
        let stats = &mut client.ps.stats;
        stats[STAT_HEALTH_ICON] = pic_health as i16;
        stats[STAT_HEALTH] = health.max(-99) as i16;
        let ammo_count = client.pers.inventory[client.ammo_index];
        stats[STAT_AMMO_ICON] = ammo_icon as i16;
        stats[STAT_AMMO] = if ammo_icon == 0 { 0 } else { ammo_count as i16 };
        // Flash between power armor and other armor icon.
        let show_power = power_armor != POWER_ARMOR_NONE && (armor.is_none() || framenum & 8 != 0);
        match armor {
            _ if show_power => {
                stats[STAT_ARMOR_ICON] = power_icon as i16;
                stats[STAT_ARMOR] = cells as i16;
            }
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
        let (timer_icon, timer_left) = timer.unwrap_or((0, 0));
        stats[STAT_TIMER_ICON] = timer_icon as i16;
        stats[STAT_TIMER] = timer_left as i16;
        stats[STAT_SELECTED_ICON] = selected_icon as i16;
        stats[STAT_SELECTED_ITEM] = selected as i16;
        stats[STAT_LAYOUTS] = 0;
        stats[STAT_FRAGS] = client.resp.score as i16;
    }
}

impl GameWorld {
    /// End the level through `targ` (a changelevel): respawn the dead,
    /// note the next map, and in single player go straight on; otherwise
    /// move everyone to an intermission spot until a button is pressed
    /// five seconds later. A new coop unit takes everyone's keys. C ref:
    /// `BeginIntermission` (player/hud.c), without the `fact1` spot fix
    /// and the scoreboard.
    pub fn begin_intermission(&mut self, targ: EntityKey) {
        if self.level.intermission_time != 0.0 {
            return; // already activated
        }
        let clients: Vec<EntityKey> = (1..=self.game.maxclients.max(0) as usize)
            .filter_map(|n| self.entities.key_for_number(n))
            .filter(|&k| self.entities.get(k).is_some_and(|e| e.in_use))
            .collect();
        // Respawn any dead clients.
        for &key in &clients {
            if self.entities.get(key).is_some_and(|e| e.game.health <= 0) {
                self.respawn(key);
            }
        }
        self.level.intermission_time = self.level.time;
        self.level.changemap = self
            .entities
            .get(targ)
            .map(|e| e.game.map.clone())
            .unwrap_or_default();
        if self.level.changemap.contains('*') {
            if self.game.coop {
                // Strip players of all keys between units.
                let keys: Vec<usize> = (0..self.items.len())
                    .filter(|&i| self.items[i].flags.contains(ItemFlags::KEY))
                    .collect();
                for &key in &clients {
                    if let Some(client) = self.client_mut(key) {
                        keys.iter().for_each(|&i| client.pers.inventory[i] = 0);
                    }
                }
            }
        } else if !self.game.deathmatch {
            self.level.exitintermission = true; // go immediately to the next level
            return;
        }
        self.level.exitintermission = false;

        // An intermission spot: one of the first four at random, or a
        // start when the map has none.
        let spot = match self.find_by_classname(None, "info_player_intermission") {
            Some(first) => {
                let mut spot = Some(first);
                for _ in 0..self.random_u32() & 3 {
                    // Wrap around the list.
                    spot = self
                        .find_by_classname(spot, "info_player_intermission")
                        .or_else(|| self.find_by_classname(None, "info_player_intermission"));
                }
                spot
            }
            None => self
                .find_by_classname(None, "info_player_start")
                .or_else(|| self.find_by_classname(None, "info_player_deathmatch")),
        };
        if let Some(spot) = spot.and_then(|k| self.entities.get(k)) {
            self.level.intermission_origin = spot.state.origin;
            self.level.intermission_angle = spot.state.angles;
        }
        for key in clients {
            self.move_client_to_intermission(key);
        }
    }

    /// Freeze a player at the intermission spot with nothing showing.
    /// C ref: `MoveClientToIntermission` (player/hud.c), without the
    /// scoreboard.
    pub fn move_client_to_intermission(&mut self, key: EntityKey) {
        let (origin, angles) = (
            self.level.intermission_origin,
            self.level.intermission_angle,
        );
        let Some((ent, client)) = self.entity_and_client_mut(key) else {
            return;
        };
        ent.state.origin = origin;
        for i in 0..3 {
            client.ps.pmove.origin[i] = (origin[i] * 8.0) as i16;
        }
        client.ps.viewangles = angles;
        client.ps.pmove.pm_type = PmType::Freeze;
        client.ps.gunindex = 0;
        client.ps.blend[3] = 0.0;
        client.ps.rdflags &= !q2_shared::constants::RDF_UNDERWATER;
        // Clean up powerup info.
        client.quad_framenum = 0;
        client.invincible_framenum = 0;
        client.breather_framenum = 0;
        client.enviro_framenum = 0;
        client.grenade_blew_up = false;
        client.grenade_time = 0.0;
        ent.game.viewheight = 0.0;
        ent.state.modelindex = 0;
        ent.state.modelindex2 = 0;
        ent.state.modelindex3 = 0;
        ent.state.effects = 0;
        ent.state.sound = 0;
        ent.solid = Solid::Not;
        self.link_entity(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world;

    use q2_shared::constants::{
        CS_ITEMS, STAT_AMMO, STAT_AMMO_ICON, STAT_ARMOR, STAT_ARMOR_ICON, STAT_PICKUP_ICON,
        STAT_PICKUP_STRING, STAT_TIMER, STAT_TIMER_ICON,
    };
    fn stats(world: &GameWorld, key: EntityKey) -> [i16; 32] {
        world.client(key).unwrap().ps.stats
    }

    fn armed_player() -> (
        GameWorld,
        EntityKey,
        std::rc::Rc<crate::world::MockGameImport>,
    ) {
        let (mut world, gi) = crate::world::test_world_recording();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        world.level.framenum = 100;
        (world, key, gi)
    }

    #[test]
    fn the_ammo_stat_shows_the_current_weapons_ammo_icon_and_count() {
        let (mut world, key, _) = armed_player();
        world.update_player_stats(key);
        assert_eq!(
            (
                stats(&world, key)[STAT_AMMO_ICON],
                stats(&world, key)[STAT_AMMO]
            ),
            (0, 0),
            "blaster"
        );
        let shells = world.find_item_by_name("Shells").unwrap();
        let client = world.client_mut(key).unwrap();
        client.ammo_index = shells;
        client.pers.inventory[shells] = 17;
        world.update_player_stats(key);
        let icon = world.gi.image_index("a_shells") as i16;
        assert_eq!(
            (
                stats(&world, key)[STAT_AMMO_ICON],
                stats(&world, key)[STAT_AMMO]
            ),
            (icon, 17)
        );
    }

    #[test]
    fn the_timer_stat_shows_the_first_running_powerup_in_c_order() {
        let (mut world, key, _) = armed_player();
        world.update_player_stats(key);
        assert_eq!(
            (
                stats(&world, key)[STAT_TIMER_ICON],
                stats(&world, key)[STAT_TIMER]
            ),
            (0, 0)
        );
        world.client_mut(key).unwrap().breather_framenum = 150;
        world.update_player_stats(key);
        let breather = world.gi.image_index("p_rebreather") as i16;
        assert_eq!(
            (
                stats(&world, key)[STAT_TIMER_ICON],
                stats(&world, key)[STAT_TIMER]
            ),
            (breather, 5)
        );
        world.client_mut(key).unwrap().enviro_framenum = 200;
        world.update_player_stats(key);
        assert_eq!(
            stats(&world, key)[STAT_TIMER_ICON],
            world.gi.image_index("p_envirosuit") as i16
        );
        world.client_mut(key).unwrap().invincible_framenum = 300;
        world.update_player_stats(key);
        assert_eq!(
            stats(&world, key)[STAT_TIMER_ICON],
            world.gi.image_index("p_invulnerability") as i16
        );
        world.client_mut(key).unwrap().quad_framenum = 399;
        world.update_player_stats(key);
        assert_eq!(
            (
                stats(&world, key)[STAT_TIMER_ICON],
                stats(&world, key)[STAT_TIMER]
            ),
            (world.gi.image_index("p_quad") as i16, 29)
        );
    }

    #[test]
    fn running_power_armor_shows_cells_and_flashes_with_body_armor() {
        let (mut world, key, _) = armed_player();
        let cells = world.find_item_by_name("Cells").unwrap();
        let screen = world.find_item_by_name("Power Screen").unwrap();
        let client = world.client_mut(key).unwrap();
        client.pers.inventory[cells] = 40;
        client.pers.inventory[screen] = 1;
        world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::POWER_ARMOR;
        world.update_player_stats(key);
        let shield_icon = world.gi.image_index("i_powershield") as i16;
        assert_eq!(
            (
                stats(&world, key)[STAT_ARMOR_ICON],
                stats(&world, key)[STAT_ARMOR]
            ),
            (shield_icon, 40)
        );

        // With body armor too, the icons alternate every 8 frames.
        let jacket = world.find_item_by_name("Jacket Armor").unwrap();
        world.client_mut(key).unwrap().pers.inventory[jacket] = 25;
        world.level.framenum = 96; // & 8 == 0: body armor
        world.update_player_stats(key);
        assert_eq!(stats(&world, key)[STAT_ARMOR], 25);
        world.level.framenum = 104; // & 8 != 0: power armor
        world.update_player_stats(key);
        assert_eq!(stats(&world, key)[STAT_ARMOR], 40);
    }

    #[test]
    fn power_armor_switches_off_when_the_cells_run_out() {
        let (mut world, key, gi) = armed_player();
        let screen = world.find_item_by_name("Power Screen").unwrap();
        world.client_mut(key).unwrap().pers.inventory[screen] = 1;
        world.entities.get_mut(key).unwrap().game.flags |= EntityFlags::POWER_ARMOR;
        world.update_player_stats(key);
        assert!(!world
            .entities
            .get(key)
            .unwrap()
            .game
            .flags
            .contains(EntityFlags::POWER_ARMOR));
        let power2 = world.gi.sound_index("misc/power2.wav");
        assert!(gi.sound_calls.borrow().iter().any(|c| c.2 == power2));
        assert_eq!(stats(&world, key)[STAT_ARMOR_ICON], 0);
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
        world.client_mut(key).unwrap().pers.inventory[jacket] = 25;
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

        let stats = world.client(key).unwrap().ps.stats;
        assert_eq!(stats[STAT_HEALTH], 75);
    }

    #[test]
    fn stats_reflect_armor() {
        let mut world = test_world();
        world.init_items();
        let body = world.find_item("item_armor_body").unwrap();
        let key = world.connect_client(1, "\\name\\P").unwrap();
        world.client_mut(key).unwrap().pers.inventory[body] = 50;

        world.update_player_stats(key);

        let stats = world.client(key).unwrap().ps.stats;
        assert_eq!(stats[STAT_ARMOR], 50);
    }

    // ---- BeginIntermission / MoveClientToIntermission ----

    use q2_shared::types::Vec3f;

    fn spot(world: &mut GameWorld, classname: &str, origin: Vec3f, yaw: f32) -> EntityKey {
        let key = world.spawn().unwrap();
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.classname = classname.into();
        ent.state.origin = origin;
        ent.state.angles = Vec3f::new(0.0, yaw, 0.0);
        key
    }

    fn exit_to(world: &mut GameWorld, map: &str) -> EntityKey {
        let exit = world.spawn().unwrap();
        world.entities.get_mut(exit).unwrap().game.map = map.into();
        exit
    }

    #[test]
    fn single_player_intermission_goes_straight_to_the_next_level() {
        let (mut world, key, _) = armed_player();
        world.level.time = 12.0;
        let exit = exit_to(&mut world, "base2");
        world.begin_intermission(exit);
        assert_eq!(world.level.intermission_time, 12.0);
        assert_eq!(world.level.changemap, "base2");
        assert!(world.level.exitintermission);
        assert_eq!(
            world.client(key).unwrap().ps.pmove.pm_type,
            PmType::Normal,
            "not moved"
        );
    }

    #[test]
    fn deathmatch_intermission_freezes_everyone_at_the_intermission_spot() {
        let (mut world, key, _) = armed_player();
        world.game.deathmatch = true;
        world.level.time = 12.0;
        spot(
            &mut world,
            "info_player_intermission",
            Vec3f::new(64.0, 32.0, 16.0),
            90.0,
        );
        let client = world.client_mut(key).unwrap();
        client.quad_framenum = 500;
        client.ps.blend = [1.0, 0.0, 0.0, 0.5];
        let exit = exit_to(&mut world, "q2dm2");
        world.begin_intermission(exit);
        assert!(!world.level.exitintermission);
        assert_eq!(
            world.level.intermission_origin,
            Vec3f::new(64.0, 32.0, 16.0)
        );
        let ent = world.entities.get(key).unwrap();
        assert_eq!(ent.state.origin, Vec3f::new(64.0, 32.0, 16.0));
        assert_eq!(
            (ent.state.modelindex, ent.solid),
            (0, q2_shared::types::Solid::Not)
        );
        let client = world.client(key).unwrap();
        assert_eq!(client.ps.pmove.origin, [512, 256, 128]);
        assert_eq!(client.ps.viewangles, Vec3f::new(0.0, 90.0, 0.0));
        assert_eq!(client.ps.pmove.pm_type, PmType::Freeze);
        assert_eq!(
            (client.ps.gunindex, client.ps.blend[3], client.quad_framenum),
            (0, 0.0, 0)
        );

        // Already in intermission: a second exit changes nothing.
        let other = exit_to(&mut world, "q2dm3");
        world.begin_intermission(other);
        assert_eq!(world.level.changemap, "q2dm2");
    }

    #[test]
    fn without_an_intermission_spot_the_player_start_is_used() {
        let (mut world, key, _) = armed_player();
        world.game.deathmatch = true;
        spot(
            &mut world,
            "info_player_start",
            Vec3f::new(8.0, 0.0, 0.0),
            0.0,
        );
        let exit = exit_to(&mut world, "q2dm2");
        world.begin_intermission(exit);
        assert_eq!(
            world.entities.get(key).unwrap().state.origin,
            Vec3f::new(8.0, 0.0, 0.0)
        );
    }

    #[test]
    fn a_new_coop_unit_takes_the_players_keys() {
        let (mut world, key, _) = armed_player();
        world.game.coop = true;
        let pass = world.find_item_by_name("Security Pass").unwrap();
        world.client_mut(key).unwrap().pers.inventory[pass] = 1;
        spot(&mut world, "info_player_start", Vec3f::ZERO, 0.0);
        let exit = exit_to(&mut world, "*bunk1$start");
        world.begin_intermission(exit);
        assert_eq!(world.client(key).unwrap().pers.inventory[pass], 0);
    }

    #[test]
    fn the_intermission_ends_on_a_button_after_five_seconds() {
        use q2_shared::constants::BUTTON_ANY;
        use q2_shared::types::UserCmd;
        let (mut world, key, _) = armed_player();
        world.game.deathmatch = true;
        spot(&mut world, "info_player_start", Vec3f::ZERO, 0.0);
        world.level.time = 10.0;
        let exit = exit_to(&mut world, "q2dm2");
        world.begin_intermission(exit);
        let press = UserCmd {
            msec: 10,
            buttons: BUTTON_ANY,
            forwardmove: 400,
            ..Default::default()
        };
        world.level.time = 14.0;
        world.think_client(key, &press);
        assert!(!world.level.exitintermission, "too soon");
        assert_eq!(
            world.entities.get(key).unwrap().state.origin,
            Vec3f::ZERO,
            "frozen"
        );
        world.level.time = 15.5;
        world.think_client(key, &press);
        assert!(world.level.exitintermission);
    }
}
