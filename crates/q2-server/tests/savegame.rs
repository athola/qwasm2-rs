//! Save and load on the demo maps: every callback and monster move the
//! maps reach can be named, and a loaded level runs on as the original.

mod common;

use common::{stock_bsp, ServerWorld};
use q2_shared::types::UserCmd;

const FRAME_US: u32 = 100_000;

/// Run `frames` game frames, one command before each, moving forward and
/// turning so the player wakes monsters and crosses triggers. Stepping by
/// game frame, not wall time, gives a loaded run the same sequence: a map
/// load restarts the server clock (C SV_SpawnServer), so wall time puts
/// its game frames elsewhere.
fn play(world: &mut ServerWorld, frames: u32) {
    play_from(world, frames, [0; 3]);
}

/// `play` for a client whose view is `view_offset` from where its angles
/// say (short angle units).
fn play_from(world: &mut ServerWorld, frames: u32, view_offset: [i16; 3]) {
    for _ in 0..frames {
        let framenum = world.game.level.framenum;
        let cmd = UserCmd {
            msec: 100,
            forwardmove: 200,
            angles: [
                view_offset[0],
                (framenum as i16)
                    .wrapping_mul(91)
                    .wrapping_add(view_offset[1]),
                view_offset[2],
            ],
            ..UserCmd::default()
        };
        world.think(&cmd);
        // A game frame is due within the server's restarted first second.
        for _ in 0..=10 {
            if world.game.level.framenum != framenum {
                break;
            }
            world.frame(FRAME_US);
        }
        assert_ne!(
            world.game.level.framenum, framenum,
            "the server stopped running game frames"
        );
    }
}

#[test]
fn every_demo_map_saves_after_a_minute_of_play() {
    for map in ["demo1", "demo2", "demo3"] {
        let Some(bsp) = stock_bsp(map) else {
            return;
        };
        let mut world = ServerWorld::start(map, &bsp);
        for second in 0..60 {
            play(&mut world, 10);
            if let Err(e) = world.game.encode_level() {
                panic!("{map} at {second} s: {e}");
            }
        }
        world.game.write_game(false).expect("game saves");
    }
}

/// C SV_Loadgame_f in one process: a fresh game reads the game file, the
/// map spawns, its level file replaces the entities (SV_CheckForSavegame),
/// and the client connects and begins on the body waiting for it.
fn load(map: &str, bsp: &[u8], game_file: &[u8], level_file: &[u8]) -> ServerWorld {
    let gi = std::rc::Rc::new(q2_server::ServerGameImport::new());
    let mut game = common::new_world(&gi);
    game.read_game(game_file).expect("game loads");
    let mut svs = q2_server::state::ServerStatic::default();
    svs.init_game(1);
    let mut sv = q2_server::state::Server::default();
    sv.map(&mut svs, map, bsp, "", &gi, &mut game)
        .expect("map loads");
    gi.clear_world();
    game.read_level(level_file).expect("level loads");
    svs.connect_local_client(&mut game, "")
        .expect("client accepted");
    ServerWorld { sv, svs, gi, game }
}

#[test]
fn a_loaded_level_runs_on_exactly_as_the_original() {
    let Some(bsp) = stock_bsp("demo1") else {
        return;
    };
    let mut original = ServerWorld::start("demo1", &bsp);
    play(&mut original, 150);
    let game_file = original.game.write_game(false).unwrap();
    let level_file = original.game.encode_level().unwrap();

    let mut loaded = load("demo1", &bsp, &game_file, &level_file);
    // The loaded client connected anew, its angles restarting from zero,
    // and ClientBegin moved the saved view into its delta angles. A real
    // client keeps looking the same way, so its angles carry the change.
    let delta = |w: &ServerWorld| w.game.clients[0].ps.pmove.delta_angles;
    let (was, now) = (delta(&original), delta(&loaded));
    let view_offset: [i16; 3] = std::array::from_fn(|i| was[i].wrapping_sub(now[i]));
    play(&mut original, 200);
    play_from(&mut loaded, 200, view_offset);
    let differences = differences(&original, &loaded);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

/// Where two worlds differ, as `path: a vs b`, apart from what C changes
/// on a load as well: ReadLevel relinks every entity, and SV_LinkEdict
/// counts each link, so `linkcount` runs one ahead; SV_LinkEdict never
/// area-links a SOLID_NOT entity, so one left linked when it stopped being
/// solid (a triggered monster waiting to spawn) comes back unlinked with
/// `s.solid` 0; free entities are not saved and come back zeroed; and
/// ClientBegin moves the view into the delta angles, which the test
/// client's angles make up for (`resp.cmd_angles` records those angles).
fn differences(a: &ServerWorld, b: &ServerWorld) -> Vec<String> {
    fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).expect("game state is JSON")
    }
    let mut out = Vec::new();
    diff(
        "clients",
        &json(&a.game.clients),
        &json(&b.game.clients),
        &mut out,
    );
    diff("game", &json(&a.game.game), &json(&b.game.game), &mut out);
    diff(
        "level",
        &json(&a.game.level),
        &json(&b.game.level),
        &mut out,
    );
    for (key, ent) in a.game.entities.iter() {
        let path = format!("entity {} ({})", ent.state.number, ent.game.classname);
        match b.game.entities.get(key) {
            Some(other) if !ent.in_use && !other.in_use => {}
            Some(other) => {
                let mut lines = Vec::new();
                diff(&path, &json(ent), &json(other), &mut lines);
                if ent.solid == q2_shared::types::Solid::Not {
                    lines.retain(|l| !l.contains(".area_linked:") && !l.contains(".state.solid:"));
                }
                out.extend(lines);
            }
            None => out.push(format!("{path}: missing")),
        }
    }
    out.retain(|line| {
        !line.contains(".linkcount:")
            && !line.contains(".delta_angles[")
            && !line.contains(".resp.cmd_angles[")
    });
    out
}

fn diff(path: &str, a: &serde_json::Value, b: &serde_json::Value, out: &mut Vec<String>) {
    use serde_json::Value::{Array, Null, Object};
    match (a, b) {
        (Object(x), Object(y)) => {
            for (k, v) in x {
                diff(&format!("{path}.{k}"), v, y.get(k).unwrap_or(&Null), out);
            }
        }
        (Array(x), Array(y)) if x.len() == y.len() => {
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}[{i}]"), v, w, out);
            }
        }
        _ if a != b => out.push(format!("{path}: {a} vs {b}")),
        _ => {}
    }
}
