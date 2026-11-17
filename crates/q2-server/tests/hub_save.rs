//! The server's save files in one process: a unit's levels keep their
//! state between visits (C SV_GameMap_f, SV_CheckForSavegame), entering a
//! level writes the autosave, and `savegame` / `loadgame` and the death
//! menu's `menu_loadgame` restore a game.

mod common;

use q2_server::init::MapRequest;
use q2_server::local::LocalGame;
use q2_shared::constants::{STAT_ARMOR, STAT_HEALTH};
use q2_shared::types::{UserCmd, Vec3f};

fn play(game: &mut LocalGame, frames: usize, cmd: UserCmd) {
    for _ in 0..frames {
        game.think(&cmd).unwrap();
        game.frame(100_000).unwrap();
    }
}

fn stat(game: &LocalGame, stat: usize) -> i16 {
    game.player().expect("player in game").1.stats[stat]
}

/// Go to `level`, reading its map as the platform does.
fn go(game: &mut LocalGame, level: &str) {
    let request = MapRequest::parse(level);
    let bsp = if request.map == "one" {
        common::armor_map()
    } else {
        common::floor_wall_bsp()
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
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    assert_eq!(stat(&game, STAT_ARMOR), 25, "picked up the jacket armor");

    go(&mut game, "two");
    go(&mut game, "one");
    common::idle(&mut game, 5);
    // The armor was taken on the first visit: picking up a second jacket
    // would raise the 25 to 50.
    assert_eq!(stat(&game, STAT_ARMOR), 25);
}

#[test]
fn a_starred_level_starts_its_unit_afresh() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    go(&mut game, "two");
    go(&mut game, "*one");
    common::idle(&mut game, 5);
    assert_eq!(stat(&game, STAT_ARMOR), 50, "the jacket is back");
}

#[test]
fn entering_a_level_writes_the_autosave() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    go(&mut game, "two");
    for file in ["server.ssv", "game.ssv", "one.sav", "one.sv2"] {
        assert!(game.saves().file("save0", file).is_some(), "save0/{file}");
    }
}

#[test]
fn loadgame_restores_the_saved_level_and_player() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    let (spawn, _) = game.player().unwrap();
    let forward = UserCmd {
        msec: 100,
        forwardmove: 200,
        ..UserCmd::default()
    };
    play(&mut game, 8, forward);
    let (origin, _) = game.player().unwrap();
    assert_ne!(
        origin, spawn,
        "moved off the start, so a restart would show"
    );
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
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 55); // C Cmd_Kill_f waits five seconds after a spawn
    let before = snapshot(&game);
    // Alive, so only the name refuses these.
    for slot in ["current", "../escape", "..", "a/b", "a\\b"] {
        game.console(&format!("save {slot}")).unwrap();
        assert!(snapshot(&game) == before, "{slot}: nothing written");
    }

    game.client_command("kill");
    common::idle(&mut game, 2);
    assert!(stat(&game, STAT_HEALTH) <= 0);
    game.console("save dead").unwrap();
    assert!(snapshot(&game) == before, "dead: nothing written");
}

#[test]
fn dying_loads_the_latest_save() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 55); // C Cmd_Kill_f waits five seconds after a spawn
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
        common::idle(&mut game, 1);
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
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    go(&mut game, "two");
    game.console("load save0").unwrap();
    assert_eq!(follow(&mut game), "two");
    assert_eq!(stat(&game, STAT_ARMOR), 25, "the armor carried into two");
}

#[test]
fn a_damaged_save_is_refused_and_play_goes_on() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    game.console("save quick").unwrap();
    let (origin, _) = game.player().unwrap();
    for file in ["game.ssv", "server.ssv", "one.sav", "one.sv2"] {
        let mut damaged = game.saves().clone();
        let path = format!("quick/{file}");
        let bytes = damaged.file("quick", file).unwrap().to_vec();
        damaged.insert(&path, bytes[..bytes.len() / 2].to_vec());
        *game.saves_mut() = damaged;

        game.console("load quick").unwrap();
        assert!(game.take_map_change().is_none(), "{file}: no level queued");
        common::idle(&mut game, 1);
        assert_eq!(
            game.player().map(|p| p.0),
            Some(origin),
            "{file}: still playing"
        );
        game.console("save quick").unwrap();
    }
}

