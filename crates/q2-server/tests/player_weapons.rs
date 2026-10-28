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

#[test]
fn walking_over_a_shotgun_picks_it_up_and_raises_it() {
    let shotgun = "{\n\"classname\" \"weapon_shotgun\"\n\"origin\" \"0 0 16\"\n}\n";
    let map = common::floor_wall_bsp_with(&format!("{}{shotgun}", common::ENTSTRING));
    let mut game = LocalGame::start("floor_wall", &map).expect("map loads");
    let gun = |game: &LocalGame| {
        let (_, ps) = game.player().unwrap();
        game.configstring(CS_MODELS + ps.gunindex as usize)
            .to_string()
    };
    // Configstrings reach the server after the first game frame.
    run(&mut game, 2, 0);
    assert_eq!(gun(&game), "models/weapons/v_blast/tris.md2");
    run(&mut game, 20, 0);
    assert_eq!(gun(&game), "models/weapons/v_shotg/tris.md2");
}

#[test]
fn a_use_command_switches_back_to_the_blaster() {
    let shotgun = "{\n\"classname\" \"weapon_shotgun\"\n\"origin\" \"0 0 16\"\n}\n";
    let map = common::floor_wall_bsp_with(&format!("{}{shotgun}", common::ENTSTRING));
    let mut game = LocalGame::start("floor_wall", &map).expect("map loads");
    let gun = |game: &LocalGame| {
        let (_, ps) = game.player().unwrap();
        game.configstring(CS_MODELS + ps.gunindex as usize)
            .to_string()
    };
    run(&mut game, 20, 0);
    assert_eq!(gun(&game), "models/weapons/v_shotg/tris.md2");
    game.client_command("use Blaster");
    run(&mut game, 20, 0);
    assert_eq!(gun(&game), "models/weapons/v_blast/tris.md2");
    game.client_command("weaplast");
    run(&mut game, 20, 0);
    assert_eq!(gun(&game), "models/weapons/v_shotg/tris.md2");
}

#[test]
fn firing_sends_the_blaster_muzzle_flash_to_the_local_client() {
    use q2_shared::constants::MZ_BLASTER;
    use q2_shared::protocol::SvcOp;
    let mut game = started();
    run(&mut game, 10, 0); // the blaster comes up
    game.take_messages();
    run(&mut game, 1, BUTTON_ATTACK);
    let flash = vec![SvcOp::MuzzleFlash as u8, 1, 0, MZ_BLASTER as u8];
    assert!(game.take_messages().contains(&flash));
}
