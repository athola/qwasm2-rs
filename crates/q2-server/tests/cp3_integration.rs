//! CP-3 integration test: server starts, accepts a simulated client
//! connection, and runs 100 frames without panicking.
//!
//! This is the acceptance test for Phase 3 of the C-to-Rust conversion plan.
//! It wires the full q2-game GameWorld to q2-server, with no mocks for
//! either side.

use std::rc::Rc;

use q2_game::{
    traits::{GameExport, GameImport},
    world::GameWorld,
};
use q2_server::{
    state::{Server, ServerState, ServerStatic},
    ServerGameImport,
};
use q2_shared::constants::{CS_MAXCLIENTS, CS_SKY, MAX_EDICTS};

/// The game with one client slot, sharing `gi` with the server as the C
/// server and game DLL share one `game_import_t`.
fn new_game(gi: &Rc<ServerGameImport>) -> GameWorld {
    let mut game = GameWorld::new(gi.clone(), MAX_EDICTS);
    game.set_max_clients(1);
    game.init();
    game
}

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
    let gi = Rc::new(ServerGameImport::new());

    // 2. Create and initialise the game module.
    let mut game = new_game(&gi);

    // 3. Initialise server with 1 client slot.
    let mut svs = ServerStatic::default();
    svs.init_game(1);

    // 4. Load a minimal map (no real BSP — sets state to Game).
    let mut sv = Server::default();
    sv.map(&mut svs, "cp3_test", &[], &gi, &mut game)
        .expect("map() must succeed");
    assert_eq!(sv.state, ServerState::Game);

    // 5. Spawn game entities from the entity string.
    game.spawn_entities("cp3_test", MINIMAL_ENTSTRING, "");

    // 6. Simulate a client connecting and entering the game.
    // Client slot 0 is entity number 1 (C passes EDICT_NUM(slot + 1)).
    let accepted = game.client_connect(1, r"name\TestPlayer\skin\male/grunt");
    assert!(accepted, "client_connect must accept the connection");
    assert!(
        !game.client_connect(2, r"name\Extra"),
        "a 1-slot game must reject entity 2"
    );
    game.client_begin(1);

    // 7. Call the server at 10 Hz (100 ms = 100,000 µs per call). map() sets
    //    sv.time = 1000 like C SV_SpawnServer, so the first call only clamps
    //    realtime up to 900 ms; each later call runs exactly one game frame.
    for _ in 0..101 {
        sv.frame(&mut svs, 100_000, &mut game, &gi);
    }

    // 8. Verify frame counters.
    assert_eq!(sv.framenum, 100, "server must have run exactly 100 frames");
    assert_eq!(sv.time, 100 * 100, "sv.time is framenum * 100 ms");
    assert_eq!(svs.realtime, sv.time, "realtime tracks game time at 10 Hz");
    assert_eq!(
        sv.configstrings[CS_MAXCLIENTS], "1",
        "configstring written by the game must reach the server"
    );
}

#[test]
fn cp3_client_disconnect_does_not_panic() {
    let gi = Rc::new(ServerGameImport::new());
    let mut game = new_game(&gi);

    let mut svs = ServerStatic::default();
    svs.init_game(1);
    let mut sv = Server::default();
    sv.map(&mut svs, "test", &[], &gi, &mut game).unwrap();

    game.client_connect(1, "");
    game.client_begin(1);

    // Run a few 60 Hz calls, then disconnect.
    for _ in 0..5 {
        sv.frame(&mut svs, 16_667, &mut game, &gi);
    }
    let framenum_before = sv.framenum;
    game.client_disconnect(1);

    // Server must keep running game frames after the disconnect.
    for _ in 0..10 {
        sv.frame(&mut svs, 16_667, &mut game, &gi);
    }
    assert!(
        sv.framenum > framenum_before,
        "game frames must continue after disconnect"
    );
}

#[test]
fn cp3_configstring_written_by_game_survives_frame() {
    let gi = Rc::new(ServerGameImport::new());
    let mut game = new_game(&gi);
    gi.drain_configstring_updates(); // init's own writes are not under test
    let mut svs = ServerStatic::default();
    svs.init_game(1);
    let mut sv = Server::default();
    sv.map(&mut svs, "cp3_test", &[], &gi, &mut game).unwrap();

    // A game-side configstring write (C: gi.configstring during a frame).
    gi.configstring(CS_SKY as i32, "unit1_");
    assert!(sv.configstrings[CS_SKY].is_empty());

    // First call clamps realtime to 900 ms, second runs the game frame.
    sv.frame(&mut svs, 100_000, &mut game, &gi);
    sv.frame(&mut svs, 100_000, &mut game, &gi);

    assert_eq!(sv.framenum, 1);
    assert_eq!(sv.configstrings[CS_SKY], "unit1_");
}

/// Verify that a configstring the game writes while spawning a map reaches
/// the concrete `ServerGameImport` through the game's `Rc<dyn GameImport>`.
///
/// This exercises the same dispatch path every game-frame callback follows.
#[test]
fn cp3_game_spawn_dispatches_configstring_through_vtable() {
    let gi = Rc::new(ServerGameImport::new());
    let mut game = new_game(&gi);

    // SP_worldspawn calls gi.configstring(CS_MAXCLIENTS, "1").
    game.spawn_entities("cp3_test", MINIMAL_ENTSTRING, "");

    let updates = gi.drain_configstring_updates();
    let found = updates
        .iter()
        .any(|(idx, val)| *idx == CS_MAXCLIENTS && val == "1");
    assert!(
        found,
        "spawning must dispatch configstring(CS_MAXCLIENTS) to ServerGameImport"
    );
}
