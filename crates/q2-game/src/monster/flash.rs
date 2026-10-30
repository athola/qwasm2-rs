//! Monster muzzle positions and the source-point projection every monster
//! attack starts from.
//!
//! # C source reference
//! `~/Qwasm2/src/common/shared/flash.c` (`monster_flash_offset`) and
//! `G_ProjectSource` in `~/Qwasm2/src/game/g_utils.c`.

use q2_shared::types::Vec3f;

use crate::entity::EntityKey;
use crate::world::GameWorld;

/// C `monster_flash_offset` lives in `common/shared/flash.c`, shared with
/// the client's `CL_AddMuzzleFlash2`.
pub use q2_shared::flash::MONSTER_FLASH_OFFSET;

/// The point `distance` (forward, right, up) from `point`. Up is world z,
/// not the `up` vector, as in C. C ref: `G_ProjectSource` (g_utils.c).
pub fn project_source(point: Vec3f, distance: [f32; 3], forward: Vec3f, right: Vec3f) -> Vec3f {
    Vec3f::new(
        point.x + forward.x * distance[0] + right.x * distance[1],
        point.y + forward.y * distance[0] + right.y * distance[1],
        point.z + forward.z * distance[0] + right.z * distance[1] + distance[2],
    )
}

/// Muzzle start, forward and right for `key` firing flash `flash`: the
/// `AngleVectors` + `G_ProjectSource(origin, monster_flash_offset[flash])`
/// pair that opens every C monster attack. A flash number outside the
/// table, which C would read past the array for, gives `None`.
pub fn monster_flash_source(
    world: &GameWorld,
    key: EntityKey,
    flash: i32,
) -> Option<(Vec3f, Vec3f, Vec3f)> {
    let offset = *MONSTER_FLASH_OFFSET.get(usize::try_from(flash).ok()?)?;
    let ent = world.entities.get(key)?;
    let (forward, right, _) = crate::physics::angle_vectors(ent.state.angles);
    let start = project_source(ent.state.origin, offset, forward, right);
    Some((start, forward, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2_shared::constants::*;

    #[test]
    fn flash_table_has_an_entry_per_mz2_number_plus_c_end_marker() {
        assert_eq!(
            MONSTER_FLASH_OFFSET.len(),
            MZ2_WIDOW2_BEAM_SWEEP_11 as usize + 2
        );
        assert_eq!(MONSTER_FLASH_OFFSET[0], [0.0; 3]);
        assert_eq!(MONSTER_FLASH_OFFSET[211], [0.0; 3]);
    }

    #[test]
    fn flash_table_spot_values_match_c_flash_c() {
        let at = |n: i32| MONSTER_FLASH_OFFSET[n as usize];
        assert_eq!(at(MZ2_TANK_BLASTER_1), [20.7, -18.5, 28.7]);
        assert_eq!(at(MZ2_TANK_MACHINEGUN_11), [3.2, 40.1, 24.7]);
        assert_eq!(at(MZ2_SOLDIER_BLASTER_1)[1], (7.7_f64 * 1.2) as f32);
        assert_eq!(at(MZ2_WIDOW2_BEAMER_5), [69.0, -18.3, 97.98]);
        assert_eq!(at(MZ2_WIDOW2_BEAM_SWEEP_11), [58.29, 27.11, 92.0]);
    }

    #[test]
    fn scaled_offsets_round_the_c_double_product_to_float() {
        // C: `34.5 * 1.2` is a double product stored into a float.
        let c = (34.5_f64 * 1.2) as f32;
        assert_eq!(
            MONSTER_FLASH_OFFSET[MZ2_SOLDIER_MACHINEGUN_8 as usize][0],
            c
        );
    }

    #[test]
    fn project_source_moves_forward_right_and_straight_up() {
        let forward = Vec3f::new(0.0, 1.0, 0.0);
        let right = Vec3f::new(1.0, 0.0, 0.0);
        let start = project_source(
            Vec3f::new(100.0, 200.0, 300.0),
            [10.0, 20.0, 30.0],
            forward,
            right,
        );
        // forward * 10 + right * 20, and distance[2] on z regardless of up.
        assert_eq!(start, Vec3f::new(120.0, 210.0, 330.0));
    }

    #[test]
    fn monster_flash_source_projects_the_flash_offset_from_the_monster_facing() {
        let mut world = crate::world::test_world();
        let monster = world.spawn().unwrap();
        {
            let ent = world.entities.get_mut(monster).unwrap();
            ent.state.origin = Vec3f::new(0.0, 0.0, 24.0);
            ent.state.angles = Vec3f::new(0.0, 90.0, 0.0);
        }
        let (start, forward, right) =
            monster_flash_source(&world, monster, MZ2_TANK_BLASTER_1).unwrap();
        // Yaw 90: forward is +y, right is +x.
        assert!((forward - Vec3f::new(0.0, 1.0, 0.0)).length() < 1e-6);
        assert!((right - Vec3f::new(1.0, 0.0, 0.0)).length() < 1e-6);
        let expected = Vec3f::new(-18.5, 20.7, 24.0 + 28.7);
        assert!((start - expected).length() < 1e-4, "{start:?}");
    }

    #[test]
    fn monster_flash_source_of_a_freed_entity_is_none() {
        let mut world = crate::world::test_world();
        let monster = world.spawn().unwrap();
        world.entities.free(monster);
        assert!(monster_flash_source(&world, monster, MZ2_TANK_BLASTER_1).is_none());
    }

    #[test]
    fn monster_flash_source_of_a_flash_outside_the_table_is_none() {
        let mut world = crate::world::test_world();
        let monster = world.spawn().unwrap();
        let past_end = MONSTER_FLASH_OFFSET.len() as i32;
        assert!(monster_flash_source(&world, monster, -1).is_none());
        assert!(monster_flash_source(&world, monster, past_end).is_none());
    }
}
