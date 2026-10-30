//! What LocalGame hands the renderer each frame: the entities C's
//! SV_BuildClientFrame would send, and the model configstrings naming them.

mod common;

use q2_server::local::{LocalGame, LOCAL_PLAYER};
use q2_shared::constants::{CS_MODELS, EV_PLAYER_TELEPORT, SVF_NOCLIENT};
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

#[test]
fn demo1_sends_only_entities_in_the_players_pvs_and_always_the_player() {
    let Some(map) = common::demo1_bsp() else {
        return;
    };
    let mut game = LocalGame::start("demo1", &map).expect("demo1 loads");
    game.think(&UserCmd::default());
    game.frame(100_000);
    // The same map in a server whose game the test can read.
    let world = common::ServerWorld::start("demo1", &map);
    let modelled = world
        .game
        .entities
        .iter()
        .filter(|(_, e)| {
            e.in_use
                && e.state.number > 0
                && e.state.modelindex != 0
                && e.svflags & SVF_NOCLIENT == 0
        })
        .count();
    let sent = game.entity_states();
    assert!(sent.iter().any(|s| s.number == LOCAL_PLAYER as i32));
    assert!(
        sent.len() < modelled,
        "sent {} of {modelled} modelled entities",
        sent.len()
    );
}

/// Game frames run in `seconds` of frames of `frame_us` each.
fn game_frames_in(seconds: u32, frame_us: u32) -> i32 {
    let mut game = started("");
    let first = game.framenum();
    for _ in 0..seconds * 1_000_000 / frame_us {
        game.frame(frame_us);
    }
    game.framenum() - first
}

#[test]
fn game_clock_keeps_pace_at_any_refresh_rate() {
    // Sub-millisecond frame time is carried, not dropped per call: 144 Hz
    // runs as many 10 Hz game frames as 60 Hz.
    let at_60hz = game_frames_in(10, 16_666);
    let at_144hz = game_frames_in(10, 6_944);
    assert!(
        (at_60hz - at_144hz).abs() <= 1,
        "60 Hz ran {at_60hz} game frames, 144 Hz ran {at_144hz}"
    );
    assert!(at_60hz >= 99, "{at_60hz} game frames in 10 s");
}

#[test]
fn hidden_brush_entity_is_not_sent_and_visible_one_is() {
    // Player start in cluster 0 (y > 0); one wall in each cluster.
    let walls = |y: i32| {
        format!("{{\n\"classname\" \"func_wall\"\n\"model\" \"*1\"\n\"origin\" \"0 {y} 0\"\n}}\n")
    };
    let entities = format!(
        "{{\n\"classname\" \"worldspawn\"\n}}\n{{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 200 24\"\n}}\n{}{}",
        walls(300),
        walls(-300)
    );
    let mut game = LocalGame::start("two_cluster", &common::two_cluster_bsp_with(&entities))
        .expect("map loads");
    game.think(&UserCmd::default());
    game.frame(100_000);
    let sent: Vec<f32> = game
        .entity_states()
        .iter()
        .filter(|s| s.number != LOCAL_PLAYER as i32)
        .map(|s| s.origin.y)
        .collect();
    assert_eq!(sent, [300.0]);
}

#[test]
fn framenum_counts_server_frames_from_the_map_load() {
    let mut game = started("");
    let before = game.framenum();
    game.frame(100_000);
    assert_eq!(game.framenum(), before + 1);
}

#[test]
fn a_level_exit_hands_the_gamemap_command_to_the_local_server() {
    let mut game = started(concat!(
        // Delayed past the two settling frames, when the player is in.
        "{\n\"classname\" \"trigger_always\"\n\"target\" \"exit\"\n\"delay\" \"0.5\"\n}\n",
        "{\n\"classname\" \"target_changelevel\"\n\"targetname\" \"exit\"\n\"map\" \"base2\"\n}\n",
    ));
    let mut next = None;
    for _ in 0..10 {
        game.think(&UserCmd::default());
        game.frame(100_000);
        next = next.or_else(|| game.take_map_change());
    }
    assert_eq!(next.as_deref(), Some("base2"));
    assert_eq!(game.take_map_change(), None, "taken once");
}

#[test]
fn demo1_area_bits_show_the_players_own_area() {
    let Some(map) = common::demo1_bsp() else {
        return;
    };
    let mut game = LocalGame::start("demo1", &map).expect("demo1 loads");
    game.think(&UserCmd::default());
    game.frame(100_000);
    let area = game.player_area().expect("player in an area");
    let bits = game.area_bits().expect("area bits");
    let byte = bits[(area >> 3) as usize];
    assert_ne!(byte & (1 << (area & 7)), 0, "own area {area} visible");
}

#[test]
fn a_teleport_event_is_sent_in_exactly_one_frame() {
    // The player starts on a teleporter pad and is sent to the dest pad.
    let mut game = started(concat!(
        "{\n\"classname\" \"misc_teleporter\"\n\"target\" \"dest\"\n\"origin\" \"0 0 16\"\n}\n",
        "{\n\"classname\" \"misc_teleporter_dest\"\n\"targetname\" \"dest\"\n\"origin\" \"128 0 16\"\n}\n",
    ));
    let mut frames_with_event = 0;
    let mut framenum = game.framenum();
    for _ in 0..20 {
        game.think(&UserCmd::default());
        game.frame(100_000);
        if game.framenum() == framenum {
            continue;
        }
        framenum = game.framenum();
        let player = game
            .entity_states()
            .into_iter()
            .find(|s| s.number == LOCAL_PLAYER as i32)
            .expect("player sent");
        if player.event == EV_PLAYER_TELEPORT {
            frames_with_event += 1;
        }
    }
    let (origin, _) = game.player().expect("player in game");
    assert_eq!(origin.x, 128.0, "teleported to the dest pad");
    // C SV_PrepWorldFrame: events only last for a single message.
    assert_eq!(frames_with_event, 1);
}
