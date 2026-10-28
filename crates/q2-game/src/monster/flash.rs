//! Monster muzzle positions and the source-point projection every monster
//! attack starts from.
//!
//! # C source reference
//! `~/Qwasm2/src/common/shared/flash.c` (`monster_flash_offset`) and
//! `G_ProjectSource` in `~/Qwasm2/src/game/g_utils.c`.

use q2_shared::types::Vec3f;

use crate::entity::EntityKey;
use crate::world::GameWorld;

/// Muzzle offsets (forward, right, up) from a monster's origin, indexed by
/// its `MZ2_*` flash number. Entry 0 is unused and the last is C's
/// end-of-table marker. C ref: `monster_flash_offset` (flash.c).
pub const MONSTER_FLASH_OFFSET: [[f32; 3]; 212] = [
    // flash 0 is not used
    [0.0, 0.0, 0.0],
    // MZ2_TANK_BLASTER_1
    [20.7, -18.5, 28.7],
    // MZ2_TANK_BLASTER_2
    [16.6, -21.5, 30.1],
    // MZ2_TANK_BLASTER_3
    [11.8, -23.9, 32.1],
    // MZ2_TANK_MACHINEGUN_1
    [22.9, -0.7, 25.3],
    // MZ2_TANK_MACHINEGUN_2
    [22.2, 6.2, 22.3],
    // MZ2_TANK_MACHINEGUN_3
    [19.4, 13.1, 18.6],
    // MZ2_TANK_MACHINEGUN_4
    [19.4, 18.8, 18.6],
    // MZ2_TANK_MACHINEGUN_5
    [17.9, 25.0, 18.6],
    // MZ2_TANK_MACHINEGUN_6
    [14.1, 30.5, 20.6],
    // MZ2_TANK_MACHINEGUN_7
    [9.3, 35.3, 22.1],
    // MZ2_TANK_MACHINEGUN_8
    [4.7, 38.4, 22.1],
    // MZ2_TANK_MACHINEGUN_9
    [-1.1, 40.4, 24.1],
    // MZ2_TANK_MACHINEGUN_10
    [-6.5, 41.2, 24.1],
    // MZ2_TANK_MACHINEGUN_11
    [3.2, 40.1, 24.7],
    // MZ2_TANK_MACHINEGUN_12
    [11.7, 36.7, 26.0],
    // MZ2_TANK_MACHINEGUN_13
    [18.9, 31.3, 26.0],
    // MZ2_TANK_MACHINEGUN_14
    [24.4, 24.4, 26.4],
    // MZ2_TANK_MACHINEGUN_15
    [27.1, 17.1, 27.2],
    // MZ2_TANK_MACHINEGUN_16
    [28.5, 9.1, 28.0],
    // MZ2_TANK_MACHINEGUN_17
    [27.1, 2.2, 28.0],
    // MZ2_TANK_MACHINEGUN_18
    [24.9, -2.8, 28.0],
    // MZ2_TANK_MACHINEGUN_19
    [21.6, -7.0, 26.4],
    // MZ2_TANK_ROCKET_1
    [6.2, 29.1, 49.1],
    // MZ2_TANK_ROCKET_2
    [6.9, 23.8, 49.1],
    // MZ2_TANK_ROCKET_3
    [8.3, 17.8, 49.5],
    // MZ2_INFANTRY_MACHINEGUN_1
    [26.6, 7.1, 13.1],
    // MZ2_INFANTRY_MACHINEGUN_2
    [18.2, 7.5, 15.4],
    // MZ2_INFANTRY_MACHINEGUN_3
    [17.2, 10.3, 17.9],
    // MZ2_INFANTRY_MACHINEGUN_4
    [17.0, 12.8, 20.1],
    // MZ2_INFANTRY_MACHINEGUN_5
    [15.1, 14.1, 21.8],
    // MZ2_INFANTRY_MACHINEGUN_6
    [11.8, 17.2, 23.1],
    // MZ2_INFANTRY_MACHINEGUN_7
    [11.4, 20.2, 21.0],
    // MZ2_INFANTRY_MACHINEGUN_8
    [9.0, 23.0, 18.9],
    // MZ2_INFANTRY_MACHINEGUN_9
    [13.9, 18.6, 17.7],
    // MZ2_INFANTRY_MACHINEGUN_10
    [15.4, 15.6, 15.8],
    // MZ2_INFANTRY_MACHINEGUN_11
    [10.2, 15.2, 25.1],
    // MZ2_INFANTRY_MACHINEGUN_12
    [-1.9, 15.1, 28.2],
    // MZ2_INFANTRY_MACHINEGUN_13
    [-12.4, 13.0, 20.2],
    // MZ2_SOLDIER_BLASTER_1
    [
        (10.6_f64 * 1.2) as f32,
        (7.7_f64 * 1.2) as f32,
        (7.8_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_BLASTER_2
    [
        (21.1_f64 * 1.2) as f32,
        (3.6_f64 * 1.2) as f32,
        (19.0_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_1
    [
        (10.6_f64 * 1.2) as f32,
        (7.7_f64 * 1.2) as f32,
        (7.8_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_2
    [
        (21.1_f64 * 1.2) as f32,
        (3.6_f64 * 1.2) as f32,
        (19.0_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_1
    [
        (10.6_f64 * 1.2) as f32,
        (7.7_f64 * 1.2) as f32,
        (7.8_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_2
    [
        (21.1_f64 * 1.2) as f32,
        (3.6_f64 * 1.2) as f32,
        (19.0_f64 * 1.2) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_1
    [
        (30.1_f64 * 1.15) as f32,
        (3.9_f64 * 1.15) as f32,
        (19.6_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_2
    [
        (29.1_f64 * 1.15) as f32,
        (2.5_f64 * 1.15) as f32,
        (20.7_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_3
    [
        (28.2_f64 * 1.15) as f32,
        (2.5_f64 * 1.15) as f32,
        (22.2_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_4
    [
        (28.2_f64 * 1.15) as f32,
        (3.6_f64 * 1.15) as f32,
        (22.0_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_5
    [
        (26.9_f64 * 1.15) as f32,
        (2.0_f64 * 1.15) as f32,
        (23.4_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_6
    [
        (26.5_f64 * 1.15) as f32,
        (0.6_f64 * 1.15) as f32,
        (20.8_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_7
    [
        (26.9_f64 * 1.15) as f32,
        (0.5_f64 * 1.15) as f32,
        (21.5_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_MACHINEGUN_8
    [
        (29.0_f64 * 1.15) as f32,
        (2.4_f64 * 1.15) as f32,
        (19.5_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_GRENADE_1
    [
        (4.6_f64 * 1.15) as f32,
        (-16.8_f64 * 1.15) as f32,
        (7.3_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_GRENADE_2
    [
        (4.6_f64 * 1.15) as f32,
        (-16.8_f64 * 1.15) as f32,
        (7.3_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_GRENADE_3
    [
        (4.6_f64 * 1.15) as f32,
        (-16.8_f64 * 1.15) as f32,
        (7.3_f64 * 1.15) as f32,
    ],
    // MZ2_GUNNER_GRENADE_4
    [
        (4.6_f64 * 1.15) as f32,
        (-16.8_f64 * 1.15) as f32,
        (7.3_f64 * 1.15) as f32,
    ],
    // MZ2_CHICK_ROCKET_1
    [24.8, -9.0, 39.0],
    // MZ2_FLYER_BLASTER_1
    [12.1, 13.4, -14.5],
    // MZ2_FLYER_BLASTER_2
    [12.1, -7.4, -14.5],
    // MZ2_MEDIC_BLASTER_1
    [12.1, 5.4, 16.5],
    // MZ2_GLADIATOR_RAILGUN_1
    [30.0, 18.0, 28.0],
    // MZ2_HOVER_BLASTER_1
    [32.5, -0.8, 10.0],
    // MZ2_ACTOR_MACHINEGUN_1
    [18.4, 7.4, 9.6],
    // MZ2_SUPERTANK_MACHINEGUN_1
    [30.0, 30.0, 88.5],
    // MZ2_SUPERTANK_MACHINEGUN_2
    [30.0, 30.0, 88.5],
    // MZ2_SUPERTANK_MACHINEGUN_3
    [30.0, 30.0, 88.5],
    // MZ2_SUPERTANK_MACHINEGUN_4
    [30.0, 30.0, 88.5],
    // MZ2_SUPERTANK_MACHINEGUN_5
    [30.0, 30.0, 88.5],
    // MZ2_SUPERTANK_MACHINEGUN_6
    [30.0, 30.0, 88.5],
    // MZ2_SUPERTANK_ROCKET_1
    [16.0, -22.5, 91.2],
    // MZ2_SUPERTANK_ROCKET_2
    [16.0, -33.4, 86.7],
    // MZ2_SUPERTANK_ROCKET_3
    [16.0, -42.8, 83.3],
    // MZ2_BOSS2_MACHINEGUN_L1
    [32.0, -40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_L2
    [32.0, -40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_L3
    [32.0, -40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_L4
    [32.0, -40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_L5
    [32.0, -40.0, 70.0],
    // MZ2_BOSS2_ROCKET_1
    [22.0, 16.0, 10.0],
    // MZ2_BOSS2_ROCKET_2
    [22.0, 8.0, 10.0],
    // MZ2_BOSS2_ROCKET_3
    [22.0, -8.0, 10.0],
    // MZ2_BOSS2_ROCKET_4
    [22.0, -16.0, 10.0],
    // MZ2_FLOAT_BLASTER_1
    [32.5, -0.8, 10.0],
    // MZ2_SOLDIER_BLASTER_3
    [
        (20.8_f64 * 1.2) as f32,
        (10.1_f64 * 1.2) as f32,
        (-2.7_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_3
    [
        (20.8_f64 * 1.2) as f32,
        (10.1_f64 * 1.2) as f32,
        (-2.7_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_3
    [
        (20.8_f64 * 1.2) as f32,
        (10.1_f64 * 1.2) as f32,
        (-2.7_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_BLASTER_4
    [
        (7.6_f64 * 1.2) as f32,
        (9.3_f64 * 1.2) as f32,
        (0.8_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_4
    [
        (7.6_f64 * 1.2) as f32,
        (9.3_f64 * 1.2) as f32,
        (0.8_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_4
    [
        (7.6_f64 * 1.2) as f32,
        (9.3_f64 * 1.2) as f32,
        (0.8_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_BLASTER_5
    [
        (30.5_f64 * 1.2) as f32,
        (9.9_f64 * 1.2) as f32,
        (-18.7_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_5
    [
        (30.5_f64 * 1.2) as f32,
        (9.9_f64 * 1.2) as f32,
        (-18.7_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_5
    [
        (30.5_f64 * 1.2) as f32,
        (9.9_f64 * 1.2) as f32,
        (-18.7_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_BLASTER_6
    [
        (27.6_f64 * 1.2) as f32,
        (3.4_f64 * 1.2) as f32,
        (-10.4_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_6
    [
        (27.6_f64 * 1.2) as f32,
        (3.4_f64 * 1.2) as f32,
        (-10.4_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_6
    [
        (27.6_f64 * 1.2) as f32,
        (3.4_f64 * 1.2) as f32,
        (-10.4_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_BLASTER_7
    [
        (28.9_f64 * 1.2) as f32,
        (4.6_f64 * 1.2) as f32,
        (-8.1_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_7
    [
        (28.9_f64 * 1.2) as f32,
        (4.6_f64 * 1.2) as f32,
        (-8.1_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_7
    [
        (28.9_f64 * 1.2) as f32,
        (4.6_f64 * 1.2) as f32,
        (-8.1_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_BLASTER_8
    [
        (31.5_f64 * 1.2) as f32,
        (9.6_f64 * 1.2) as f32,
        (10.1_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_SHOTGUN_8
    [
        (34.5_f64 * 1.2) as f32,
        (9.6_f64 * 1.2) as f32,
        (6.1_f64 * 1.2) as f32,
    ],
    // MZ2_SOLDIER_MACHINEGUN_8
    [
        (34.5_f64 * 1.2) as f32,
        (9.6_f64 * 1.2) as f32,
        (6.1_f64 * 1.2) as f32,
    ],
    // MZ2_MAKRON_BFG
    [17.0, -19.5, 62.9],
    // MZ2_MAKRON_BLASTER_1
    [-3.6, -24.1, 59.5],
    // MZ2_MAKRON_BLASTER_2
    [-1.6, -19.3, 59.5],
    // MZ2_MAKRON_BLASTER_3
    [-0.1, -14.4, 59.5],
    // MZ2_MAKRON_BLASTER_4
    [2.0, -7.6, 59.5],
    // MZ2_MAKRON_BLASTER_5
    [3.4, 1.3, 59.5],
    // MZ2_MAKRON_BLASTER_6
    [3.7, 11.1, 59.5],
    // MZ2_MAKRON_BLASTER_7
    [-0.3, 22.3, 59.5],
    // MZ2_MAKRON_BLASTER_8
    [-6.0, 33.0, 59.5],
    // MZ2_MAKRON_BLASTER_9
    [-9.3, 36.4, 59.5],
    // MZ2_MAKRON_BLASTER_10
    [-7.0, 35.0, 59.5],
    // MZ2_MAKRON_BLASTER_11
    [-2.1, 29.0, 59.5],
    // MZ2_MAKRON_BLASTER_12
    [3.9, 17.3, 59.5],
    // MZ2_MAKRON_BLASTER_13
    [6.1, 5.8, 59.5],
    // MZ2_MAKRON_BLASTER_14
    [5.9, -4.4, 59.5],
    // MZ2_MAKRON_BLASTER_15
    [4.2, -14.1, 59.5],
    // MZ2_MAKRON_BLASTER_16
    [2.4, -18.8, 59.5],
    // MZ2_MAKRON_BLASTER_17
    [-1.8, -25.5, 59.5],
    // MZ2_MAKRON_RAILGUN_1
    [-17.3, 7.8, 72.4],
    // MZ2_JORG_MACHINEGUN_L1
    [78.5, -47.1, 96.0],
    // MZ2_JORG_MACHINEGUN_L2
    [78.5, -47.1, 96.0],
    // MZ2_JORG_MACHINEGUN_L3
    [78.5, -47.1, 96.0],
    // MZ2_JORG_MACHINEGUN_L4
    [78.5, -47.1, 96.0],
    // MZ2_JORG_MACHINEGUN_L5
    [78.5, -47.1, 96.0],
    // MZ2_JORG_MACHINEGUN_L6
    [78.5, -47.1, 96.0],
    // MZ2_JORG_MACHINEGUN_R1
    [78.5, 46.7, 96.0],
    // MZ2_JORG_MACHINEGUN_R2
    [78.5, 46.7, 96.0],
    // MZ2_JORG_MACHINEGUN_R3
    [78.5, 46.7, 96.0],
    // MZ2_JORG_MACHINEGUN_R4
    [78.5, 46.7, 96.0],
    // MZ2_JORG_MACHINEGUN_R5
    [78.5, 46.7, 96.0],
    // MZ2_JORG_MACHINEGUN_R6
    [78.5, 46.7, 96.0],
    // MZ2_JORG_BFG_1
    [6.3, -9.0, 111.2],
    // MZ2_BOSS2_MACHINEGUN_R1
    [32.0, 40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_R2
    [32.0, 40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_R3
    [32.0, 40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_R4
    [32.0, 40.0, 70.0],
    // MZ2_BOSS2_MACHINEGUN_R5
    [32.0, 40.0, 70.0],
    // MZ2_CARRIER_MACHINEGUN_L1
    [56.0, -32.0, 32.0],
    // MZ2_CARRIER_MACHINEGUN_R1
    [56.0, 32.0, 32.0],
    // MZ2_CARRIER_GRENADE
    [42.0, 24.0, 50.0],
    // MZ2_TURRET_MACHINEGUN
    [16.0, 0.0, 0.0],
    // MZ2_TURRET_ROCKET
    [16.0, 0.0, 0.0],
    // MZ2_TURRET_BLASTER
    [16.0, 0.0, 0.0],
    // MZ2_STALKER_BLASTER
    [24.0, 0.0, 6.0],
    // MZ2_DAEDALUS_BLASTER
    [32.5, -0.8, 10.0],
    // MZ2_MEDIC_BLASTER_2
    [12.1, 5.4, 16.5],
    // MZ2_CARRIER_RAILGUN
    [32.0, 0.0, 6.0],
    // MZ2_WIDOW_DISRUPTOR
    [57.72, 14.50, 88.81],
    // MZ2_WIDOW_BLASTER
    [56.0, 32.0, 32.0],
    // MZ2_WIDOW_RAIL
    [62.0, -20.0, 84.0],
    // MZ2_WIDOW_PLASMABEAM
    [32.0, 0.0, 6.0],
    // MZ2_CARRIER_MACHINEGUN_L2
    [61.0, -32.0, 12.0],
    // MZ2_CARRIER_MACHINEGUN_R2
    [61.0, 32.0, 12.0],
    // MZ2_WIDOW_RAIL_LEFT
    [17.0, -62.0, 91.0],
    // MZ2_WIDOW_RAIL_RIGHT
    [68.0, 12.0, 86.0],
    // MZ2_WIDOW_BLASTER_SWEEP1
    [47.5, 56.0, 89.0],
    // MZ2_WIDOW_BLASTER_SWEEP2
    [54.0, 52.0, 91.0],
    // MZ2_WIDOW_BLASTER_SWEEP3
    [58.0, 40.0, 91.0],
    // MZ2_WIDOW_BLASTER_SWEEP4
    [68.0, 30.0, 88.0],
    // MZ2_WIDOW_BLASTER_SWEEP5
    [74.0, 20.0, 88.0],
    // MZ2_WIDOW_BLASTER_SWEEP6
    [73.0, 11.0, 87.0],
    // MZ2_WIDOW_BLASTER_SWEEP7
    [73.0, 3.0, 87.0],
    // MZ2_WIDOW_BLASTER_SWEEP8
    [70.0, -12.0, 87.0],
    // MZ2_WIDOW_BLASTER_SWEEP9
    [67.0, -20.0, 90.0],
    // MZ2_WIDOW_BLASTER_100
    [-20.0, 76.0, 90.0],
    // MZ2_WIDOW_BLASTER_90
    [-8.0, 74.0, 90.0],
    // MZ2_WIDOW_BLASTER_80
    [0.0, 72.0, 90.0],
    // MZ2_WIDOW_BLASTER_70
    [10.0, 71.0, 89.0],
    // MZ2_WIDOW_BLASTER_60
    [23.0, 70.0, 87.0],
    // MZ2_WIDOW_BLASTER_50
    [32.0, 64.0, 85.0],
    // MZ2_WIDOW_BLASTER_40
    [40.0, 58.0, 84.0],
    // MZ2_WIDOW_BLASTER_30
    [48.0, 50.0, 83.0],
    // MZ2_WIDOW_BLASTER_20
    [54.0, 42.0, 82.0],
    // MZ2_WIDOW_BLASTER_10
    [56.0, 34.0, 82.0],
    // MZ2_WIDOW_BLASTER_0
    [58.0, 26.0, 82.0],
    // MZ2_WIDOW_BLASTER_10L
    [60.0, 16.0, 82.0],
    // MZ2_WIDOW_BLASTER_20L
    [59.0, 6.0, 81.0],
    // MZ2_WIDOW_BLASTER_30L
    [58.0, -2.0, 80.0],
    // MZ2_WIDOW_BLASTER_40L
    [57.0, -10.0, 79.0],
    // MZ2_WIDOW_BLASTER_50L
    [54.0, -18.0, 78.0],
    // MZ2_WIDOW_BLASTER_60L
    [42.0, -32.0, 80.0],
    // MZ2_WIDOW_BLASTER_70L
    [36.0, -40.0, 78.0],
    // MZ2_WIDOW_RUN_1
    [68.4, 10.88, 82.08],
    // MZ2_WIDOW_RUN_2
    [68.51, 8.64, 85.14],
    // MZ2_WIDOW_RUN_3
    [68.66, 6.38, 88.78],
    // MZ2_WIDOW_RUN_4
    [68.73, 5.1, 84.47],
    // MZ2_WIDOW_RUN_5
    [68.82, 4.79, 80.52],
    // MZ2_WIDOW_RUN_6
    [68.77, 6.11, 85.37],
    // MZ2_WIDOW_RUN_7
    [68.67, 7.99, 90.24],
    // MZ2_WIDOW_RUN_8
    [68.55, 9.54, 87.36],
    // MZ2_CARRIER_ROCKET_1
    [0.0, 0.0, -5.0],
    // MZ2_CARRIER_ROCKET_2
    [0.0, 0.0, -5.0],
    // MZ2_CARRIER_ROCKET_3
    [0.0, 0.0, -5.0],
    // MZ2_CARRIER_ROCKET_4
    [0.0, 0.0, -5.0],
    // MZ2_WIDOW2_BEAMER_1
    [69.00, -17.63, 93.77],
    // MZ2_WIDOW2_BEAMER_2
    [69.00, -17.08, 89.82],
    // MZ2_WIDOW2_BEAMER_3
    [69.00, -18.40, 90.70],
    // MZ2_WIDOW2_BEAMER_4
    [69.00, -18.34, 94.32],
    // MZ2_WIDOW2_BEAMER_5
    [69.00, -18.30, 97.98],
    // MZ2_WIDOW2_BEAM_SWEEP_1
    [45.04, -59.02, 92.24],
    // MZ2_WIDOW2_BEAM_SWEEP_2
    [50.68, -54.70, 91.96],
    // MZ2_WIDOW2_BEAM_SWEEP_3
    [56.57, -47.72, 91.65],
    // MZ2_WIDOW2_BEAM_SWEEP_4
    [61.75, -38.75, 91.38],
    // MZ2_WIDOW2_BEAM_SWEEP_5
    [65.55, -28.76, 91.24],
    // MZ2_WIDOW2_BEAM_SWEEP_6
    [67.79, -18.90, 91.22],
    // MZ2_WIDOW2_BEAM_SWEEP_7
    [68.60, -9.52, 91.23],
    // MZ2_WIDOW2_BEAM_SWEEP_8
    [68.08, 0.18, 91.32],
    // MZ2_WIDOW2_BEAM_SWEEP_9
    [66.14, 9.79, 91.44],
    // MZ2_WIDOW2_BEAM_SWEEP_10
    [62.77, 18.91, 91.65],
    // MZ2_WIDOW2_BEAM_SWEEP_11
    [58.29, 27.11, 92.00],
    // end of table
    [0.0, 0.0, 0.0],
];

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
/// pair that opens every C monster attack.
pub fn monster_flash_source(
    world: &GameWorld,
    key: EntityKey,
    flash: i32,
) -> Option<(Vec3f, Vec3f, Vec3f)> {
    let ent = world.entities.get(key)?;
    let (forward, right, _) = crate::physics::angle_vectors(ent.state.angles);
    let start = project_source(
        ent.state.origin,
        MONSTER_FLASH_OFFSET[flash as usize],
        forward,
        right,
    );
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
}
