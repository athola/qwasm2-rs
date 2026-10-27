//! Qwasm2-rs: Dedicated server — state management, client handling, game interface
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod frame;
pub mod game_iface;
pub mod init;
pub mod local;
pub mod state;
pub mod world;

pub use game_iface::ServerGameImport;

/// A `GameExport` that counts frames and does nothing else, for server unit
/// tests that don't need game logic.
#[cfg(test)]
pub(crate) mod test_support {
    use q2_game::traits::GameExport;
    use q2_shared::types::UserCmd;

    #[derive(Default)]
    pub(crate) struct NullGame {
        pub(crate) frames_run: u32,
    }

    impl GameExport for NullGame {
        fn init(&mut self) {}
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
}
