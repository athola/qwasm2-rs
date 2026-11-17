//! Every map in the retail game data spawns, runs, saves, and loads: the
//! smoke run under the save/load campaign harness (#15). It needs the
//! retail paks in `gamedata/retail/baseq2` and skips without them.
//!
//! Its own test binary so the subscriber that collects the game's prints
//! is the only one in the process (see `game_print_log.rs`).

mod common;

use common::{retail_fs, ServerWorld};
use q2_shared::types::UserCmd;

const FRAME_US: u32 = 100_000;

/// One second of walking forward and turning, ten game frames.
fn walk_a_second(world: &mut ServerWorld) {
    for i in 0..10i16 {
        world.think(&UserCmd {
            msec: 100,
            forwardmove: 200,
            angles: [0, i.wrapping_mul(1200), 0],
            ..UserCmd::default()
        });
        world.frame(FRAME_US);
    }
}

/// `start`, but a map that fails to load is an error, not a panic.
fn try_start(name: &str, bsp: &[u8]) -> Result<ServerWorld, String> {
    std::panic::catch_unwind(|| ServerWorld::start(name, bsp)).map_err(|panic| {
        panic
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "panicked".into())
    })
}

#[test]
fn every_retail_map_spawns_runs_saves_and_loads() {
    let Some(fs) = retail_fs() else {
        return;
    };
    let (prints, _guard) = common::capture_info_logs();

    let mut maps: Vec<String> = fs
        .list_files("bsp")
        .into_iter()
        .filter_map(|path| {
            Some(
                path.strip_prefix("maps/")?
                    .strip_suffix(".bsp")?
                    .to_string(),
            )
        })
        .collect();
    maps.sort();
    let mut failures = Vec::new();
    for map in &maps {
        let bsp = fs
            .load_file(&format!("maps/{map}.bsp"))
            .expect("listed map loads");
        prints.lock().unwrap().clear();
        let mut world = match try_start(map, &bsp) {
            Ok(world) => world,
            Err(e) => {
                failures.push(format!("{map}: spawn: {e}"));
                continue;
            }
        };
        let mut saved = None;
        for second in 0..30 {
            walk_a_second(&mut world);
            match (world.game.write_game(true), world.game.encode_level()) {
                (Ok(game), Ok(level)) => saved = Some((game, level)),
                (Err(e), _) | (_, Err(e)) => {
                    failures.push(format!("{map}: save at {second} s: {e}"));
                    break;
                }
            }
        }
        if let Some((game, level)) = saved {
            let mut fresh = common::new_world(&world.gi);
            if let Err(e) = fresh
                .read_game(&game)
                .and_then(|()| fresh.read_level(&level))
            {
                failures.push(format!("{map}: load: {e}"));
            }
        }
        let text = String::from_utf8_lossy(&prints.lock().unwrap()).into_owned();
        let mut missing: Vec<&str> = text
            .lines()
            .filter_map(|line| {
                line.split("game: ")
                    .nth(1)?
                    .strip_suffix(" doesn't have a spawn function")
            })
            .collect();
        missing.sort_unstable();
        missing.dedup();
        if !missing.is_empty() {
            eprintln!("NOTE {map}: no spawn function for {}", missing.join(", "));
        }
    }
    eprintln!("{} maps run", maps.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
