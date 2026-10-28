//! A level exit in one process: C `SV_GameMap_f` -> `SV_Map` ->
//! `SV_SpawnServer` on the same game, then the client's `begin` (no new
//! `ClientConnect`), so the player keeps its inventory.

mod common;

use q2_server::init::MapRequest;
use q2_server::local::{LocalGame, LOCAL_PLAYER};
use q2_shared::constants::{CS_MODELS, CS_NAME, STAT_ARMOR};
use q2_shared::types::{UserCmd, Vec3f};

/// Map one: the player starts on a jacket armor and picks it up.
fn armor_map() -> Vec<u8> {
    common::floor_wall_bsp_with(&format!(
        "{}{}",
        common::ENTSTRING,
        "{\n\"classname\" \"item_armor_jacket\"\n\"origin\" \"0 0 16\"\n}\n"
    ))
}

/// Map two: an unnamed start and one named "east".
fn two_start_map() -> Vec<u8> {
    common::floor_wall_bsp_with(concat!(
        "{\n\"classname\" \"worldspawn\"\n}\n",
        "{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}\n",
        "{\n\"classname\" \"info_player_start\"\n\"targetname\" \"east\"\n\"origin\" \"128 0 24\"\n}\n",
    ))
}

fn play(game: &mut LocalGame, frames: usize) {
    for _ in 0..frames {
        game.think(&UserCmd {
            msec: 100,
            ..Default::default()
        });
        game.frame(100_000);
    }
}

fn armor(game: &LocalGame) -> i16 {
    game.player().expect("player in game").1.stats[STAT_ARMOR]
}

#[test]
fn a_map_change_keeps_the_pickup_and_starts_at_the_named_spawnpoint() {
    let mut game = LocalGame::start("one", &armor_map()).expect("map loads");
    play(&mut game, 5);
    assert_eq!(armor(&game), 25, "picked up the jacket armor");

    game.change_map(&MapRequest::parse("two$east"), &two_start_map())
        .expect("map two loads");

    // SelectSpawnPoint lifts the start 9 units, PutClientInServer 1 more.
    let (origin, _) = game.player().expect("player in the new map");
    assert_eq!(origin, Vec3f::new(128.0, 0.0, 34.0));
    assert_eq!(game.framenum(), 0, "C SV_SpawnServer zeroes sv");
    assert_eq!(game.configstring(CS_NAME), "two");
    assert_eq!(game.configstring(CS_MODELS + 1), "maps/two.bsp");
    play(&mut game, 2);
    assert_eq!(armor(&game), 25, "inventory carried over");
    assert!(game
        .entity_states()
        .iter()
        .any(|s| s.number == LOCAL_PLAYER as i32));
}

#[test]
fn a_picture_level_shows_the_picture_with_no_player_and_the_game_still_runs() {
    let mut game = LocalGame::start("one", &armor_map()).expect("map loads");
    play(&mut game, 3);
    assert_eq!(game.picture(), None);
    game.change_map(&MapRequest::parse("victory.pcx"), &[])
        .expect("C SV_SpawnServer(ss_pic) with no map");
    assert_eq!(game.picture(), Some("victory.pcx"));
    // C SV_New_f sends playernum -1 and no begin: the client never spawns.
    assert!(game.player().is_none());
    assert!(game.entity_states().is_empty());
    assert_eq!(game.configstring(CS_NAME), "victory.pcx");
    assert_eq!(game.configstring(CS_MODELS + 1), "", "no real map");
    assert_eq!(game.framenum(), 0);
    // C SV_RunGameFrame calls ge->RunFrame whatever the server state.
    // The first game frame is due at 1000 ms, a frame after the load.
    play(&mut game, 2);
    assert_eq!(game.framenum(), 1);
}

#[test]
fn the_game_kept_through_a_picture_level_carries_the_inventory_to_the_next_map() {
    // C coop sets nextserver to `gamemap "*base1"` for victory.pcx; the
    // player's game.clients entry survives the picture level.
    let mut game = LocalGame::start("one", &armor_map()).expect("map loads");
    play(&mut game, 5);
    assert_eq!(armor(&game), 25);
    game.change_map(&MapRequest::parse("victory.pcx"), &[])
        .expect("picture level loads");
    play(&mut game, 2);
    game.change_map(&MapRequest::parse("two"), &two_start_map())
        .expect("map two loads");
    assert_eq!(game.picture(), None);
    play(&mut game, 2);
    assert_eq!(armor(&game), 25, "inventory carried over");
}

#[test]
fn demo2s_target_changelevel_names_the_victory_picture() {
    let Some(demo2) = common::stock_bsp("demo2") else {
        return;
    };
    let gi = q2_server::ServerGameImport::new();
    gi.load_map(&demo2).expect("demo2 loads");
    let changelevel = gi
        .entity_string()
        .split('}')
        .find(|block| block.contains("\"target_changelevel\"") && block.contains("victory"))
        .map(str::to_string)
        .expect("demo2 ends the demo with a picture");
    assert!(changelevel.contains("\"map\" \"victory.pcx\""));
    let mut game = LocalGame::start("demo2", &demo2).expect("demo2 loads");
    game.change_map(&MapRequest::parse("victory.pcx"), &[])
        .expect("picture level loads");
    assert_eq!(game.picture(), Some("victory.pcx"));
}

