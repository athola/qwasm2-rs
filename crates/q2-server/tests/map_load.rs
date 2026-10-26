//! Server::map with a real BSP: C SV_SpawnServer loads collision, registers
//! the world and inline models, and spawns the game's entities.

mod common;

use std::rc::Rc;

use q2_game::{traits::GameExport, world::GameWorld};
use q2_server::{
    state::{Server, ServerState, ServerStatic},
    ServerGameImport,
};
use q2_shared::constants::{CS_MAPCHECKSUM, CS_MODELS, MAX_EDICTS};

fn setup() -> (Server, ServerStatic, Rc<ServerGameImport>, GameWorld) {
    let gi = Rc::new(ServerGameImport::new());
    let mut game = GameWorld::new(gi.clone(), MAX_EDICTS);
    game.set_max_clients(1);
    game.init();
    let mut svs = ServerStatic::default();
    svs.init_game(1);
    (Server::default(), svs, gi, game)
}

#[test]
fn map_loads_collision_registers_the_world_and_spawns_entities() {
    let (mut sv, mut svs, gi, mut game) = setup();
    sv.map(
        &mut svs,
        "floor_wall",
        &common::floor_wall_bsp(),
        &gi,
        &mut game,
    )
    .expect("fixture map loads");

    assert_eq!(sv.state, ServerState::Game);
    assert_eq!(sv.configstrings[CS_MODELS + 1], "maps/floor_wall.bsp");
    assert!(!sv.configstrings[CS_MAPCHECKSUM].is_empty());
    let world = game.entities.key_for_number(0).expect("worldspawn at 0");
    assert_eq!(
        game.entities.get(world).unwrap().game.classname,
        "worldspawn"
    );
    assert!(game.find_by_classname(None, "info_player_start").is_some());
    // C runs two game frames so everything settles before clients enter.
    assert_eq!(game.level.framenum, 2);
}

#[test]
fn map_with_a_bad_bsp_fails_and_leaves_the_server_not_running() {
    let (mut sv, mut svs, gi, mut game) = setup();
    assert!(sv
        .map(&mut svs, "broken", b"not a bsp", &gi, &mut game)
        .is_err());
    assert_ne!(sv.state, ServerState::Game);
}

#[test]
fn map_registers_every_inline_model_of_demo1() {
    let Some(bsp) = common::demo1_bsp() else {
        eprintln!("skipped: gamedata/baseq2/pak0.pak not installed");
        return;
    };
    let (mut sv, mut svs, gi, mut game) = setup();
    sv.map(&mut svs, "demo1", &bsp, &gi, &mut game)
        .expect("demo1 loads");

    let inline = gi.num_inline_models();
    assert!(inline > 1);
    for i in 1..inline {
        assert_eq!(sv.configstrings[CS_MODELS + 1 + i], format!("*{i}"));
    }
    assert!(game.entities.count() > 100, "demo1 spawns its entities");
}
