//! Server frame loop.
//!
//! Rust equivalent of `SV_Frame()` and `SV_RunGameFrame()` from
//! `sv_main.c` (C reference: Qwasm2/src/server/sv_main.c).

use q2_game::traits::GameExport;

use crate::state::{Server, ServerState, ServerStatic};

impl Server {
    /// Run one server frame.
    ///
    /// Advances timing, bumps the frame counter, and drives the game frame.
    /// `frametime_us` is elapsed wall-clock time in microseconds since the
    /// last frame.
    pub fn frame(&mut self, svs: &mut ServerStatic, frametime_us: u32, game: &mut dyn GameExport) {
        if self.state == ServerState::Dead {
            return;
        }

        self.frametime = frametime_us as f32 / 1_000_000.0;
        svs.realtime += self.frametime;

        // framenum must always increment — delta compression depends on it.
        self.framenum += 1;
        self.time += self.frametime;

        // TODO: SV_CheckTimeouts — drop clients that haven't sent packets.
        // TODO: SV_ReadPackets   — process incoming client packets.
        // TODO: SV_CalcPings     — update per-client ping estimates.
        // TODO: SV_GiveMsec      — refresh command-millisecond budgets.

        game.run_frame();

        // TODO: SV_SendClientMessages — push state to connected clients.
        // TODO: SV_PrepWorldFrame     — clear per-frame entity events.
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use q2_shared::types::UserCmd;

    /// Minimal GameExport stub for frame loop tests.
    struct NullGame {
        frames_run: u32,
    }

    impl GameExport for NullGame {
        fn api_version(&self) -> i32 {
            3
        }
        fn init(&mut self, _: &dyn q2_game::traits::GameImport) {}
        fn shutdown(&mut self) {}
        fn spawn_entities(&mut self, _: &str, _: &str, _: &str) {}
        fn client_connect(&mut self, _: usize, _: &str) -> bool {
            true
        }
        fn client_begin(&mut self, _: usize) {}
        fn client_disconnect(&mut self, _: usize) {}
        fn client_command(&mut self, _: usize) {}
        fn client_think(&mut self, _: usize, _: &UserCmd) {}
        fn run_frame(&mut self) {
            self.frames_run += 1;
        }
        fn server_command(&mut self) {}
    }

    #[test]
    fn frame_noop_when_dead() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        let mut game = NullGame { frames_run: 0 };

        sv.frame(&mut svs, 16_000, &mut game);

        assert_eq!(sv.framenum, 0);
        assert_eq!(sv.time, 0.0);
        assert_eq!(svs.realtime, 0.0);
        assert_eq!(game.frames_run, 0);
    }

    #[test]
    fn server_frame_increments() {
        let mut sv = Server::default();
        sv.state = ServerState::Game;
        let mut svs = ServerStatic::default();
        svs.initialized = true;
        let mut game = NullGame { frames_run: 0 };

        for _ in 0..3 {
            sv.frame(&mut svs, 16_667, &mut game);
        }

        assert_eq!(sv.framenum, 3);
        assert_eq!(game.frames_run, 3);
        let expected = 3.0 * (16_667.0 / 1_000_000.0);
        assert!((sv.time - expected).abs() < 1e-4);
    }

    #[test]
    fn frame_advances_realtime() {
        let mut sv = Server::default();
        sv.state = ServerState::Game;
        let mut svs = ServerStatic::default();
        let mut game = NullGame { frames_run: 0 };

        sv.frame(&mut svs, 100_000, &mut game);
        assert!((svs.realtime - 0.1).abs() < 1e-6);
    }

    #[test]
    fn run_game_called_on_active_server() {
        let mut sv = Server::default();
        sv.state = ServerState::Game;
        let mut svs = ServerStatic::default();
        let mut game = NullGame { frames_run: 0 };

        sv.frame(&mut svs, 16_667, &mut game);
        assert_eq!(
            game.frames_run, 1,
            "run_frame must be called each active frame"
        );
    }
}
