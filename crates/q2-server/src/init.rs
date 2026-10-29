//! Server initialization and map loading.
//!
//! Rust equivalent of `SV_Map()` / `SV_SpawnServer()` / `SV_InitGame()` from
//! `sv_init.c` (C reference: Qwasm2/src/server/sv_init.c).

use q2_common::error::{Q2Error, Q2Result};
use q2_game::traits::{GameExport, GameImport};
use q2_shared::constants::*;
use q2_shared::types::EntityState;

use crate::state::{ClientState, Server, ServerClient, ServerState, ServerStatic};
use crate::ServerGameImport;

impl Server {
    /// Load a map and spawn it (C `SV_SpawnServer`):
    ///
    /// 1. Reset frame counters, configstrings and baselines.
    /// 2. Load `bsp` into the import's collision map and area tree; an empty
    ///    `bsp` is C's no-map case (cinematic servers).
    /// 3. Register the world as model 1 and each inline model `*n` as model
    ///    `n + 1`, and record the map checksum.
    /// 4. Let the game spawn the map's entities, with players starting at
    ///    the `info_player_start` whose targetname is `spawnpoint` ("" for
    ///    the unnamed one), and run two frames so everything settles before
    ///    clients enter, then check the game left the checksum alone.
    ///
    /// A map that fails to load leaves the server `Dead`, as C's ERR_DROP
    /// shuts the server down. There is no deathmatch cvar yet, so
    /// `CS_AIRACCEL` is always C's single-player "0".
    pub fn map(
        &mut self,
        svs: &mut ServerStatic,
        map_name: &str,
        bsp: &[u8],
        spawnpoint: &str,
        gi: &ServerGameImport,
        game: &mut dyn GameExport,
    ) -> Q2Result<()> {
        self.spawn_server(svs, map_name, bsp, spawnpoint, ServerState::Game, gi, game)
    }

    /// Spawn a picture level such as `victory.pcx` (C `SV_SpawnServer`
    /// with `ss_pic`): C loads no real map (`CM_LoadMap("")`) but still
    /// runs the game's `SpawnEntities` on the empty entity string, which
    /// clears the level and keeps `game.clients`, and two settling frames.
    /// Clients are left `Connected`: C `SV_New_f` sends them playernum -1
    /// and no `begin`, so no player enters the game.
    pub fn pic(
        &mut self,
        svs: &mut ServerStatic,
        name: &str,
        spawnpoint: &str,
        gi: &ServerGameImport,
        game: &mut dyn GameExport,
    ) -> Q2Result<()> {
        self.spawn_server(svs, name, &[], spawnpoint, ServerState::Pic, gi, game)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_server(
        &mut self,
        svs: &mut ServerStatic,
        map_name: &str,
        bsp: &[u8],
        spawnpoint: &str,
        serverstate: ServerState,
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

        // Fresh configstrings and baselines: C SV_SpawnServer zeroes all of
        // `sv` (memset), so nothing from the previous map survives.
        self.configstrings = vec![String::new(); MAX_CONFIGSTRINGS];
        self.configstrings[CS_NAME] = map_name.to_string();
        self.configstrings[CS_AIRACCEL] = "0".to_string();
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

        game.spawn_entities(map_name, &gi.entity_string(), spawnpoint);
        game.run_frame();
        game.run_frame();

        self.sync_configstrings(gi);
        // C strtol: a string that is not a number reads as 0.
        if self.configstrings[CS_MAPCHECKSUM]
            .parse::<i32>()
            .unwrap_or(0)
            != checksum as i32
        {
            self.state = ServerState::Dead;
            return Err(Q2Error::Drop(
                "Game DLL corrupted server configstrings".into(),
            ));
        }
        // TODO(#10): SV_CreateBaseline once client messages are sent.

        // All precaches complete — go live.
        self.state = serverstate;
        Ok(())
    }
}

/// What a level string loads (C `SV_Map`'s parsing of its `levelstring`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRequest {
    /// Map (or cinematic, demo or picture) name, without the `*` new-unit
    /// flag, the `$spawnpoint` or the `+` remainder.
    pub map: String,
    /// Target name of the player start to use (after `$`), "" for none.
    pub spawnpoint: String,
    /// What C stores as `nextserver` (`gamemap "<rest>"`) to run once a
    /// cinematic ends: the text after the first `+`.
    pub next_server: Option<String>,
    pub kind: LevelKind,
}

