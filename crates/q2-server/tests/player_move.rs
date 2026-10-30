//! The player moves through the full chain C uses: GameWorld::client_think
//! runs pmove, whose traces go through ServerGameImport (SV_Trace) against
//! the loaded map. The tests drive the concrete `GameWorld` so they can
//! check game state (ground entities, killed targets) that `LocalGame`
//! keeps behind `GameExport`.

mod common;

use q2_common::pmove::PMF_ON_GROUND;
use q2_shared::types::{PlayerState, UserCmd, Vec3f};

struct Game(common::ServerWorld);

impl Game {
    /// The fixture map with one client in the game at the player start.
    fn start() -> Self {
        Self::start_on("floor_wall", &common::floor_wall_bsp())
    }

    /// `bsp` with one client in the game at its player start.
    fn start_on(name: &str, bsp: &[u8]) -> Self {
        Game(common::ServerWorld::start(name, bsp))
    }

    /// 100 ms of play: two 50 ms client commands, then a server frame, the
    /// way a client sends usercmds faster than the 10 Hz game runs.
    fn tick(&mut self, cmd: &UserCmd) {
        for _ in 0..2 {
            self.0.think(&UserCmd { msec: 50, ..*cmd });
        }
        self.0.frame(100_000);
    }

    fn player(&self) -> &q2_game::entity::Entity {
        let world = &self.0.game;
        let key = world.entities.key_for_number(1).expect("player in game");
        world.entities.get(key).expect("player in game")
    }

    fn origin(&self) -> Vec3f {
        self.player().state.origin
    }

    fn ps(&self) -> PlayerState {
        let world = &self.0.game;
        let key = world.entities.key_for_number(1).expect("player in game");
        world.client(key).expect("a client").ps.clone()
    }
}

/// ANGLE2SHORT.
fn angle2short(degrees: f32) -> i16 {
    ((degrees * 65536.0 / 360.0) as i32 & 65535) as i16
}

#[test]
fn player_falls_to_the_floor_and_stays_on_the_ground() {
    let mut g = Game::start();
    // SelectSpawnPoint lifts the start 9 units and PutClientInServer 1 more.
    assert_eq!(g.origin(), Vec3f::new(0.0, 0.0, 34.0));

    for _ in 0..20 {
        g.tick(&UserCmd::default());
    }
    // Feet (mins.z = -24) on the floor at z = 0; pmove snaps to 1/8 units.
    assert!((g.origin().z - 24.0).abs() <= 0.25, "{:?}", g.origin());
    assert_ne!(g.ps().pmove.pm_flags & PMF_ON_GROUND, 0);
}

#[test]
fn player_faces_the_spawn_angle_with_a_zero_command_angle() {
    let mut g = Game::start();
    g.tick(&UserCmd::default());
    // info_player_start "angle" "90": delta_angles turn cmd yaw 0 into 90.
    assert!(
        (g.ps().viewangles.y - 90.0).abs() < 0.01,
        "{:?}",
        g.ps().viewangles
    );
}

#[test]
fn walking_forward_stops_at_the_wall() {
    let mut g = Game::start();
    for _ in 0..10 {
        g.tick(&UserCmd::default());
    }
    // Face +x: the spawn turned the view 90, so command yaw -90.
    let forward = UserCmd {
        forwardmove: 200,
        angles: [0, angle2short(-90.0), 0],
        ..Default::default()
    };
    for _ in 0..40 {
        g.tick(&forward);
    }
    let face = common::WALL_X - 16.0;
    assert!(
        g.origin().x > face - 1.0,
        "reached the wall: {:?}",
        g.origin()
    );
    assert!(
        g.origin().x <= face + 0.125,
        "did not pass it: {:?}",
        g.origin()
    );
    assert!((g.origin().z - 24.0).abs() <= 0.25, "still on the floor");
}

#[test]
fn walking_forward_stops_at_a_func_wall_brush_entity() {
    let entities = format!(
        "{}{{\n\"classname\" \"func_wall\"\n\"model\" \"*1\"\n}}\n",
        common::ENTSTRING
    );
    let mut g = Game::start_on("floor_wall", &common::floor_wall_bsp_with(&entities));
    for _ in 0..10 {
        g.tick(&UserCmd::default());
    }
    let forward = UserCmd {
        forwardmove: 200,
        angles: [0, angle2short(-90.0), 0],
        ..Default::default()
    };
    for _ in 0..40 {
        g.tick(&forward);
    }
    let face = common::DOOR_MINS[0] - 16.0;
    assert!(g.origin().x > face - 1.0, "reached it: {:?}", g.origin());
    assert!(
        g.origin().x <= face + 0.125,
        "blocked by *1: {:?}",
        g.origin()
    );
}

