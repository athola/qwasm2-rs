//! Server frame loop.
//!
//! Rust equivalent of `SV_Frame()` and `SV_RunGameFrame()` from
//! `sv_main.c` (C reference: Qwasm2/src/server/sv_main.c).

use crate::state::{Server, ServerState, ServerStatic};

impl Server {
    /// Run one server frame.
    ///
    /// This is the Rust equivalent of `SV_Frame(int usec)` combined with
    /// `SV_RunGameFrame()` from the C codebase. The `frametime_us` parameter
    /// is the elapsed wall-clock time in microseconds since the last frame.
    ///
    /// The frame performs the following steps:
    /// 1. Early-out if the server is dead (no map loaded).
    /// 2. Advance `realtime` on the persistent server state.
    /// 3. Compute `frametime` and advance `time` / `framenum`.
    /// 4. (TODO) Process client input.
    /// 5. (TODO) Run game frame.
    /// 6. (TODO) Send client updates.
    pub fn frame(&mut self, svs: &mut ServerStatic, frametime_us: u32) {
        // Nothing to do when no map is loaded.
        if self.state == ServerState::Dead {
            return;
        }

        // Convert microseconds to seconds for per-frame timing.
        self.frametime = frametime_us as f32 / 1_000_000.0;

        // Advance persistent real-time clock (mirrors `svs.realtime += usec / 1000`
        // but stored in seconds here instead of milliseconds).
        svs.realtime += self.frametime;

        // Bump frame counter — must always increment so delta compression
        // stays in sync, even when the world is paused.
        self.framenum += 1;

        // Advance server time (C uses `sv.time = sv.framenum * 100`).
        self.time += self.frametime;

        // TODO: SV_CheckTimeouts — drop clients that haven't sent packets.
        // TODO: SV_ReadPackets   — process incoming client packets.
        // TODO: SV_CalcPings     — update per-client ping estimates.
        // TODO: SV_GiveMsec      — refresh command-millisecond budgets.
        // TODO: ge->RunFrame()   — run the game DLL logic.
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

    #[test]
    fn frame_noop_when_dead() {
        let mut sv = Server::default();
        let mut svs = ServerStatic::default();
        assert_eq!(sv.state, ServerState::Dead);

        sv.frame(&mut svs, 16_000); // 16 ms

        // Nothing should change when the server is dead.
        assert_eq!(sv.framenum, 0);
        assert_eq!(sv.time, 0.0);
        assert_eq!(svs.realtime, 0.0);
    }

    #[test]
    fn server_frame_increments() {
        let mut sv = Server::default();
        sv.state = ServerState::Game;
        let mut svs = ServerStatic::default();
        svs.initialized = true;

        // Run three frames at ~16.667 ms each (60 Hz).
        for _ in 0..3 {
            sv.frame(&mut svs, 16_667);
        }

        assert_eq!(sv.framenum, 3);
        // 3 * 16667 us ≈ 0.050001 s
        let expected_time = 3.0 * (16_667.0 / 1_000_000.0);
        assert!((sv.time - expected_time).abs() < 1e-4);
    }

    #[test]
    fn frame_advances_realtime() {
        let mut sv = Server::default();
        sv.state = ServerState::Game;
        let mut svs = ServerStatic::default();

        sv.frame(&mut svs, 100_000); // 100 ms
        assert!((svs.realtime - 0.1).abs() < 1e-6);
    }
}
