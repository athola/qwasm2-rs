//! C `SV_CreateBaseline`: after the map spawns, each edict in use that
//! shows something (a model, a sound or effects) is copied as its
//! baseline, with `old_origin` at its origin and its own number.

mod common;

use q2_game::traits::GameExport;

#[test]
fn a_spawned_map_takes_its_visible_edicts_as_baselines() {
    let world = common::ServerWorld::start("one", &common::armor_map());
    // From 2: the player connected after the map spawned, as in C.
    let visible: Vec<usize> = (2..world.game.num_edicts())
        .filter(|&n| {
            world.game.edict(n).is_some_and(|e| {
                e.state.modelindex != 0 || e.state.sound != 0 || e.state.effects != 0
            })
        })
        .collect();
    assert!(!visible.is_empty(), "the armor shows");
    for n in 2..world.game.num_edicts() {
        let baseline = &world.sv.baselines[n];
        if visible.contains(&n) {
            assert_eq!(baseline.number, n as i32, "edict {n}");
            assert_eq!(
                baseline.modelindex,
                world.game.edict(n).unwrap().state.modelindex
            );
            assert_eq!(baseline.old_origin, baseline.origin, "edict {n}");
        } else {
            assert_eq!(baseline.number, 0, "edict {n} shows nothing");
        }
    }
}
