use super::wav::tests::{fmt, riff};
use super::*;
use q2_shared::constants::CS_SOUNDS;
use std::collections::HashMap;

const PLAYER: i32 = 1;

/// A 100 ms one-shot (100 samples at 1 kHz).
fn one_shot() -> Vec<u8> {
    riff(&[(b"fmt ", fmt(1, 1000, 8)), (b"data", vec![200; 100])])
}

/// A 100 ms sound whose cue point loops back to sample 50.
fn cue_looped() -> Vec<u8> {
    let mut cue = vec![0u8; 28];
    cue[24..28].copy_from_slice(&50i32.to_le_bytes());
    riff(&[
        (b"fmt ", fmt(1, 1000, 8)),
        (b"cue ", cue),
        (b"data", vec![200; 100]),
    ])
}

fn files() -> HashMap<String, Vec<u8>> {
    [
        ("sound/a.wav", one_shot()),
        ("sound/b.wav", one_shot()),
        ("sound/loop.wav", cue_looped()),
        ("sound/player/male/jump1.wav", one_shot()),
        ("players/female/jump1.wav", one_shot()),
        ("raw/path.wav", one_shot()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Configstrings: sounds 1..=5 and the player skins given.
fn strings(skin: &'static str) -> impl Fn(usize) -> &'static str {
    move |i| match i {
        _ if i == CS_SOUNDS + 1 => "a.wav",
        _ if i == CS_SOUNDS + 2 => "b.wav",
        _ if i == CS_SOUNDS + 3 => "loop.wav",
        _ if i == CS_SOUNDS + 4 => "*jump1.wav",
        _ if i == CS_SOUNDS + 5 => "#raw/path.wav",
        _ if i == CS_PLAYERSKINS => skin,
        _ => "",
    }
}

fn system() -> SoundSystem {
    let mut sys = SoundSystem::new(PLAYER);
    let files = files();
    sys.register_sounds(strings(""), |p| files.get(p).cloned());
    sys
}

fn start(sys: &mut SoundSystem, ev: StartSound) {
    let files = files();
    sys.start_sound(&ev, strings(""), |p| files.get(p).cloned());
}

fn sound(entnum: i32, entchannel: i32, sound_index: i32) -> StartSound {
    StartSound {
        entnum,
        entchannel,
        sound_index,
        volume: 1.0,
        attenuation: 1.0,
        timeofs: 0.0,
        origin: None,
    }
}

fn listener() -> Listener {
    Listener {
        origin: Vec3f::ZERO,
        right: Vec3f::new(0.0, -1.0, 0.0),
    }
}

fn update(sys: &mut SoundSystem, now: f64) -> Mix {
    sys.update(now, &listener(), |_| None, &[])
}

fn plays(mix: &Mix) -> Vec<(usize, SfxId)> {
    mix.commands
        .iter()
        .filter_map(|c| match *c {
            MixCommand::Play { channel, sfx, .. } => Some((channel, sfx)),
            _ => None,
        })
        .collect()
}

fn stops(mix: &Mix) -> Vec<usize> {
    mix.commands
        .iter()
        .filter_map(|c| match *c {
            MixCommand::Stop { channel } => Some(channel),
            _ => None,
        })
        .collect()
}

// -- spatialization (C SDL_SpatializeOrigin) --------------------------------

#[test]
fn a_sound_at_the_listener_is_half_volume_in_each_ear() {
    // C normalizes a zero vector to zero, so the dot product is 0.
    assert_eq!(
        spatialize_origin(Vec3f::ZERO, &listener(), 255.0, 0.0005),
        (127, 127)
    );
}

#[test]
fn a_sound_to_the_right_attenuates_past_eighty_units() {
    // dist = (1000 - 80) * 0.0005 = 0.46; right = 255 * 0.54 * 1.
    let right_of = Vec3f::new(0.0, -1000.0, 0.0);
    assert_eq!(
        spatialize_origin(right_of, &listener(), 255.0, 0.0005),
        (0, 137)
    );
    let left_of = Vec3f::new(0.0, 50.0, 0.0);
    assert_eq!(
        spatialize_origin(left_of, &listener(), 255.0, 0.0005),
        (255, 0)
    );
}

#[test]
fn a_sound_out_of_range_is_silent_not_negative() {
    let far = Vec3f::new(5000.0, 0.0, 0.0);
    assert_eq!(spatialize_origin(far, &listener(), 255.0, 0.001), (0, 0));
}

#[test]
fn attenuation_none_is_full_volume_in_both_ears_anywhere() {
    let far = Vec3f::new(0.0, -5000.0, 0.0);
    assert_eq!(spatialize_origin(far, &listener(), 200.0, 0.0), (200, 200));
}

#[test]
fn static_attenuation_falls_off_twice_as_fast() {
    assert_eq!(dist_mult(1.0), 0.0005);
    assert_eq!(dist_mult(2.0), 0.001);
    assert_eq!(dist_mult(ATTN_STATIC), 0.003);
}

#[test]
fn gain_follows_c_s_volume_scaling() {
    assert_eq!(gain(0), 0.0);
    assert_eq!(gain(255), 255.0 * 179.0 / 65536.0);
}

#[test]
fn listener_right_is_c_angle_vectors_right() {
    let l = Listener::from_view(Vec3f::ZERO, Vec3f::new(0.0, 90.0, 0.0));
    assert!((l.right - Vec3f::new(1.0, 0.0, 0.0)).length() < 1e-6);
    let l = Listener::from_view(Vec3f::ZERO, Vec3f::ZERO);
    assert!((l.right - Vec3f::new(0.0, -1.0, 0.0)).length() < 1e-6);
}

// -- registration (C CL_RegisterSounds, S_LoadSound) -------------------------

#[test]
fn registration_loads_sound_prefixed_and_raw_paths_and_stops_at_a_gap() {
    let sys = system();
    let id = |i: usize| sys.precache[i].unwrap();
    assert!(sys.sound(id(1)).is_some(), "sound/a.wav");
    assert!(sys.sound(id(5)).is_some(), "#raw/path.wav");
    assert!(sys.sound(id(4)).is_none(), "* names never load");
    assert_eq!(sys.precache[6], None);
    assert_eq!(sys.sound(id(3)).unwrap().loop_start, Some(50));
}

#[test]
fn registering_a_new_map_drops_the_old_precache_but_keeps_loaded_sounds() {
    let mut sys = system();
    let a = sys.precache[1].unwrap();
    sys.register_sounds(
        |i| if i == CS_SOUNDS + 1 { "b.wav" } else { "" },
        |p| files().get(p).cloned(),
    );
    assert_eq!(sys.precache[2], None);
    assert_eq!(sys.name(sys.precache[1].unwrap()), "b.wav");
    assert!(sys.sound(a).is_some());
}

// -- starting sounds (C S_StartSound, S_IssuePlaysound) ----------------------

#[test]
fn an_unregistered_sound_index_is_ignored() {
    let mut sys = system();
    start(&mut sys, sound(2, 1, 9));
    start(&mut sys, sound(2, 1, -1));
    assert_eq!(plays(&update(&mut sys, 0.0)), vec![]);
}

#[test]
fn started_sound_plays_at_the_next_update_with_its_offset_as_delay() {
    let mut sys = system();
    let mut ev = sound(2, 1, 1);
    ev.timeofs = 0.1;
    ev.origin = Some(Vec3f::new(0.0, -100.0, 0.0));
    start(&mut sys, ev);
    let mix = update(&mut sys, 0.0);
    // dist (100 - 80) * 0.0005 = 0.01, all on the right.
    assert_eq!(
        mix.commands,
        vec![MixCommand::Play {
            channel: 0,
            sfx: sys.precache[1].unwrap(),
            left: gain(0),
            right: gain((255.0f32 * 0.99) as i32),
            delay: 0.1,
        }]
    );
    assert_eq!(update(&mut sys, 50.0).commands.len(), 1, "still playing");
    // Ends 100 ms after the 100 ms delay.
    assert_eq!(stops(&update(&mut sys, 201.0)), vec![0]);
}

#[test]
fn the_view_entitys_sounds_are_full_volume() {
    let mut sys = system();
    let mut ev = sound(PLAYER, 1, 1);
    ev.volume = 0.5;
    ev.origin = Some(Vec3f::new(0.0, -3000.0, 0.0));
    start(&mut sys, ev);
    let mix = update(&mut sys, 0.0);
    let MixCommand::Play { left, right, .. } = mix.commands[0] else {
        panic!("{mix:?}");
    };
    assert_eq!((left, right), (gain(127), gain(127)));
}

#[test]
fn entity_sound_follows_the_entity_and_stops_when_out_of_range() {
    let mut sys = system();
    start(&mut sys, sound(2, 1, 1));
    let near = |_: i32| Some(Vec3f::new(0.0, 10.0, 0.0));
    let mix = sys.update(0.0, &listener(), near, &[]);
    assert!(matches!(
        mix.commands[0],
        MixCommand::Play { left, .. } if left == gain(255)
    ));
    let far = |_: i32| Some(Vec3f::new(5000.0, 0.0, 0.0));
    assert_eq!(stops(&sys.update(10.0, &listener(), far, &[])), vec![0]);
}

#[test]
fn a_fixed_origin_does_not_follow_the_entity() {
    let mut sys = system();
    let mut ev = sound(2, 1, 1);
    ev.origin = Some(Vec3f::new(0.0, 10.0, 0.0));
    start(&mut sys, ev);
    sys.update(0.0, &listener(), |_| None, &[]);
    let far = |_: i32| Some(Vec3f::new(5000.0, 0.0, 0.0));
    let mix = sys.update(10.0, &listener(), far, &[]);
    assert!(matches!(
        mix.commands[..],
        [MixCommand::Gain { channel: 0, .. }]
    ));
}

#[test]
fn a_sound_with_a_cue_point_keeps_looping() {
    let mut sys = system();
    let mut ev = sound(2, 1, 3);
    ev.origin = Some(Vec3f::ZERO);
    start(&mut sys, ev);
    update(&mut sys, 0.0);
    let later = update(&mut sys, 1000.0);
    assert!(stops(&later).is_empty(), "{later:?}");
}

#[test]
fn sexed_sound_falls_back_to_the_male_sound_without_a_model() {
    let mut sys = system();
    start(&mut sys, sound(PLAYER, 2, 4));
    let mix = update(&mut sys, 0.0);
    let (_, sfx) = plays(&mix)[0];
    assert_eq!(sys.name(sfx), "#players/male/jump1.wav");
    assert!(sys.sound(sfx).is_some(), "aliased to player/male/jump1.wav");
}

#[test]
fn sexed_sound_uses_the_players_model_directory_when_it_has_the_file() {
    let mut sys = system();
    let files = files();
    sys.start_sound(&sound(PLAYER, 2, 4), strings("Ann\\female/athena"), |p| {
        files.get(p).cloned()
    });
    sys.start_sound(&sound(PLAYER, 3, 4), strings("Bob\\cyborg/ps9000"), |p| {
        files.get(p).cloned()
    });
    let mix = update(&mut sys, 0.0);
    let names: Vec<&str> = plays(&mix).iter().map(|&(_, s)| sys.name(s)).collect();
    assert_eq!(
        names,
        vec!["#players/female/jump1.wav", "#players/cyborg/jump1.wav"]
    );
    assert!(plays(&mix).iter().all(|&(_, s)| sys.sound(s).is_some()));
}

// -- channel choice (C S_PickChannel) ----------------------------------------

#[test]
fn same_entity_and_channel_overrides_the_playing_sound() {
    let mut sys = system();
    start(&mut sys, sound(2, 1, 1));
    update(&mut sys, 0.0);
    start(&mut sys, sound(2, 1, 2));
    let mix = update(&mut sys, 10.0);
    assert_eq!(stops(&mix), vec![0]);
    assert_eq!(plays(&mix), vec![(0, sys.precache[2].unwrap())]);
}

#[test]
fn channel_zero_never_overrides() {
    let mut sys = system();
    start(&mut sys, sound(2, 0, 1));
    update(&mut sys, 0.0);
    start(&mut sys, sound(2, 0, 2));
    let mix = update(&mut sys, 10.0);
    assert_eq!(plays(&mix), vec![(1, sys.precache[2].unwrap())]);
}

#[test]
fn the_channel_nearest_its_end_is_replaced_when_all_are_busy() {
    let mut sys = system();
    for (t, ent) in (2..2 + MAX_CHANNELS as i32).enumerate() {
        start(&mut sys, sound(ent, 1, 1));
        update(&mut sys, t as f64);
    }
    start(&mut sys, sound(99, 1, 2));
    let mix = update(&mut sys, 40.0);
    assert_eq!(plays(&mix), vec![(0, sys.precache[2].unwrap())]);
    assert_eq!(stops(&mix), vec![0]);
}

#[test]
fn monster_sounds_do_not_take_player_channels() {
    let mut sys = system();
    for t in 0..MAX_CHANNELS {
        start(&mut sys, sound(PLAYER, 0, 1));
        update(&mut sys, t as f64);
    }
    start(&mut sys, sound(2, 1, 2));
    assert_eq!(plays(&update(&mut sys, 40.0)), vec![]);
    start(&mut sys, sound(PLAYER, 0, 2));
    assert_eq!(plays(&update(&mut sys, 41.0)).len(), 1);
}

#[test]
fn stop_all_stops_playing_channels_and_drops_pending_sounds() {
    let mut sys = system();
    start(&mut sys, sound(2, 1, 1));
    update(&mut sys, 0.0);
    start(&mut sys, sound(3, 1, 1));
    sys.stop_all();
    let mix = update(&mut sys, 1.0);
    assert_eq!(mix.commands, vec![MixCommand::Stop { channel: 0 }]);
}

// -- entity loops (C SDL_AddLoopSounds) --------------------------------------

fn looping(number: i32, sound: i32, origin: Vec3f) -> EntityState {
    EntityState {
        number,
        sound,
        origin,
        ..Default::default()
    }
}

#[test]
fn entities_playing_the_same_loop_are_merged_and_capped() {
    let mut sys = system();
    let at_ear = Vec3f::new(0.0, 10.0, 0.0); // full left
    let ents = [
        looping(2, 3, at_ear),
        looping(3, 3, at_ear),
        looping(4, 1, Vec3f::new(0.0, -10.0, 0.0)),
    ];
    let mix = sys.update(0.0, &listener(), |_| None, &ents);
    let (looped, one_shot) = (sys.precache[3].unwrap(), sys.precache[1].unwrap());
    assert_eq!(
        mix.loops,
        vec![
            LoopMix {
                sfx: looped,
                left: gain(255),
                right: gain(0)
            },
            LoopMix {
                sfx: one_shot,
                left: gain(0),
                right: gain(255)
            },
        ]
    );
}

#[test]
fn loop_volumes_sum_across_entities() {
    let mut sys = system();
    // dist (200 - 80) * 0.003 = 0.36, ahead: 0.5 * 0.64 * 255 = 81 each ear.
    let ahead = Vec3f::new(200.0, 0.0, 0.0);
    let ents = [looping(2, 3, ahead), looping(3, 3, ahead)];
    let mix = sys.update(0.0, &listener(), |_| None, &ents);
    assert_eq!(mix.loops[0].left, gain(162));
}

#[test]
fn inaudible_and_unregistered_loops_are_skipped() {
    let mut sys = system();
    let ents = [
        looping(2, 3, Vec3f::new(5000.0, 0.0, 0.0)),
        looping(3, 9, Vec3f::ZERO),
        looping(4, 0, Vec3f::ZERO),
    ];
    assert_eq!(sys.update(0.0, &listener(), |_| None, &ents).loops, vec![]);
}

#[test]
fn loops_are_rebuilt_every_frame() {
    let mut sys = system();
    let ents = [looping(2, 3, Vec3f::ZERO)];
    assert_eq!(sys.update(0.0, &listener(), |_| None, &ents).loops.len(), 1);
    let mix = sys.update(10.0, &listener(), |_| None, &[]);
    assert_eq!(mix, Mix::default(), "no stop: the loop list is the state");
}

#[test]
fn a_loop_taking_a_busy_channel_stops_its_one_shot() {
    let mut sys = system();
    for (t, ent) in (2..2 + MAX_CHANNELS as i32).enumerate() {
        start(&mut sys, sound(ent, 1, 1));
        update(&mut sys, t as f64);
    }
    let ents = [looping(2, 3, Vec3f::ZERO)];
    let mix = sys.update(40.0, &listener(), |_| None, &ents);
    assert_eq!(mix.loops.len(), 1);
    assert_eq!(stops(&mix), vec![0]);
}

#[test]
fn entity_origin_lerps_between_frames_and_is_none_for_absent_entities() {
    let previous = [looping(2, 0, Vec3f::ZERO)];
    let current = [
        looping(2, 0, Vec3f::new(100.0, 0.0, 0.0)),
        looping(3, 0, Vec3f::new(0.0, 8.0, 0.0)),
    ];
    let at = |n| entity_origin(&current, &previous, 0.25, n);
    assert_eq!(at(2), Some(Vec3f::new(25.0, 0.0, 0.0)));
    assert_eq!(at(3), Some(Vec3f::new(0.0, 8.0, 0.0)), "new: no lerp");
    assert_eq!(at(4), None);
}

fn named(entnum: i32, name: &str) -> NamedSound<'_> {
    NamedSound {
        name,
        entnum,
        entchannel: 1,
        volume: 1.0,
        attenuation: 1.0,
        timeofs: 0.0,
        origin: None,
    }
}

#[test]
fn a_sound_started_by_name_registers_it_like_s_registersound() {
    let mut sys = system();
    let files = files();
    sys.start_named_sound(&named(2, "b.wav"), strings(""), |p| files.get(p).cloned());
    let mix = update(&mut sys, 0.0);
    assert_eq!(plays(&mix).len(), 1);
    assert_eq!(sys.name(plays(&mix)[0].1), "b.wav");
}

#[test]
fn a_named_sound_that_does_not_load_is_dropped() {
    let mut sys = system();
    sys.start_named_sound(&named(2, "missing.wav"), strings(""), |_| None);
    assert!(plays(&update(&mut sys, 0.0)).is_empty());
}

#[test]
fn a_named_sound_at_a_fixed_origin_does_not_follow_its_entity() {
    // Temp entity sounds play at their origin from entity 0.
    let mut sys = system();
    let files = files();
    let sound = NamedSound {
        origin: Some(Vec3f::new(0.0, 10.0, 0.0)),
        ..named(0, "b.wav")
    };
    sys.start_named_sound(&sound, strings(""), |p| files.get(p).cloned());
    sys.update(0.0, &listener(), |_| None, &[]);
    let far = |_: i32| Some(Vec3f::new(5000.0, 0.0, 0.0));
    let mix = sys.update(10.0, &listener(), far, &[]);
    assert!(matches!(
        mix.commands[..],
        [MixCommand::Gain { channel: 0, .. }]
    ));
}

#[test]
fn a_sexed_named_sound_resolves_through_the_players_model() {
    // Entity events start `*fall1.wav` and the like by name.
    let mut sys = system();
    let files = files();
    sys.start_named_sound(
        &named(PLAYER, "*jump1.wav"),
        strings("Ann\\female/athena"),
        |p| files.get(p).cloned(),
    );
    let mix = update(&mut sys, 0.0);
    assert_eq!(sys.name(plays(&mix)[0].1), "#players/female/jump1.wav");
}
