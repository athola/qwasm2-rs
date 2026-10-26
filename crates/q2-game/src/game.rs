//! `GameExport` for `GameWorld`: the interface the server drives.
//!
//! C ref: `game_export_t` (game.h) filled in by `GetGameAPI` (g_main.c).
//! Client calls take the client's entity number (`slot + 1`), as C passes
//! `EDICT_NUM(slot + 1)`.

use crate::traits::GameExport;
use crate::world::GameWorld;
use q2_shared::{constants::MAX_CLIENTS, types::UserCmd};

impl GameWorld {
    /// Set `game.maxclients` (clamped to `MAX_CLIENTS`, as the server clamps
    /// its cvar) and reserve entity numbers `1..=maxclients` for clients.
    pub fn set_max_clients(&mut self, max_clients: usize) {
        let max_clients = max_clients.min(MAX_CLIENTS);
        self.game.maxclients = max_clients as i32;
        self.entities.set_max_clients(max_clients);
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
        // After a map change the new map's entities start empty (C keeps the
        // client in game.clients and rebuilds the edict in ClientBegin).
        let key = match self.entities.key_for_number(number) {
            Some(key) if self.entities.get(key).is_some_and(|e| e.in_use) => Some(key),
            _ => self.connect_client(number, ""),
        };
        if let Some(key) = key {
            self.begin_client(key);
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

    fn server_command(&mut self) {}
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
        assert_eq!(ent.state.origin, q2_shared::Vec3f::new(64.0, 0.0, 24.0));
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