#[test]
fn a_cinematic_level_is_refused_and_the_map_keeps_running() {
    let mut game = LocalGame::start("one", &armor_map()).expect("map loads");
    play(&mut game, 3);
    let framenum = game.framenum();
    assert!(game.change_map(&MapRequest::parse("end.cin"), &[]).is_err());
    assert!(game
        .change_map(&MapRequest::parse("demo1.dm2"), &[])
        .is_err());
    assert_eq!(game.configstring(CS_NAME), "one");
    assert_eq!(game.picture(), None);
    play(&mut game, 1);
    assert_eq!(game.framenum(), framenum + 1);
}

#[test]
fn a_map_that_fails_to_load_is_an_error() {
    let mut game = LocalGame::start("one", &armor_map()).expect("map loads");
    assert!(game
        .change_map(&MapRequest::parse("broken"), b"not a bsp")
        .is_err());
}

/// demo2's `info_player_start` with targetname `base1`, from its entity
/// string, lifted 10 units as SelectSpawnPoint and PutClientInServer do.
fn demo2_base1_start(bsp: &[u8]) -> Vec3f {
    let gi = q2_server::ServerGameImport::new();
    gi.load_map(bsp).expect("demo2 loads");
    let entities = gi.entity_string();
    let start = entities
        .split('}')
        .find(|block| {
            block.contains("\"info_player_start\"") && block.contains("\"targetname\" \"base1\"")
        })
        .expect("demo2 has a base1 start");
    let origin = start
        .lines()
        .find_map(|l| l.trim().strip_prefix("\"origin\" "))
        .expect("start has an origin")
        .trim_matches('"');
    let v: Vec<f32> = origin.split(' ').map(|c| c.parse().unwrap()).collect();
    Vec3f::new(v[0], v[1], v[2] + 10.0)
}

#[test]
fn demo1s_exit_to_demo2_base1_keeps_the_inventory_at_the_base1_start() {
    let (Some(demo1), Some(demo2)) = (common::stock_bsp("demo1"), common::stock_bsp("demo2"))
    else {
        return;
    };
    let mut w = common::ServerWorld::start("demo1", &demo1);
    for _ in 0..5 {
        w.think(&UserCmd::default());
        w.frame(100_000);
    }
    let shotgun = w.game.find_item("weapon_shotgun").unwrap();
    let shells = w.game.find_item_by_name("Shells").unwrap();
    {
        let key = w.game.entities.key_for_number(1).unwrap();
        let ent = w.game.entities.get_mut(key).unwrap();
        ent.game.health = 64;
        let pers = &mut w.game.client_mut(key).unwrap().pers;
        pers.inventory[shotgun] = 1;
        pers.inventory[shells] = 30;
    }

    // What LocalGame::change_map does, on a game the test can read.
    let level = MapRequest::parse("demo2$base1");
    w.sv.map(
        &mut w.svs,
        &level.map,
        &demo2,
        &level.spawnpoint,
        &w.gi,
        &mut w.game,
    )
    .expect("demo2 loads");
    w.svs.begin_connected_clients(&mut w.game);

    assert_eq!(w.sv.framenum, 0);
    // demo2's worldspawn message replaces CS_NAME, as in C.
    assert_eq!(w.sv.configstrings[CS_NAME], "Installation");
    assert_eq!(w.sv.configstrings[CS_MODELS + 1], "maps/demo2.bsp");
    let key = w.game.entities.key_for_number(1).unwrap();
    let ent = w.game.entities.get(key).unwrap();
    assert_eq!(ent.state.origin, demo2_base1_start(&demo2));
    assert_eq!(ent.game.health, 64);
    let pers = &w.game.client(key).unwrap().pers;
    assert_eq!(pers.inventory[shotgun], 1);
    assert_eq!(pers.inventory[shells], 30);
    assert_eq!(w.game.level.mapname, "demo2");
}

#[test]
fn local_game_follows_demo1s_exit_string_into_demo2() {
    let (Some(demo1), Some(demo2)) = (common::stock_bsp("demo1"), common::stock_bsp("demo2"))
    else {
        return;
    };
    let mut game = LocalGame::start("demo1", &demo1).expect("demo1 loads");
    play(&mut game, 3);
    game.change_map(&MapRequest::parse("demo2$base1"), &demo2)
        .expect("demo2 loads");
    assert_eq!(game.player().unwrap().0, demo2_base1_start(&demo2));
    assert_eq!(game.framenum(), 0);
    assert_eq!(game.configstring(CS_MODELS + 1), "maps/demo2.bsp");
    // The first game frame is due at 1000 ms, a frame after the load.
    play(&mut game, 2);
    assert_eq!(game.framenum(), 1);
    assert!(game.num_edicts() > 100, "demo2 spawned its entities");
}

#[test]
fn a_model_both_maps_use_is_registered_again_on_the_new_map() {
    let mut game = LocalGame::start("one", &armor_map()).expect("map loads");
    play(&mut game, 5);
    let map_two = common::floor_wall_bsp_with(concat!(
        "{\n\"classname\" \"worldspawn\"\n}\n",
        "{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}\n",
        "{\n\"classname\" \"item_armor_jacket\"\n\"origin\" \"64 0 16\"\n}\n",
    ));
    game.change_map(&MapRequest::parse("two"), &map_two)
        .expect("map two loads");
    play(&mut game, 2);
    let jacket = game
        .entity_states()
        .into_iter()
        .find(|s| s.number != LOCAL_PLAYER as i32 && s.modelindex != 0)
        .expect("the jacket is sent");
    assert_eq!(
        game.configstring(CS_MODELS + jacket.modelindex as usize),
        "models/items/armor/jacket/tris.md2"
    );
}
