//! Platform-agnostic player controller — first-person movement with collision.
//!
//! Extracted from `q2-wasm::Engine::tick()` so that player movement logic is
//! testable without a WASM environment and reusable across entry points
//! (native, WASM, dedicated server prediction).

use crate::collision::CollisionMap;
use q2_shared::types::Vec3f;

// ---------------------------------------------------------------------------
// Physics constants (matching Quake 2 defaults)
// ---------------------------------------------------------------------------

const GRAVITY: f32 = 800.0;
const JUMP_VELOCITY: f32 = 270.0;
const MOVE_SPEED: f32 = 300.0;
/// CONTENTS_SOLID | CONTENTS_PLAYERCLIP | CONTENTS_WINDOW | CONTENTS_MONSTER
const MASK_PLAYERSOLID: i32 = 1 | 0x10000 | 2 | 0x0200_0000;
const STEP_HEIGHT: f32 = 18.0;

const PLAYER_MINS_STAND: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const PLAYER_MAXS_STAND: Vec3f = Vec3f::new(16.0, 16.0, 32.0);
const PLAYER_MINS_DUCK: Vec3f = Vec3f::new(-16.0, -16.0, -24.0);
const PLAYER_MAXS_DUCK: Vec3f = Vec3f::new(16.0, 16.0, 4.0);

// ---------------------------------------------------------------------------
// Input abstraction
// ---------------------------------------------------------------------------

/// Platform-agnostic movement input for one frame.
pub struct MoveInput {
    /// Horizontal wish direction: +X = forward, +Y = right (unnormalized ok).
    pub forward: f32,
    pub right: f32,
    /// Yaw change in degrees (positive = turn left).
    pub yaw_delta: f32,
    /// Pitch change in degrees (positive = look up).
    pub pitch_delta: f32,
    pub jump: bool,
    pub duck: bool,
    pub run: bool,
}

// ---------------------------------------------------------------------------
// Player state
// ---------------------------------------------------------------------------

/// First-person player state: position, orientation, velocity.
pub struct PlayerController {
    pub pos: Vec3f,
    pub yaw: f32,
    pub pitch: f32,
    pub velocity_z: f32,
    pub on_ground: bool,
    pub ducked: bool,
}

impl PlayerController {
    pub fn new(spawn_pos: Vec3f, spawn_yaw: f32) -> Self {
        Self {
            pos: spawn_pos,
            yaw: spawn_yaw,
            pitch: 0.0,
            velocity_z: 0.0,
            on_ground: false,
            ducked: false,
        }
    }

    /// Snap the player downward to the ground surface.
    pub fn snap_to_ground(&mut self, collision: &mut CollisionMap) {
        let start = self.pos;
        let end = Vec3f::new(start.x, start.y, start.z - 1000.0);
        let trace = collision.box_trace(
            start,
            end,
            PLAYER_MINS_STAND,
            PLAYER_MAXS_STAND,
            0,
            MASK_PLAYERSOLID,
        );
        if trace.fraction < 1.0 {
            self.pos = trace.endpos;
            self.on_ground = true;
        }
    }

    /// Viewpoint position (origin + viewheight offset).
    pub fn view_origin(&self) -> Vec3f {
        let viewheight: f32 = if self.ducked { -2.0 } else { 22.0 };
        Vec3f::new(self.pos.x, self.pos.y, self.pos.z + viewheight)
    }

