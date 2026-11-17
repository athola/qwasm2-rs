//! Shared fixtures for the server integration tests. The synthetic maps
//! live in `bsp.rs`, which the crate's own unit tests include as well.

#![allow(dead_code, unused_imports)] // each test binary uses a different subset

mod bsp;
pub use bsp::*;

/// `gamedata/baseq2/pak0.pak`'s demo1 map, or `None` when the game data is
/// not installed (the case in CI). A test that returns early on `None`
/// passes without checking anything, so the skip is printed to keep it
/// visible in `cargo test -- --nocapture` and in failure output.
pub fn demo1_bsp() -> Option<Vec<u8>> {
    stock_bsp("demo1")
}

/// `maps/<name>.bsp` from `gamedata/baseq2/pak0.pak`, or `None` (printing
/// the skip) when the game data is not installed, as for `demo1_bsp`.
pub fn stock_bsp(name: &str) -> Option<Vec<u8>> {
    let pak = concat!(env!("CARGO_MANIFEST_DIR"), "/../../gamedata/baseq2");
    let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
    let map = fs
        .add_game_directory(std::path::Path::new(pak))
        .ok()
        .and_then(|_| fs.load_file(&format!("maps/{name}.bsp")).ok());
    if map.is_none() {
        // CI fetches the demo pak; a broken cache must fail, not skip.
        assert!(
            std::env::var_os("CI").is_none(),
            "gamedata/baseq2/pak0.pak missing under CI"
        );
        eprintln!("SKIPPED: gamedata/baseq2/pak0.pak not installed; {name} test checks nothing");
    }
    map
}

/// The retail game data the user installs at `gamedata/retail/baseq2`
/// (never committed), or `None`, printing the skip, as for `demo1_bsp`.
pub fn retail_fs() -> Option<q2_common::filesystem::FileSystem> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../gamedata/retail/baseq2");
    let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
    let loaded = fs.add_game_directory(std::path::Path::new(dir)).is_ok()
        && fs.file_exists("maps/base1.bsp");
    if !loaded {
        eprintln!("SKIPPED: gamedata/retail/baseq2 not installed; retail test checks nothing");
    }
    loaded.then_some(fs)
}

/// A server and the concrete `GameWorld` it runs, with one client in the
/// game, for tests that inspect game state `LocalGame` keeps behind
/// `GameExport`. `frame` and `think` drive it as `LocalGame` does.
pub struct ServerWorld {
    pub sv: q2_server::state::Server,
    pub svs: q2_server::state::ServerStatic,
    pub gi: std::rc::Rc<q2_server::ServerGameImport>,
    pub game: q2_game::world::GameWorld,
}

impl ServerWorld {
    /// `bsp` loaded as `maps/<name>.bsp` with the client at its player start.
    pub fn start(name: &str, bsp: &[u8]) -> Self {
        use q2_game::traits::GameExport;
        let gi = std::rc::Rc::new(q2_server::ServerGameImport::new());
        let mut game = new_world(&gi);
        let mut svs = q2_server::state::ServerStatic::default();
        svs.init_game(1);
        let mut sv = q2_server::state::Server::default();
        sv.map(&mut svs, name, bsp, "", &gi, &mut game)
            .expect("map loads");
        svs.connect_local_client(&mut game, "")
            .expect("client accepted");
        Self { sv, svs, gi, game }
    }

    pub fn think(&mut self, cmd: &q2_shared::types::UserCmd) {
        use q2_game::traits::GameExport;
        self.game.client_think(1, cmd);
    }

    pub fn frame(&mut self, frametime_us: u32) {
        self.sv
            .frame(&mut self.svs, frametime_us, &mut self.game, &self.gi)
            .unwrap();
    }
}

/// Map one: the player starts on a jacket armor and picks it up.
pub fn armor_map() -> Vec<u8> {
    floor_wall_bsp_with(&format!(
        "{}{}",
        ENTSTRING, "{\n\"classname\" \"item_armor_jacket\"\n\"origin\" \"0 0 16\"\n}\n"
    ))
}

/// A fresh one-player game, as C InitGame leaves it before a map spawns.
pub fn new_world(gi: &std::rc::Rc<q2_server::ServerGameImport>) -> q2_game::world::GameWorld {
    use q2_game::traits::GameExport;
    let mut game = q2_game::world::GameWorld::new(gi.clone(), q2_shared::constants::MAX_EDICTS);
    game.set_max_clients(1);
    game.init();
    game
}

/// `frames` game frames of a player standing still.
pub fn idle(game: &mut q2_server::local::LocalGame, frames: usize) {
    for _ in 0..frames {
        game.think(&q2_shared::types::UserCmd {
            msec: 100,
            ..Default::default()
        })
        .unwrap();
        game.frame(100_000).unwrap();
    }
}

/// The INFO-and-above log lines written while the returned guard lives,
/// collected from this thread.
pub fn capture_info_logs() -> (
    std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    tracing::subscriber::DefaultGuard,
) {
    let out = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = out.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || LogSink(writer.clone()))
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .finish();
    (out, tracing::subscriber::set_default(subscriber))
}

struct LogSink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
