//! Sounds the game starts reach the local client through
//! `LocalGame::take_sounds` (C `SV_StartSound` -> `svc_sound`).

mod common;

use q2_server::local::LocalGame;
use q2_shared::constants::CS_SOUNDS;
use q2_shared::types::{UserCmd, Vec3f};

#[test]
fn a_triggered_target_speaker_is_heard_once_at_its_origin() {
    let map = common::floor_wall_bsp_with(&format!(
        "{}{}",
        common::ENTSTRING,
        concat!(
            "{\n\"classname\" \"trigger_always\"\n\"target\" \"spk\"\n}\n",
            "{\n\"classname\" \"target_speaker\"\n\"targetname\" \"spk\"\n",
            "\"noise\" \"world/klaxon2.wav\"\n\"origin\" \"32 0 48\"\n}\n",
        )
    ));
    let mut game = LocalGame::start("floor_wall", &map).expect("map loads");
    let mut heard = Vec::new();
    for _ in 0..5 {
        game.think(&UserCmd::default());
        game.frame(100_000);
        heard.extend(game.take_sounds());
    }
    assert_eq!(heard.len(), 1, "{heard:?}");
    let sound = heard[0];
    assert_eq!(sound.origin, Some(Vec3f::new(32.0, 0.0, 48.0)));
    assert_eq!(sound.channel, 2, "CHAN_VOICE");
    assert_eq!(
        game.configstring(CS_SOUNDS + sound.sound_index as usize),
        "world/klaxon2.wav"
    );
}