/// `ENTSTRING` plus `extra` entity blocks.
fn map_with(extra: &str) -> Vec<u8> {
    common::floor_wall_bsp_with(&format!("{}{extra}", common::ENTSTRING))
}

fn walk_east(g: &mut Game, ticks: usize) {
    let forward = UserCmd {
        forwardmove: 200,
        angles: [0, angle2short(-90.0), 0],
        ..Default::default()
    };
    for _ in 0..ticks {
        g.tick(&forward);
    }
}

#[test]
fn walking_up_to_a_door_opens_it_and_the_player_walks_through() {
    let map = map_with("{\n\"classname\" \"func_door\"\n\"model\" \"*1\"\n\"angle\" \"-1\"\n}\n");
    let mut g = Game::start_on("floor_wall", &map);
    for _ in 0..10 {
        g.tick(&UserCmd::default());
    }
    walk_east(&mut g, 60);
    let face = common::WALL_X - 16.0;
    assert!(
        g.origin().x > face - 1.0,
        "passed the door to the wall: {:?}",
        g.origin()
    );
}

#[test]
fn walking_into_a_trigger_once_fires_its_killtarget() {
    let map = map_with(concat!(
        "{\n\"classname\" \"trigger_once\"\n\"model\" \"*1\"\n\"killtarget\" \"victim\"\n}\n",
        "{\n\"classname\" \"info_notnull\"\n\"targetname\" \"victim\"\n}\n",
    ));
    let mut g = Game::start_on("floor_wall", &map);
    let alive = |g: &Game| {
        g.0.game
            .find_by_targetname(None, "victim")
            .is_some_and(|k| g.0.game.entities.get(k).is_some_and(|e| e.in_use))
    };
    for _ in 0..10 {
        g.tick(&UserCmd::default());
    }
    assert!(alive(&g));
    walk_east(&mut g, 40);
    assert!(!alive(&g), "trigger fired when the player entered *1");
}

#[test]
fn standing_on_a_brush_entity_makes_it_the_ground_entity() {
    // Start above *1's brush (top z = 128) instead of the floor.
    let entities = concat!(
        "{\n\"classname\" \"worldspawn\"\n}\n",
        "{\n\"classname\" \"info_player_start\"\n\"origin\" \"112 0 160\"\n}\n",
        "{\n\"classname\" \"func_wall\"\n\"model\" \"*1\"\n}\n",
    );
    let mut g = Game::start_on("floor_wall", &common::floor_wall_bsp_with(entities));
    for _ in 0..20 {
        g.tick(&UserCmd::default());
    }
    let game = &g.0.game;
    let wall = game.find_by_classname(None, "func_wall").unwrap();
    let player = game.entities.key_for_number(1).unwrap();
    assert_eq!(
        game.entities.get(player).unwrap().game.ground_entity,
        Some(wall)
    );
    assert!(
        (g.origin().z - (common::DOOR_MAXS[2] + 24.0)).abs() <= 0.25,
        "{:?}",
        g.origin()
    );
}

#[test]
fn jumping_leaves_the_ground_and_lands_again() {
    let mut g = Game::start();
    for _ in 0..10 {
        g.tick(&UserCmd::default());
    }
    let jump = UserCmd {
        upmove: 200,
        ..Default::default()
    };
    g.tick(&jump);
    assert!(g.origin().z > 25.0, "in the air: {:?}", g.origin());
    for _ in 0..20 {
        g.tick(&UserCmd::default());
    }
    assert!(
        (g.origin().z - 24.0).abs() <= 0.25,
        "landed: {:?}",
        g.origin()
    );
    assert_ne!(g.ps().pmove.pm_flags & PMF_ON_GROUND, 0);
}

#[test]
fn player_settles_on_the_ground_in_demo1() {
    let Some(bsp) = common::demo1_bsp() else {
        eprintln!("skipped: gamedata/baseq2/pak0.pak not installed");
        return;
    };
    let mut g = Game::start_on("demo1", &bsp);
    let start = g.origin();
    for _ in 0..20 {
        g.tick(&UserCmd::default());
    }
    assert_ne!(
        g.ps().pmove.pm_flags & PMF_ON_GROUND,
        0,
        "at {:?}",
        g.origin()
    );
    // demo1's start is at z = 32 over a floor at z = 0: spawned at 42, the
    // player lands standing (origin 24 above the floor, snapped to 1/8).
    assert_eq!(start, Vec3f::new(128.0, -320.0, 42.0));
    assert!(
        (g.origin().z - 24.125).abs() < 0.01,
        "landed at {:?}",
        g.origin()
    );
}
