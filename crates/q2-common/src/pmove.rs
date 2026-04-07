//! Player movement prediction (pmove) — shared between client and server.
//!
//! This is a faithful port of Quake II's `pmove.c`. It is the most
//! numerically sensitive module in the engine: the 12.3 fixed-point
//! snap positions MUST match the C version bit-for-bit for client
//! prediction to stay in sync with the authoritative server.

use q2_shared::types::*;

// ---------------------------------------------------------------------------
// PMF flags (matches pmove_state_t.pm_flags bits)
// ---------------------------------------------------------------------------

pub const PMF_DUCKED: u8 = 1;
pub const PMF_JUMP_HELD: u8 = 2;
pub const PMF_ON_GROUND: u8 = 4;
pub const PMF_TIME_WATERJUMP: u8 = 8;
pub const PMF_TIME_LAND: u8 = 16;
pub const PMF_TIME_TELEPORT: u8 = 32;
pub const PMF_NO_PREDICTION: u8 = 64;

// ---------------------------------------------------------------------------
// Movement constants
// ---------------------------------------------------------------------------

pub const STEPSIZE: f32 = 18.0;
pub const MIN_STEP_NORMAL: f32 = 0.7;
pub const MAX_CLIP_PLANES: usize = 5;
pub const STOP_EPSILON: f32 = 0.1;

pub const PM_STOPSPEED: f32 = 100.0;
pub const PM_MAXSPEED: f32 = 300.0;
pub const PM_DUCKSPEED: f32 = 100.0;
pub const PM_ACCELERATE: f32 = 10.0;
pub const PM_WATERACCELERATE: f32 = 10.0;
pub const PM_FRICTION: f32 = 6.0;
pub const PM_WATERFRICTION: f32 = 1.0;
pub const PM_WATERSPEED: f32 = 400.0;
pub const PM_AIRACCELERATE: f32 = 0.0;

// ---------------------------------------------------------------------------
// Content / surface flags needed by pmove (matches Quake 2 defines)
// ---------------------------------------------------------------------------

const CONTENTS_SOLID: i32 = 1;
const CONTENTS_WATER: i32 = 0x00000020;
const CONTENTS_SLIME: i32 = 0x00000010;
const CONTENTS_LADDER: i32 = 0x20000000;
const CONTENTS_CURRENT_0: i32 = 0x00040000;
const CONTENTS_CURRENT_90: i32 = 0x00080000;
const CONTENTS_CURRENT_180: i32 = 0x00100000;
const CONTENTS_CURRENT_270: i32 = 0x00200000;
const CONTENTS_CURRENT_UP: i32 = 0x00400000;
const CONTENTS_CURRENT_DOWN: i32 = 0x00800000;
const MASK_CURRENT: i32 = CONTENTS_CURRENT_0
    | CONTENTS_CURRENT_90
    | CONTENTS_CURRENT_180
    | CONTENTS_CURRENT_270
    | CONTENTS_CURRENT_UP
    | CONTENTS_CURRENT_DOWN;
const MASK_WATER: i32 = CONTENTS_WATER | CONTENTS_SLIME | 0x00000008; // lava = 0x8
const SURF_SLICK: i32 = 0x02;

// Angle indices
const PITCH: usize = 0;
const YAW: usize = 1;
#[allow(dead_code)]
const ROLL: usize = 2;

// ---------------------------------------------------------------------------
// Per-frame working data (zeroed each frame)
// ---------------------------------------------------------------------------

struct PmLocal {
    origin: Vec3f,
    velocity: Vec3f,
    forward: Vec3f,
    right: Vec3f,
    up: Vec3f,
    frametime: f32,
    ground_surface: Option<Surface>,
    ground_plane: Plane,
    ground_contents: i32,
    previous_origin: [i16; 3],
    ladder: bool,
}

