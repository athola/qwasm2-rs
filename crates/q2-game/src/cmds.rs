//! Client commands a player types or binds: using, dropping and cycling
//! items and weapons. C ref: `g_cmds.c`; chat, scores, cheats, `kill` and
//! the chase camera are not ported.

use q2_shared::constants::PRINT_HIGH;

use crate::constants::{ItemFlags, MAX_ITEMS};
use crate::entity::EntityKey;
use crate::world::GameWorld;

impl GameWorld {
    /// Run the client command in `gi.argv`/`gi.args` for `key`. C ref:
    /// `ClientCommand` (g_cmds.c); commands not ported here change
    /// nothing (C would say them as chat).
    pub fn client_command(&mut self, key: EntityKey) {
        if self.client(key).is_none() {
            return; // not fully in game yet
        }
        match self.gi.argv(0).to_ascii_lowercase().as_str() {
            "use" => self.cmd_use(key),
            "drop" => self.cmd_drop(key),
            "invnext" => self.select_next_item(key, ItemFlags::all()),
            "invprev" => self.select_prev_item(key, ItemFlags::all()),
            "invnextw" => self.select_next_item(key, ItemFlags::WEAPON),
            "invprevw" => self.select_prev_item(key, ItemFlags::WEAPON),
            "invnextp" => self.select_next_item(key, ItemFlags::POWERUP),
            "invprevp" => self.select_prev_item(key, ItemFlags::POWERUP),
            "invuse" => self.cmd_inv_use(key),
            "invdrop" => self.cmd_inv_drop(key),
            "weapprev" => self.cmd_weap_cycle(key, 1),
            "weapnext" => self.cmd_weap_cycle(key, MAX_ITEMS - 1),
            "weaplast" => self.cmd_weap_last(key),
            "help" => self.cmd_help(key),
            "score" => self.cmd_score(key),
            _ => {}
        }
    }

    fn print_to(&self, key: EntityKey, msg: &str) {
        self.gi.cprintf(self.number_of(key), PRINT_HIGH, msg);
    }

    /// The item named by `gi.args`, if the player has one of it.
    fn named_owned_item(&self, key: EntityKey) -> Option<usize> {
        let name = self.gi.args();
        let Some(index) = self.find_item_by_name(&name) else {
            self.print_to(key, &format!("unknown item: {name}\n"));
            return None;
        };
        Some(index)
    }

    fn owns(&self, key: EntityKey, index: usize) -> bool {
        self.client(key)
            .is_some_and(|c| c.pers.inventory[index] > 0)
    }

    /// C ref: `Cmd_Use_f`.
    fn cmd_use(&mut self, key: EntityKey) {
        let Some(index) = self.named_owned_item(key) else {
            return;
        };
        let Some(use_fn) = self.items[index].use_fn else {
            self.print_to(key, "Item is not usable.\n");
            return;
        };
        if !self.owns(key, index) {
            self.print_to(key, &format!("Out of item: {}\n", self.gi.args()));
            return;
        }
        use_fn(self, key, index);
    }

    /// C ref: `Cmd_Drop_f`.
    fn cmd_drop(&mut self, key: EntityKey) {
        let Some(index) = self.named_owned_item(key) else {
            return;
        };
        let Some(drop_fn) = self.items[index].drop_fn else {
            self.print_to(key, "Item is not dropable.\n");
            return;
        };
        if !self.owns(key, index) {
            self.print_to(key, &format!("Out of item: {}\n", self.gi.args()));
            return;
        }
        drop_fn(self, key, index);
    }

    /// Select the next owned, usable item with `flags`, scanning by
    /// `step` through the item list (C `SelectNextItem`/`SelectPrevItem`;
    /// no chase camera). None selects -1.
    fn select_item(&mut self, key: EntityKey, flags: ItemFlags, step: usize) {
        let Some(client) = self.client(key) else {
            return;
        };
        let start = client.pers.selected_item.rem_euclid(MAX_ITEMS as i32) as usize;
        let found = (1..=MAX_ITEMS)
            .map(|i| (start + i * step) % MAX_ITEMS)
            .find(|&index| {
                client.pers.inventory[index] > 0
                    && self
                        .items
                        .get(index)
                        .is_some_and(|item| item.use_fn.is_some() && item.flags.intersects(flags))
            });
        if let Some(client) = self.client_mut(key) {
            client.pers.selected_item = found.map_or(-1, |index| index as i32);
        }
    }