    /// Run one frame of movement: apply input, simulate physics, resolve
    /// collisions. `dt` is the time step in seconds.
    pub fn tick(&mut self, dt: f32, input: &MoveInput, collision: &mut CollisionMap) {
        let dt = dt.min(0.1);

        // Look
        self.yaw += input.yaw_delta;
        self.pitch += input.pitch_delta;
        self.pitch = self.pitch.clamp(-89.0, 89.0);

        // Movement wish vector
        let move_speed = if input.run {
            MOVE_SPEED * 2.0
        } else {
            MOVE_SPEED
        };
        let speed = move_speed * dt;

        let yaw_rad = self.yaw.to_radians();
        let fwd = Vec3f::new(yaw_rad.cos(), yaw_rad.sin(), 0.0);
        let rgt = Vec3f::new(yaw_rad.sin(), -yaw_rad.cos(), 0.0);

        let mut wish = Vec3f::ZERO;
        wish += fwd * input.forward;
        wish += rgt * input.right;

        let wish_len = (wish.x * wish.x + wish.y * wish.y).sqrt();
        if wish_len > 0.0 {
            wish.x = wish.x / wish_len * speed;
            wish.y = wish.y / wish_len * speed;
        }

        // Jump
        if input.jump && self.on_ground {
            self.velocity_z = JUMP_VELOCITY;
            self.on_ground = false;
        }

        // Crouch
        self.ducked = input.duck;

        let player_mins = if self.ducked {
            PLAYER_MINS_DUCK
        } else {
            PLAYER_MINS_STAND
        };
        let player_maxs = if self.ducked {
            PLAYER_MAXS_DUCK
        } else {
            PLAYER_MAXS_STAND
        };

        // Gravity
        if !self.on_ground {
            self.velocity_z -= GRAVITY * dt;
        }

        // Gravity displacement — saved before step-up can overwrite new_pos.z.
        let gravity_dz = self.velocity_z * dt;

        let mut new_pos = self.pos;
        new_pos.x += wish.x;
        new_pos.y += wish.y;
        // Gravity is applied via the vertical trace below, not here, so that
        // step-up resolution doesn't lose the gravity displacement.

        // Horizontal trace (slide against walls)
        let h_target = Vec3f::new(new_pos.x, new_pos.y, self.pos.z);
        let trace = collision.box_trace(
            self.pos,
            h_target,
            player_mins,
            player_maxs,
            0,
            MASK_PLAYERSOLID,
        );
        let landed = Vec3f::new(
            self.pos.x + (h_target.x - self.pos.x) * trace.fraction,
            self.pos.y + (h_target.y - self.pos.y) * trace.fraction,
            self.pos.z,
        );

        // Step up if blocked
        if trace.fraction < 1.0 {
            let step_start = Vec3f::new(landed.x, landed.y, landed.z + STEP_HEIGHT);
            let step_trace = collision.box_trace(
                Vec3f::new(landed.x, landed.y, landed.z),
                step_start,
                player_mins,
                player_maxs,
                0,
                MASK_PLAYERSOLID,
            );
            let step_z = landed.z + (step_start.z - landed.z) * step_trace.fraction;
            let slide_trace = collision.box_trace(
                Vec3f::new(landed.x, landed.y, step_z),
                Vec3f::new(h_target.x, h_target.y, step_z),
                player_mins,
                player_maxs,
                0,
                MASK_PLAYERSOLID,
            );
            let stepped = Vec3f::new(
                landed.x + (h_target.x - landed.x) * slide_trace.fraction,
                landed.y + (h_target.y - landed.y) * slide_trace.fraction,
                step_z,
            );
            let down_trace = collision.box_trace(
                stepped,
                Vec3f::new(stepped.x, stepped.y, landed.z),
                player_mins,
                player_maxs,
                0,
                MASK_PLAYERSOLID,
            );
            new_pos.x = stepped.x;
            new_pos.y = stepped.y;
            new_pos.z = stepped.z + (landed.z - stepped.z) * down_trace.fraction;
        } else {
            new_pos.x = landed.x;
            new_pos.y = landed.y;
        }

        // Vertical trace (gravity / jump) — start from post-horizontal Z so
        // step-up results are not bypassed by a stale self.pos.z.
        let v_start = new_pos;
        let v_end = Vec3f::new(new_pos.x, new_pos.y, new_pos.z + gravity_dz);
        let v_trace = collision.box_trace(
            v_start,
            v_end,
            player_mins,
            player_maxs,
            0,
            MASK_PLAYERSOLID,
        );
        new_pos.z = v_start.z + (v_end.z - v_start.z) * v_trace.fraction;

        if v_trace.fraction < 1.0 && self.velocity_z < 0.0 {
            self.velocity_z = 0.0;
            self.on_ground = true;
        }

        // Ground check
        let ground_trace = collision.box_trace(
            new_pos,
            Vec3f::new(new_pos.x, new_pos.y, new_pos.z - 1.0),
            player_mins,
            player_maxs,
            0,
            MASK_PLAYERSOLID,
        );
        if ground_trace.fraction < 1.0 {
            self.on_ground = true;
            if self.velocity_z < 0.0 {
                self.velocity_z = 0.0;
            }
        } else {
            self.on_ground = false;
        }

        self.pos = new_pos;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::{build_floor_bsp, build_minimal_bsp};

    #[test]
    fn new_controller_has_spawn_state() {
        let pc = PlayerController::new(Vec3f::new(1.0, 2.0, 3.0), 90.0);
        assert_eq!(pc.pos, Vec3f::new(1.0, 2.0, 3.0));
        assert_eq!(pc.yaw, 90.0);
        assert_eq!(pc.pitch, 0.0);
        assert_eq!(pc.velocity_z, 0.0);
        assert!(!pc.on_ground);
        assert!(!pc.ducked);
    }

    #[test]
    fn view_origin_standing_vs_ducked() {
        let mut pc = PlayerController::new(Vec3f::new(0.0, 0.0, 100.0), 0.0);
        let standing = pc.view_origin();
        assert_eq!(standing.z, 122.0);

        pc.ducked = true;
        let ducked = pc.view_origin();
        assert_eq!(ducked.z, 98.0);
    }

    #[test]
    fn look_clamps_pitch() {
        let mut pc = PlayerController::new(Vec3f::ZERO, 0.0);
        let input = MoveInput {
            forward: 0.0,
            right: 0.0,
            yaw_delta: 0.0,
            pitch_delta: 200.0,
            jump: false,
            duck: false,
            run: false,
        };
        // Empty collision map — no geometry, traces return fraction=1.0
        let mut cm = CollisionMap::new();
        pc.tick(0.016, &input, &mut cm);
        assert_eq!(pc.pitch, 89.0);
    }

    #[test]
    fn gravity_pulls_down_when_airborne() {
        let mut pc = PlayerController::new(Vec3f::new(0.0, 0.0, 500.0), 0.0);
        pc.on_ground = false;
        let input = MoveInput {
            forward: 0.0,
            right: 0.0,
            yaw_delta: 0.0,
            pitch_delta: 0.0,
            jump: false,
            duck: false,
            run: false,
        };
        let mut cm = CollisionMap::new();
        let z_before = pc.pos.z;
        pc.tick(0.1, &input, &mut cm);
        // With no geometry, player falls freely
        assert!(pc.pos.z < z_before, "gravity should pull player down");
    }

    #[test]
    fn jump_sets_velocity_when_grounded() {
        let mut pc = PlayerController::new(Vec3f::ZERO, 0.0);
        pc.on_ground = true;
        let input = MoveInput {
            forward: 0.0,
            right: 0.0,
            yaw_delta: 0.0,
            pitch_delta: 0.0,
            jump: true,
            duck: false,
            run: false,
        };
        let mut cm = CollisionMap::new();
        pc.tick(0.016, &input, &mut cm);
        assert!(!pc.on_ground);
        // velocity_z was set to JUMP_VELOCITY then gravity applied for one tick
        assert!(pc.velocity_z > 0.0);
    }

    // -----------------------------------------------------------------------
    // Collision-integrated movement tests (use real BSP geometry)
    // -----------------------------------------------------------------------

    #[test]
    fn wall_collision_stops_movement() {
        // Minimal BSP: wall at x=0 (solid for x<0, empty for x>0).
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Player at (50, 0, 0), facing -X (yaw=180). Walk forward with run speed.
        let mut pc = PlayerController::new(Vec3f::new(50.0, 0.0, 0.0), 180.0);
        pc.on_ground = true;
        let input = MoveInput {
            forward: 1.0,
            right: 0.0,
            yaw_delta: 0.0,
            pitch_delta: 0.0,
            jump: false,
            duck: false,
            run: true,
        };

        // Run several ticks — enough to walk 50 units into the wall.
        // Force on_ground each tick since this BSP has no floor.
        for _ in 0..20 {
            pc.on_ground = true;
            pc.tick(0.1, &input, &mut cm);
        }

        // Player should be stopped by wall. With half-width 16, center stops at x≈16.
        assert!(
            pc.pos.x > 14.0,
            "player should not pass through wall, got x={}",
            pc.pos.x
        );
    }

    #[test]
    fn ground_detection_on_floor() {
        // Floor BSP: solid below z=0, empty above.
        let bsp = build_floor_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Player spawns high above the floor
        let mut pc = PlayerController::new(Vec3f::new(0.0, 0.0, 200.0), 0.0);
        pc.on_ground = false;
        let input = MoveInput {
            forward: 0.0,
            right: 0.0,
            yaw_delta: 0.0,
            pitch_delta: 0.0,
            jump: false,
            duck: false,
            run: false,
        };

        // Let player fall for many ticks
        for _ in 0..50 {
            pc.tick(0.1, &input, &mut cm);
        }

        // Player should have landed on the floor.
        // Player mins.z = -24, so center z ≈ 24 when standing on z=0 surface.
        assert!(pc.on_ground, "player should be on_ground after falling");
        assert!(
            (pc.pos.z - 24.0).abs() < 2.0,
            "expected z≈24 (standing on floor at z=0), got z={}",
            pc.pos.z
        );
        assert_eq!(pc.velocity_z, 0.0, "velocity should be zeroed on landing");
    }
}