#[test]
fn a_save_missing_a_file_is_refused_and_play_goes_on() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    game.console("save quick").unwrap();
    let (origin, _) = game.player().unwrap();
    let complete = game.saves().clone();
    for file in ["game.ssv", "one.sv2"] {
        let mut partial = q2_server::local::SaveFiles::default();
        for (path, bytes) in complete.iter() {
            if path != format!("quick/{file}") {
                partial.insert(path, bytes.to_vec());
            }
        }
        *game.saves_mut() = partial;
        let before = snapshot(&game);

        game.console("load quick").unwrap();
        assert!(game.take_map_change().is_none(), "{file}: no level queued");
        assert!(snapshot(&game) == before, "{file}: current kept");
        common::idle(&mut game, 1);
        assert_eq!(
            game.player().map(|p| p.0),
            Some(origin),
            "{file}: still playing"
        );
    }
}

/// Map one with a func_rotating spinning at 100 degrees a second.
fn spinner_map() -> Vec<u8> {
    common::floor_wall_bsp_with(&format!(
        "{}{}",
        common::ENTSTRING,
        "{\n\"classname\" \"func_rotating\"\n\"model\" \"*1\"\n\"speed\" \"100\"\n\"spawnflags\" \"1\"\n}\n"
    ))
}

fn spinner_yaw(game: &LocalGame) -> f32 {
    game.entity_states()
        .iter()
        .find(|s| s.number > 1 && s.modelindex != 0)
        .expect("the spinner")
        .angles[1]
}

#[test]
fn a_revisited_level_runs_ten_seconds_but_a_loaded_one_does_not() {
    // C SV_CheckForSavegame runs 100 frames only when not loading.
    let mut game = LocalGame::start("one", &spinner_map()).unwrap();
    common::idle(&mut game, 5);
    game.console("save quick").unwrap();
    let saved = spinner_yaw(&game);

    let request = MapRequest::parse("two");
    game.change_map(&request, &common::floor_wall_bsp())
        .unwrap();
    game.change_map(&MapRequest::parse("one"), &spinner_map())
        .unwrap();
    let revisited = spinner_yaw(&game);
    // Ten seconds at 100 degrees a second, plus the frame that left.
    assert!(
        (revisited - saved).rem_euclid(360.0) > 1.0,
        "revisit ran frames: {saved} -> {revisited}"
    );

    game.console("load quick").unwrap();
    let level = game.take_map_change().unwrap();
    game.change_map(&MapRequest::parse(&level), &spinner_map())
        .unwrap();
    assert_eq!(spinner_yaw(&game), saved, "a load runs no frames");
}

#[test]
fn two_loads_before_the_map_change_still_reconnect_the_player() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    game.console("save quick").unwrap();
    game.console("load quick").unwrap();
    game.console("load quick").unwrap();
    assert_eq!(follow(&mut game), "one");
    assert!(stat(&game, q2_shared::constants::STAT_HEALTH) > 0);
}

#[test]
fn a_save_of_a_map_the_game_data_lacks_is_refused_and_the_death_menu_passes_it() {
    // The browser's save store is shared by every pak on the page's origin.
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    game.console("save mine").unwrap();
    let mut other = LocalGame::start("elsewhere", &common::floor_wall_bsp()).unwrap();
    common::idle(&mut other, 5);
    other.console("save theirs").unwrap();
    for (path, bytes) in other.saves().iter() {
        if path.starts_with("theirs/") {
            game.saves_mut().insert(path, bytes.to_vec());
        }
    }
    game.set_has_map(|map| map != "elsewhere");

    game.console("load theirs").unwrap();
    assert!(game.take_map_change().is_none(), "refused");
    game.console("menu_loadgame").unwrap();
    assert_eq!(
        game.take_map_change().as_deref(),
        Some("one"),
        "the newest save it can load"
    );
}

#[test]
fn console_text_goes_to_the_server_only_for_its_own_commands() {
    let game = LocalGame::start("one", &common::armor_map()).unwrap();
    for server in ["gamemap base2", "save quick", "load quick", "menu_loadgame"] {
        assert!(game.is_server_command(server), "{server}");
    }
    // C's console forwards what it does not know to the game.
    for client in ["kill", "use Shotgun", "savegames", ""] {
        assert!(!game.is_server_command(client), "{client}");
    }
}
