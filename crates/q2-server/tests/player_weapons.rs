//! The local player's weapon through LocalGame: an attack command fires the
//! blaster (C ClientThink -> Think_Weapon) and its bolts kill a monster.

mod common;

use q2_server::local::LocalGame;
use q2_shared::constants::{BUTTON_ATTACK, CS_MODELS, EF_BLASTER};
use q2_shared::types::{EntityState, UserCmd};

/// A light soldier (20 health, two blaster bolts) 160 units in front of
/// the player start, which faces +y.
const SOLDIER: &str =
    "{\n\"classname\" \"monster_soldier_light\"\n\"origin\" \"0 160 24\"\n\"angle\" \"270\"\n}\n";

/// Soldier death moves start at FRAME_death101 (m_soldier.h).
const FIRST_DEATH_FRAME: i32 = 272;

fn started() -> LocalGame {
    let entities = format!("{}{SOLDIER}", common::ENTSTRING);
    let map = common::floor_wall_bsp_with(&entities);
    LocalGame::start("floor_wall", &map).expect("map loads")
}

fn run(game: &mut LocalGame, frames: usize, buttons: u8) -> Vec<Vec<EntityState>> {
    (0..frames)
        .map(|_| {
            game.think(&UserCmd {
                msec: 100,
                buttons,
                ..Default::default()
            });
            game.frame(100_000);
            game.entity_states()
        })
        .collect()
}

fn soldier(game: &LocalGame, states: &[EntityState]) -> EntityState {
    states
        .iter()
        .find(|s| {
            game.configstring(CS_MODELS + s.modelindex as usize)
                == "models/monsters/soldier/tris.md2"
        })
        .cloned()
        .expect("the soldier is sent")
}

#[test]
fn holding_attack_fires_blaster_bolts_that_kill_a_soldier() {
    let mut game = started();
    let frames = run(&mut game, 40, BUTTON_ATTACK);

    assert!(
        frames
            .iter()
            .any(|states| states.iter().any(|s| s.effects & EF_BLASTER != 0)),
        "a blaster bolt is sent"
    );
    let last = soldier(&game, frames.last().unwrap());
    assert!(
        last.frame >= FIRST_DEATH_FRAME,
        "dead: frame {}",
        last.frame
    );
}

#[test]
fn without_attack_the_soldier_lives() {
    let mut game = started();
    let frames = run(&mut game, 40, 0);
    assert!(frames
        .iter()
        .all(|states| states.iter().all(|s| s.effects & EF_BLASTER == 0)));
    let last = soldier(&game, frames.last().unwrap());
    assert!(
        last.frame < FIRST_DEATH_FRAME,
        "alive: frame {}",
        last.frame
    );
}