/// The server state `SV_Map` spawns for a level, from its suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelKind {
    Game,
    /// `.cin` (C `ss_cinematic`).
    Cinematic,
    /// `.dm2` (C `ss_demo`).
    Demo,
    /// `.pcx` (C `ss_pic`).
    Pic,
}

impl MapRequest {
    /// C `SV_Map`: cut at the first `+` (the rest becomes `nextserver`),
    /// then at the first `$` (the rest is the spawnpoint), then drop a
    /// leading `*`. A `*` only matters to C's savegames, which this port
    /// does not write. The coop `victory.pcx` hack needs a coop cvar and is
    /// not ported.
    pub fn parse(level: &str) -> Self {
        let (level, next_server) = match level.split_once('+') {
            Some((level, rest)) => (level, Some(rest.to_string())),
            None => (level, None),
        };
        let (level, spawnpoint) = level.split_once('$').unwrap_or((level, ""));
        let map = level.strip_prefix('*').unwrap_or(level);
        let suffix = |ext: &str| map.len() > 4 && map.ends_with(ext);
        let kind = if suffix(".cin") {
            LevelKind::Cinematic
        } else if suffix(".dm2") {
            LevelKind::Demo
        } else if suffix(".pcx") {
            LevelKind::Pic
        } else {
            LevelKind::Game
        };
        Self {
            map: map.to_string(),
            spawnpoint: spawnpoint.to_string(),
            next_server,
            kind,
        }
    }
}

impl Server {
    /// Copy the configstrings the game wrote through the import's mirror
    /// into `configstrings`. In C the game writes `sv.configstrings`
    /// directly, so this runs after every call into the game that can
    /// write them, before anything reads them.
    pub(crate) fn sync_configstrings(&mut self, gi: &ServerGameImport) {
        for (index, value) in gi.drain_configstring_updates() {
            if let Some(slot) = self.configstrings.get_mut(index) {
                *slot = value;
            }
        }
    }
}

impl ServerStatic {
    /// Put an in-process client in the first free slot and into the game:
    /// C `SV_DirectConnect` (slot `Connected` once `ClientConnect`
    /// accepts) then `SV_Begin_f` (`ClientBegin`, slot `Spawned`).
    /// `userinfo` is what C's connect packet carries (the client's
    /// `CVAR_USERINFO` cvars). Returns the client's entity number. A
    /// refused connection is an error and leaves the slot free.
    pub fn connect_local_client(
        &mut self,
        game: &mut dyn GameExport,
        userinfo: &str,
    ) -> Q2Result<usize> {
        let Some(client) = self
            .clients
            .iter_mut()
            .find(|c| c.state == ClientState::Free)
        else {
            return Err(Q2Error::Drop("Server is full.".into()));
        };
        client.userinfo = userinfo.to_string();
        if !game.client_connect(client.edict_num, &client.userinfo) {
            return Err(Q2Error::Drop("Connection refused by the game.".into()));
        }
        client.state = ClientState::Connected;
        game.client_begin(client.edict_num);
        client.state = ClientState::Spawned;
        Ok(client.edict_num)
    }