    /// C ref: `SelectNextItem`.
    pub fn select_next_item(&mut self, key: EntityKey, flags: ItemFlags) {
        self.select_item(key, flags, 1);
    }

    /// C ref: `SelectPrevItem`.
    pub fn select_prev_item(&mut self, key: EntityKey, flags: ItemFlags) {
        self.select_item(key, flags, MAX_ITEMS - 1);
    }

    /// Keep the selection on an item the player still has. C ref:
    /// `ValidateSelectedItem`.
    pub fn validate_selected_item(&mut self, key: EntityKey) {
        let valid = self.client(key).is_some_and(|c| {
            usize::try_from(c.pers.selected_item).is_ok_and(|i| c.pers.inventory[i] > 0)
        });
        if !valid {
            self.select_next_item(key, ItemFlags::all());
        }
    }

    /// The selected item, after validating it, or the `none` message.
    fn selected_item(&mut self, key: EntityKey, none: &str) -> Option<usize> {
        self.validate_selected_item(key);
        let selected = self.client(key).map_or(-1, |c| c.pers.selected_item);
        let index = usize::try_from(selected).ok();
        if index.is_none() {
            self.print_to(key, none);
        }
        index
    }

    /// C ref: `Cmd_InvUse_f`.
    fn cmd_inv_use(&mut self, key: EntityKey) {
        let Some(index) = self.selected_item(key, "No item to use.\n") else {
            return;
        };
        match self.items[index].use_fn {
            Some(use_fn) => use_fn(self, key, index),
            None => self.print_to(key, "Item is not usable.\n"),
        }
    }

    /// C ref: `Cmd_InvDrop_f`.
    fn cmd_inv_drop(&mut self, key: EntityKey) {
        let Some(index) = self.selected_item(key, "No item to drop.\n") else {
            return;
        };
        match self.items[index].drop_fn {
            Some(drop_fn) => drop_fn(self, key, index),
            None => self.print_to(key, "Item is not dropable.\n"),
        }
    }

    /// Use each owned weapon in scan order from the current one, stopping
    /// once one becomes current. `Use_Weapon` only sets `newweapon`, so in
    /// practice the last weapon reached wins, as in C. C ref:
    /// `Cmd_WeapPrev_f` (`step` 1) and `Cmd_WeapNext_f` (`step` -1).
    fn cmd_weap_cycle(&mut self, key: EntityKey, step: usize) {
        let Some(current) = self.client(key).and_then(|c| c.pers.weapon) else {
            return;
        };
        for i in 1..=MAX_ITEMS {
            let index = (current + i * step) % MAX_ITEMS;
            if !self.owns(key, index) {
                continue;
            }
            let Some(use_fn) = self.items.get(index).and_then(|item| {
                item.flags
                    .contains(ItemFlags::WEAPON)
                    .then_some(item.use_fn)
                    .flatten()
            }) else {
                continue;
            };
            use_fn(self, key, index);
            if self.client(key).and_then(|c| c.pers.weapon) == Some(index) {
                return; // successful
            }
        }
    }

