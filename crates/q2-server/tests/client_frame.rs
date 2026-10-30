//! The entity filter of C SV_BuildClientFrame against the two-cluster
//! fixture, whose clusters (y > 0 and y < 0) cannot see each other.

mod common;

use q2_game::traits::{EdictView, EntityLink, GameImport};
use q2_server::ents::build_client_frame;
use q2_server::ServerGameImport;
use q2_shared::constants::SVF_NOCLIENT;
use q2_shared::types::{EntityState, Solid, Vec3f};

const CLIENT: usize = 1;
/// The client's eye, in cluster 0.
const EYE: Vec3f = Vec3f::new(0.0, 200.0, 40.0);
use q2_shared::constants::RF_BEAM;

fn loaded() -> ServerGameImport {
    let gi = ServerGameImport::new();
    gi.load_map(&common::two_cluster_bsp_with(common::ENTSTRING))
        .expect("fixture loads");
    gi
}

fn link(gi: &ServerGameImport, number: usize, origin: Vec3f) {
    let half = Vec3f::splat(8.0);
    gi.link_entity(&EntityLink {
        number,
        origin,
        mins: -half,
        maxs: half,
        absmin: origin - half - Vec3f::ONE,
        absmax: origin + half + Vec3f::ONE,
        solid: Solid::Bbox,
        ..Default::default()
    });
}

fn state(number: i32, origin: Vec3f) -> EntityState {
    EntityState {
        number,
        origin,
        modelindex: 3,
        solid: 31,
        ..Default::default()
    }
}

fn entity(state: &EntityState) -> EdictView<'_> {
    EdictView {
        state,
        svflags: 0,
        owner: None,
        linked: true,
    }
}

fn sent(gi: &ServerGameImport, entities: Vec<EdictView<'_>>) -> Vec<i32> {
    build_client_frame(gi, CLIENT, EYE, entities)
        .iter()
        .map(|s| s.number)
        .collect()
}

#[test]
fn entity_in_a_cluster_the_eye_cannot_see_is_not_sent() {
    let gi = loaded();
    let (seen, hidden) = (Vec3f::new(0.0, 300.0, 40.0), Vec3f::new(0.0, -300.0, 40.0));
    link(&gi, 2, seen);
    link(&gi, 3, hidden);
    let (a, b) = (state(2, seen), state(3, hidden));
    assert_eq!(sent(&gi, vec![entity(&a), entity(&b)]), [2]);
}

#[test]
fn the_client_itself_is_sent_wherever_it_is() {
    let gi = loaded();
    let at = Vec3f::new(0.0, -300.0, 40.0);
    link(&gi, CLIENT, at);
    let me = state(CLIENT as i32, at);
    assert_eq!(sent(&gi, vec![entity(&me)]), [CLIENT as i32]);
}

#[test]
fn entity_relinked_into_the_visible_cluster_is_sent() {
    let gi = loaded();
    link(&gi, 4, Vec3f::new(0.0, -300.0, 40.0));
    let moved = Vec3f::new(0.0, 300.0, 40.0);
    link(&gi, 4, moved);
    let s = state(4, moved);
    assert_eq!(sent(&gi, vec![entity(&s)]), [4]);
}

#[test]
fn entity_never_linked_since_spawn_is_not_sent() {
    // C G_FreeEdict clears num_clusters, so a reused slot that is never
    // linked is not in any PVS, even though the slot's previous occupant
    // was linked in view.
    let gi = loaded();
    let at = Vec3f::new(0.0, 300.0, 40.0);
    link(&gi, 5, at);
    let s = state(5, at);
    let mut reused = entity(&s);
    reused.linked = false;
    assert!(sent(&gi, vec![reused]).is_empty());
}

#[test]
fn noclient_and_empty_entities_are_not_sent() {
    let gi = loaded();
    let at = Vec3f::new(0.0, 300.0, 40.0);
    link(&gi, 6, at);
    link(&gi, 7, at);
    let hidden = state(6, at);
    let mut noclient = entity(&hidden);
    noclient.svflags = SVF_NOCLIENT;
    let mut nothing = state(7, at);
    nothing.modelindex = 0;
    assert!(sent(&gi, vec![noclient, entity(&nothing)]).is_empty());
}

#[test]
fn sound_only_entity_is_sent_within_400_units_of_the_eye() {
    let gi = loaded();
    let (near, far) = (
        EYE + Vec3f::new(0.0, 0.0, 390.0),
        EYE + Vec3f::new(0.0, 0.0, 410.0),
    );
    link(&gi, 8, near);
    link(&gi, 9, far);
    let mut a = state(8, near);
    let mut b = state(9, far);
    for s in [&mut a, &mut b] {
        s.modelindex = 0;
        s.sound = 1;
    }
    assert_eq!(sent(&gi, vec![entity(&a), entity(&b)]), [8]);
}

#[test]
fn beams_are_sent_by_the_phs_of_the_eyes_cluster_not_the_pvs() {
    // C: a beam checks its first cluster against CM_ClusterPHS of the
    // client's cluster. Cluster 0 hears cluster 1 but cannot see it.
    let gi = loaded();
    let south = Vec3f::new(0.0, -300.0, 40.0);
    link(&gi, 10, south);
    let mut beam = state(10, south);
    beam.renderfx = RF_BEAM;
    assert_eq!(sent(&gi, vec![entity(&beam)]), [10]);

    // From cluster 1, which hears only itself, a beam in cluster 0 is not.
    let north = Vec3f::new(0.0, 300.0, 40.0);
    link(&gi, 11, north);
    let mut beam = state(11, north);
    beam.renderfx = RF_BEAM;
    let from_south = build_client_frame(
        &gi,
        CLIENT,
        Vec3f::new(0.0, -200.0, 40.0),
        vec![entity(&beam)],
    );
    assert!(from_south.is_empty());
}

#[test]
fn beam_without_a_model_is_not_distance_culled() {
    // C culls sounds by distance only in the non-beam branch.
    let gi = loaded();
    let far = EYE + Vec3f::new(0.0, 0.0, 410.0);
    link(&gi, 10, far);
    let mut beam = state(10, far);
    beam.modelindex = 0;
    beam.sound = 1;
    beam.renderfx = RF_BEAM;
    assert_eq!(sent(&gi, vec![entity(&beam)]), [10]);
}

#[test]
fn entities_the_client_owns_are_sent_non_solid() {
    let gi = loaded();
    let at = Vec3f::new(0.0, 300.0, 40.0);
    link(&gi, 11, at);
    link(&gi, 12, at);
    let (rocket, other) = (state(11, at), state(12, at));
    let mut owned = entity(&rocket);
    owned.owner = Some(CLIENT);
    let states = build_client_frame(&gi, CLIENT, EYE, vec![owned, entity(&other)]);
    assert_eq!(
        states
            .iter()
            .map(|s| (s.number, s.solid))
            .collect::<Vec<_>>(),
        [(11, 0), (12, 31)]
    );
}

#[test]
fn without_a_map_only_the_client_is_sent() {
    let gi = ServerGameImport::new();
    let me = state(CLIENT as i32, EYE);
    let other = state(2, EYE);
    assert_eq!(
        sent(&gi, vec![entity(&other), entity(&me)]),
        [CLIENT as i32]
    );
}

#[test]
fn sent_entities_are_ordered_by_number() {
    let gi = loaded();
    let at = Vec3f::new(0.0, 300.0, 40.0);
    link(&gi, 13, at);
    link(&gi, 14, at);
    let (a, b) = (state(14, at), state(13, at));
    assert_eq!(sent(&gi, vec![entity(&a), entity(&b)]), [13, 14]);
}
