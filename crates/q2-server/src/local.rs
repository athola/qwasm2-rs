//! A server and game in one process with one local client: C's listen
//! server without the network. The client's commands go straight to
//! `ClientThink`, so there is no prediction to do.
//!
//! Like C's server, `LocalGame` sees the game only through its export
//! (`GameExport`, C `ge`): it reads entity and player state through the
//! export's `ge->edicts` surface. `GameWorld` is named once, where `start`
//! creates the game (C `SV_InitGameProgs` loading the game library).

use std::cell::RefCell;
use std::rc::Rc;

use q2_common::cmd::CmdSystem;
use q2_common::error::{Q2Error, Q2Result};
use q2_game::traits::GameExport;
use q2_game::world::GameWorld;
use q2_shared::constants::MAX_EDICTS;
use q2_shared::types::{EntityState, PlayerState, UserCmd, Vec3f};

use crate::ents::build_client_frame;
use crate::init::{LevelKind, MapRequest};
use crate::state::{Server, ServerState, ServerStatic};
use crate::ServerGameImport;

/// Entity number of the local client (client slot 0).
pub const LOCAL_PLAYER: usize = 1;

pub struct LocalGame {
    sv: Server,
    svs: ServerStatic,
    gi: Rc<ServerGameImport>,
    /// The game, kept across map changes as C keeps the game library
    /// loaded: its `game.clients` carry the players' inventory over.
    game: Box<dyn GameExport>,
    /// Frame time not yet handed to the server: under a millisecond, which
    /// `SV_Frame`'s `usec / 1000` would drop.
    pending_us: u32,
    /// Runs the console text the game queues (C `Cbuf_Execute`); only
    /// `gamemap` is registered.
    cmd: CmdSystem,
    /// Map a `gamemap` command asked for, until the caller takes it.
    next_map: Rc<RefCell<Option<String>>>,
}

impl LocalGame {
    /// Load `bsp` as `maps/<map_name>.bsp`, spawn it, and put the local
    /// client in the game at the player start.
    pub fn start(map_name: &str, bsp: &[u8]) -> Q2Result<Self> {
        Self::start_with_userinfo(map_name, bsp, "")
    }

    /// `start` with the local client's `userinfo` (C `hand` and the like).
    pub fn start_with_userinfo(map_name: &str, bsp: &[u8], userinfo: &str) -> Q2Result<Self> {
        let gi = Rc::new(ServerGameImport::new());
        let mut world = GameWorld::new(gi.clone(), MAX_EDICTS);
        world.set_max_clients(1);
        let mut game: Box<dyn GameExport> = Box::new(world);
        game.init();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        let mut sv = Server::default();
        sv.map(&mut svs, map_name, bsp, "", &gi, game.as_mut())?;
        svs.connect_local_client(game.as_mut(), userinfo)?;
        let next_map = Rc::new(RefCell::new(None));
        let mut cmd = CmdSystem::new();
        let requested = next_map.clone();
        cmd.add_command("gamemap", move |args: &[&str]| {
            if let Some(map) = args.first() {
                *requested.borrow_mut() = Some((*map).to_string());
            }
        });
        Ok(Self {
            sv,
            svs,
            gi,
            game,
            pending_us: 0,
            cmd,
            next_map,
        })
    }

    /// Run a console command as the local player's (C
    /// `SV_ExecuteUserCommand` for a command the server does not handle
    /// itself): `use Shotgun`, `weapnext`, `invdrop`, ...
    pub fn client_command(&mut self, text: &str) {
        self.gi.set_client_command(text);
        self.game.client_command(LOCAL_PLAYER);
    }

    /// Apply one user command to the local player (C ClientThink).
    pub fn think(&mut self, cmd: &UserCmd) {
        self.game.client_think(LOCAL_PLAYER, cmd);
    }

    /// Advance the server by `frametime_us`; game frames run at 10 Hz.
    ///
    /// C accumulates whole microseconds between server frames
    /// (`servertimedelta`) and `SV_Frame` truncates to milliseconds once per
    /// packet frame. This is called every rendered frame, where that
    /// truncation would slow the game clock by up to 14% at 144 Hz, so the
    /// sub-millisecond remainder is carried to the next call instead.
    pub fn frame(&mut self, frametime_us: u32) {
        let total = self.pending_us.saturating_add(frametime_us);
        self.pending_us = total % 1000;
        self.sv.frame(
            &mut self.svs,
            total - self.pending_us,
            self.game.as_mut(),
            &self.gi,
        );
        let commands = self.gi.take_commands();
        if !commands.is_empty() {
            self.cmd.cbuf_add_text(&commands);
            self.cmd.cbuf_execute();
        }
    }

    /// The level string a game `gamemap` command (C `ExitLevel`) asked
    /// for, once. Reading the new map's BSP is the caller's job; it then
    /// calls `change_map` with it.
    pub fn take_map_change(&mut self) -> Option<String> {
        self.next_map.borrow_mut().take()
    }

