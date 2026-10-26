//! ServerGameImport against a real collision map: SV_Trace,
//! SV_PointContents, entity links and SV_FindIndex.

mod common;

use q2_game::traits::{EntityLink, GameImport};
use q2_server::ServerGameImport;
use q2_shared::constants::{
    CONTENTS_MONSTER, CONTENTS_SOLID, CS_IMAGES, CS_MODELS, CS_SOUNDS, MASK_PLAYERSOLID,
    SVF_DEADMONSTER,
};
use q2_shared::types::{Solid, Vec3f};

const PLAYER_MINS: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const PLAYER_MAXS: Vec3f = Vec3f::new(16.0, 16.0, 32.0);

fn loaded() -> ServerGameImport {
    let gi = ServerGameImport::new();
    gi.load_map(&common::floor_wall_bsp())
        .expect("fixture loads");
    gi
}

/// A 32x32x56 monster-sized box at `origin`, linked as `number`.
fn box_link(number: usize, origin: Vec3f) -> EntityLink {
    EntityLink {
        number,
        origin,
        mins: PLAYER_MINS,
        maxs: PLAYER_MAXS,
        absmin: origin + PLAYER_MINS - Vec3f::ONE,
        absmax: origin + PLAYER_MAXS + Vec3f::ONE,
        solid: Solid::Bbox,
        ..Default::default()
    }
}

fn point_trace(
    gi: &ServerGameImport,
    start: Vec3f,
    end: Vec3f,
    pass: Option<usize>,
) -> q2_shared::Trace {
    gi.trace(start, Vec3f::ZERO, Vec3f::ZERO, end, pass, MASK_PLAYERSOLID)
}

#[test]
fn world_trace_hits_the_floor_and_wall_and_reports_the_world() {
    let gi = loaded();
    let down = point_trace(
        &gi,
        Vec3f::new(0.0, 0.0, 100.0),
        Vec3f::new(0.0, 0.0, -100.0),
        None,
    );
    assert!(
        (down.endpos.z - 0.0).abs() < 0.1,
        "floor at z=0: {:?}",
        down.endpos
    );
    assert_eq!(down.ent_index, Some(0));

    let across = gi.trace(
        Vec3f::new(0.0, 0.0, 40.0),
        PLAYER_MINS,
        PLAYER_MAXS,
        Vec3f::new(400.0, 0.0, 40.0),
        None,
        MASK_PLAYERSOLID,
    );
    let face = common::WALL_X - PLAYER_MAXS.x;
    assert!(
        (across.endpos.x - face).abs() < 0.1,
        "wall stop: {:?}",
        across.endpos
    );

    // C SV_Trace stamps the world even when nothing is hit.
    let miss = point_trace(
        &gi,
        Vec3f::new(0.0, 0.0, 40.0),
        Vec3f::new(0.0, 0.0, 60.0),
        None,
    );
    assert_eq!((miss.fraction, miss.ent_index), (1.0, Some(0)));
}

#[test]
fn linked_box_entity_blocks_and_is_reported_by_number() {
    let gi = loaded();
    gi.link_entity(&box_link(5, Vec3f::new(128.0, 0.0, 40.0)));
    let tr = point_trace(
        &gi,
        Vec3f::new(0.0, 0.0, 40.0),
        Vec3f::new(200.0, 0.0, 40.0),
        None,
    );
    assert!(
        (tr.endpos.x - 112.0).abs() < 0.1,
        "box face at 128-16: {:?}",
        tr.endpos
    );
    assert_eq!(tr.ent_index, Some(5));

    // Unlinked entities are no longer clipped against.
    gi.unlink_entity(5);
    let tr = point_trace(
        &gi,
        Vec3f::new(0.0, 0.0, 40.0),
        Vec3f::new(200.0, 0.0, 40.0),
        None,
    );
    assert_eq!(tr.fraction, 1.0);
}

#[test]
fn trace_skips_the_pass_entity_its_missiles_and_its_owner() {
    let gi = loaded();
    let (start, end) = (Vec3f::new(0.0, 0.0, 40.0), Vec3f::new(200.0, 0.0, 40.0));
    gi.link_entity(&box_link(5, Vec3f::new(128.0, 0.0, 40.0)));
    assert_eq!(point_trace(&gi, start, end, Some(5)).fraction, 1.0);

    // Entity 6, owned by 5, sits in front of 5.
    gi.link_entity(&EntityLink {
        owner: Some(5),
        ..box_link(6, Vec3f::new(64.0, 0.0, 40.0))
    });
    // From 5: its own missile (6) and itself are skipped.
    assert_eq!(point_trace(&gi, start, end, Some(5)).fraction, 1.0);
    // From 6: its owner (5) is skipped, 6 itself too.
    assert_eq!(point_trace(&gi, start, end, Some(6)).fraction, 1.0);
    // From the world, 6 blocks first.
    assert_eq!(point_trace(&gi, start, end, None).ent_index, Some(6));
}