impl Default for PmLocal {
    fn default() -> Self {
        Self {
            origin: Vec3f::ZERO,
            velocity: Vec3f::ZERO,
            forward: Vec3f::ZERO,
            right: Vec3f::ZERO,
            up: Vec3f::ZERO,
            frametime: 0.0,
            ground_surface: None,
            ground_plane: Plane::default(),
            ground_contents: 0,
            previous_origin: [0; 3],
            ladder: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Callback types
// ---------------------------------------------------------------------------

/// Trace callback: (start, mins, maxs, end) -> Trace
pub type TraceFn = Box<dyn Fn(Vec3f, Vec3f, Vec3f, Vec3f) -> Trace>;

/// Point-contents callback: (point) -> content flags
pub type PointContentsFn = Box<dyn Fn(Vec3f) -> i32>;

// ---------------------------------------------------------------------------
// The main Pmove interface
// ---------------------------------------------------------------------------

pub struct Pmove {
    /// Networked player-movement state (modified in place).
    pub s: PmoveState,
    /// Current input command.
    pub cmd: UserCmd,
    /// Whether an external snap should be applied on the first frame.
    pub snap_initial: bool,
    /// Computed view angles (output).
    pub viewangles: Vec3f,
    /// View height relative to origin (output).
    pub viewheight: f32,
    /// Bounding box mins.
    pub mins: Vec3f,
    /// Bounding box maxs.
    pub maxs: Vec3f,
    /// True when standing on a ground entity.
    pub ground_entity: bool,
    /// Water content flags at player position.
    pub watertype: i32,
    /// 0 = not in water, 1..3 = depth.
    pub waterlevel: i32,
    /// Trace callback supplied by caller.
    pub trace: TraceFn,
    /// Point-contents callback supplied by caller.
    pub pointcontents: PointContentsFn,
}

// ---------------------------------------------------------------------------
// Helper: angle conversion (matches SHORT2ANGLE / AngleVectors)
// ---------------------------------------------------------------------------

/// Convert a 16-bit angle to degrees.  Matches `SHORT2ANGLE(x)` in C.
fn short2angle(s: i16) -> f32 {
    (s as f32) * (360.0 / 65536.0)
}

/// Compute forward / right / up unit vectors from Euler angles (degrees).
/// Matches `AngleVectors(angles, forward, right, up)` in the C engine.
fn angle_vectors(angles: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    let pitch = angles.x.to_radians();
    let yaw = angles.y.to_radians();
    let roll = angles.z.to_radians();

    let (sp, cp) = (pitch.sin(), pitch.cos());
    let (sy, cy) = (yaw.sin(), yaw.cos());
    let (sr, cr) = (roll.sin(), roll.cos());

    let forward = Vec3f::new(cp * cy, cp * sy, -sp);
    let right = Vec3f::new(
        -(sr * sp * cy) + (cr * sy),
        -(sr * sp * sy) + -(cr * cy),
        -sr * cp,
    );
    let up = Vec3f::new(
        cr * sp * cy + -sr * -sy,
        cr * sp * sy + -sr * cy,
        cr * cp,
    );
    (forward, right, up)
}

// ---------------------------------------------------------------------------
// Core movement helpers
// ---------------------------------------------------------------------------

/// Clip `in_vel` against a surface with the given `normal`.
/// `overbounce` is typically 1.01 for sliding surfaces.
fn pm_clip_velocity(in_vel: Vec3f, normal: Vec3f, overbounce: f32) -> Vec3f {
    let backoff = in_vel.dot(normal) * overbounce;
    let mut out = in_vel - normal * backoff;
    for i in 0..3 {
        if out[i] > -STOP_EPSILON && out[i] < STOP_EPSILON {
            out[i] = 0.0;
        }
    }
    out
}

/// Inner slide-move: iteratively clip velocity against contacted planes.
/// Returns `true` if any surface was hit.
fn pm_slide_move(pm: &mut Pmove, pml: &mut PmLocal) -> bool {
    let numbumps = 4;
    let primal_velocity = pml.velocity;
    let mut numplanes: usize = 0;
    let mut planes = [Vec3f::ZERO; MAX_CLIP_PLANES];
    let mut time_left = pml.frametime;
    let mut blocked = false;

    for _bumpcount in 0..numbumps {
        let end = pml.origin + pml.velocity * time_left;
        let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, end);

        if trace.allsolid {
            // Trapped in solid — kill vertical velocity.
            pml.velocity.z = 0.0;
            return true;
        }

        if trace.fraction > 0.0 {
            pml.origin = trace.endpos;
            numplanes = 0;
        }

        if trace.fraction == 1.0 {
            break; // moved the entire distance
        }

        blocked = true;
        time_left -= time_left * trace.fraction;

        if numplanes >= MAX_CLIP_PLANES {
            pml.velocity = Vec3f::ZERO;
            break;
        }

        planes[numplanes] = trace.plane.normal;
        numplanes += 1;

        // Modify velocity so it parallels all clip planes.
        let mut found = false;
        for i in 0..numplanes {
            pml.velocity = pm_clip_velocity(pml.velocity, planes[i], 1.01);

            let mut ok = true;
            for (j, plane) in planes[..numplanes].iter().enumerate() {
                if j != i && pml.velocity.dot(*plane) < 0.0 {
                    ok = false;
                    break;
                }
            }

            if ok {
                found = true;
                break;
            }
        }

        if !found {
            if numplanes != 2 {
                pml.velocity = Vec3f::ZERO;
                break;
            }

            // Slide along the crease formed by two planes.
            let dir = planes[0].cross(planes[1]);
            let d = dir.dot(pml.velocity);
            pml.velocity = dir * d;
        }

        // If velocity is against the original velocity, stop dead.
        if pml.velocity.dot(primal_velocity) <= 0.0 {
            pml.velocity = Vec3f::ZERO;
            break;
        }
    }

    if pm.s.pm_time != 0 {
        pml.velocity = primal_velocity;
    }

    blocked
}

/// Try to step over obstacles: first slide, then try stepping up and sliding.
fn pm_step_slide_move(pm: &mut Pmove, pml: &mut PmLocal) {
    let start_o = pml.origin;
    let start_v = pml.velocity;

    // First, try a normal slide.
    pm_slide_move(pm, pml);

    let down_o = pml.origin;
    let down_v = pml.velocity;

    // Try stepping up.
    let mut up = start_o;
    up.z += STEPSIZE;

    let trace = (pm.trace)(up, pm.mins, pm.maxs, up);
    if trace.allsolid {
        return; // can't step up
    }

    // Try sliding from the stepped-up position.
    pml.origin = up;
    pml.velocity = start_v;
    pm_slide_move(pm, pml);

    // Push back down.
    let mut down = pml.origin;
    down.z -= STEPSIZE;
    let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, down);

    if !trace.allsolid {
        pml.origin = trace.endpos;
    }

    let up_pos = pml.origin;

    // Decide which one went farther (horizontal distance only).
    let down_dist = (down_o.x - start_o.x).powi(2) + (down_o.y - start_o.y).powi(2);
    let up_dist = (up_pos.x - start_o.x).powi(2) + (up_pos.y - start_o.y).powi(2);

    if down_dist > up_dist || trace.plane.normal.z < MIN_STEP_NORMAL {
        pml.origin = down_o;
        pml.velocity = down_v;
        return;
    }

    // Keep the step-up result, but preserve the down-move's vertical velocity.
    pml.velocity.z = down_v.z;
}

// ---------------------------------------------------------------------------
// Friction
// ---------------------------------------------------------------------------

fn pm_friction(pm: &mut Pmove, pml: &mut PmLocal) {
    let speed = pml.velocity.length();

    if speed < 1.0 {
        pml.velocity.x = 0.0;
        pml.velocity.y = 0.0;
        return;
    }

    let mut drop = 0.0_f32;

    // Ground friction.
    let on_ground_non_slick = pm.ground_entity
        && pml
            .ground_surface
            .as_ref()
            .is_none_or(|s| (s.flags & SURF_SLICK) == 0);
    if on_ground_non_slick || pml.ladder {
        let control = if speed < PM_STOPSPEED {
            PM_STOPSPEED
        } else {
            speed
        };
        drop += control * PM_FRICTION * pml.frametime;
    }

    // Water friction.
    if pm.waterlevel != 0 && !pml.ladder {
        drop += speed * PM_WATERFRICTION * (pm.waterlevel as f32) * pml.frametime;
    }

    let mut newspeed = speed - drop;
    if newspeed < 0.0 {
        newspeed = 0.0;
    }
    newspeed /= speed;

    pml.velocity *= newspeed;
}

// ---------------------------------------------------------------------------
// Acceleration
// ---------------------------------------------------------------------------

fn pm_accelerate(pml: &mut PmLocal, wishdir: Vec3f, wishspeed: f32, accel: f32) {
    let currentspeed = pml.velocity.dot(wishdir);
    let addspeed = wishspeed - currentspeed;
    if addspeed <= 0.0 {
        return;
    }

    let mut accelspeed = accel * pml.frametime * wishspeed;
    if accelspeed > addspeed {
        accelspeed = addspeed;
    }

    pml.velocity += wishdir * accelspeed;
}

fn pm_air_accelerate(pml: &mut PmLocal, wishdir: Vec3f, wishspeed: f32, accel: f32) {
    let wishspd = if wishspeed > 30.0 { 30.0 } else { wishspeed };

    let currentspeed = pml.velocity.dot(wishdir);
    let addspeed = wishspd - currentspeed;
    if addspeed <= 0.0 {
        return;
    }

    let mut accelspeed = accel * wishspeed * pml.frametime;
    if accelspeed > addspeed {
        accelspeed = addspeed;
    }

    pml.velocity += wishdir * accelspeed;
}

// ---------------------------------------------------------------------------
// Add environmental currents (ladders, water currents, conveyors)
// ---------------------------------------------------------------------------

fn pm_add_currents(pm: &Pmove, pml: &PmLocal, wishvel: &mut Vec3f) {
    // Ladders
    if pml.ladder && pml.velocity.z.abs() <= 200.0 {
        if pm.viewangles[PITCH] <= -15.0 && pm.cmd.forwardmove > 0 {
            wishvel.z = 200.0;
        } else if pm.viewangles[PITCH] >= 15.0 && pm.cmd.forwardmove > 0 {
            wishvel.z = -200.0;
        } else if pm.cmd.upmove > 0 {
            wishvel.z = 200.0;
        } else if pm.cmd.upmove < 0 {
            wishvel.z = -200.0;
        } else {
            wishvel.z = 0.0;
        }

        wishvel.x = wishvel.x.clamp(-25.0, 25.0);
        wishvel.y = wishvel.y.clamp(-25.0, 25.0);
    }

    // Water currents
    if (pm.watertype & MASK_CURRENT) != 0 {
        let mut v = Vec3f::ZERO;
        if (pm.watertype & CONTENTS_CURRENT_0) != 0 {
            v.x += 1.0;
        }
        if (pm.watertype & CONTENTS_CURRENT_90) != 0 {
            v.y += 1.0;
        }
        if (pm.watertype & CONTENTS_CURRENT_180) != 0 {
            v.x -= 1.0;
        }
        if (pm.watertype & CONTENTS_CURRENT_270) != 0 {
            v.y -= 1.0;
        }
        if (pm.watertype & CONTENTS_CURRENT_UP) != 0 {
            v.z += 1.0;
        }
        if (pm.watertype & CONTENTS_CURRENT_DOWN) != 0 {
            v.z -= 1.0;
        }

        let mut s = PM_WATERSPEED;
        if pm.waterlevel == 1 && pm.ground_entity {
            s /= 2.0;
        }
        *wishvel += v * s;
    }

    // Conveyor belts (ground currents)
    if pm.ground_entity {
        let mut v = Vec3f::ZERO;
        if (pml.ground_contents & CONTENTS_CURRENT_0) != 0 {
            v.x += 1.0;
        }
        if (pml.ground_contents & CONTENTS_CURRENT_90) != 0 {
            v.y += 1.0;
        }
        if (pml.ground_contents & CONTENTS_CURRENT_180) != 0 {
            v.x -= 1.0;
        }
        if (pml.ground_contents & CONTENTS_CURRENT_270) != 0 {
            v.y -= 1.0;
        }
        if (pml.ground_contents & CONTENTS_CURRENT_UP) != 0 {
            v.z += 1.0;
        }
        if (pml.ground_contents & CONTENTS_CURRENT_DOWN) != 0 {
            v.z -= 1.0;
        }
        *wishvel += v * 100.0;
    }
}

// ---------------------------------------------------------------------------
// Movement styles
// ---------------------------------------------------------------------------

fn pm_water_move(pm: &mut Pmove, pml: &mut PmLocal) {
    let fmove = pm.cmd.forwardmove as f32;
    let smove = pm.cmd.sidemove as f32;

    let mut wishvel = pml.forward * fmove + pml.right * smove;

    if pm.cmd.forwardmove == 0 && pm.cmd.sidemove == 0 && pm.cmd.upmove == 0 {
        wishvel.z -= 60.0; // drift towards bottom
    } else {
        wishvel.z += pm.cmd.upmove as f32;
    }

    pm_add_currents(pm, pml, &mut wishvel);

    let mut wishdir = wishvel;
    let mut wishspeed = wishdir.length();
    if wishspeed != 0.0 {
        wishdir /= wishspeed;
    }

    if wishspeed > PM_MAXSPEED {
        wishvel *= PM_MAXSPEED / wishspeed;
        wishspeed = PM_MAXSPEED;
    }

    wishspeed *= 0.5;

    pm_accelerate(pml, wishdir, wishspeed, PM_WATERACCELERATE);

    pm_step_slide_move(pm, pml);
}

fn pm_air_move(pm: &mut Pmove, pml: &mut PmLocal) {
    let fmove = pm.cmd.forwardmove as f32;
    let smove = pm.cmd.sidemove as f32;

    let mut wishvel = Vec3f::new(
        pml.forward.x * fmove + pml.right.x * smove,
        pml.forward.y * fmove + pml.right.y * smove,
        0.0,
    );

    pm_add_currents(pm, pml, &mut wishvel);

    let mut wishdir = wishvel;
    let mut wishspeed = wishdir.length();
    if wishspeed != 0.0 {
        wishdir /= wishspeed;
    }

    let maxspeed = if (pm.s.pm_flags & PMF_DUCKED) != 0 {
        PM_DUCKSPEED
    } else {
        PM_MAXSPEED
    };

    if wishspeed > maxspeed {
        wishvel *= maxspeed / wishspeed;
        wishspeed = maxspeed;
    }

    if pml.ladder {
        pm_accelerate(pml, wishdir, wishspeed, PM_ACCELERATE);

        if wishvel.z == 0.0 {
            if pml.velocity.z > 0.0 {
                pml.velocity.z -= pm.s.gravity as f32 * pml.frametime;
                if pml.velocity.z < 0.0 {
                    pml.velocity.z = 0.0;
                }
            } else {
                pml.velocity.z += pm.s.gravity as f32 * pml.frametime;
                if pml.velocity.z > 0.0 {
                    pml.velocity.z = 0.0;
                }
            }
        }

        pm_step_slide_move(pm, pml);
    } else if pm.ground_entity {
        // Walking on ground.
        pml.velocity.z = 0.0;
        pm_accelerate(pml, wishdir, wishspeed, PM_ACCELERATE);

        if pm.s.gravity > 0 {
            pml.velocity.z = 0.0;
        } else {
            pml.velocity.z -= pm.s.gravity as f32 * pml.frametime;
        }

        if pml.velocity.x == 0.0 && pml.velocity.y == 0.0 {
            return;
        }

        pm_step_slide_move(pm, pml);
    } else {
        // In the air.
        if PM_AIRACCELERATE != 0.0 {
            pm_air_accelerate(pml, wishdir, wishspeed, PM_AIRACCELERATE);
        } else {
            pm_accelerate(pml, wishdir, wishspeed, 1.0);
        }

        // Add gravity.
        pml.velocity.z -= pm.s.gravity as f32 * pml.frametime;
        pm_step_slide_move(pm, pml);
    }
}

fn pm_fly_move(pm: &mut Pmove, pml: &mut PmLocal, do_clip: bool) {
    pm.viewheight = 22.0;

    // Friction
    let speed = pml.velocity.length();
    if speed < 1.0 {
        pml.velocity = Vec3f::ZERO;
    } else {
        let friction = PM_FRICTION * 1.5;
        let control = if speed < PM_STOPSPEED {
            PM_STOPSPEED
        } else {
            speed
        };
        let drop = control * friction * pml.frametime;

        let mut newspeed = speed - drop;
        if newspeed < 0.0 {
            newspeed = 0.0;
        }
        newspeed /= speed;
        pml.velocity *= newspeed;
    }

    // Accelerate
    let fmove = pm.cmd.forwardmove as f32;
    let smove = pm.cmd.sidemove as f32;

    let fwd = pml.forward.normalize_or_zero();
    let rgt = pml.right.normalize_or_zero();

    let mut wishvel = fwd * fmove + rgt * smove;
    wishvel.z += pm.cmd.upmove as f32;

    let mut wishdir = wishvel;
    let mut wishspeed = wishdir.length();
    if wishspeed != 0.0 {
        wishdir /= wishspeed;
    }

    if wishspeed > PM_MAXSPEED {
        wishvel *= PM_MAXSPEED / wishspeed;
        wishspeed = PM_MAXSPEED;
    }

    let currentspeed = pml.velocity.dot(wishdir);
    let addspeed = wishspeed - currentspeed;
    if addspeed <= 0.0 {
        // Already at or above wish speed — just move.
        if do_clip {
            let end = pml.origin + pml.velocity * pml.frametime;
            let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, end);
            pml.origin = trace.endpos;
        } else {
            pml.origin += pml.velocity * pml.frametime;
        }
        return;
    }

