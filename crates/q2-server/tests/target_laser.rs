//! A map's target_laser through LocalGame: it starts a second in, is sent
//! as a beam ending at the wall it hits, and sparks there (C
//! target_laser_start / target_laser_think).

mod common;

use q2_server::local::LocalGame;
use q2_shared::constants::{RF_BEAM, RF_TRANSLUCENT, TE_LASER_SPARKS};
use q2_shared::protocol::SvcOp;
use q2_shared::types::{EntityState, UserCmd, Vec3f};

/// A red laser (spawnflags 1 on, 2 red) firing +x at the wall.
const LASER: &str = "{\n\"classname\" \"target_laser\"\n\"origin\" \"0 100 24\"\n\"angle\" \"0\"\n\"spawnflags\" \"3\"\n}\n";

fn run(game: &mut LocalGame, frames: usize) -> (Vec<EntityState>, Vec<Vec<u8>>) {
    let mut messages = Vec::new();
    for _ in 0..frames {
        game.think(&UserCmd {
            msec: 100,
            ..Default::default()
        });
        game.frame(100_000);
        messages.extend(game.take_messages());
    }
    (game.entity_states(), messages)
}

fn beams(states: &[EntityState]) -> Vec<&EntityState> {
    states
        .iter()
        .filter(|s| s.renderfx & RF_BEAM != 0)
        .collect()
}

fn sparks(messages: &[Vec<u8>]) -> usize {
    messages
        .iter()
        .filter(|m| m.starts_with(&[SvcOp::TempEntity as u8, TE_LASER_SPARKS as u8]))
        .count()
}

#[test]
fn a_target_laser_beams_to_the_wall_and_sparks_where_it_hits() {
    let map = common::floor_wall_bsp_with(&format!("{}{LASER}", common::ENTSTRING));
    let mut game = LocalGame::start("floor_wall", &map).expect("map loads");
    let (states, messages) = run(&mut game, 5);
    assert!(beams(&states).is_empty(), "not started before a second");
    assert_eq!(sparks(&messages), 0);

    let (states, messages) = run(&mut game, 10);
    let beam = beams(&states);
    assert_eq!(beam.len(), 1);
    let beam = beam[0];
    assert_eq!(beam.renderfx, RF_BEAM | RF_TRANSLUCENT);
    assert_eq!(beam.modelindex, 1);
    assert_eq!((beam.frame, beam.skinnum), (4, 0xf2f2_f0f0_u32 as i32));
    assert_eq!(beam.origin, Vec3f::new(0.0, 100.0, 24.0));
    assert!(
        (beam.old_origin.x - common::WALL_X).abs() < 0.5,
        "{:?}",
        beam.old_origin
    );
    // Sparks when it switches on, then not while it holds still.
    assert_eq!(sparks(&messages), 1);
}

#[test]
fn demo3s_lasers_are_on_from_the_start() {
    let Some(map) = common::stock_bsp("demo3") else {
        return;
    };
    let mut world = common::ServerWorld::start("demo3", &map);
    for _ in 0..15 {
        world.think(&UserCmd {
            msec: 100,
            ..Default::default()
        });
        world.frame(100_000);
    }
    let lasers: Vec<_> = world
        .game
        .entities
        .iter()
        .filter(|(_, e)| e.game.classname == "target_laser")
        .map(|(_, e)| e)
        .collect();
    assert_eq!(lasers.len(), 2);
    for laser in lasers {
        assert_eq!(laser.svflags & q2_shared::constants::SVF_NOCLIENT, 0, "on");
        assert_eq!(laser.state.renderfx, RF_BEAM | RF_TRANSLUCENT);
        let end = laser.state.old_origin;
        assert!(
            end.x < laser.state.origin.x,
            "fires along angle 180: {end:?}"
        );
    }
}
