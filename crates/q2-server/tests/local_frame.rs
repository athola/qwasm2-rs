//! What LocalGame hands the renderer each frame: the entities C's
//! SV_BuildClientFrame would send, and the model configstrings naming them.

mod common;

use q2_server::local::{LocalGame, LOCAL_PLAYER};
use q2_shared::constants::CS_MODELS;
use q2_shared::types::UserCmd;

fn started(extra: &str) -> LocalGame {
    let map = common::floor_wall_bsp_with(&format!("{}{extra}", common::ENTSTRING));
    let mut game = LocalGame::start("floor_wall", &map).expect("map loads");
    game.think(&UserCmd::default());
    game.frame(100_000);
    game
}

#[test]
fn brush_entities_are_sent_with_their_inline_model() {
    let game = started("{\n\"classname\" \"func_wall\"\n\"model\" \"*1\"\n}\n");
    let states = game.entity_states();
    let wall = states
        .iter()
        .find(|s| s.number > LOCAL_PLAYER as i32)
        .expect("func_wall sent");
    assert_eq!(
        game.configstring(CS_MODELS + wall.modelindex as usize),
        "*1"
    );
}

#[test]
fn triggers_and_point_entities_are_not_sent() {
    let game = started(concat!(
        "{\n\"classname\" \"trigger_multiple\"\n\"model\" \"*1\"\n}\n",
        "{\n\"classname\" \"info_notnull\"\n}\n",
    ));
    let numbers: Vec<i32> = game.entity_states().iter().map(|s| s.number).collect();
    assert_eq!(numbers, vec![LOCAL_PLAYER as i32], "only the player");
}

#[test]
fn the_player_is_sent_with_the_player_model_index() {
    let game = started("");
    let player = game
        .entity_states()
        .into_iter()
        .find(|s| s.number == LOCAL_PLAYER as i32)
        .expect("player sent");
    assert_eq!(player.modelindex, 255);
}
