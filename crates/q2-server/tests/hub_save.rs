//! The server's save files in one process: a unit's levels keep their
//! state between visits (C SV_GameMap_f, SV_CheckForSavegame), entering a
//! level writes the autosave, and `savegame` / `loadgame` and the death
//! menu's `menu_loadgame` restore a game.

mod common;

use q2_server::init::MapRequest;
use q2_server::local::LocalGame;
use q2_shared::constants::{STAT_ARMOR, STAT_HEALTH};
use q2_shared::types::{UserCmd, Vec3f};

/// Map one: the player starts on a jacket armor and picks it up.
fn armor_map() -> Vec<u8> {
    common::floor_wall_bsp_with(&format!(
        "{}{}",
        common::ENTSTRING,
        "{\n\"classname\" \"item_armor_jacket\"\n\"origin\" \"0 0 16\"\n}\n"
    ))
}

fn other_map() -> Vec<u8> {
    common::floor_wall_bsp()
}

fn play(game: &mut LocalGame, frames: usize, cmd: UserCmd) {
    for _ in 0..frames {
        game.think(&cmd).unwrap();
        game.frame(100_000).unwrap();
    }
}

fn idle(game: &mut LocalGame, frames: usize) {
    play(
        game,
        frames,
        UserCmd {
            msec: 100,
            ..UserCmd::default()
        },
    );
}

fn stat(game: &LocalGame, stat: usize) -> i16 {
    game.player().expect("player in game").1.stats[stat]
}

/// Go to `level`, reading its map as the platform does.
fn go(game: &mut LocalGame, level: &str) {
    let request = MapRequest::parse(level);
    let bsp = if request.map == "one" {
        armor_map()
    } else {
        other_map()
    };
    game.change_map(&request, &bsp).expect("level loads");
}

/// Follow the level change a command queued, as the platform does.
fn follow(game: &mut LocalGame) -> String {
    let level = game.take_map_change().expect("a level change is queued");
    go(game, &level);
    level
}

#[test]
fn a_unit_level_keeps_its_state_between_visits() {
    let mut game = LocalGame::start("one", &armor_map()).unwrap();
    idle(&mut game, 5);
    assert_eq!(stat(&game, STAT_ARMOR), 25, "picked up the jacket armor");

    go(&mut game, "two");
    go(&mut game, "one");
    idle(&mut game, 5);
    // The armor was taken on the first visit: picking up a second jacket
    // would raise the 25 to 50.
    assert_eq!(stat(&game, STAT_ARMOR), 25);
}

#[test]
fn a_starred_level_starts_its_unit_afresh() {
    let mut game = LocalGame::start("one", &armor_map()).unwrap();
    idle(&mut game, 5);
    go(&mut game, "two");
    go(&mut game, "*one");
    idle(&mut game, 5);
    assert_eq!(stat(&game, STAT_ARMOR), 50, "the jacket is back");
}

#[test]
fn entering_a_level_writes_the_autosave() {
    let mut game = LocalGame::start("one", &armor_map()).unwrap();
    idle(&mut game, 5);
    go(&mut game, "two");
    for file in ["server.ssv", "game.ssv", "one.sav", "one.sv2"] {
        assert!(game.saves().file("save0", file).is_some(), "save0/{file}");
    }
}

#[test]
fn loadgame_restores_the_saved_level_and_player() {
    let mut game = LocalGame::start("one", &armor_map()).unwrap();
    let forward = UserCmd {
        msec: 100,
        forwardmove: 200,
        ..UserCmd::default()
    };
    play(&mut game, 8, forward);
    let (origin, _) = game.player().unwrap();
    game.console("save quick").unwrap();

    go(&mut game, "two");
    game.console("load quick").unwrap();
    assert_eq!(follow(&mut game), "one");
    assert_eq!(stat(&game, STAT_ARMOR), 25);
    assert_eq!(game.player().unwrap().0, origin);
}

fn snapshot(game: &LocalGame) -> Vec<(String, Vec<u8>)> {
    game.saves()
        .iter()
        .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
        .collect()
}

#[test]
fn savegame_refuses_current_a_bad_name_and_a_dead_player() {
    let mut game = LocalGame::start("one", &armor_map()).unwrap();
    idle(&mut game, 55); // C Cmd_Kill_f waits five seconds after a spawn
    let before = snapshot(&game);
    game.console("save current").unwrap();
    assert!(snapshot(&game) == before, "nothing written");

    game.client_command("kill");
    idle(&mut game, 2);
    assert!(stat(&game, STAT_HEALTH) <= 0);
    game.console("save dead").unwrap();
    assert!(game.saves().file("dead", "server.ssv").is_none());
    game.console("save ../escape").unwrap();
    assert!(snapshot(&game) == before, "nothing written");
}

#[test]
fn dying_loads_the_latest_save() {
    let mut game = LocalGame::start("one", &armor_map()).unwrap();
    idle(&mut game, 55); // C Cmd_Kill_f waits five seconds after a spawn
    game.console("save first").unwrap();
    let saved_at: Vec3f = game.player().unwrap().0;

    game.client_command("kill");
    // C respawn: a button press after the death delay pushes the load menu
    // (menu_loadgame), which loads the latest save here.
    let attack = UserCmd {
        msec: 100,
        buttons: 1,
        ..UserCmd::default()
    };
    for _ in 0..60 {
        play(&mut game, 1, attack);
        idle(&mut game, 1);
        if let Some(level) = game.take_map_change() {
            go(&mut game, &level);
            assert!(stat(&game, STAT_HEALTH) > 0);
            assert_eq!(game.player().unwrap().0, saved_at);
            return;
        }
    }
    panic!("death never loaded the save");
}

#[test]
fn loading_the_autosave_restarts_the_level_with_the_entering_inventory() {
    let mut game = LocalGame::start("one", &armor_map()).unwrap();
    idle(&mut game, 5);
    go(&mut game, "two");
    game.console("load save0").unwrap();
    assert_eq!(follow(&mut game), "two");
    assert_eq!(stat(&game, STAT_ARMOR), 25, "the armor carried into two");
}
