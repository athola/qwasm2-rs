//! Server initialization and map loading.
//!
//! Rust equivalent of `SV_Map()` / `SV_SpawnServer()` / `SV_InitGame()` from
//! `sv_init.c` (C reference: Qwasm2/src/server/sv_init.c).

use q2_common::error::Q2Result;
use q2_game::traits::{GameExport, GameImport};
use q2_shared::constants::*;
use q2_shared::types::EntityState;

use crate::state::{Server, ServerClient, ServerState, ServerStatic};
use crate::ServerGameImport;

impl Server {
    /// Load a map and spawn it (C `SV_SpawnServer`):
    ///
    /// 1. Reset frame counters, configstrings and baselines.
    /// 2. Load `bsp` into the import's collision map and area tree; an empty
    ///    `bsp` is C's no-map case (cinematic servers).
    /// 3. Register the world as model 1 and each inline model `*n` as model
    ///    `n + 1`, and record the map checksum.
    /// 4. Let the game spawn the map's entities and run two frames so
    ///    everything settles before clients enter.
    ///
    /// A map that fails to load leaves the server `Dead`, as C's ERR_DROP
    /// shuts the server down.
    pub fn map(
        &mut self,
        svs: &mut ServerStatic,
        map_name: &str,
        bsp: &[u8],
        gi: &ServerGameImport,
        game: &mut dyn GameExport,
    ) -> Q2Result<()> {
        // Transition into loading state.
        self.state = ServerState::Loading;
        self.name = map_name.to_string();
        self.framenum = 0;
        // C SV_SpawnServer: realtime restarts and the first game frame is
        // due at 1000 ms (sv_init.c:210, :244).
        self.time = 1000;
        svs.realtime = 0;

        // Reset model list (slot 0 is unused; slot 1 will be the world model).
        self.models.clear();
        self.models.resize(MAX_MODELS, String::new());

        // Fresh configstrings and baselines: C SV_SpawnServer zeroes all of
        // `sv` (memset), so nothing from the previous map survives.
        self.configstrings = vec![String::new(); MAX_CONFIGSTRINGS];
        self.configstrings[CS_NAME] = map_name.to_string();
        self.baselines = vec![EntityState::default(); MAX_EDICTS];

        // Demote any spawned clients back to connected so they will
        // re-enter via ClientBegin after the map loads.
        for client in &mut svs.clients {
            if client.state == crate::state::ClientState::Spawned {
                client.state = crate::state::ClientState::Connected;
            }
            client.last_frame = -1;
        }

        let checksum = match gi.load_map(bsp) {
            Ok(checksum) => checksum,
            Err(e) => {
                self.state = ServerState::Dead;
                return Err(e);
            }
        };
        if !bsp.is_empty() {
            gi.configstring((CS_MODELS + 1) as i32, &format!("maps/{map_name}.bsp"));
        }
        // C formats the checksum with %i.
        gi.configstring(CS_MAPCHECKSUM as i32, &(checksum as i32).to_string());
        for i in 1..gi.num_inline_models() {
            gi.configstring((CS_MODELS + 1 + i) as i32, &format!("*{i}"));
        }

        game.spawn_entities(map_name, &gi.entity_string(), "");
        game.run_frame();
        game.run_frame();

        // The game wrote configstrings through the import's mirror.
        for (index, value) in gi.drain_configstring_updates() {
            if let Some(slot) = self.configstrings.get_mut(index) {
                *slot = value;
            }
        }
        // TODO(#10): SV_CreateBaseline once client messages are sent.

        // All precaches complete — go live.
        self.state = ServerState::Game;
        Ok(())
    }
}

