//! Server frame loop.
//!
//! Rust equivalent of `SV_Frame()` and `SV_RunGameFrame()` from
//! `sv_main.c` (C reference: Qwasm2/src/server/sv_main.c).

use q2_game::traits::GameExport;

use crate::state::{Server, ServerState, ServerStatic};
use crate::ServerGameImport;

/// Length of one game frame in milliseconds (10 Hz, C `sv.framenum * 100`).
const FRAME_MSEC: u32 = 100;

impl Server {
    /// Run one server frame (`SV_Frame`).
    ///
    /// `frametime_us` is elapsed wall-clock time in microseconds since the
    /// last call. Game frames run at a fixed 10 Hz: a call runs at most one
    /// game frame, and only once `svs.realtime` has reached `self.time`. The
    /// client derives server time as `framenum * 100` ms, so bumping
    /// `framenum` on every call would run the game at the caller's rate.
    pub fn frame(
        &mut self,
        svs: &mut ServerStatic,
        frametime_us: u32,
        game: &mut dyn GameExport,
        gi: &ServerGameImport,
    ) {
        if self.state == ServerState::Dead {
            return;
        }

        svs.realtime = svs.realtime.wrapping_add(frametime_us / 1000);

        // TODO(#10): SV_CheckTimeouts — drop clients that haven't sent packets.
        // TODO(#10): SV_ReadPackets   — process incoming client packets.

        // Not time for the next game frame yet.
        if svs.realtime < self.time {
            // Never let realtime fall more than one frame behind.
            if self.time - svs.realtime > FRAME_MSEC {
                svs.realtime = self.time - FRAME_MSEC;
            }
            return;
        }

        // TODO(#10): SV_CalcPings     — update per-client ping estimates.
        // TODO(#10): SV_GiveMsec      — refresh command-millisecond budgets.

        self.run_game_frame(svs, game, gi);

        // TODO(#10): SV_SendClientMessages — push state to connected clients.
        // SV_PrepWorldFrame clears the entity events once a frame is sent,
        // so it runs after the send: `LocalGame::frame` sends the local
        // client's frame and then calls `GameExport::clear_entity_events`.
    }

