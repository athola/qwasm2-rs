//! ServerGameImport::multicast routing against a real collision map: C
//! SV_Multicast sends a PVS or PHS message only to a client whose origin
//! is in the origin cluster's row and in a connected area.

mod common;

use q2_game::traits::{EntityLink, GameImport};
use q2_server::state::ClientState;
use q2_server::ServerGameImport;
use q2_shared::constants::CONTENTS_WATER;
use q2_shared::types::{Multicast, Solid, Vec3f};

const CLIENT: usize = 1;
/// Points in the open air of each half of the split maps.
const NORTH: Vec3f = Vec3f::new(0.0, 200.0, 40.0);
const SOUTH: Vec3f = Vec3f::new(0.0, -200.0, 40.0);

fn loaded(bsp: &[u8], client: ClientState) -> ServerGameImport {
    let gi = ServerGameImport::new();
    gi.load_map(bsp).expect("fixture loads");
    gi.set_client_states([client]);
    gi
}

/// Link the client's player entity at `origin`.
fn place_client(gi: &ServerGameImport, origin: Vec3f) {
    let half = Vec3f::splat(8.0);
    gi.link_entity(&EntityLink {
        number: CLIENT,
        origin,
        mins: -half,
        maxs: half,
        absmin: origin - half - Vec3f::ONE,
        absmax: origin + half + Vec3f::ONE,
        solid: Solid::Bbox,
        ..Default::default()
    });
}

/// Whether one message multicast from `origin` reaches the client.
fn reaches(gi: &ServerGameImport, origin: Vec3f, to: Multicast) -> bool {
    gi.take_messages();
    gi.write_byte(7);
    gi.multicast(origin, to);
    !gi.take_messages().is_empty()
}

fn two_clusters(client: ClientState) -> ServerGameImport {
    loaded(&common::two_cluster_bsp_with(common::ENTSTRING), client)
}

#[test]
fn a_pvs_multicast_reaches_only_a_client_whose_cluster_the_origin_sees() {
    let gi = two_clusters(ClientState::Spawned);
    place_client(&gi, NORTH);
    assert!(reaches(&gi, Vec3f::new(0.0, 300.0, 40.0), Multicast::PVS));
    assert!(!reaches(&gi, SOUTH, Multicast::PVS), "other cluster");
    assert!(!reaches(&gi, SOUTH, Multicast::PVSR), "reliable alike");
}

#[test]
fn a_phs_multicast_follows_the_hearing_row_of_the_origins_cluster() {
    let gi = two_clusters(ClientState::Spawned);
    place_client(&gi, SOUTH);
    assert!(reaches(&gi, NORTH, Multicast::PHS), "cluster 0 hears 1");
    place_client(&gi, NORTH);
    assert!(!reaches(&gi, SOUTH, Multicast::PHS), "1 hears only itself");
    assert!(!reaches(&gi, SOUTH, Multicast::PHSR));
}

#[test]
fn a_multicast_to_all_ignores_the_client_position() {
    let gi = two_clusters(ClientState::Spawned);
    place_client(&gi, NORTH);
    assert!(reaches(&gi, SOUTH, Multicast::All));
    assert!(reaches(&gi, SOUTH, Multicast::AllR));
}

#[test]
fn a_pvs_multicast_from_an_area_behind_a_closed_portal_does_not_reach() {
    let gi = loaded(
        &common::two_area_bsp_with(common::ENTSTRING),
        ClientState::Spawned,
    );
    place_client(&gi, NORTH);
    assert!(!reaches(&gi, SOUTH, Multicast::PVS), "portal 1 is closed");
    gi.set_area_portal_state(1, true);
    assert!(reaches(&gi, SOUTH, Multicast::PVS), "portal 1 is open");
}

#[test]
fn only_reliable_multicasts_reach_a_client_not_yet_in_the_game() {
    let gi = two_clusters(ClientState::Connected);
    place_client(&gi, NORTH);
    assert!(!reaches(&gi, NORTH, Multicast::PVS));
    assert!(!reaches(&gi, NORTH, Multicast::All));
    assert!(reaches(&gi, NORTH, Multicast::PVSR));
    assert!(reaches(&gi, NORTH, Multicast::AllR));
}

#[test]
fn no_multicast_reaches_a_free_slot() {
    let gi = two_clusters(ClientState::Free);
    place_client(&gi, NORTH);
    assert!(!reaches(&gi, NORTH, Multicast::AllR));
}

/// The two-cluster map with node 3 splitting at z = 32 instead of y = 0:
/// above is leaf 1 (cluster 0), below leaf 4 (cluster 1), which holds
/// `below` contents. Neither cluster sees the other.
fn layered_bsp(below: i32) -> Vec<u8> {
    let mut bsp = common::two_cluster_bsp_with(common::ENTSTRING);
    let lump = |bsp: &[u8], n: usize| {
        let at = 8 + n * 8;
        i32::from_le_bytes(bsp[at..at + 4].try_into().unwrap()) as usize
    };
    // dplane_t (20 bytes) 20: normal, dist, type.
    let plane = lump(&bsp, 1) + 20 * 20;
    let fields = [0.0f32, 0.0, 1.0, 32.0]
        .iter()
        .flat_map(|f| f.to_le_bytes());
    bsp.splice(plane..plane + 16, fields);
    bsp[plane + 16..plane + 20].copy_from_slice(&2i32.to_le_bytes());
    // dleaf_t (28 bytes) 4: contents first.
    let leaf = lump(&bsp, 8) + 4 * 28;
    bsp[leaf..leaf + 4].copy_from_slice(&below.to_le_bytes());
    bsp
}

#[test]
fn a_client_whose_origin_is_under_water_is_also_tried_32_units_higher() {
    // C SV_Multicast: a client half in water still sees what is above it.
    let above = Vec3f::new(0.0, 0.0, 100.0);
    let wading = Vec3f::new(0.0, 0.0, 16.0);
    let gi = loaded(&layered_bsp(CONTENTS_WATER), ClientState::Spawned);
    place_client(&gi, wading);
    assert!(reaches(&gi, above, Multicast::PVS));
    let gi = loaded(&layered_bsp(0), ClientState::Spawned);
    place_client(&gi, wading);
    assert!(!reaches(&gi, above, Multicast::PVS), "dry: one try only");
}