    /// C ref: `Cmd_WeapLast_f`.
    fn cmd_weap_last(&mut self, key: EntityKey) {
        let Some(client) = self.client(key) else {
            return;
        };
        let (Some(_), Some(last)) = (client.pers.weapon, client.pers.last_weapon) else {
            return;
        };
        if !self.owns(key, last) {
            return;
        }
        let use_fn = self.items.get(last).and_then(|item| {
            item.flags
                .contains(ItemFlags::WEAPON)
                .then_some(item.use_fn)
                .flatten()
        });
        if let Some(use_fn) = use_fn {
            use_fn(self, key, last);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::entity::EntityKey;
    use crate::world::{test_world_recording, GameWorld, MockGameImport};

    fn player() -> (GameWorld, EntityKey, Rc<MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        world.init_items();
        let key = world.connect_client(1, "\\name\\P1").unwrap();
        world.begin_client(key);
        (world, key, gi)
    }

    fn give(world: &mut GameWorld, key: EntityKey, name: &str, count: i32) -> usize {
        let index = world.find_item_by_name(name).unwrap();
        world.client_mut(key).unwrap().pers.inventory[index] = count;
        index
    }

    fn run(world: &mut GameWorld, gi: &MockGameImport, key: EntityKey, text: &str) {
        *gi.command.borrow_mut() = text.split(' ').map(String::from).collect();
        world.client_command(key);
    }

    fn printed(gi: &MockGameImport) -> Vec<String> {
        gi.cprints
            .borrow()
            .iter()
            .map(|(_, text)| text.clone())
            .collect()
    }

    fn item(world: &GameWorld, name: &str) -> Option<usize> {
        world.find_item_by_name(name)
    }

    #[test]
    fn use_readies_an_owned_loaded_weapon() {
        let (mut world, key, gi) = player();
        let shotgun = give(&mut world, key, "Shotgun", 1);
        give(&mut world, key, "Shells", 10);
        run(&mut world, &gi, key, "use Shotgun");
        assert_eq!(world.client(key).unwrap().newweapon, Some(shotgun));
        assert!(printed(&gi).is_empty());
    }

    #[test]
    fn use_matches_the_item_name_case_insensitively_with_spaces() {
        let (mut world, key, gi) = player();
        let ssg = give(&mut world, key, "Super Shotgun", 1);
        give(&mut world, key, "Shells", 10);
        run(&mut world, &gi, key, "use super shotgun");
        assert_eq!(world.client(key).unwrap().newweapon, Some(ssg));
    }

    #[test]
    fn use_explains_why_it_cannot() {
        let (mut world, key, gi) = player();
        run(&mut world, &gi, key, "use Thing");
        run(&mut world, &gi, key, "use Shells");
        run(&mut world, &gi, key, "use Shotgun");
        give(&mut world, key, "Shotgun", 1);
        run(&mut world, &gi, key, "use Shotgun");
        give(&mut world, key, "Super Shotgun", 1);
        give(&mut world, key, "Shells", 1);
        run(&mut world, &gi, key, "use Super Shotgun");
        assert_eq!(
            printed(&gi),
            [
                "unknown item: Thing\n",
                "Item is not usable.\n",
                "Out of item: Shotgun\n",
                "No Shells for Shotgun.\n",
                "Not enough Shells for Super Shotgun.\n",
            ]
        );
        assert!(gi.cprints.borrow().iter().all(|(ent, _)| *ent == Some(1)));
        assert_eq!(world.client(key).unwrap().newweapon, None);
    }

    #[test]
    fn using_the_current_weapon_does_nothing() {
        let (mut world, key, gi) = player();
        run(&mut world, &gi, key, "use Blaster");
        assert_eq!(world.client(key).unwrap().newweapon, None);
    }

    #[test]
    fn drop_throws_a_weapon_out_of_the_inventory() {
        let (mut world, key, gi) = player();
        let shotgun = give(&mut world, key, "Shotgun", 1);
        run(&mut world, &gi, key, "drop Shotgun");
        assert_eq!(world.client(key).unwrap().pers.inventory[shotgun], 0);
        let dropped = world.find_by_classname(None, "weapon_shotgun").unwrap();
        assert_eq!(world.entities.get(dropped).unwrap().owner, Some(key));
    }

    #[test]
    fn drop_refuses_what_cannot_go() {
        let (mut world, key, gi) = player();
        run(&mut world, &gi, key, "drop Blaster");
        run(&mut world, &gi, key, "drop Shotgun");
        let shotgun = give(&mut world, key, "Shotgun", 1);
        world.client_mut(key).unwrap().pers.weapon = Some(shotgun);
        run(&mut world, &gi, key, "drop Shotgun");
        assert_eq!(
            printed(&gi),
            [
                "Item is not dropable.\n",
                "Out of item: Shotgun\n",
                "Can't drop current weapon\n",
            ]
        );
        assert_eq!(world.client(key).unwrap().pers.inventory[shotgun], 1);
    }

    #[test]
    fn weapons_stay_dmflag_keeps_weapons_from_being_dropped() {
        let (mut world, key, gi) = player();
        world.game.dmflags = crate::constants::DF_WEAPONS_STAY;
        let shotgun = give(&mut world, key, "Shotgun", 1);
        run(&mut world, &gi, key, "drop Shotgun");
        assert_eq!(world.client(key).unwrap().pers.inventory[shotgun], 1);
    }

    #[test]
    fn dropping_ammo_drops_one_pickup_worth_at_most() {
        let (mut world, key, gi) = player();
        let shells = give(&mut world, key, "Shells", 25);
        run(&mut world, &gi, key, "drop Shells");
        assert_eq!(world.client(key).unwrap().pers.inventory[shells], 15);
        let dropped = world.find_by_classname(None, "ammo_shells").unwrap();
        assert_eq!(world.entities.get(dropped).unwrap().game.count, 10);

        run(&mut world, &gi, key, "drop Shells");
        run(&mut world, &gi, key, "drop Shells");
        assert_eq!(
            world.client(key).unwrap().pers.inventory[shells],
            0,
            "the last 5"
        );
    }

    #[test]
    fn the_last_held_grenades_cannot_be_dropped() {
        let (mut world, key, gi) = player();
        let grenades = give(&mut world, key, "Grenades", 3);
        world.client_mut(key).unwrap().pers.weapon = Some(grenades);
        run(&mut world, &gi, key, "drop Grenades");
        assert_eq!(printed(&gi), ["Can't drop current weapon\n"]);
        assert_eq!(world.client(key).unwrap().pers.inventory[grenades], 3);
        assert!(world.find_by_classname(None, "ammo_grenades").is_none());
    }

    /// Blaster, Shotgun, Machinegun owned and loaded.
    fn armory() -> (GameWorld, EntityKey, Rc<MockGameImport>) {
        let (mut world, key, gi) = player();
        give(&mut world, key, "Shotgun", 1);
        give(&mut world, key, "Shells", 10);
        give(&mut world, key, "Machinegun", 1);
        give(&mut world, key, "Bullets", 10);
        (world, key, gi)
    }

    #[test]
    fn weapnext_and_weapprev_step_through_owned_weapons_like_c() {
        // C's scan keeps calling Use_Weapon, which only sets newweapon, so
        // the last weapon the scan reaches wins: weapprev settles on the
        // next lower item index, weapnext on the next higher one.
        let (mut world, key, gi) = armory();
        run(&mut world, &gi, key, "weapnext");
        assert_eq!(
            world.client(key).unwrap().newweapon,
            item(&world, "Shotgun")
        );
        let machinegun = item(&world, "Machinegun");
        world.client_mut(key).unwrap().pers.weapon = machinegun;
        run(&mut world, &gi, key, "weapprev");
        assert_eq!(
            world.client(key).unwrap().newweapon,
            item(&world, "Shotgun")
        );
    }

    #[test]
    fn weaplast_goes_back_to_the_previous_weapon() {
        let (mut world, key, gi) = armory();
        let client = world.client_mut(key).unwrap();
        client.pers.last_weapon = client.pers.weapon;
        world.client_mut(key).unwrap().pers.weapon = item(&world, "Machinegun");
        run(&mut world, &gi, key, "weaplast");
        assert_eq!(
            world.client(key).unwrap().newweapon,
            item(&world, "Blaster")
        );
    }

    #[test]
    fn weaplast_without_a_last_weapon_does_nothing() {
        let (mut world, key, gi) = armory();
        world.client_mut(key).unwrap().pers.last_weapon = None;
        run(&mut world, &gi, key, "weaplast");
        assert_eq!(world.client(key).unwrap().newweapon, None);
    }

    #[test]
    fn invnext_and_invprev_select_usable_items_and_invuse_uses_it() {
        let (mut world, key, gi) = armory();
        world.client_mut(key).unwrap().pers.selected_item = item(&world, "Blaster").unwrap() as i32;
        run(&mut world, &gi, key, "invnext");
        assert_eq!(
            world.client(key).unwrap().pers.selected_item,
            item(&world, "Shotgun").unwrap() as i32
        );
        run(&mut world, &gi, key, "invnext");
        assert_eq!(
            world.client(key).unwrap().pers.selected_item,
            item(&world, "Machinegun").unwrap() as i32
        );
        run(&mut world, &gi, key, "invprev");
        assert_eq!(
            world.client(key).unwrap().pers.selected_item,
            item(&world, "Shotgun").unwrap() as i32
        );
        run(&mut world, &gi, key, "invuse");
        assert_eq!(
            world.client(key).unwrap().newweapon,
            item(&world, "Shotgun")
        );
    }

    #[test]
    fn invnextw_skips_items_that_are_not_weapons() {
        let (mut world, key, gi) = armory();
        world.client_mut(key).unwrap().pers.selected_item =
            item(&world, "Machinegun").unwrap() as i32;
        run(&mut world, &gi, key, "invnextw");
        // Shells and bullets have no use; wraps round to the blaster.
        assert_eq!(
            world.client(key).unwrap().pers.selected_item,
            item(&world, "Blaster").unwrap() as i32
        );
        run(&mut world, &gi, key, "invprevw");
        assert_eq!(
            world.client(key).unwrap().pers.selected_item,
            item(&world, "Machinegun").unwrap() as i32
        );
    }

    #[test]
    fn invdrop_drops_the_selected_item() {
        let (mut world, key, gi) = armory();
        let shotgun = item(&world, "Shotgun").unwrap();
        world.client_mut(key).unwrap().pers.selected_item = shotgun as i32;
        run(&mut world, &gi, key, "invdrop");
        assert_eq!(world.client(key).unwrap().pers.inventory[shotgun], 0);
        // Drop_Weapon leaves the selection; the next invuse moves it on.
        assert_eq!(
            world.client(key).unwrap().pers.selected_item,
            shotgun as i32
        );
        run(&mut world, &gi, key, "invuse");
        assert_eq!(
            world.client(key).unwrap().pers.selected_item,
            item(&world, "Machinegun").unwrap() as i32
        );
    }

    #[test]
    fn with_nothing_selectable_invuse_and_invdrop_say_so() {
        let (mut world, key, gi) = player();
        let blaster = item(&world, "Blaster").unwrap();
        world.client_mut(key).unwrap().pers.inventory[blaster] = 0;
        run(&mut world, &gi, key, "invuse");
        run(&mut world, &gi, key, "invdrop");
        assert_eq!(printed(&gi), ["No item to use.\n", "No item to drop.\n"]);
        assert_eq!(world.client(key).unwrap().pers.selected_item, -1);
    }

    #[test]
    fn a_selected_item_without_use_or_drop_says_so() {
        let (mut world, key, gi) = player();
        let blaster = item(&world, "Blaster").unwrap();
        world.client_mut(key).unwrap().pers.selected_item = blaster as i32;
        run(&mut world, &gi, key, "invdrop");
        let shells = give(&mut world, key, "Shells", 5);
        world.client_mut(key).unwrap().pers.selected_item = shells as i32;
        run(&mut world, &gi, key, "invuse");
        assert_eq!(
            printed(&gi),
            ["Item is not dropable.\n", "Item is not usable.\n"]
        );
    }

    #[test]
    fn commands_from_a_number_without_a_client_are_ignored() {
        let (mut world, _, gi) = player();
        let other = world.spawn().unwrap();
        run(&mut world, &gi, other, "use Thing");
        assert!(printed(&gi).is_empty());
    }

    #[test]
    fn help_and_score_are_client_commands() {
        let (mut world, key, gi) = player();
        run(&mut world, &gi, key, "help");
        assert!(world.client(key).unwrap().showhelp);
        run(&mut world, &gi, key, "score");
        assert!(!world.client(key).unwrap().showhelp, "score hides help");
    }

    #[test]
    fn unknown_commands_change_nothing() {
        let (mut world, key, gi) = player();
        run(&mut world, &gi, key, "fly");
        assert!(printed(&gi).is_empty());
        assert_eq!(world.client(key).unwrap().newweapon, None);
    }
}