impl ServerStatic {
    /// Server (re)initialization (corresponds to `SV_InitGame()`).
    ///
    /// Allocates client slots and marks the server as initialized. Like C,
    /// calling it on an initialized server restarts it: every client slot is
    /// reallocated (connected clients are dropped) with the new count.
    /// `max_clients` is clamped to `MAX_CLIENTS` (sv_init.c:417).
    pub fn init_game(&mut self, max_clients: usize) {
        let max_clients = max_clients.min(MAX_CLIENTS);
        self.max_clients = max_clients;
        self.clients = (0..max_clients)
            .map(|i| ServerClient {
                edict_num: i + 1, // 1-based; entity 0 is the world.
                ..ServerClient::default()
            })
            .collect();
        self.initialized = true;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::NullGame;

    #[test]
    fn server_map_init() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(4);
        svs.realtime = 5000;

        sv.map(
            &mut svs,
            "base1",
            &[],
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .expect("map() should succeed");

        assert_eq!(sv.state, ServerState::Game);
        assert_eq!(sv.name, "base1");
        assert_eq!(sv.framenum, 0);
        assert_eq!(sv.time, 1000, "first game frame is due at 1000 ms");
        assert_eq!(svs.realtime, 0, "realtime restarts with the map");
        assert_eq!(sv.configstrings[CS_NAME], "base1");
        assert_eq!(sv.configstrings.len(), MAX_CONFIGSTRINGS);
        assert_eq!(sv.baselines.len(), MAX_EDICTS);
        assert_eq!(sv.models.len(), MAX_MODELS);
    }

    #[test]
    fn map_change_clears_previous_map_configstrings_and_baselines() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(1);

        sv.map(
            &mut svs,
            "a",
            &[],
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .unwrap();
        sv.configstrings[CS_MODELS + 1] = "maps/a.bsp".into();
        sv.configstrings[CS_SOUNDS + 3] = "world/amb1.wav".into();
        sv.baselines[5].modelindex = 7;

        sv.map(
            &mut svs,
            "b",
            &[],
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .unwrap();

        assert_eq!(sv.configstrings[CS_NAME], "b");
        // Every map writes its own name and checksum; nothing else survives.
        for (i, cs) in sv.configstrings.iter().enumerate() {
            if i != CS_NAME && i != CS_MAPCHECKSUM {
                assert!(
                    cs.is_empty(),
                    "configstring {i} leaked from previous map: {cs:?}"
                );
            }
        }
        assert_eq!(sv.baselines.len(), MAX_EDICTS);
        assert_eq!(
            sv.baselines[5].modelindex, 0,
            "baseline leaked from previous map"
        );
    }

    #[test]
    fn server_state_transitions() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(1);

        // Starts Dead.
        assert_eq!(sv.state, ServerState::Dead);

        // map() transitions Dead -> Loading -> Game.
        sv.map(
            &mut svs,
            "test_map",
            &[],
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .unwrap();
        assert_eq!(sv.state, ServerState::Game);
    }

    #[test]
    fn init_game_allocates_clients() {
        let mut svs = ServerStatic::default();
        svs.init_game(8);

        assert!(svs.initialized);
        assert_eq!(svs.max_clients, 8);
        assert_eq!(svs.clients.len(), 8);

        // Edict numbers are 1-based.
        for (i, cl) in svs.clients.iter().enumerate() {
            assert_eq!(cl.edict_num, i + 1);
        }
    }

    #[test]
    fn init_game_second_call_restarts_with_new_client_count() {
        let mut svs = ServerStatic::default();
        svs.init_game(4);
        svs.clients[0].state = crate::state::ClientState::Spawned;

        // C SV_InitGame on an initialized server shuts it down and
        // reallocates the client slots from the current maxclients.
        svs.init_game(16);

        assert!(svs.initialized);
        assert_eq!(svs.max_clients, 16);
        assert_eq!(svs.clients.len(), 16);
        assert!(svs
            .clients
            .iter()
            .all(|c| c.state == crate::state::ClientState::Free));
    }

    #[test]
    fn init_game_clamps_client_count_to_max_clients() {
        let mut svs = ServerStatic::default();
        svs.init_game(MAX_CLIENTS + 1);

        assert_eq!(svs.max_clients, MAX_CLIENTS);
        assert_eq!(svs.clients.len(), MAX_CLIENTS);
    }

    #[test]
    fn map_demotes_spawned_clients() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(2);

        svs.clients[0].state = crate::state::ClientState::Spawned;
        svs.clients[1].state = crate::state::ClientState::Connected;

        sv.map(
            &mut svs,
            "base2",
            &[],
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .unwrap();

        // Spawned -> Connected; Connected stays Connected.
        assert_eq!(svs.clients[0].state, crate::state::ClientState::Connected);
        assert_eq!(svs.clients[1].state, crate::state::ClientState::Connected);
    }
}
