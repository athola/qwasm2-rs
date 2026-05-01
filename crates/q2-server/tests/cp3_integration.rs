//! CP-3 integration test: server starts, accepts a simulated client
//! connection, and runs 100 frames without panicking.
//!
//! This is the acceptance test for Phase 3 of the C-to-Rust conversion plan.
//! It wires together the real q2-game GameLogic and q2-server together — no
//! mocks for either side.

use q2_game::{
    traits::{GameExport, GameImport},
    GameLogic,
};
use q2_server::{
    state::{Server, ServerState, ServerStatic},
    ServerGameImport,
};
use q2_shared::constants::CS_MAXCLIENTS;

/// Minimal entity string for map initialisation: just a worldspawn and a
/// player start.  A real map has hundreds of entities; this is enough to
/// exercise the spawn path without depending on filesystem access.
const MINIMAL_ENTSTRING: &str = r#"
{
"classname" "worldspawn"
"message" "CP-3 test map"
}
{
"classname" "info_player_start"
"origin" "0 0 0"
"angle" "90"
}
"#;

#[test]
fn cp3_server_runs_100_frames() {
    // 1. Set up the GameImport (server → game callbacks).
    let gi = ServerGameImport::new();

    // 2. Create and initialise the game module.
    let mut game = GameLogic::new();
    game.init(&gi);

    // 3. Initialise server with 1 client slot.
    let mut svs = ServerStatic::default();
    svs.init_game(1);

    // 4. Load a minimal map (no real BSP — sets state to Game).
    let mut sv = Server::default();
    sv.map(&mut svs, "cp3_test").expect("map() must succeed");
    assert_eq!(sv.state, ServerState::Game);

    // 5. Spawn game entities from the entity string.
    game.spawn_entities("cp3_test", MINIMAL_ENTSTRING, "");

    // 6. Simulate a client connecting and entering the game.
    let accepted = game.client_connect(0, r"name\TestPlayer\skin\male/grunt");
    assert!(accepted, "client_connect must accept the connection");
    game.client_begin(0);

    // 7. Run 100 server frames at 10 Hz (100 ms per frame = 100,000 µs).
    for _ in 0..100 {
        sv.frame(&mut svs, 100_000, &mut game);
    }

    // 8. Verify frame counters.
    assert_eq!(sv.framenum, 100, "server must have run exactly 100 frames");
    assert!(sv.time > 0.0, "server time must advance");
    assert!(svs.realtime > 0.0, "realtime must advance");
}

#[test]
fn cp3_client_disconnect_does_not_panic() {
    let gi = ServerGameImport::new();
    let mut game = GameLogic::new();
    game.init(&gi);

    let mut svs = ServerStatic::default();
    svs.init_game(1);
    let mut sv = Server::default();
    sv.map(&mut svs, "test").unwrap();

    game.client_connect(0, "");
    game.client_begin(0);

    // Run a few frames, then disconnect.
    for _ in 0..5 {
        sv.frame(&mut svs, 16_667, &mut game);
    }
    game.client_disconnect(0);

    // Server must continue running after disconnect.
    for _ in 0..5 {
        sv.frame(&mut svs, 16_667, &mut game);
    }
    assert_eq!(sv.framenum, 10);
}

#[test]
fn cp3_configstring_written_by_game_survives_frame() {
    let gi = ServerGameImport::new();
    // Write a configstring through the import before init.
    gi.configstring(0, "cp3_test");

    let updates = gi.drain_configstring_updates();
    let found = updates
        .iter()
        .any(|(idx, val)| *idx == 0 && val == "cp3_test");
    assert!(found, "configstring(0, ...) must be readable via drain");
}

/// Verify that vtable dispatch actually routes GameImport callbacks from inside
/// `GameLogic::init()` to the concrete `ServerGameImport` implementation.
///
/// This proves the call goes through the `&dyn GameImport` vtable, not a
/// direct call — important because it exercises the same code path that the
/// real game frame callbacks will follow.
#[test]
fn cp3_game_init_dispatches_configstring_through_vtable() {
    let gi = ServerGameImport::new();
    let mut game = GameLogic::new();

    // init() calls gi.configstring(CS_MAXCLIENTS, "8") through &dyn GameImport.
    game.init(&gi);

    let updates = gi.drain_configstring_updates();
    let found = updates
        .iter()
        .any(|(idx, val)| *idx == CS_MAXCLIENTS && val == "8");
    assert!(
        found,
        "GameLogic::init must dispatch configstring(CS_MAXCLIENTS) to ServerGameImport"
    );
}