    /// Put every client a map change left `Connected` back in the game.
    /// In C the server sends "reconnect" and each client answers with
    /// `new` and `begin` on its existing netchan, so only `SV_Begin_f`
    /// (`ClientBegin`) runs: `ClientConnect` would reset the inventory the
    /// game carried over in `game.clients`.
    pub fn begin_connected_clients(&mut self, game: &mut dyn GameExport) {
        for client in &mut self.clients {
            if client.state == ClientState::Connected {
                game.client_begin(client.edict_num);
                client.state = ClientState::Spawned;
            }
        }
    }

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
            "",
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
    }

    #[test]
    fn a_picture_server_loads_no_map_spawns_the_game_and_ends_in_the_pic_state() {
        let gi = ServerGameImport::new();
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        sv.map(&mut svs, "base1", &[], "", &gi, &mut NullGame::default())
            .unwrap();
        svs.connect_local_client(&mut NullGame::default(), "")
            .unwrap();
        sv.pic(&mut svs, "victory.pcx", "", &gi, &mut NullGame::default())
            .expect("C SV_SpawnServer(ss_pic) loads the empty map");
        assert_eq!(sv.state, ServerState::Pic);
        assert_eq!(sv.name, "victory.pcx");
        assert_eq!(sv.configstrings[CS_NAME], "victory.pcx");
        assert_eq!(sv.configstrings[CS_MODELS + 1], "");
        assert_eq!(sv.time, 1000);
        // The client is demoted and stays connected: no begin for ss_pic.
        assert_eq!(svs.clients[0].state, ClientState::Connected);
    }

    #[test]
    fn map_sets_airaccel_off_as_in_single_player() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        sv.map(
            &mut svs,
            "a",
            &[],
            "",
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .unwrap();
        assert_eq!(sv.configstrings[CS_AIRACCEL], "0");
    }

    /// A game that overwrites the map checksum while it spawns entities.
    struct ClobberGame(std::rc::Rc<ServerGameImport>);

    impl GameExport for ClobberGame {
        fn init(&mut self) {}
        fn shutdown(&mut self) {}
        fn spawn_entities(&mut self, _: &str, _: &str, _: &str) {
            self.0.configstring(CS_MAPCHECKSUM as i32, "12345");
        }
        fn client_connect(&mut self, _: usize, _: &str) -> bool {
            true
        }
        fn client_begin(&mut self, _: usize) {}
        fn client_disconnect(&mut self, _: usize) {}
        fn client_command(&mut self, _: usize) {}
        fn client_think(&mut self, _: usize, _: &q2_shared::types::UserCmd) {}
        fn run_frame(&mut self) {}
        fn server_command(&mut self) {}
        fn num_edicts(&self) -> usize {
            0
        }
        fn edict(&self, _: usize) -> Option<q2_game::traits::EdictView<'_>> {
            None
        }
        fn client_player_state(&self, _: usize) -> Option<&q2_shared::types::PlayerState> {
            None
        }
    }

    #[test]
    fn map_fails_when_the_game_overwrites_the_map_checksum() {
        let gi = std::rc::Rc::new(ServerGameImport::new());
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        let result = sv.map(&mut svs, "a", &[], "", &gi, &mut ClobberGame(gi.clone()));
        assert!(
            matches!(result, Err(q2_common::error::Q2Error::Drop(ref m)) if m.contains("corrupted")),
            "{result:?}"
        );
        assert_eq!(sv.state, ServerState::Dead);
    }

    #[test]
    fn connect_local_client_spawns_the_first_free_slot() {
        let mut svs = ServerStatic::default();
        svs.init_game(2);
        let number = svs
            .connect_local_client(&mut NullGame::default(), "")
            .expect("accepted");
        assert_eq!(number, 1);
        assert_eq!(svs.clients[0].state, crate::state::ClientState::Spawned);
        assert_eq!(svs.clients[1].state, crate::state::ClientState::Free);
    }

    #[test]
    fn the_local_clients_userinfo_reaches_clientconnect_and_stays_on_the_slot() {
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        let mut game = NullGame::default();
        svs.connect_local_client(&mut game, "\\hand\\1").unwrap();
        assert_eq!(game.userinfo, "\\hand\\1");
        assert_eq!(svs.clients[0].userinfo, "\\hand\\1");
    }

    #[test]
    fn connect_local_client_fails_and_frees_the_slot_when_the_game_refuses() {
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        let mut game = NullGame {
            refuse_connect: true,
            ..NullGame::default()
        };
        assert!(svs.connect_local_client(&mut game, "").is_err());
        assert_eq!(svs.clients[0].state, crate::state::ClientState::Free);
    }

    #[test]
    fn connect_local_client_fails_when_every_slot_is_taken() {
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        svs.clients[0].state = crate::state::ClientState::Spawned;
        assert!(svs
            .connect_local_client(&mut NullGame::default(), "")
            .is_err());
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
            "",
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
            "",
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .unwrap();

        assert_eq!(sv.configstrings[CS_NAME], "b");
        // Every map writes its own name, checksum and airaccel; nothing
        // else survives.
        for (i, cs) in sv.configstrings.iter().enumerate() {
            if i != CS_NAME && i != CS_MAPCHECKSUM && i != CS_AIRACCEL {
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
    fn map_hands_the_spawnpoint_to_spawn_entities() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        let mut game = NullGame::default();
        sv.map(
            &mut svs,
            "demo2",
            &[],
            "base1",
            &ServerGameImport::new(),
            &mut game,
        )
        .unwrap();
        assert_eq!(game.spawnpoint, "base1");
    }

    #[test]
    fn begin_connected_clients_sends_only_begin_to_clients_the_map_demoted() {
        let mut svs = ServerStatic::default();
        svs.init_game(3);
        let mut game = NullGame::default();
        svs.connect_local_client(&mut game, "").unwrap();
        svs.clients[1].state = crate::state::ClientState::Connected;
        let connects = game.connects;
        let mut sv = Server::default();
        sv.map(&mut svs, "b", &[], "", &ServerGameImport::new(), &mut game)
            .unwrap();
        game.begun.clear();

        svs.begin_connected_clients(&mut game);

        // C: the client answers "reconnect" with `new` and `begin` over the
        // same netchan; SV_DirectConnect (ClientConnect) does not run again.
        assert_eq!(game.connects, connects);
        assert_eq!(game.begun, [1, 2]);
        assert_eq!(svs.clients[0].state, crate::state::ClientState::Spawned);
        assert_eq!(svs.clients[1].state, crate::state::ClientState::Spawned);
        assert_eq!(svs.clients[2].state, crate::state::ClientState::Free);
    }

    #[test]
    fn a_plain_level_is_a_game_map_with_no_spawnpoint() {
        let level = MapRequest::parse("base2");
        assert_eq!(level.map, "base2");
        assert_eq!(level.spawnpoint, "");
        assert_eq!(level.next_server, None);
        assert_eq!(level.kind, LevelKind::Game);
    }

    #[test]
    fn a_dollar_names_the_spawnpoint() {
        let level = MapRequest::parse("demo2$base1");
        assert_eq!(
            (level.map.as_str(), level.spawnpoint.as_str()),
            ("demo2", "base1")
        );
    }

    #[test]
    fn a_leading_star_marks_a_new_unit_and_is_skipped() {
        let level = MapRequest::parse("*base1$start");
        assert_eq!(
            (level.map.as_str(), level.spawnpoint.as_str()),
            ("base1", "start")
        );
    }

    #[test]
    fn a_plus_queues_the_rest_as_the_next_server() {
        // C cuts at the first '+' before looking for '$'.
        let level = MapRequest::parse("ntro.cin+*base1$start");
        assert_eq!(level.map, "ntro.cin");
        assert_eq!(level.spawnpoint, "");
        assert_eq!(level.next_server.as_deref(), Some("*base1$start"));
        assert_eq!(level.kind, LevelKind::Cinematic);
    }

    #[test]
    fn cinematic_demo_and_picture_levels_are_not_game_maps() {
        assert_eq!(MapRequest::parse("end.cin").kind, LevelKind::Cinematic);
        assert_eq!(MapRequest::parse("demo1.dm2").kind, LevelKind::Demo);
        assert_eq!(MapRequest::parse("victory.pcx").kind, LevelKind::Pic);
        // C needs more than the four suffix characters.
        assert_eq!(MapRequest::parse(".cin").kind, LevelKind::Game);
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
            "",
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
            "",
            &ServerGameImport::new(),
            &mut NullGame::default(),
        )
        .unwrap();

        // Spawned -> Connected; Connected stays Connected.
        assert_eq!(svs.clients[0].state, crate::state::ClientState::Connected);
        assert_eq!(svs.clients[1].state, crate::state::ClientState::Connected);
    }
}
