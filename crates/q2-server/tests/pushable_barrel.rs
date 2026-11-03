//! Walking into a misc_explobox through LocalGame pushes it along (C
//! barrel_touch from ClientThink's pmove touches), once it has dropped to
//! the floor.

mod common;

use q2_server::local::LocalGame;
use q2_shared::constants::CS_MODELS;
use q2_shared::types::{EntityState, UserCmd};

/// A barrel 64 units in front of the player start, which faces +y.
const BARREL: &str = "{\n\"classname\" \"misc_explobox\"\n\"origin\" \"0 64 0\"\n}\n";

fn barrel(game: &LocalGame) -> EntityState {
    game.entity_states()
        .into_iter()
        .find(|s| {
            game.configstring(CS_MODELS + s.modelindex as usize)
                == "models/objects/barrels/tris.md2"
        })
        .expect("the barrel is sent")
}

fn run(game: &mut LocalGame, frames: usize, forwardmove: i16) {
    for _ in 0..frames {
        game.think(&UserCmd {
            msec: 100,
            forwardmove,
            ..Default::default()
        });
        game.frame(100_000);
    }
}

#[test]
fn walking_into_a_barrel_pushes_it_forward() {
    let map = common::floor_wall_bsp_with(&format!("{}{BARREL}", common::ENTSTRING));
    let mut game = LocalGame::start("floor_wall", &map).expect("map loads");
    // It drops to the floor two frames in; standing still it stays put.
    run(&mut game, 5, 0);
    let before = barrel(&game).origin;
    run(&mut game, 20, 400);
    let after = barrel(&game).origin;
    assert!(
        after.y > before.y + 8.0,
        "pushed along +y: {before:?} -> {after:?}"
    );
    assert!((after.x - before.x).abs() < 1.0, "straight ahead");
}
