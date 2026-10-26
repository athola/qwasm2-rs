//! The player moves through the full chain C uses: GameWorld::client_think
//! runs pmove, whose traces go through ServerGameImport (SV_Trace) against
//! the loaded map.

mod common;

use std::rc::Rc;

use q2_common::pmove::PMF_ON_GROUND;
use q2_game::{traits::GameExport, world::GameWorld};
use q2_server::{
    state::{Server, ServerStatic},
    ServerGameImport,
};
use q2_shared::constants::MAX_EDICTS;
use q2_shared::types::{PlayerState, UserCmd, Vec3f};

/// Entity number of the only client.
const PLAYER: usize = 1;

struct Game {
    sv: Server,
    svs: ServerStatic,
    gi: Rc<ServerGameImport>,
    game: GameWorld,
}

impl Game {
    /// The fixture map with one client in the game at the player start.
    fn start() -> Self {
        Self::start_on("floor_wall", &common::floor_wall_bsp())
    }

    /// `bsp` with one client in the game at its player start.
    fn start_on(name: &str, bsp: &[u8]) -> Self {
        let gi = Rc::new(ServerGameImport::new());
        let mut game = GameWorld::new(gi.clone(), MAX_EDICTS);
        game.set_max_clients(1);
        game.init();
        let mut svs = ServerStatic::default();
        svs.init_game(1);
        let mut sv = Server::default();
        sv.map(&mut svs, name, bsp, &gi, &mut game)
            .expect("fixture loads");
        assert!(game.client_connect(PLAYER, r"\name\Tester"));
        game.client_begin(PLAYER);
        Game { sv, svs, gi, game }
    }

    /// 100 ms of play: two 50 ms client commands, then a server frame, the
    /// way a client sends usercmds faster than the 10 Hz game runs.
    fn tick(&mut self, cmd: &UserCmd) {
        for _ in 0..2 {
            self.game
                .client_think(PLAYER, &UserCmd { msec: 50, ..*cmd });
        }
        self.sv
            .frame(&mut self.svs, 100_000, &mut self.game, &self.gi);
    }

    fn origin(&self) -> Vec3f {
        let key = self.game.entities.key_for_number(PLAYER).unwrap();
        self.game.entities.get(key).unwrap().state.origin
    }

    fn ps(&self) -> PlayerState {
        let key = self.game.entities.key_for_number(PLAYER).unwrap();
        let ent = self.game.entities.get(key).unwrap();
        ent.client.as_ref().unwrap().ps.clone()
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