    /// Advance one 100 ms game tick (`SV_RunGameFrame`).
    fn run_game_frame(
        &mut self,
        svs: &mut ServerStatic,
        game: &mut dyn GameExport,
        gi: &ServerGameImport,
    ) {
        // framenum must always increment — delta compression depends on it.
        self.framenum += 1;
        self.time = (self.framenum as u32).wrapping_mul(FRAME_MSEC);

        game.run_frame();

        self.sync_configstrings(gi);

        // Never get more than one tick behind.
        if self.time < svs.realtime {
            svs.realtime = self.time;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::NullGame;
    use q2_game::traits::GameImport;
    use q2_shared::constants::{CS_MAXCLIENTS, CS_SKY, MAX_CONFIGSTRINGS};

    fn running_server() -> (Server, ServerStatic, NullGame) {
        let sv = Server {
            state: ServerState::Game,
            ..Default::default()
        };
        let svs = ServerStatic {
            initialized: true,
            ..Default::default()
        };
        (sv, svs, NullGame::default())
    }

    #[test]
    fn game_frame_copies_game_written_configstrings_into_server() {
        let gi = ServerGameImport::new();
        let (mut sv, mut svs, mut game) = running_server();
        sv.configstrings = vec![String::new(); MAX_CONFIGSTRINGS];
        gi.configstring(CS_MAXCLIENTS as i32, "4");
        gi.configstring(CS_SKY as i32, "unit1_");

        sv.frame(&mut svs, 100_000, &mut game, &gi);

        assert_eq!(sv.configstrings[CS_MAXCLIENTS], "4");
        assert_eq!(sv.configstrings[CS_SKY], "unit1_");
        assert!(
            gi.drain_configstring_updates().is_empty(),
            "updates consumed"
        );
    }

    #[test]
    fn configstring_updates_wait_until_a_game_frame_runs() {
        let gi = ServerGameImport::new();
        let (mut sv, mut svs, mut game) = running_server();
        sv.configstrings = vec![String::new(); MAX_CONFIGSTRINGS];
        sv.time = 1000;
        gi.configstring(CS_SKY as i32, "unit1_");

        sv.frame(&mut svs, 16_000, &mut game, &gi);
        assert!(sv.configstrings[CS_SKY].is_empty());
    }

    #[test]
    fn frame_noop_when_dead() {
        let gi = ServerGameImport::new();
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        let mut game = NullGame::default();

        sv.frame(&mut svs, 16_000, &mut game, &gi);

        assert_eq!(sv.framenum, 0);
        assert_eq!(sv.time, 0);
        assert_eq!(svs.realtime, 0);
        assert_eq!(game.frames_run, 0);
    }

    #[test]
    fn game_frames_run_at_10hz_regardless_of_call_rate() {
        let gi = ServerGameImport::new();
        // Sixty 60 Hz calls after a map load (sv.time = 1000, realtime = 0).
        // C adds usec / 1000 (16 ms) per call and runs one game frame each
        // time realtime catches up with sv.time.
        let (mut sv, mut svs, mut game) = running_server();
        sv.time = 1000;

        for _ in 0..60 {
            sv.frame(&mut svs, 16_667, &mut game, &gi);
        }

        assert_eq!(sv.framenum, 10);
        assert_eq!(game.frames_run, 10);
        assert_eq!(sv.time, 1000, "time must be framenum * 100 ms");
    }

    #[test]
    fn frame_before_due_time_runs_no_game_frame() {
        let gi = ServerGameImport::new();
        let (mut sv, mut svs, mut game) = running_server();
        sv.time = 100;

        sv.frame(&mut svs, 50_000, &mut game, &gi);

        assert_eq!(sv.framenum, 0);
        assert_eq!(game.frames_run, 0);
        assert_eq!(svs.realtime, 50);
    }

    #[test]
    fn frame_runs_at_most_one_game_frame_per_call() {
        let gi = ServerGameImport::new();
        // A one-second hitch: C never catches up with several frames; it
        // runs one and clamps realtime to sv.time ("sv highclamp").
        let (mut sv, mut svs, mut game) = running_server();

        sv.frame(&mut svs, 1_000_000, &mut game, &gi);

        assert_eq!(sv.framenum, 1);
        assert_eq!(game.frames_run, 1);
        assert_eq!(sv.time, 100);
        assert_eq!(svs.realtime, 100);
    }

    #[test]
    fn realtime_far_behind_is_clamped_to_one_frame_before_time() {
        let gi = ServerGameImport::new();
        // "sv lowclamp": realtime is never left more than 100 ms behind.
        let (mut sv, mut svs, mut game) = running_server();
        sv.time = 1000;

        sv.frame(&mut svs, 16_000, &mut game, &gi);

        assert_eq!(svs.realtime, 900);
        assert_eq!(game.frames_run, 0);
    }

    #[test]
    fn frame_with_zero_elapsed_time_runs_due_frame_only_once() {
        let gi = ServerGameImport::new();
        let (mut sv, mut svs, mut game) = running_server();

        sv.frame(&mut svs, 0, &mut game, &gi);
        sv.frame(&mut svs, 0, &mut game, &gi);

        assert_eq!(game.frames_run, 1, "second call is not due until 100 ms");
    }

    #[test]
    fn frame_stops_running_game_once_server_is_dead_again() {
        let gi = ServerGameImport::new();
        let (mut sv, mut svs, mut game) = running_server();

        sv.frame(&mut svs, 100_000, &mut game, &gi);
        sv.state = ServerState::Dead;
        sv.frame(&mut svs, 100_000, &mut game, &gi);

        assert_eq!(game.frames_run, 1);
        assert_eq!(sv.framenum, 1);
    }

    #[test]
    fn run_game_called_on_active_server() {
        let gi = ServerGameImport::new();
        let (mut sv, mut svs, mut game) = running_server();

        sv.frame(&mut svs, 100_000, &mut game, &gi);
        assert_eq!(game.frames_run, 1, "run_frame must be called when due");
        assert_eq!(sv.framenum, 1);
    }
}
