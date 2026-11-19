//! Qwasm2-rs: Dedicated server — state management, client handling, game interface
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

pub mod ents;
pub mod frame;
pub mod game_iface;
pub mod init;
pub mod local;
pub mod mapfixes;
pub mod state;
pub mod vis;
pub mod world;

pub use game_iface::ServerGameImport;
/// Saved callback and move names that share one function in this build, so
/// the platform's self test can check the binary it ships. Here because the
/// platform may not depend on `q2-game` (ADR-001).
pub use q2_game::savegame::registry::aliased_names as aliased_save_names;

/// The integration tests' synthetic maps, for unit tests that need a map
/// with a player start.
#[cfg(test)]
#[path = "../tests/common/bsp.rs"]
pub(crate) mod test_bsp;

/// A `GameExport` that counts frames and does nothing else, for server unit
/// tests that don't need game logic.
#[cfg(test)]
pub(crate) mod test_support {
    use q2_game::traits::{EdictView, GameExport};
    use q2_shared::types::{PlayerState, UserCmd};

    #[derive(Default)]
    pub(crate) struct NullGame {
        pub(crate) frames_run: u32,
        pub(crate) refuse_connect: bool,
        /// Spawnpoint of the last `spawn_entities`.
        pub(crate) spawnpoint: String,
        /// Entity string of the last `spawn_entities`.
        pub(crate) entities: String,
        pub(crate) connects: u32,
        /// The userinfo of the last `client_connect`.
        pub(crate) userinfo: String,
        /// Entity numbers `client_begin` was called with, in order.
        pub(crate) begun: Vec<usize>,
        /// `client_think` calls, shared so a test can read it after
        /// boxing the game.
        pub(crate) thinks: std::rc::Rc<std::cell::Cell<u32>>,
        /// `client_command` calls, shared like `thinks`.
        pub(crate) commands: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl GameExport for NullGame {
        fn init(&mut self) {}
        fn shutdown(&mut self) {}
        fn spawn_entities(&mut self, _: &str, entities: &str, spawnpoint: &str) {
            self.spawnpoint = spawnpoint.to_string();
            self.entities = entities.to_string();
        }
        fn client_connect(&mut self, _: usize, userinfo: &str) -> bool {
            self.connects += 1;
            self.userinfo = userinfo.to_string();
            !self.refuse_connect
        }
        fn client_begin(&mut self, number: usize) {
            self.begun.push(number);
        }
        fn client_disconnect(&mut self, _: usize) {}
        fn client_command(&mut self, _: usize) {
            self.commands.set(self.commands.get() + 1);
        }
        fn client_think(&mut self, _: usize, _: &UserCmd) {
            self.thinks.set(self.thinks.get() + 1);
        }
        fn run_frame(&mut self) {
            self.frames_run += 1;
        }
        fn clear_entity_events(&mut self) {}
        fn server_command(&mut self) {}
        fn num_edicts(&self) -> usize {
            0
        }
        fn edict(&self, _: usize) -> Option<EdictView<'_>> {
            None
        }
        fn client_player_state(&self, _: usize) -> Option<&PlayerState> {
            None
        }
        fn write_game(&mut self, _: bool) -> q2_common::error::Q2Result<Vec<u8>> {
            Ok(Vec::new())
        }
        fn read_game(&mut self, _: &[u8]) -> q2_common::error::Q2Result<()> {
            Ok(())
        }
        fn write_level(&mut self, _: bool) -> q2_common::error::Q2Result<Vec<u8>> {
            Ok(Vec::new())
        }
        fn read_level(&mut self, _: &[u8]) -> q2_common::error::Q2Result<()> {
            Ok(())
        }
        fn check_game(&self, _: &[u8]) -> q2_common::error::Q2Result<()> {
            Ok(())
        }
        fn check_level(&self, _: &[u8]) -> q2_common::error::Q2Result<()> {
            Ok(())
        }
    }
}