    /// The sounds the game started since the last call (C `svc_sound`
    /// messages). The local client hears them all; C's PHS culling and
    /// reliable routing only decide which network client is sent one.
    pub fn take_sounds(&self) -> Vec<crate::game_iface::SoundEvent> {
        self.gi.take_sounds()
    }

    /// The messages the game multicast since the last call (C
    /// `svc_muzzleflash`, `svc_temp_entity`, ...), each whole, in order.
    pub fn take_messages(&self) -> Vec<Vec<u8>> {
        self.gi.take_messages()
    }

    /// Load `level`'s map from `bsp` into the same game and put the local
    /// client back in at the level's spawnpoint (C `SV_GameMap_f` ->
    /// `SV_Map` -> `SV_SpawnServer`, then the client's `begin`). The
    /// game's `SpawnEntities` saves the player's persistent data and
    /// `ClientBegin` restores it, so the inventory carries over.
    ///
    /// A picture level (`.pcx`) needs no `bsp`: the server shows it with no
    /// map and no player (`Server::pic`) until the next map change, and the
    /// game keeps the player's persistent data meanwhile. Cinematic and
    /// demo levels cannot be played back and are refused with
    /// `Q2Error::Drop`, leaving the current map running.
    pub fn change_map(&mut self, level: &MapRequest, bsp: &[u8]) -> Q2Result<()> {
        match level.kind {
            LevelKind::Game => self.sv.map(
                &mut self.svs,
                &level.map,
                bsp,
                &level.spawnpoint,
                &self.gi,
                self.game.as_mut(),
            )?,
            LevelKind::Pic => self.sv.pic(
                &mut self.svs,
                &level.map,
                &level.spawnpoint,
                &self.gi,
                self.game.as_mut(),
            )?,
            LevelKind::Cinematic | LevelKind::Demo => {
                return Err(Q2Error::Drop(format!(
                    "{}: cinematic and demo playback is not supported",
                    level.map
                )));
            }
        }
        // C SV_SpawnServer resets `svs.realtime`; the old map's leftover
        // sub-millisecond time goes with it.
        self.pending_us = 0;
        if level.kind == LevelKind::Game {
            self.svs.begin_connected_clients(self.game.as_mut());
        }
        Ok(())
    }

    /// The picture level being shown (e.g. `victory.pcx`, drawn from
    /// `pics/<name>`), or `None` in a game map. C's client draws it full
    /// screen and sends `nextserver` on a key press, which `SV_Nextserver`
    /// ignores for `ss_pic` outside coop: in single player the picture
    /// stays until the player leaves.
    pub fn picture(&self) -> Option<&str> {
        (self.sv.state == ServerState::Pic).then_some(self.sv.name.as_str())
    }

    /// The server frame number (C `sv.framenum`, the client's
    /// `cl.frame.serverframe`); it counts from 0 at the map load.
    pub fn framenum(&self) -> i32 {
        self.sv.framenum
    }

    /// The local player's origin and player state, if it is in the game.
    pub fn player(&self) -> Option<(Vec3f, &PlayerState)> {
        let origin = self.game.edict(LOCAL_PLAYER)?.state.origin;
        Some((origin, self.game.client_player_state(LOCAL_PLAYER)?))
    }

    /// The entities the local client is sent this frame, by number (C
    /// `SV_BuildClientFrame`, see `ents::build_client_frame`). Nothing is
    /// sent while the player is not in the game.
    pub fn entity_states(&self) -> Vec<EntityState> {
        let Some(eye) = self.eye() else {
            return Vec::new();
        };
        let game = self.game.as_ref();
        let entities = (1..game.num_edicts()).filter_map(|number| game.edict(number));
        build_client_frame(&self.gi, LOCAL_PLAYER, eye, entities)
    }

    /// The local player's eye (C `SV_BuildClientFrame`'s `org`).
    fn eye(&self) -> Option<Vec3f> {
        let (_, ps) = self.player()?;
        let origin = Vec3f::from_array(ps.pmove.origin.map(|c| f32::from(c) * 0.125));
        Some(origin + ps.viewoffset)
    }

    /// The area holding the local player's eye.
    pub fn player_area(&self) -> Option<i32> {
        Some(self.gi.point_area(self.eye()?))
    }

    /// Which areas the local player can see this frame, for the renderer's
    /// `RefDef::areabits` (C `frame->areabits`); `None` draws them all.
    pub fn area_bits(&self) -> Option<Vec<u8>> {
        self.gi.area_bits(self.player_area()?)
    }

    /// One past the highest entity number the game has used (C
    /// `ge->num_edicts`).
    pub fn num_edicts(&self) -> usize {
        self.game.num_edicts()
    }

    /// The server's configstring `index` (model names live under
    /// `CS_MODELS`).
    pub fn configstring(&self, index: usize) -> &str {
        self.sv.configstrings.get(index).map_or("", String::as_str)
    }
}
