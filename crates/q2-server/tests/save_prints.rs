//! What the save commands print when they refuse or misuse (C
//! SV_Savegame_f, SV_Loadgame_f and the death menu). Its own test binary
//! so its log subscriber is the only one in the process (see
//! `game_print_log.rs`).

mod common;

use q2_server::init::MapRequest;
use q2_server::local::{LocalGame, SaveFiles};

/// The server prints `command` makes, and whether it changed the saves.
fn run(game: &mut LocalGame, command: &str) -> (String, bool) {
    let before = game.saves().clone();
    let (out, guard) = common::capture_info_logs();
    game.console(command).unwrap();
    drop(guard);
    let text = String::from_utf8(out.lock().unwrap().clone()).unwrap();
    let changed = game.saves().iter().ne(before.iter());
    (text, changed)
}

#[test]
fn save_commands_print_why_they_refuse() {
    let mut game = LocalGame::start("one", &common::armor_map()).unwrap();
    common::idle(&mut game, 5);
    for (command, print) in [
        ("save", "USAGE: savegame <directory>"),
        ("load", "USAGE: loadgame <directory>"),
        ("save a b", "USAGE: savegame <directory>"),
        ("save ..", "Bad savedir."),
        ("load ../x", "Bad savedir."),
        ("load nope", "No such savegame: nope"),
        ("menu_loadgame x", "USAGE: menu_loadgame"),
    ] {
        let (text, changed) = run(&mut game, command);
        assert!(text.contains(print), "{command}: {text}");
        assert!(!changed, "{command}: saves changed");
    }

    *game.saves_mut() = SaveFiles::default();
    let (text, _) = run(&mut game, "menu_loadgame");
    assert!(text.contains("No savegame to load"), "{text}");
    assert!(game.take_map_change().is_none());

    game.set_cvar("deathmatch", "1");
    let (text, changed) = run(&mut game, "save dm");
    assert!(text.contains("Can't savegame in a deathmatch"), "{text}");
    assert!(!changed);
    game.set_cvar("deathmatch", "0");

    // Between a load and its map change the server is not in a game.
    run(&mut game, "save quick");
    run(&mut game, "load quick");
    let (text, changed) = run(&mut game, "save late");
    assert!(text.contains("You must be in a game to save."), "{text}");
    assert!(!changed);
    let level = game.take_map_change().unwrap();
    game.change_map(&MapRequest::parse(&level), &common::armor_map())
        .unwrap();
}
