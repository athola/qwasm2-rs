//! `GameExport` for `GameWorld`: the interface the server drives.
//!
//! C ref: `game_export_t` (game.h) filled in by `GetGameAPI` (g_main.c).
//! Client calls take the client's entity number (`slot + 1`), as C passes
//! `EDICT_NUM(slot + 1)`.

use crate::traits::{EdictView, GameExport};
use crate::world::GameWorld;
use q2_shared::{
    constants::MAX_CLIENTS,
    types::{PlayerState, UserCmd},
};

impl GameWorld {
    /// Set `game.maxclients` (clamped to `MAX_CLIENTS`, as the server clamps
    /// its cvar) and reserve entity numbers `1..=maxclients` for clients.
    pub fn set_max_clients(&mut self, max_clients: usize) {
        let max_clients = max_clients.min(MAX_CLIENTS);
        self.game.maxclients = max_clients as i32;
        self.entities.set_max_clients(max_clients);
        // C InitGame allocates game.clients for maxclients.
        self.clients.resize_with(max_clients, Default::default);
    }

    /// `true` if `number` is a client entity number. The server only passes
    /// valid ones in C; anything else is logged and ignored here.
    fn is_client_number(&self, number: usize) -> bool {
        let valid = (1..=self.game.maxclients as usize).contains(&number);
        if !valid {
            tracing::warn!(number, "client entity number out of range");
        }
        valid
    }
}

impl GameExport for GameWorld {
    fn init(&mut self) {
        self.init_game();
    }

    fn shutdown(&mut self) {}

    fn spawn_entities(&mut self, mapname: &str, entstring: &str, spawnpoint: &str) {
        GameWorld::spawn_entities(self, mapname, entstring, spawnpoint);
    }

    fn client_connect(&mut self, number: usize, userinfo: &str) -> bool {
        if !self.is_client_number(number) {
            return false;
        }
        // A reconnect replaces whatever the slot held.
        if let Some(old) = self.entities.key_for_number(number) {
            if let Some(ent) = self.entities.get_mut(old) {
                ent.in_use = false;
            }
        }
        self.connect_client(number, userinfo).is_some()
    }

    fn client_begin(&mut self, number: usize) {
        if !self.is_client_number(number) {
            return;
        }
        let slot = number - 1;
        // C ClientBeginDeathmatch G_InitEdicts even a body waiting for the
        // client.
        if self.game.deathmatch {
            if let Some(ent) = self
                .entities
                .key_for_number(number)
                .and_then(|k| self.entities.get_mut(k))
            {
                ent.in_use = false;
            }
        }
        // After a map change the new map's entities start empty: C
        // ClientBegin G_InitEdicts the body and attaches `game.clients[i]`.
        let key = match self.entities.key_for_number(number) {
            Some(key) if self.entities.is_live(key) => Some(key),
            _ => self.entities.spawn_numbered(number).inspect(|&key| {
                if let Some(ent) = self.entities.get_mut(key) {
                    ent.game.classname = "player".to_string();
                    ent.client = Some(slot);
                }
            }),
        };
        let Some(key) = key else {
            return;
        };
        // C runs InitClientResp for every body it spawns here. The live body
        // `connect_client` leaves counts as one: C sees a live body here only
        // after a loadgame (not ported). This also takes the coop respawn
        // snapshot after ClientConnect's InitClientPersistant, as C does.
        if let Some(record) = self.clients.get_mut(slot) {
            let mut client = std::mem::take(record);
            self.init_client_resp(&mut client);
            self.clients[slot] = client;
        }
        self.begin_client(key);
        // MoveClientToIntermission is not ported: a client joining during
        // an intermission only misses the effect.
        let intermission = self.level.intermission_time != 0.0;
        if self.game.deathmatch {
            if !intermission {
                self.send_login_effect(key);
            }
            self.print_entered_the_game(key);
        } else if !intermission && self.game.maxclients > 1 {
            self.send_login_effect(key);
            self.print_entered_the_game(key);
        }
    }