#[test]
fn dead_monsters_block_only_masks_with_contents_deadmonster() {
    let gi = loaded();
    gi.link_entity(&EntityLink {
        svflags: SVF_DEADMONSTER,
        ..box_link(5, Vec3f::new(128.0, 0.0, 40.0))
    });
    let tr = point_trace(
        &gi,
        Vec3f::new(0.0, 0.0, 40.0),
        Vec3f::new(200.0, 0.0, 40.0),
        None,
    );
    assert_eq!(
        tr.fraction, 1.0,
        "MASK_PLAYERSOLID has no CONTENTS_DEADMONSTER"
    );
}

#[test]
fn bsp_entity_clips_with_its_inline_model_moved_to_its_origin() {
    let gi = loaded();
    // Model index 1 is the world model (C sv.models[1]); placed 1000 up, its
    // floor surface is at z = 1000.
    let origin = Vec3f::new(0.0, 0.0, 1000.0);
    gi.link_entity(&EntityLink {
        number: 7,
        origin,
        mins: Vec3f::splat(-500.0),
        maxs: Vec3f::splat(500.0),
        absmin: origin - Vec3f::splat(501.0),
        absmax: origin + Vec3f::splat(501.0),
        solid: Solid::Bsp,
        modelindex: 1,
        ..Default::default()
    });
    let tr = point_trace(
        &gi,
        Vec3f::new(0.0, 0.0, 1100.0),
        Vec3f::new(0.0, 0.0, 900.0),
        None,
    );
    assert!((tr.endpos.z - 1000.0).abs() < 0.1, "{:?}", tr.endpos);
    assert_eq!(tr.ent_index, Some(7));
}

#[test]
fn point_contents_ors_the_world_with_linked_entities() {
    let gi = loaded();
    assert_eq!(
        gi.point_contents(Vec3f::new(0.0, 0.0, -10.0)),
        CONTENTS_SOLID
    );
    assert_eq!(gi.point_contents(Vec3f::new(0.0, 0.0, 40.0)), 0);
    gi.link_entity(&box_link(5, Vec3f::new(0.0, 0.0, 40.0)));
    assert_eq!(
        gi.point_contents(Vec3f::new(0.0, 0.0, 40.0)),
        CONTENTS_MONSTER
    );
}

#[test]
fn indexes_are_handed_out_once_per_name_from_one_like_sv_findindex() {
    let gi = loaded();
    let a = gi.model_index("models/monsters/soldier/tris.md2");
    let b = gi.model_index("models/items/armor/body/tris.md2");
    assert_eq!((a, b), (1, 2));
    assert_eq!(gi.model_index("models/monsters/soldier/tris.md2"), 1);
    assert_eq!(gi.model_index(""), 0);
    assert_eq!(gi.sound_index("weapons/blastf1a.wav"), 1);
    assert_eq!(gi.image_index("pics/i_health"), 1);

    let updates: std::collections::HashMap<usize, String> =
        gi.drain_configstring_updates().into_iter().collect();
    assert_eq!(
        updates[&(CS_MODELS + 2)],
        "models/items/armor/body/tris.md2"
    );
    assert_eq!(updates[&(CS_SOUNDS + 1)], "weapons/blastf1a.wav");
    assert_eq!(updates[&(CS_IMAGES + 1)], "pics/i_health");
}

#[test]
fn box_edicts_returns_linked_numbers_in_the_region_up_to_the_limit() {
    let gi = loaded();
    for n in 5..9 {
        gi.link_entity(&box_link(n, Vec3f::new(n as f32 * 10.0, 0.0, 40.0)));
    }
    let mut hits = gi.box_edicts(Vec3f::splat(-200.0), Vec3f::splat(200.0), 16, 1);
    hits.sort();
    assert_eq!(hits, [5, 6, 7, 8]);
    assert_eq!(
        gi.box_edicts(Vec3f::splat(-200.0), Vec3f::splat(200.0), 2, 1)
            .len(),
        2
    );
}

#[test]
fn load_map_exposes_the_entity_string_and_rejects_bad_bsp() {
    let gi = loaded();
    assert!(gi.entity_string().contains("info_player_start"));
    assert!(gi.load_map(b"not a bsp").is_err());
}
