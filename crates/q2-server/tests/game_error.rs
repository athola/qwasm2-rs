//! C `gi.error` (`PF_error`) is `Com_Error(ERR_DROP, "Game Error: %s")`:
//! the server refuses the map instead of running a broken level.

mod common;

use q2_common::error::Q2Error;
use q2_server::init::MapRequest;
use q2_server::local::LocalGame;

fn drop_text(result: Result<impl Sized, Q2Error>) -> String {
    match result {
        Err(Q2Error::Drop(msg)) => msg,
        Err(other) => panic!("expected a Drop, got {other}"),
        Ok(_) => panic!("expected a Drop, got Ok"),
    }
}

#[test]
fn an_entity_lump_without_its_closing_brace_refuses_the_map() {
    let map = common::floor_wall_bsp_with("{ \"classname\" \"worldspawn\"");
    let msg = drop_text(LocalGame::start_with_userinfo("broken", &map, ""));
    assert_eq!(msg, "Game Error: ED_ParseEntity: EOF without closing brace");
}

#[test]
fn a_map_without_a_player_start_refuses_the_client() {
    let map = common::floor_wall_bsp_with("{\n\"classname\" \"worldspawn\"\n}\n");
    let msg = drop_text(LocalGame::start("nostart", &map));
    assert_eq!(msg, "Game Error: Couldn't find spawn point \n");
}

#[test]
fn a_level_change_to_a_missing_spawnpoint_is_a_drop() {
    let map = common::floor_wall_bsp();
    let mut game = LocalGame::start("floor_wall", &map).expect("map loads");
    let msg = drop_text(game.change_map(&MapRequest::parse("floor_wall$nowhere"), &map));
    assert_eq!(msg, "Game Error: Couldn't find spawn point nowhere\n");
}

#[test]
fn a_bsp_entity_without_its_model_raises_err_fatal() {
    use q2_game::traits::{EntityLink, GameImport};
    use q2_shared::types::{Solid, Vec3f};
    let world = common::ServerWorld::start("floor_wall", &common::floor_wall_bsp());
    world.gi.take_error().expect("the map loads clean");
    world.gi.link_entity(&EntityLink {
        number: 50,
        origin: Vec3f::new(0.0, 0.0, 24.0),
        mins: Vec3f::new(-8.0, -8.0, -8.0),
        maxs: Vec3f::new(8.0, 8.0, 8.0),
        absmin: Vec3f::new(-8.0, -8.0, 16.0),
        absmax: Vec3f::new(8.0, 8.0, 32.0),
        solid: Solid::Bsp,
        modelindex: 99,
        ..Default::default()
    });
    world.gi.point_contents(Vec3f::new(0.0, 0.0, 24.0));
    match world.gi.take_error() {
        Err(Q2Error::Fatal(msg)) => assert_eq!(msg, "MOVETYPE_PUSH with a non bsp model"),
        other => panic!("expected ERR_FATAL, got {other:?}"),
    }
}
