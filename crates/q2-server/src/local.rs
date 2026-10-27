//! A server and game in one process with one local client: C's listen
//! server without the network. The client's commands go straight to
//! `ClientThink`, so there is no prediction to do.

use std::rc::Rc;

use q2_common::error::Q2Result;
use q2_game::traits::GameExport;
use q2_game::world::GameWorld;
use q2_shared::constants::MAX_EDICTS;
use q2_shared::constants::SVF_NOCLIENT;
use q2_shared::types::{EntityState, PlayerState, UserCmd, Vec3f};

use crate::state::{Server, ServerStatic};
use crate::ServerGameImport;

/// Entity number of the local client (client slot 0).
pub const LOCAL_PLAYER: usize = 1;

pub struct LocalGame {
    sv: Server,
    svs: ServerStatic,
    gi: Rc<ServerGameImport>,
    game: GameWorld,
}

impl LocalGame {
    /// Load `bsp` as `maps/<map_name>.bsp`, spawn it, and put the local
    /// client in the game at the player start.
    pub fn start(map_name: &str, bsp: &[u8]) -> Q2Result<Self> {
        let gi = Rc::new(ServerGameImport::new());
        let mut game = GameWorld::new(gi.clone(), MAX_EDICTS);
        game.set_max_clients(1);
        game.init();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        let mut sv = Server::default();
        sv.map(&mut svs, map_name, bsp, &gi, &mut game)?;
        game.client_connect(LOCAL_PLAYER, "");
        game.client_begin(LOCAL_PLAYER);
        Ok(Self { sv, svs, gi, game })
    }

    /// Apply one user command to the local player (C ClientThink).
    pub fn think(&mut self, cmd: &UserCmd) {
        self.game.client_think(LOCAL_PLAYER, cmd);
    }

    /// Advance the server by `frametime_us`; game frames run at 10 Hz.
    pub fn frame(&mut self, frametime_us: u32) {
        self.sv
            .frame(&mut self.svs, frametime_us, &mut self.game, &self.gi);
    }

    /// The local player's origin and player state, if it is in the game.
    pub fn player(&self) -> Option<(Vec3f, PlayerState)> {
        let key = self.game.entities.key_for_number(LOCAL_PLAYER)?;
        let ent = self.game.entities.get(key)?;
        Some((ent.state.origin, ent.client.as_ref()?.ps.clone()))
    }

    /// The entities a client is sent this frame, by number. C ref:
    /// `SV_BuildClientFrame`'s filter; without vis data every entity is in
    /// the PVS.
    pub fn entity_states(&self) -> Vec<EntityState> {
        let mut states: Vec<EntityState> = self
            .game
            .entities
            .iter()
            .map(|(_, e)| e)
            .filter(|e| e.in_use && e.state.number > 0 && e.svflags & SVF_NOCLIENT == 0)
            // Ignore entities that have nothing to show or play.
            .filter(|e| {
                let s = &e.state;
                s.modelindex != 0 || s.effects != 0 || s.sound != 0 || s.event != 0
            })
            .map(|e| e.state.clone())
            .collect();
        states.sort_by_key(|s| s.number);
        states
    }

    /// The server's configstring `index` (model names live under
    /// `CS_MODELS`).
    pub fn configstring(&self, index: usize) -> &str {
        self.sv.configstrings.get(index).map_or("", String::as_str)
    }

    /// The game, for inspection.
    pub fn game(&self) -> &GameWorld {
        &self.game
    }
}