    let mut accelspeed = PM_ACCELERATE * pml.frametime * wishspeed;
    if accelspeed > addspeed {
        accelspeed = addspeed;
    }

    pml.velocity += wishdir * accelspeed;

    if do_clip {
        let end = pml.origin + pml.velocity * pml.frametime;
        let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, end);
        pml.origin = trace.endpos;
    } else {
        pml.origin += pml.velocity * pml.frametime;
    }
}

// ---------------------------------------------------------------------------
// Position categorization
// ---------------------------------------------------------------------------

fn pm_categorize_position(pm: &mut Pmove, pml: &mut PmLocal) {
    // Check if standing on something solid.
    let point = Vec3f::new(pml.origin.x, pml.origin.y, pml.origin.z - 0.25);

    if pml.velocity.z > 180.0 {
        pm.s.pm_flags &= !PMF_ON_GROUND;
        pm.ground_entity = false;
    } else {
        let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, point);
        pml.ground_plane = trace.plane.clone();
        pml.ground_surface = trace.surface.clone();
        pml.ground_contents = trace.contents;

        let has_ent = trace.ent_index.is_some();
        if !has_ent || (trace.plane.normal.z < 0.7 && !trace.startsolid) {
            pm.ground_entity = false;
            pm.s.pm_flags &= !PMF_ON_GROUND;
        } else {
            pm.ground_entity = true;

            // Hitting solid ground ends a waterjump.
            if (pm.s.pm_flags & PMF_TIME_WATERJUMP) != 0 {
                pm.s.pm_flags &=
                    !(PMF_TIME_WATERJUMP | PMF_TIME_LAND | PMF_TIME_TELEPORT);
                pm.s.pm_time = 0;
            }

            if (pm.s.pm_flags & PMF_ON_GROUND) == 0 {
                // Just hit the ground.
                pm.s.pm_flags |= PMF_ON_GROUND;

                if pml.velocity.z < -200.0 {
                    pm.s.pm_flags |= PMF_TIME_LAND;
                    if pml.velocity.z < -400.0 {
                        pm.s.pm_time = 25;
                    } else {
                        pm.s.pm_time = 18;
                    }
                }
            }
        }
    }

    // Determine water level.
    pm.waterlevel = 0;
    pm.watertype = 0;

    let sample2 = pm.viewheight - pm.mins.z;
    let sample1 = sample2 / 2.0;

    let mut point = Vec3f::new(pml.origin.x, pml.origin.y, pml.origin.z + pm.mins.z + 1.0);
    let cont = (pm.pointcontents)(point);

    if (cont & MASK_WATER) != 0 {
        pm.watertype = cont;
        pm.waterlevel = 1;
        point.z = pml.origin.z + pm.mins.z + sample1;
        let cont = (pm.pointcontents)(point);

        if (cont & MASK_WATER) != 0 {
            pm.waterlevel = 2;
            point.z = pml.origin.z + pm.mins.z + sample2;
            let cont = (pm.pointcontents)(point);

            if (cont & MASK_WATER) != 0 {
                pm.waterlevel = 3;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Duck check — sets mins, maxs, viewheight
// ---------------------------------------------------------------------------

fn pm_check_duck(pm: &mut Pmove, pml: &PmLocal) {
    pm.mins.x = -16.0;
    pm.mins.y = -16.0;
    pm.maxs.x = 16.0;
    pm.maxs.y = 16.0;

    if pm.s.pm_type == PmType::Gib {
        pm.mins.z = 0.0;
        pm.maxs.z = 16.0;
        pm.viewheight = 8.0;
        return;
    }

    pm.mins.z = -24.0;

    if pm.s.pm_type == PmType::Dead
        || (pm.cmd.upmove < 0 && (pm.s.pm_flags & PMF_ON_GROUND) != 0)
    {
        pm.s.pm_flags |= PMF_DUCKED;
    } else {
        // Try to stand up if currently ducked.
        if (pm.s.pm_flags & PMF_DUCKED) != 0 {
            pm.maxs.z = 32.0;
            let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, pml.origin);
            if !trace.allsolid {
                pm.s.pm_flags &= !PMF_DUCKED;
            }
        }
    }

    if (pm.s.pm_flags & PMF_DUCKED) != 0 {
        pm.maxs.z = 4.0;
        pm.viewheight = -2.0;
    } else {
        pm.maxs.z = 32.0;
        pm.viewheight = 22.0;
    }
}

// ---------------------------------------------------------------------------
// Dead move — extra friction only
// ---------------------------------------------------------------------------

fn pm_dead_move(pm: &Pmove, pml: &mut PmLocal) {
    if !pm.ground_entity {
        return;
    }

    let forward = pml.velocity.length() - 20.0;

    if forward <= 0.0 {
        pml.velocity = Vec3f::ZERO;
    } else {
        let dir = pml.velocity.normalize_or_zero();
        pml.velocity = dir * forward;
    }
}

// ---------------------------------------------------------------------------
// Special movement detection (ladders, waterjump)
// ---------------------------------------------------------------------------

fn pm_check_special_movement(pm: &mut Pmove, pml: &mut PmLocal) {
    if pm.s.pm_time != 0 {
        return;
    }

    pml.ladder = false;

    // Check for ladder.
    let mut flatforward = Vec3f::new(pml.forward.x, pml.forward.y, 0.0);
    let len = flatforward.length();
    if len != 0.0 {
        flatforward /= len;
    }

    let spot = pml.origin + flatforward;
    let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, spot);

    if trace.fraction < 1.0 && (trace.contents & CONTENTS_LADDER) != 0 {
        pml.ladder = true;
    }

    // Check for waterjump.
    if pm.waterlevel != 2 {
        return;
    }

    let mut spot = pml.origin + flatforward * 30.0;
    spot.z += 4.0;
    let cont = (pm.pointcontents)(spot);

    if (cont & CONTENTS_SOLID) == 0 {
        return;
    }

    spot.z += 16.0;
    let cont = (pm.pointcontents)(spot);
    if cont != 0 {
        return;
    }

    // Jump out of water.
    pml.velocity = flatforward * 50.0;
    pml.velocity.z = 350.0;

    pm.s.pm_flags |= PMF_TIME_WATERJUMP;
    pm.s.pm_time = 255;
}

// ---------------------------------------------------------------------------
// Jump
// ---------------------------------------------------------------------------

fn pm_check_jump(pm: &mut Pmove, pml: &mut PmLocal) {
    if (pm.s.pm_flags & PMF_TIME_LAND) != 0 {
        return;
    }

    if pm.cmd.upmove < 10 {
        pm.s.pm_flags &= !PMF_JUMP_HELD;
        return;
    }

    if (pm.s.pm_flags & PMF_JUMP_HELD) != 0 {
        return;
    }

    if pm.s.pm_type == PmType::Dead {
        return;
    }

    if pm.waterlevel >= 2 {
        pm.ground_entity = false;

        if pml.velocity.z <= -300.0 {
            return;
        }

        if pm.watertype == CONTENTS_WATER {
            pml.velocity.z = 100.0;
        } else if pm.watertype == CONTENTS_SLIME {
            pml.velocity.z = 80.0;
        } else {
            pml.velocity.z = 50.0;
        }

        return;
    }

    if !pm.ground_entity {
        return;
    }

    pm.s.pm_flags |= PMF_JUMP_HELD;
    pm.ground_entity = false;
    pml.velocity.z += 270.0;

    if pml.velocity.z < 270.0 {
        pml.velocity.z = 270.0;
    }
}

// ---------------------------------------------------------------------------
// Position snapping (12.3 fixed point)
// ---------------------------------------------------------------------------

fn pm_good_position(pm: &Pmove) -> bool {
    if pm.s.pm_type == PmType::Spectator {
        return true;
    }

    let origin = Vec3f::new(
        pm.s.origin[0] as f32 * 0.125,
        pm.s.origin[1] as f32 * 0.125,
        pm.s.origin[2] as f32 * 0.125,
    );
    let trace = (pm.trace)(origin, pm.mins, pm.maxs, origin);
    !trace.allsolid
}

fn pm_snap_position(pm: &mut Pmove, pml: &PmLocal) {
    static JITTERBITS: [i32; 8] = [0, 4, 1, 2, 3, 5, 6, 7];

    // Snap velocity to eighths.
    for (dst, src) in pm.s.velocity.iter_mut().zip(pml.velocity.as_ref().iter()) {
        *dst = (*src * 8.0) as i16;
    }

    // Compute sign for each axis and snap origin.
    let mut sign = [0i16; 3];
    let mut base = [0i16; 3];
    for (i, (s, orig)) in sign.iter_mut().zip(pml.origin.as_ref().iter()).enumerate() {
        if *orig >= 0.0 {
            *s = 1;
        } else {
            *s = -1;
        }

        pm.s.origin[i] = (*orig * 8.0) as i16;

        if pm.s.origin[i] as f32 * 0.125 == *orig {
            *s = 0;
        }
    }

    base.copy_from_slice(&pm.s.origin);

    // Try all 8 jitter combinations.
    for &bits in &JITTERBITS {
        pm.s.origin.copy_from_slice(&base);

        for (i, &s) in sign.iter().enumerate() {
            if (bits & (1 << i)) != 0 {
                pm.s.origin[i] += s;
            }
        }

        if pm_good_position(pm) {
            return;
        }
    }

    // Fall back to previous origin.
    pm.s.origin.copy_from_slice(&pml.previous_origin);
}

fn pm_initial_snap_position(pm: &mut Pmove, pml: &mut PmLocal) {
    static OFFSET: [i16; 3] = [0, -1, 1];

    let base = pm.s.origin;

    for &oz in &OFFSET {
        pm.s.origin[2] = base[2] + oz;
        for &oy in &OFFSET {
            pm.s.origin[1] = base[1] + oy;
            for &ox in &OFFSET {
                pm.s.origin[0] = base[0] + ox;

                if pm_good_position(pm) {
                    pml.origin.x = pm.s.origin[0] as f32 * 0.125;
                    pml.origin.y = pm.s.origin[1] as f32 * 0.125;
                    pml.origin.z = pm.s.origin[2] as f32 * 0.125;
                    pml.previous_origin = pm.s.origin;
                    return;
                }
            }
        }
    }

    // If no valid position found, keep the original.
    tracing::debug!("Bad InitialSnapPosition");
}

// ---------------------------------------------------------------------------
// Angle clamping
// ---------------------------------------------------------------------------

fn pm_clamp_angles(pm: &mut Pmove, pml: &mut PmLocal) {
    if (pm.s.pm_flags & PMF_TIME_TELEPORT) != 0 {
        pm.viewangles.y = short2angle(
            pm.cmd.angles[YAW].wrapping_add(pm.s.delta_angles[YAW]),
        );
        pm.viewangles.x = 0.0;
        pm.viewangles.z = 0.0;
    } else {
        for i in 0..3 {
            let temp = pm.cmd.angles[i].wrapping_add(pm.s.delta_angles[i]);
            pm.viewangles[i] = short2angle(temp);
        }

        // Clamp pitch to [-89, 89] (using the 0..360 representation).
        if pm.viewangles[PITCH] > 89.0 && pm.viewangles[PITCH] < 180.0 {
            pm.viewangles[PITCH] = 89.0;
        } else if pm.viewangles[PITCH] < 271.0 && pm.viewangles[PITCH] >= 180.0 {
            pm.viewangles[PITCH] = 271.0;
        }
    }

    let (fwd, rgt, up) = angle_vectors(pm.viewangles);
    pml.forward = fwd;
    pml.right = rgt;
    pml.up = up;
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

impl Pmove {
    /// Run one movement frame.  This is the main entry point for player
    /// movement prediction — called by both server and client.
    pub fn run(&mut self) {
        // Clear per-frame outputs.
        self.viewangles = Vec3f::ZERO;
        self.viewheight = 0.0;
        self.ground_entity = false;
        self.watertype = 0;
        self.waterlevel = 0;

        // Fresh local state each frame.
        let mut pml = PmLocal {
            origin: Vec3f::new(
                self.s.origin[0] as f32 * 0.125,
                self.s.origin[1] as f32 * 0.125,
                self.s.origin[2] as f32 * 0.125,
            ),
            velocity: Vec3f::new(
                self.s.velocity[0] as f32 * 0.125,
                self.s.velocity[1] as f32 * 0.125,
                self.s.velocity[2] as f32 * 0.125,
            ),
            previous_origin: self.s.origin,
            frametime: self.cmd.msec as f32 * 0.001,
            ..Default::default()
        };

        pm_clamp_angles(self, &mut pml);

        if self.s.pm_type == PmType::Spectator {
            pm_fly_move(self, &mut pml, false);
            pm_snap_position(self, &pml);
            return;
        }

        // Dead/gib/freeze players have no input.
        if self.s.pm_type >= PmType::Dead {
            self.cmd.forwardmove = 0;
            self.cmd.sidemove = 0;
            self.cmd.upmove = 0;
        }

        if self.s.pm_type == PmType::Freeze {
            return; // no movement at all
        }

        // Set mins, maxs, viewheight.
        pm_check_duck(self, &pml);

        if self.snap_initial {
            pm_initial_snap_position(self, &mut pml);
        }

        // Categorize position (ground, water).
        pm_categorize_position(self, &mut pml);

        if self.s.pm_type == PmType::Dead {
            pm_dead_move(self, &mut pml);
        }

        pm_check_special_movement(self, &mut pml);

        // Drop timing counter.
        if self.s.pm_time != 0 {
            let mut msec = self.cmd.msec >> 3;
            if msec == 0 {
                msec = 1;
            }

            if msec >= self.s.pm_time {
                self.s.pm_flags &=
                    !(PMF_TIME_WATERJUMP | PMF_TIME_LAND | PMF_TIME_TELEPORT);
                self.s.pm_time = 0;
            } else {
                self.s.pm_time -= msec;
            }
        }

        if (self.s.pm_flags & PMF_TIME_TELEPORT) != 0 {
            // Teleport pause — stays exactly in place.
        } else if (self.s.pm_flags & PMF_TIME_WATERJUMP) != 0 {
            // Waterjump — no control, but falls.
            pml.velocity.z -= self.s.gravity as f32 * pml.frametime;
            if pml.velocity.z < 0.0 {
                self.s.pm_flags &=
                    !(PMF_TIME_WATERJUMP | PMF_TIME_LAND | PMF_TIME_TELEPORT);
                self.s.pm_time = 0;
            }
            pm_step_slide_move(self, &mut pml);
        } else {
            pm_check_jump(self, &mut pml);
            pm_friction(self, &mut pml);

            if self.waterlevel >= 2 {
                pm_water_move(self, &mut pml);
            } else {
                let mut angles = self.viewangles;
                if angles[PITCH] > 180.0 {
                    angles[PITCH] -= 360.0;
                }
                angles[PITCH] /= 3.0;

                let (fwd, rgt, up) = angle_vectors(angles);
                pml.forward = fwd;
                pml.right = rgt;
                pml.up = up;

                pm_air_move(self, &mut pml);
            }
        }

        // Re-categorize for final spot.
        pm_categorize_position(self, &mut pml);

        pm_snap_position(self, &pml);
    }
}

// Note: PmType derives PartialOrd/Ord in q2-shared, matching the C enum order:
// Normal=0 < Spectator=1 < Dead=2 < Gib=3 < Freeze=4.
// glam::Vec3 implements Index<usize> and IndexMut<usize>, so v[i] works directly.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a no-op Pmove with open-air trace (nothing solid).
    fn make_test_pmove() -> Pmove {
        Pmove {
            s: PmoveState {
                pm_type: PmType::Normal,
                origin: [0; 3],
                velocity: [0; 3],
                pm_flags: PMF_ON_GROUND,
                pm_time: 0,
                gravity: 800,
                delta_angles: [0; 3],
            },
            cmd: UserCmd::default(),
            snap_initial: false,
            viewangles: Vec3f::ZERO,
            viewheight: 0.0,
            mins: Vec3f::new(-16.0, -16.0, -24.0),
            maxs: Vec3f::new(16.0, 16.0, 32.0),
            ground_entity: true,
            watertype: 0,
            waterlevel: 0,
            trace: Box::new(|_start, _mins, _maxs, end| {
                Trace {
                    allsolid: false,
                    startsolid: false,
                    fraction: 1.0,
                    endpos: end,
                    plane: Plane::default(),
                    surface: None,
                    contents: 0,
                    ent_index: None,
                }
            }),
            pointcontents: Box::new(|_| 0),
        }
    }

    #[test]
    fn fixed_point_roundtrip() {
        // Convert float -> fixed -> float; verify precision within 0.125.
        let values = [0.0f32, 1.0, -1.0, 123.456, -99.9, 0.125, 0.0625];
        for &v in &values {
            let fixed = (v * 8.0) as i16;
            let back = fixed as f32 * 0.125;
            assert!(
                (back - v).abs() <= 0.125,
                "roundtrip failed for {v}: got {back}"
            );
        }
    }

    #[test]
    fn clip_velocity_floor() {
        // Clip velocity against a floor normal (0, 0, 1).
        let in_vel = Vec3f::new(100.0, 200.0, -300.0);
        let normal = Vec3f::new(0.0, 0.0, 1.0);
        let out = pm_clip_velocity(in_vel, normal, 1.01);
        // Horizontal components should be preserved.
        assert!((out.x - 100.0).abs() < 0.01);
        assert!((out.y - 200.0).abs() < 0.01);
        // Vertical should be near zero (was going into the floor).
        assert!(out.z.abs() < 5.0, "z should be clipped, got {}", out.z);
    }

    #[test]
    fn clip_velocity_wall() {
        // Clip against a wall facing +X.
        let in_vel = Vec3f::new(-200.0, 50.0, 0.0);
        let normal = Vec3f::new(1.0, 0.0, 0.0);
        let out = pm_clip_velocity(in_vel, normal, 1.01);
        // X component (into the wall) should be zeroed or positive.
        assert!(out.x >= 0.0, "x should be clipped, got {}", out.x);
        // Y and Z should be preserved.
        assert!((out.y - 50.0).abs() < 0.01);
        assert!(out.z.abs() < 0.01);
    }

    #[test]
    fn friction_reduces_speed() {
        let mut pm = make_test_pmove();
        let mut pml = PmLocal {
            velocity: Vec3f::new(200.0, 0.0, 0.0),
            frametime: 0.016,
            ..PmLocal::default()
        };
        pm.ground_entity = true;

        let speed_before = pml.velocity.length();
        pm_friction(&mut pm, &mut pml);
        let speed_after = pml.velocity.length();

        assert!(
            speed_after < speed_before,
            "friction should reduce speed: {speed_before} -> {speed_after}"
        );
    }

    #[test]
    fn acceleration_adds_speed() {
        let mut pml = PmLocal {
            velocity: Vec3f::ZERO,
            frametime: 0.016,
            ..PmLocal::default()
        };

        let wishdir = Vec3f::new(1.0, 0.0, 0.0);
        pm_accelerate(&mut pml, wishdir, 300.0, PM_ACCELERATE);

        assert!(
            pml.velocity.length() > 0.0,
            "acceleration should add speed"
        );
        assert!(pml.velocity.x > 0.0, "should accelerate in +X");
    }

    #[test]
    fn pmf_flags_constants() {
        // Verify flag values match the Q2 protocol.
        assert_eq!(PMF_DUCKED, 1);
        assert_eq!(PMF_JUMP_HELD, 2);
        assert_eq!(PMF_ON_GROUND, 4);
        assert_eq!(PMF_TIME_WATERJUMP, 8);
        assert_eq!(PMF_TIME_LAND, 16);
        assert_eq!(PMF_TIME_TELEPORT, 32);
        assert_eq!(PMF_NO_PREDICTION, 64);

        // All flags should be distinct single bits.
        let all = PMF_DUCKED
            | PMF_JUMP_HELD
            | PMF_ON_GROUND
            | PMF_TIME_WATERJUMP
            | PMF_TIME_LAND
            | PMF_TIME_TELEPORT
            | PMF_NO_PREDICTION;
        assert_eq!(all, 127);
    }

    #[test]
    fn snap_position_basic() {
        let mut pm = make_test_pmove();
        let pml = PmLocal {
            origin: Vec3f::new(10.0, 20.0, 0.0),
            velocity: Vec3f::new(100.0, 0.0, 0.0),
            ..PmLocal::default()
        };

        pm_snap_position(&mut pm, &pml);

        // Verify origin snapped to 1/8 grid.
        assert_eq!(pm.s.origin[0], 80);  // 10.0 * 8
        assert_eq!(pm.s.origin[1], 160); // 20.0 * 8
        assert_eq!(pm.s.origin[2], 0);
        assert_eq!(pm.s.velocity[0], 800); // 100.0 * 8
    }

    #[test]
    fn dead_move_clears_input() {
        let mut pm = make_test_pmove();
        pm.s.pm_type = PmType::Dead;
        pm.cmd.forwardmove = 400;
        pm.cmd.sidemove = 200;
        pm.cmd.upmove = 100;
        pm.cmd.msec = 16;

        // Provide a ground trace so categorize_position finds ground.
        pm.trace = Box::new(|start, _mins, _maxs, _end| {
            Trace {
                allsolid: false,
                startsolid: false,
                fraction: 0.0,
                endpos: start,
                plane: Plane {
                    normal: Vec3f::new(0.0, 0.0, 1.0),
                    dist: 0.0,
                    plane_type: 0,
                    sign_bits: 0,
                },
                surface: None,
                contents: 0,
                ent_index: Some(0), // world entity
            }
        });

        pm.run();

        // After running, the dead player's movement inputs should have been zeroed.
        assert_eq!(pm.cmd.forwardmove, 0);
        assert_eq!(pm.cmd.sidemove, 0);
        assert_eq!(pm.cmd.upmove, 0);
    }
}
