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
use crate::state::{ClientState, Server, ServerState, ServerStatic};
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
    /// The entities sent to the local client in the last frame (C
    /// `SV_BuildClientFrame` into `client->frames`).
    client_frame: Vec<EntityState>,
    /// The messages and sounds sent with the last server frame and not yet
    /// taken (C `client->netchan.message` and `client->datagram` as
    /// `SV_SendClientMessages` sends them).
    sent_messages: Vec<Vec<u8>>,
    sent_sounds: Vec<crate::game_iface::SoundEvent>,
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
        gi.set_client_states(svs.clients.iter().map(|client| client.state));
        let next_map = Rc::new(RefCell::new(None));
        let mut cmd = CmdSystem::new();
        let requested = next_map.clone();
        cmd.add_command("gamemap", move |args: &[&str]| {
            if let Some(map) = args.first() {
                *requested.borrow_mut() = Some((*map).to_string());
            }
        });
        let mut local = Self {
            sv,
            svs,
            gi,
            game,
            pending_us: 0,
            cmd,
            next_map,
            client_frame: Vec::new(),
            sent_messages: Vec::new(),
            sent_sounds: Vec::new(),
        };
        local.send_client_frame();
        Ok(local)
    }

    /// Run a console command as the local player's (C
    /// `SV_ExecuteUserCommand` for a command the server does not handle
    /// itself): `use Shotgun`, `weapnext`, `invdrop`, ... C passes it to
    /// the game only in a game map (`sv.state == ss_game`), so a picture
    /// level ignores it.
    pub fn client_command(&mut self, text: &str) {
        if self.sv.state != ServerState::Game {
            return;
        }
        self.gi.set_client_command(text);
        self.game.client_command(LOCAL_PLAYER);
    }

    /// The local wall clock (hours, minutes, seconds) the game's
    /// time-of-day `func_clock` shows.
    pub fn set_local_time(&self, hms: Option<(u32, u32, u32)>) {
        self.gi.set_local_time(hms);
    }

    /// Set a cvar the game reads (C shares one cvar table between the
    /// client and the game in a listen server).
    pub fn set_cvar(&self, name: &str, value: &str) {
        self.gi.set_cvar(name, value);
    }

    /// Build the frame the local client is sent, then clear the entity
    /// events it carries (C `SV_SendClientMessages` then
    /// `SV_PrepWorldFrame`): an event is sent in one frame only. Events set
    /// between game frames, by `think`, wait for the next one.
    ///
    /// C sends no frame after `SV_SpawnServer`'s settling frames, so their
    /// events go out in the first game frame; here the frame built at the
    /// load carries them, so the client still sees each event once.
    fn send_client_frame(&mut self) {
        self.client_frame = match player_eye(self.game.as_ref()) {
            Some(eye) => {
                let game = self.game.as_ref();
                let entities = (1..game.num_edicts()).filter_map(|number| game.edict(number));
                build_client_frame(&self.gi, LOCAL_PLAYER, eye, entities)
            }
            None => Vec::new(),
        };
        self.game.clear_entity_events();
    }

    /// Apply one user command to the local player (C ClientThink). Like C
    /// `SV_ExecuteClientMessage`, the command is dropped unless the client
    /// is in the game (`cs_spawned`): a picture level or a failed map load
    /// leaves it only connected.
    pub fn think(&mut self, cmd: &UserCmd) {
        let spawned = self
            .svs
            .clients
            .first()
            .is_some_and(|client| client.state == ClientState::Spawned);
        if spawned {
            self.game.client_think(LOCAL_PLAYER, cmd);
        }
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
        let framenum = self.sv.framenum;
        self.sv.frame(
            &mut self.svs,
            total - self.pending_us,
            self.game.as_mut(),
            &self.gi,
        );
        if self.sv.framenum != framenum {
            self.send_client_frame();
            // C SV_SendClientMessages sends what ClientThink and the game
            // frame queued only now, after the game frame: a muzzle flash
            // from a command run between frames arrives with the next one.
            // What the map load's settling frames queued goes out with the
            // first game frame.
            self.sent_messages.extend(self.gi.take_messages());
            self.sent_sounds.extend(self.gi.take_sounds());
        }
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

    /// The sounds the game started before the server frames run since the
    /// last call (C `svc_sound` messages, sent by `SV_SendClientMessages`
    /// after each game frame). The local client hears them all; C's PHS
    /// culling and reliable routing only decide which network client is
    /// sent one.
    pub fn take_sounds(&mut self) -> Vec<crate::game_iface::SoundEvent> {
        std::mem::take(&mut self.sent_sounds)
    }

    /// The messages the game multicast or printed before the server frames
    /// run since the last call (C `svc_muzzleflash`, `svc_temp_entity`,
    /// `svc_print`, ...), each whole, in order. C ref: `SV_Frame` returns
    /// before `SV_SendClientMessages` until a game frame is due, so what
    /// `think` and `client_command` queue waits for the next game frame.
    pub fn take_messages(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.sent_messages)
    }

    /// Load `level`'s map from `bsp` into the same game and put the local
    /// client back in at the level's spawnpoint (C `SV_GameMap_f` ->
    /// `SV_Map` -> `SV_SpawnServer`, then the client's `begin`). The
    /// game's `SpawnEntities` saves the player's persistent data and
    /// `ClientBegin` restores it, so the inventory carries over.
    ///
    /// A picture level (`.pcx`) needs no `bsp`: the server shows it with no
    /// map and no player (`Server::pic`) until the next map change, and the
    /// game keeps the player's persistent data meanwhile.
    ///
    /// Cinematic and demo levels cannot be played back. One that names a
    /// next server (`intro.cin+base1`) goes straight to it: C plays the
    /// movie and then `SV_Nextserver` runs `gamemap "<next>"`, so the next
    /// level is handed out by `take_map_change` and the current map runs
    /// until the caller follows it. One without is refused with
    /// `Q2Error::Drop`, leaving the current map running where C would end
    /// the server (`killserver`) after the movie.
    pub fn change_map(&mut self, level: &MapRequest, bsp: &[u8]) -> Q2Result<()> {
        if matches!(level.kind, LevelKind::Game | LevelKind::Pic) {
            // C's client parses the old level's last messages before the
            // new `svc_serverdata`, and CL_ClearState then clears the
            // effects they started (CL_ClearEffects, CL_ClearTEnts) and
            // S_StopAllSounds their sounds: none reach the new level.
            self.sent_messages.clear();
            self.sent_sounds.clear();
            self.gi.take_messages();
            self.gi.take_sounds();
        }
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
                if let Some(next) = &level.next_server {
                    *self.next_map.borrow_mut() = Some(next.clone());
                    return Ok(());
                }
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
            self.gi
                .set_client_states(self.svs.clients.iter().map(|client| client.state));
        }
        self.send_client_frame();
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

    /// The entities the local client was sent in the last game frame (or
    /// at the map load), by number (C `SV_BuildClientFrame`, see
    /// `ents::build_client_frame`). Nothing is sent while the player is not
    /// in the game.
    pub fn entity_states(&self) -> Vec<EntityState> {
        self.client_frame.clone()
    }

    /// The area holding the local player's eye.
    pub fn player_area(&self) -> Option<i32> {
        Some(self.gi.point_area(player_eye(self.game.as_ref())?))
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

/// The local player's eye (C `SV_BuildClientFrame`'s `org`), if it is in
/// the game.
fn player_eye(game: &dyn GameExport) -> Option<Vec3f> {
    game.edict(LOCAL_PLAYER)?;
    let ps = game.client_player_state(LOCAL_PLAYER)?;
    let origin = Vec3f::from_array(ps.pmove.origin.map(|c| f32::from(c) * 0.125));
    Some(origin + ps.viewoffset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::NullGame;
    use q2_game::traits::GameImport;
    use std::cell::Cell;

    /// A local game whose client slot is in `client` state, and the counts
    /// of `client_think` and `client_command` calls that reach the game.
    fn local_with_client(client: ClientState) -> (LocalGame, Rc<Cell<u32>>, Rc<Cell<u32>>) {
        let game = NullGame::default();
        let thinks = game.thinks.clone();
        let commands = game.commands.clone();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        svs.clients[0].state = client;
        let local = LocalGame {
            sv: Server {
                state: ServerState::Game,
                ..Default::default()
            },
            svs,
            gi: Rc::new(ServerGameImport::new()),
            game: Box::new(game),
            pending_us: 0,
            cmd: CmdSystem::new(),
            next_map: Rc::default(),
            client_frame: Vec::new(),
            sent_messages: Vec::new(),
            sent_sounds: Vec::new(),
        };
        (local, thinks, commands)
    }

    #[test]
    fn a_print_to_the_local_player_reaches_the_client_with_the_next_server_frame() {
        // C Cmd_Use_f: `gi.cprintf(ent, PRINT_HIGH, "Out of item: %s\n")`.
        // C SV_Frame sends client messages only after a game frame.
        let mut local = LocalGame::start("empty", &[]).expect("empty map loads");
        local.frame(100_000);
        local.take_messages();
        local.client_command("use Shotgun");
        assert!(local.take_messages().is_empty(), "no server frame yet");
        local.frame(50_000);
        assert!(
            local.take_messages().is_empty(),
            "the game frame is not due"
        );
        local.frame(50_000);
        let mut print = vec![q2_shared::protocol::SvcOp::Print as u8, 2];
        print.extend_from_slice(b"Out of item: Shotgun\n\0");
        assert_eq!(local.take_messages(), vec![print]);
    }

    #[test]
    fn a_sound_started_between_server_frames_waits_for_the_next_one() {
        // C SV_StartSound writes into the client's datagram, which
        // SV_SendClientMessages sends after the next game frame.
        let mut local = LocalGame::start("empty", &[]).expect("empty map loads");
        local.frame(100_000);
        local.take_sounds();
        local.gi.sound(Some(LOCAL_PLAYER), 0, 1, 1.0, 1.0, 0.0);
        assert!(local.take_sounds().is_empty(), "no server frame yet");
        local.frame(100_000);
        assert_eq!(local.take_sounds().len(), 1);
    }

    #[test]
    fn messages_from_the_old_level_are_dropped_by_a_map_change() {
        // C's client parses them against the old level, then CL_ClearState
        // clears the effects they started; nothing reaches the new level.
        let mut local = LocalGame::start("empty", &[]).expect("empty map loads");
        local.frame(100_000);
        local.client_command("use Shotgun");
        local.gi.sound(Some(LOCAL_PLAYER), 0, 1, 1.0, 1.0, 0.0);
        local.frame(100_000);
        local
            .change_map(&MapRequest::parse("victory.pcx"), &[])
            .expect("a picture level loads");
        assert!(local.take_messages().is_empty());
        assert!(local.take_sounds().is_empty());
    }

    #[test]
    fn a_skipped_cinematic_keeps_the_running_maps_messages() {
        // The current map keeps running until the caller follows the next
        // server, so what it sent is still its own.
        let mut local = LocalGame::start("empty", &[]).expect("empty map loads");
        local.frame(100_000);
        local.client_command("use Shotgun");
        local.frame(100_000);
        local
            .change_map(&MapRequest::parse("intro.cin+base1"), &[])
            .expect("the cinematic is skipped");
        assert_eq!(local.take_messages().len(), 1);
    }

    #[test]
    fn a_client_command_reaches_the_game_in_a_game_map() {
        let (mut local, _, commands) = local_with_client(ClientState::Spawned);
        local.client_command("use Shotgun");
        assert_eq!(commands.get(), 1);
    }

    #[test]
    fn a_client_command_on_a_picture_level_does_not_reach_the_game() {
        // C SV_ExecuteUserCommand: `ge->ClientCommand` only in `ss_game`.
        let (mut local, _, commands) = local_with_client(ClientState::Connected);
        local.sv.state = ServerState::Pic;
        local.client_command("use Shotgun");
        assert_eq!(commands.get(), 0);
    }

    #[test]
    fn think_runs_client_think_for_a_spawned_client() {
        let (mut local, thinks, _) = local_with_client(ClientState::Spawned);
        local.think(&UserCmd::default());
        assert_eq!(thinks.get(), 1);
    }

    #[test]
    fn think_is_ignored_while_the_client_is_only_connected() {
        // A picture level or a failed map load leaves the client Connected.
        let (mut local, thinks, _) = local_with_client(ClientState::Connected);
        local.think(&UserCmd::default());
        assert_eq!(thinks.get(), 0);
    }

    #[test]
    fn think_is_ignored_for_a_free_client_slot() {
        let (mut local, thinks, _) = local_with_client(ClientState::Free);
        local.think(&UserCmd::default());
        assert_eq!(thinks.get(), 0);
    }
}