    fn client_disconnect(&mut self, number: usize) {
        if !self.is_client_number(number) {
            return;
        }
        if let Some(key) = self.entities.key_for_number(number) {
            self.disconnect_client(key);
        }
    }

    fn client_command(&mut self, _number: usize) {}

    fn client_think(&mut self, number: usize, cmd: &UserCmd) {
        if !self.is_client_number(number) {
            return;
        }
        if let Some(key) = self.entities.key_for_number(number) {
            self.think_client(key, cmd);
        }
    }

    fn run_frame(&mut self) {
        GameWorld::run_frame(self);
    }

    fn clear_entity_events(&mut self) {
        for (_, ent) in self.entities.iter_mut() {
            ent.state.event = 0;
        }
    }

    fn server_command(&mut self) {}

    fn num_edicts(&self) -> usize {
        // Freed edicts keep their slot and number, so the highest occupied
        // number is C's high-water mark.
        self.entities
            .iter()
            .filter_map(|(_, e)| usize::try_from(e.state.number).ok())
            .max()
            .map_or(0, |n| n + 1)
    }

    fn edict(&self, number: usize) -> Option<EdictView<'_>> {
        let ent = self.entities.get(self.entities.key_for_number(number)?)?;
        if !ent.in_use {
            return None;
        }
        let owner = ent
            .owner
            .and_then(|key| self.entities.get(key))
            .and_then(|o| usize::try_from(o.state.number).ok());
        Some(EdictView {
            state: &ent.state,
            svflags: ent.svflags,
            owner,
            linked: ent.linkcount != 0,
        })
    }

    fn client_player_state(&self, number: usize) -> Option<&PlayerState> {
        Some(&self.client(self.entities.key_for_number(number)?)?.ps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_world_recording;
    use q2_shared::constants::CS_MAXCLIENTS;

    #[test]
    fn spawning_a_map_advertises_the_client_count_like_sp_worldspawn() {
        let (mut world, gi) = test_world_recording();
        world.set_max_clients(4);
        world.init();
        world.spawn_entities("base1", "{ \"classname\" \"worldspawn\" }", "");
        assert!(gi
            .configstrings
            .borrow()
            .contains(&(CS_MAXCLIENTS as i32, "4".to_string())));
        assert!(world.spawn_table.get("monster_soldier").is_some());
    }

    #[test]
    fn client_count_is_clamped_to_max_clients() {
        let (mut world, _gi) = test_world_recording();
        world.set_max_clients(MAX_CLIENTS + 7);
        assert_eq!(world.game.maxclients as usize, MAX_CLIENTS);
    }

    #[test]
    fn client_calls_outside_the_client_slots_are_rejected_without_panic() {
        let (mut world, _gi) = test_world_recording();
        world.set_max_clients(2);
        world.init();
        assert!(!world.client_connect(0, ""), "0 is the world");
        assert!(!world.client_connect(3, ""));
        world.client_begin(3);
        world.client_disconnect(MAX_CLIENTS + 1);
        world.client_think(0, &UserCmd::default());
    }

    #[test]
    fn connected_client_becomes_a_player_entity_at_its_number() {
        let (mut world, gi) = test_world_recording();
        world.set_max_clients(2);
        world.init();
        world.spawn_entities(
            "base1",
            "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"info_player_start\" \"origin\" \"64 0 24\" }",
            "",
        );
        assert!(world.client_connect(2, r"\name\Tester"));
        world.client_begin(2);

        let key = world.entities.key_for_number(2).unwrap();
        let ent = world.entities.get(key).unwrap();
        assert!(ent.in_use);
        assert_eq!(ent.game.classname, "player");
        // SelectSpawnPoint lifts 9 units, PutClientInServer 1 more.
        assert_eq!(ent.state.origin, q2_shared::Vec3f::new(64.0, 0.0, 34.0));
        assert!(gi.links.borrow().iter().any(|l| l.number == 2));

        world.client_disconnect(2);
        let ent = world.entities.get(key).unwrap();
        assert!(!ent.in_use);
        assert_eq!(ent.game.classname, "disconnected");
    }

    #[test]
    fn spawn_entities_starts_the_new_map_from_empty_storage() {
        let (mut world, _gi) = test_world_recording();
        world.init();
        let entstring = "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"info_player_start\" \"origin\" \"0 0 0\" }";
        world.spawn_entities("one", entstring, "");
        let first = world.entities.count();
        world.spawn_entities("two", entstring, "");
        assert_eq!(world.entities.count(), first, "C memsets g_edicts per map");
        assert_eq!(world.level.mapname, "two");
    }

    const TWO_STARTS: &str = "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"info_player_start\" \"origin\" \"64 0 24\" }";

    /// A world with one client slot, a map loaded and the client in it.
    fn world_with_player() -> GameWorld {
        let (mut world, _gi) = test_world_recording();
        world.set_max_clients(1);
        world.init();
        world.spawn_entities("demo1", TWO_STARTS, "");
        assert!(world.client_connect(1, r"\name\Tester"));
        world.client_begin(1);
        world
    }

    fn player(world: &GameWorld) -> &crate::entity::Entity {
        let key = world.entities.key_for_number(1).unwrap();
        world.entities.get(key).unwrap()
    }

    fn player_mut(world: &mut GameWorld) -> &mut crate::entity::Entity {
        let key = world.entities.key_for_number(1).unwrap();
        world.entities.get_mut(key).unwrap()
    }

    fn client(world: &GameWorld) -> &crate::entity::ClientData {
        world
            .client(world.entities.key_for_number(1).unwrap())
            .unwrap()
    }

    fn client_mut(world: &mut GameWorld) -> &mut crate::entity::ClientData {
        world
            .client_mut(world.entities.key_for_number(1).unwrap())
            .unwrap()
    }

    #[test]
    fn client_persistent_data_survives_a_map_change_like_save_client_data() {
        use crate::constants::EntityFlags;
        let mut world = world_with_player();
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let shells = world.find_item_by_name("Shells").unwrap();
        {
            let ent = player_mut(&mut world);
            ent.game.health = 73;
            ent.game.max_health = 120;
            ent.game.flags |= EntityFlags::GODMODE
                | EntityFlags::NOTARGET
                | EntityFlags::POWER_ARMOR
                | EntityFlags::SWIM;
            let pers = &mut client_mut(&mut world).pers;
            pers.inventory[shotgun] = 1;
            pers.inventory[shells] = 42;
            pers.weapon = Some(shotgun);
            pers.selected_item = shotgun as i32;
            pers.ammo_max.bullets = 300;
        }

        // SV_Map for the next unit: new entities, then the client re-sends
        // `begin` (no ClientConnect on a gamemap).
        world.spawn_entities("demo2", TWO_STARTS, "");
        assert!(
            !world
                .entities
                .key_for_number(1)
                .is_some_and(|k| world.entities.is_live(k)),
            "the new map starts without the player"
        );
        world.client_begin(1);

        let ent = player(&world);
        assert!(ent.in_use);
        assert_eq!((ent.game.health, ent.game.max_health), (73, 120));
        // FetchClientEntData restores only the saved flags.
        assert!(ent
            .game
            .flags
            .contains(EntityFlags::GODMODE | EntityFlags::NOTARGET | EntityFlags::POWER_ARMOR));
        assert!(!ent.game.flags.contains(EntityFlags::SWIM));
        let pers = &client(&world).pers;
        assert_eq!(pers.inventory[shotgun], 1);
        assert_eq!(pers.inventory[shells], 42);
        assert_eq!(pers.weapon, Some(shotgun));
        assert_eq!(pers.selected_item, shotgun as i32);
        assert_eq!(pers.ammo_max.bullets, 300);
        assert_eq!(pers.netname, "Tester");
        assert!(pers.connected);
    }

    #[test]
    fn a_fresh_connect_starts_from_init_client_persistant() {
        let mut world = world_with_player();
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        {
            let ent = player_mut(&mut world);
            ent.game.health = 12;
            let pers = &mut client_mut(&mut world).pers;
            pers.inventory[shotgun] = 1;
            pers.ammo_max.bullets = 300;
        }
        world.client_disconnect(1);
        assert!(world.client_connect(1, r"\name\Again"));
        world.client_begin(1);

        let blaster = world.find_item_by_name("Blaster").unwrap();
        let ent = player(&world);
        assert_eq!((ent.game.health, ent.game.max_health), (100, 100));
        let pers = &client(&world).pers;
        assert_eq!(pers.inventory[shotgun], 0);
        assert_eq!(pers.inventory[blaster], 1);
        assert_eq!(pers.selected_item, blaster as i32);
        assert_eq!(pers.weapon, Some(blaster));
        assert_eq!(pers.ammo_max, crate::items::AMMO_MAX_DEFAULT);
        assert_eq!(pers.netname, "Again");
        assert_eq!(pers.userinfo, r"\name\Again");
        assert!(pers.connected);
    }

    #[test]
    fn a_player_dead_at_the_map_change_is_reinitialized_by_put_client_in_server() {
        let mut world = world_with_player();
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        {
            let ent = player_mut(&mut world);
            ent.game.health = -5; // SaveClientData copies this into pers
            ent.game.max_health = 120;
            client_mut(&mut world).pers.inventory[shotgun] = 1;
        }
        world.spawn_entities("demo2", TWO_STARTS, "");
        world.client_begin(1);

        let blaster = world.find_item_by_name("Blaster").unwrap();
        let ent = player(&world);
        assert_eq!((ent.game.health, ent.game.max_health), (100, 100));
        let pers = &client(&world).pers;
        assert_eq!(pers.inventory[shotgun], 0);
        assert_eq!(pers.inventory[blaster], 1);
    }

    #[test]
    fn put_client_in_server_clears_the_respawn_data_in_single_player() {
        let mut world = world_with_player();
        client_mut(&mut world).resp.score = 9;
        world.spawn_entities("demo2", TWO_STARTS, "");
        world.client_begin(1);
        assert_eq!(client(&world).resp.score, 0);
    }

    #[test]
    fn begin_without_a_connect_starts_a_client_from_init_client_persistant() {
        let (mut world, _gi) = test_world_recording();
        world.set_max_clients(1);
        world.init();
        world.spawn_entities("demo1", TWO_STARTS, "");
        world.client_begin(1);
        let blaster = world.find_item_by_name("Blaster").unwrap();
        let ent = player(&world);
        assert!(ent.in_use);
        assert_eq!(ent.game.health, 100);
        assert_eq!(client(&world).pers.inventory[blaster], 1);
    }

    /// `world_with_player` with the coop or deathmatch cvar set first.
    fn world_in_mode(
        deathmatch: bool,
        coop: bool,
    ) -> (GameWorld, std::rc::Rc<crate::world::MockGameImport>) {
        let (mut world, gi) = test_world_recording();
        world.set_max_clients(1);
        world.game.deathmatch = deathmatch;
        world.game.coop = coop;
        world.init();
        world.spawn_entities("demo1", TWO_STARTS, "");
        assert!(world.client_connect(1, r"\name\Tester"));
        world.client_begin(1);
        (world, gi)
    }

    #[test]
    fn coop_map_change_keeps_the_score_through_the_persistant_data() {
        let (mut world, _gi) = world_in_mode(false, true);
        client_mut(&mut world).resp.score = 9;
        world.spawn_entities("demo2", TWO_STARTS, "");
        assert_eq!(world.clients[0].pers.score, 9, "SaveClientData");
        world.client_begin(1);
        let client = client(&world);
        assert_eq!(client.resp.score, 9, "FetchClientEntData");
        assert_eq!(client.resp.coop_respawn.score, 9, "InitClientResp");
    }

    #[test]
    fn single_player_map_change_does_not_save_the_score() {
        let (mut world, _gi) = world_in_mode(false, false);
        client_mut(&mut world).resp.score = 9;
        world.spawn_entities("demo2", TWO_STARTS, "");
        assert_eq!(world.clients[0].pers.score, 0);
    }

    #[test]
    fn coop_respawn_restores_the_level_entry_persistant_and_keeps_the_score() {
        let (mut world, _gi) = world_in_mode(false, true);
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        let blaster = world.find_item_by_name("Blaster").unwrap();
        {
            let client = client_mut(&mut world);
            client.pers.inventory[shotgun] = 1;
            client.resp.score = 4;
        }
        player_mut(&mut world).game.health = -10;
        player_mut(&mut world).svflags |= q2_shared::constants::SVF_NOCLIENT;
        let key = world.entities.key_for_number(1).unwrap();
        world.respawn(key);

        let ent = player(&world);
        assert_eq!(ent.game.health, 100);
        assert_eq!(ent.svflags & q2_shared::constants::SVF_NOCLIENT, 0);
        assert_eq!(ent.state.event, q2_shared::constants::EV_PLAYER_TELEPORT);
        let client = client(&world);
        assert_eq!(client.pers.inventory[shotgun], 0);
        assert_eq!(client.pers.inventory[blaster], 1);
        assert_eq!((client.resp.score, client.pers.score), (4, 4));
        assert_eq!(client.pers.netname, "Tester");
        assert_eq!(
            client.ps.pmove.pm_flags,
            q2_common::pmove::PMF_TIME_TELEPORT
        );
        assert_eq!(client.ps.pmove.pm_time, 14);
    }

    #[test]
    fn deathmatch_respawn_starts_from_init_client_persistant_and_keeps_the_score() {
        let (mut world, _gi) = world_in_mode(true, false);
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        {
            let client = client_mut(&mut world);
            client.pers.inventory[shotgun] = 1;
            client.pers.max_health = 250;
            client.resp.score = 3;
        }
        player_mut(&mut world).game.health = -10;
        let key = world.entities.key_for_number(1).unwrap();
        world.respawn(key);

        assert_eq!(player(&world).game.health, 100);
        let client = client(&world);
        assert_eq!(client.pers.inventory[shotgun], 0);
        assert_eq!(client.pers.max_health, 100);
        assert_eq!(client.resp.score, 3);
        assert_eq!(client.pers.netname, "Tester", "userinfo kept");
    }

    #[test]
    fn single_player_respawn_asks_for_the_load_game_menu() {
        let (mut world, gi) = world_in_mode(false, false);
        player_mut(&mut world).game.health = -10;
        let key = world.entities.key_for_number(1).unwrap();
        world.respawn(key);
        assert_eq!(gi.commands.borrow().last().unwrap(), "menu_loadgame\n");
        assert_eq!(player(&world).game.health, -10, "not put back in");
    }

    #[test]
    fn deathmatch_begin_reinitializes_the_body_and_the_persistant_data() {
        use crate::constants::EntityFlags;
        let (mut world, _gi) = test_world_recording();
        world.set_max_clients(1);
        world.game.deathmatch = true;
        world.init();
        world.spawn_entities("q2dm1", TWO_STARTS, "");
        assert!(world.client_connect(1, r"\name\Tester"));
        let shotgun = world.find_item("weapon_shotgun").unwrap();
        client_mut(&mut world).pers.inventory[shotgun] = 1;
        player_mut(&mut world).game.flags |= EntityFlags::GODMODE;
        world.client_begin(1);

        assert!(
            !player(&world).game.flags.contains(EntityFlags::GODMODE),
            "G_InitEdict"
        );
        assert_eq!(client(&world).pers.inventory[shotgun], 0);
        assert_eq!(client(&world).pers.netname, "Tester");
    }

    #[test]
    fn deathmatch_begin_shows_the_login_flash_and_announces_the_player() {
        let (world, gi) = world_in_mode(true, false);
        let origin = player(&world).state.origin;
        assert_eq!(
            *gi.writes.borrow(),
            vec![q2_shared::protocol::SvcOp::MuzzleFlash as i32, 1, 9]
        );
        assert_eq!(
            *gi.multicasts.borrow(),
            vec![(origin, q2_shared::types::Multicast::PVS)]
        );
        assert_eq!(
            *gi.bprints.borrow(),
            vec![(
                q2_shared::constants::PRINT_HIGH,
                "Tester entered the game\n".to_string()
            )]
        );
    }

    #[test]
    fn single_player_begin_announces_the_player_only_with_more_than_one_slot() {
        let (_world, gi) = world_in_mode(false, false);
        assert!(gi.bprints.borrow().is_empty());
        assert!(gi.writes.borrow().is_empty());
        assert!(!gi
            .dprints
            .borrow()
            .iter()
            .any(|m| m.ends_with("connected\n")));

        let (mut world, gi) = test_world_recording();
        world.set_max_clients(2);
        world.init();
        world.spawn_entities("demo1", TWO_STARTS, "");
        assert!(world.client_connect(2, r"\name\Second"));
        world.client_begin(2);
        assert_eq!(
            *gi.writes.borrow(),
            vec![q2_shared::protocol::SvcOp::MuzzleFlash as i32, 2, 9]
        );
        assert_eq!(gi.multicasts.borrow().len(), 1);
        assert_eq!(gi.bprints.borrow()[0].1, "Second entered the game\n");
        assert!(gi
            .dprints
            .borrow()
            .iter()
            .any(|m| m == "Second connected\n"));
    }

    #[test]
    fn the_server_reads_an_in_use_edict_with_its_owner_by_number() {
        let mut world = world_with_player();
        let player_key = world.entities.key_for_number(1).unwrap();
        let rocket = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(rocket).unwrap();
            ent.state.modelindex = 7;
            ent.svflags = q2_shared::constants::SVF_NOCLIENT;
            ent.owner = Some(player_key);
        }
        let number = world.entities.get(rocket).unwrap().state.number as usize;

        let game: &dyn GameExport = &world;
        let view = game.edict(number).expect("in use");
        assert_eq!(view.state.number as usize, number);
        assert_eq!(view.state.modelindex, 7);
        assert_eq!(view.svflags, q2_shared::constants::SVF_NOCLIENT);
        assert_eq!(view.owner, Some(1), "owner by entity number");
        assert!(!view.linked, "never linked");
        assert!(
            game.edict(1).expect("player").linked,
            "PutClientInServer links"
        );
        assert_eq!(game.edict(1).unwrap().owner, None);
    }

    #[test]
    fn a_freed_or_unused_number_reads_as_no_edict() {
        let mut world = world_with_player();
        let ent = world.spawn().unwrap();
        let number = world.entities.get(ent).unwrap().state.number as usize;
        world.free_entity(ent);
        let game: &dyn GameExport = &world;
        assert!(
            game.edict(number).is_none(),
            "C G_FreeEdict zeroes the edict"
        );
        assert!(game.edict(q2_shared::constants::MAX_EDICTS).is_none());
    }

    #[test]
    fn num_edicts_is_one_past_the_highest_number_the_game_used() {
        let mut world = world_with_player();
        let before = GameExport::num_edicts(&world);
        let ent = world.spawn().unwrap();
        let number = world.entities.get(ent).unwrap().state.number as usize;
        assert_eq!(GameExport::num_edicts(&world), number + 1);
        assert!(number + 1 > before);
        // C never lowers num_edicts when an edict is freed.
        world.free_entity(ent);
        assert_eq!(GameExport::num_edicts(&world), number + 1);
    }

    #[test]
    fn the_server_reads_a_clients_player_state_and_nothing_for_other_numbers() {
        let mut world = world_with_player();
        client_mut(&mut world).ps.fov = 75.0;
        let game: &dyn GameExport = &world;
        assert_eq!(game.client_player_state(1).map(|ps| ps.fov), Some(75.0));
        assert!(
            game.client_player_state(0).is_none(),
            "the world has no client"
        );
        assert!(game.client_player_state(2).is_none());
    }

    #[test]
    fn run_frame_through_the_trait_advances_the_level() {
        let (mut world, _gi) = test_world_recording();
        world.init();
        let game: &mut dyn GameExport = &mut world;
        game.run_frame();
        game.run_frame();
        assert_eq!(world.level.framenum, 2);
    }
}
