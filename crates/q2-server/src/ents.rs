//! Which entities a client is sent each frame: the entity loop of C
//! `SV_BuildClientFrame` (server/sv_entities.c). Kept apart from the
//! in-process `LocalGame` so the networked server (#10) can reuse it.

use q2_game::traits::{EdictView, GameImport};
use q2_shared::constants::{RF_BEAM, SVF_NOCLIENT};
use q2_shared::types::{EntityState, Vec3f};

use crate::ServerGameImport;

/// The states sent to the client whose entity is `client`, with its eye
/// at `eye`, ordered by entity number. Like C: entities with
/// `SVF_NOCLIENT` or with nothing to show or play are skipped; every
/// other entity but the client's own must touch the fat PVS of the eye,
/// and one with no model (a sound) must also be within 400 units of it;
/// entities the client owns are sent with `solid` 0 so its own missiles
/// are not predicted as solid.
///
/// An entity in an area a closed door (area portal) cuts off from the
/// eye's is not sent; a door straddling two areas is sent if either is
/// connected. `RF_BEAM` entities instead need their first cluster in the
/// PHS of the eye's cluster, and are not distance culled.
pub fn build_client_frame<'a>(
    gi: &ServerGameImport,
    client: usize,
    eye: Vec3f,
    entities: impl IntoIterator<Item = EdictView<'a>>,
) -> Vec<EntityState> {
    let pvs = gi.fat_pvs(eye);
    let phs = gi.client_phs(eye);
    let client_area = gi.point_area(eye);
    let mut states: Vec<EntityState> = entities
        .into_iter()
        .filter(|e| e.state.number > 0 && e.svflags & SVF_NOCLIENT == 0)
        .filter(|e| {
            let s = e.state;
            s.modelindex != 0 || s.effects != 0 || s.sound != 0 || s.event != 0
        })
        .filter(|e| {
            let number = usize::try_from(e.state.number).unwrap_or(0);
            if number == client {
                return true;
            }
            if !e.linked {
                return false;
            }
            let areas = gi.entity_areas(number);
            if !in_connected_area(areas, |area| gi.areas_connected(client_area, area)) {
                return false; // blocked by a door
            }
            // Beams just check one point for PHS.
            if e.state.renderfx & RF_BEAM != 0 {
                return gi.entity_in_phs(number, &phs);
            }
            if !gi.entity_in_pvs(number, &pvs) {
                return false;
            }
            // Don't send sounds that would be attenuated away.
            e.state.modelindex != 0 || (eye - e.state.origin).length() <= 400.0
        })
        .map(|e| {
            let mut state = e.state.clone();
            if e.owner == Some(client) {
                state.solid = 0;
            }
            state
        })
        .collect();
    states.sort_by_key(|s| s.number);
    states
}

/// The area check of C SV_BuildClientFrame: an entity's `areanum` must be
/// `connected` to the client's area, or else its `areanum2`, if it has one
/// (doors can legally straddle two areas).
fn in_connected_area((area1, area2): (i32, i32), connected: impl Fn(i32) -> bool) -> bool {
    connected(area1) || (area2 != 0 && connected(area2))
}

#[cfg(test)]
mod tests {
    use super::in_connected_area;

    #[test]
    fn entity_is_in_a_connected_area_through_either_of_its_areas() {
        let connected_to = |open: &'static [i32]| move |area| open.contains(&area);
        assert!(in_connected_area((1, 0), connected_to(&[1])));
        assert!(in_connected_area((1, 2), connected_to(&[2])), "second area");
        assert!(
            !in_connected_area((1, 2), connected_to(&[3])),
            "neither area"
        );
        assert!(
            !in_connected_area((1, 0), connected_to(&[0, 2])),
            "no second area"
        );
    }
}
