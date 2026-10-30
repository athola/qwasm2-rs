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
/// A C `double` constant: `normal.z` is compared after widening to f64, so a
/// stored 0.7f32 (0.69999998) counts as too steep.
pub const MIN_STEP_NORMAL: f64 = 0.7;
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

// ---------------------------------------------------------------------------
// Content / surface flags needed by pmove (matches Quake 2 defines)
// ---------------------------------------------------------------------------

use q2_shared::constants::{
    CONTENTS_CURRENT_0, CONTENTS_CURRENT_180, CONTENTS_CURRENT_270, CONTENTS_CURRENT_90,
    CONTENTS_CURRENT_DOWN, CONTENTS_CURRENT_UP, CONTENTS_LADDER, CONTENTS_SLIME, CONTENTS_SOLID,
    CONTENTS_WATER, MASK_CURRENT, MASK_WATER,
};
const SURF_SLICK: i32 = 0x02;

// Angle indices
const PITCH: usize = 0;
const YAW: usize = 1;
const ROLL: usize = 2;

// ---------------------------------------------------------------------------
// Per-frame working data (zeroed each frame)
// ---------------------------------------------------------------------------

struct PmLocal {
    origin: Vec3f,
    velocity: Vec3f,
    forward: Vec3f,
    right: Vec3f,
    frametime: f32,
    ground_surface: Option<Surface>,
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
            frametime: 0.0,
            ground_surface: None,
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

/// C `MAXTOUCH`: entities one pmove can report touching.
pub const MAXTOUCH: usize = 32;

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
    /// Entity number stood on (C `groundentity`; the world is 0).
    pub ground_entity: Option<usize>,
    /// Entities the move ran into, up to `MAXTOUCH` (C `touchents`).
    pub touchents: Vec<usize>,
    /// Water content flags at player position.
    pub watertype: i32,
    /// 0 = not in water, 1..3 = depth.
    pub waterlevel: i32,
    /// Air acceleration (C `pm_airaccelerate`). Set by the server from
    /// `sv_airaccelerate` and by the client from `CS_AIRACCEL`; 0 disables
    /// air control as in stock Quake 2.
    pub airaccelerate: f32,
    /// Trace callback supplied by caller. Like C `SV_Trace`, it must set
    /// `ent_index` on a hit (the world is `Some(0)`): a trace with no entity
    /// never counts as ground. `CollisionMap::world_trace` does this for the
    /// world; `box_trace` leaves it `None`.
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
/// Matches `AngleVectors(angles, forward, right, up)` in the C engine,
/// including its precision: the radian conversion is a double multiply
/// stored to float, and sin/cos run in double before rounding to float.
pub fn angle_vectors(angles: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
    let sin_cos = |degrees: f32| {
        let radians = (f64::from(degrees) * (std::f64::consts::PI * 2.0 / 360.0)) as f32;
        let radians = f64::from(radians);
        (radians.sin() as f32, radians.cos() as f32)
    };
    let (sp, cp) = sin_cos(angles.x);
    let (sy, cy) = sin_cos(angles.y);
    let (sr, cr) = sin_cos(angles.z);

    let forward = Vec3f::new(cp * cy, cp * sy, -sp);
    let right = Vec3f::new(
        -(sr * sp * cy) + (cr * sy),
        -(sr * sp * sy) + -(cr * cy),
        -sr * cp,
    );
    let up = Vec3f::new(cr * sp * cy + -sr * -sy, cr * sp * sy + -sr * cy, cr * cp);
    (forward, right, up)
}

/// C `VectorNormalize`: scale by the reciprocal of the length (not divide,
/// which rounds differently) and return the length. Zero vectors are left
/// unchanged.
fn vector_normalize(v: &mut Vec3f) -> f32 {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if length != 0.0 {
        let ilength = 1.0 / length;
        v.x *= ilength;
        v.y *= ilength;
        v.z *= ilength;
    }
    length
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

        // Save the entity for contact.
        if let Some(n) = trace.ent_index {
            if pm.touchents.len() < MAXTOUCH {
                pm.touchents.push(n);
            }
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

    if down_dist > up_dist || f64::from(trace.plane.normal.z) < MIN_STEP_NORMAL {
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
    let on_ground_non_slick = pm.ground_entity.is_some()
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
        if pm.waterlevel == 1 && pm.ground_entity.is_some() {
            s /= 2.0;
        }
        *wishvel += v * s;
    }

    // Conveyor belts (ground currents)
    if pm.ground_entity.is_some() {
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
    let mut wishspeed = vector_normalize(&mut wishdir);

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
    let mut wishspeed = vector_normalize(&mut wishdir);

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
    } else if pm.ground_entity.is_some() {
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
        // In the air. C passes pm_accelerate, not pm_airaccelerate, as the
        // acceleration; the airaccelerate value only enables this path.
        if pm.airaccelerate != 0.0 {
            pm_air_accelerate(pml, wishdir, wishspeed, PM_ACCELERATE);
        } else {
            pm_accelerate(pml, wishdir, wishspeed, 1.0);
        }

        // Add gravity.
        pml.velocity.z -= pm.s.gravity as f32 * pml.frametime;
        pm_step_slide_move(pm, pml);
    }
}

/// C `PM_FlyMove(false)`: the only caller (spectators) never clips, so the
/// C `doclip` trace branch is not ported.
fn pm_fly_move(pm: &mut Pmove, pml: &mut PmLocal) {
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

    vector_normalize(&mut pml.forward);
    vector_normalize(&mut pml.right);

    let mut wishvel = pml.forward * fmove + pml.right * smove;
    wishvel.z += pm.cmd.upmove as f32;

    let mut wishdir = wishvel;
    let mut wishspeed = vector_normalize(&mut wishdir);

    if wishspeed > PM_MAXSPEED {
        wishvel *= PM_MAXSPEED / wishspeed;
        wishspeed = PM_MAXSPEED;
    }

    let currentspeed = pml.velocity.dot(wishdir);
    let addspeed = wishspeed - currentspeed;
    if addspeed <= 0.0 {
        // C PM_FlyMove returns here without moving this frame.
        return;
    }

    let mut accelspeed = PM_ACCELERATE * pml.frametime * wishspeed;
    if accelspeed > addspeed {
        accelspeed = addspeed;
    }

    pml.velocity += wishdir * accelspeed;
    pml.origin += pml.velocity * pml.frametime;
}

// ---------------------------------------------------------------------------
// Position categorization
// ---------------------------------------------------------------------------

fn pm_categorize_position(pm: &mut Pmove, pml: &mut PmLocal) {
    // Check if standing on something solid.
    let point = Vec3f::new(pml.origin.x, pml.origin.y, pml.origin.z - 0.25);

    if pml.velocity.z > 180.0 {
        pm.s.pm_flags &= !PMF_ON_GROUND;
        pm.ground_entity = None;
    } else {
        let trace = (pm.trace)(pml.origin, pm.mins, pm.maxs, point);
        pml.ground_surface = trace.surface;
        pml.ground_contents = trace.contents;

        if trace.ent_index.is_none() || (f64::from(trace.plane.normal.z) < 0.7 && !trace.startsolid)
        {
            pm.ground_entity = None;
            pm.s.pm_flags &= !PMF_ON_GROUND;
        } else {
            pm.ground_entity = trace.ent_index;

            // Hitting solid ground ends a waterjump.
            if (pm.s.pm_flags & PMF_TIME_WATERJUMP) != 0 {
                pm.s.pm_flags &= !(PMF_TIME_WATERJUMP | PMF_TIME_LAND | PMF_TIME_TELEPORT);
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

        if let Some(n) = trace.ent_index {
            if pm.touchents.len() < MAXTOUCH {
                pm.touchents.push(n);
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

    if pm.s.pm_type == PmType::Dead || (pm.cmd.upmove < 0 && (pm.s.pm_flags & PMF_ON_GROUND) != 0) {
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
    if pm.ground_entity.is_none() {
        return;
    }

    let forward = pml.velocity.length() - 20.0;

    if forward <= 0.0 {
        pml.velocity = Vec3f::ZERO;
    } else {
        // C VectorNormalize + VectorScale; vector_normalize keeps C's
        // multiply-by-reciprocal rounding.
        vector_normalize(&mut pml.velocity);
        pml.velocity *= forward;
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
    vector_normalize(&mut flatforward);

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
        pm.ground_entity = None;

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

    if pm.ground_entity.is_none() {
        return;
    }

    pm.s.pm_flags |= PMF_JUMP_HELD;
    pm.ground_entity = None;
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

    // Snap velocity to eighths. C stores `(int)(x * 8)` into a short, which
    // keeps the low 16 bits; a direct `as i16` would saturate instead.
    for (dst, src) in pm.s.velocity.iter_mut().zip(pml.velocity.as_ref().iter()) {
        *dst = (*src * 8.0) as i32 as i16;
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

        pm.s.origin[i] = (*orig * 8.0) as i32 as i16;

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
                pm.s.origin[i] = pm.s.origin[i].wrapping_add(s);
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
        pm.s.origin[2] = base[2].wrapping_add(oz);
        for &oy in &OFFSET {
            pm.s.origin[1] = base[1].wrapping_add(oy);
            for &ox in &OFFSET {
                pm.s.origin[0] = base[0].wrapping_add(ox);

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

    // No valid position: like C, pm.s.origin keeps the last candidate
    // (base + 1 on every axis) and pml.origin is left as it was.
    tracing::debug!("Bad InitialSnapPosition");
}

// ---------------------------------------------------------------------------
// Angle clamping
// ---------------------------------------------------------------------------

fn pm_clamp_angles(pm: &mut Pmove, pml: &mut PmLocal) {
    if (pm.s.pm_flags & PMF_TIME_TELEPORT) != 0 {
        // C sums in int here (no short temp), so the angle is not wrapped.
        let yaw = i32::from(pm.cmd.angles[YAW]) + i32::from(pm.s.delta_angles[YAW]);
        pm.viewangles[YAW] = yaw as f32 * (360.0 / 65536.0);
        pm.viewangles[PITCH] = 0.0;
        pm.viewangles[ROLL] = 0.0;
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

    // C also stores `up` in pml, but nothing reads it.
    let (fwd, rgt, _up) = angle_vectors(pm.viewangles);
    pml.forward = fwd;
    pml.right = rgt;
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
        self.ground_entity = None;
        self.touchents.clear();
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
            pm_fly_move(self, &mut pml);
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
                self.s.pm_flags &= !(PMF_TIME_WATERJUMP | PMF_TIME_LAND | PMF_TIME_TELEPORT);
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
                self.s.pm_flags &= !(PMF_TIME_WATERJUMP | PMF_TIME_LAND | PMF_TIME_TELEPORT);
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

                let (fwd, rgt, _up) = angle_vectors(angles);
                pml.forward = fwd;
                pml.right = rgt;

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
            ground_entity: Some(0),
            touchents: Vec::new(),
            watertype: 0,
            waterlevel: 0,
            airaccelerate: 0.0,
            trace: Box::new(|_start, _mins, _maxs, end| Trace {
                allsolid: false,
                startsolid: false,
                fraction: 1.0,
                endpos: end,
                plane: Plane::default(),
                surface: None,
                contents: 0,
                ent_index: None,
            }),
            pointcontents: Box::new(|_| 0),
        }
    }

    #[test]
    fn clip_velocity_floor() {
        // C PM_ClipVelocity: backoff = -300 * 1.01 = -303, z = -300 + 303.
        let in_vel = Vec3f::new(100.0, 200.0, -300.0);
        let out = pm_clip_velocity(in_vel, Vec3f::Z, 1.01);
        assert_eq!(out, Vec3f::new(100.0, 200.0, 3.0));
    }

    #[test]
    fn clip_velocity_wall() {
        // backoff = -200 * 1.01 = -202, x = -200 + 202; |z| < STOP_EPSILON -> 0.
        let in_vel = Vec3f::new(-200.0, 50.0, 0.05);
        let out = pm_clip_velocity(in_vel, Vec3f::X, 1.01);
        assert_eq!(out, Vec3f::new(2.0, 50.0, 0.0));
    }

    #[test]
    fn friction_reduces_speed() {
        let mut pm = make_test_pmove();
        let mut pml = PmLocal {
            velocity: Vec3f::new(200.0, 0.0, 0.0),
            frametime: 0.016,
            ..PmLocal::default()
        };
        pm.ground_entity = Some(0);

        pm_friction(&mut pm, &mut pml);

        // C PM_Friction on ground: control = speed (>= stopspeed),
        // drop = control * pm_friction * frametime, scale = (speed - drop) / speed.
        let speed = 200.0f32;
        let scale = (speed - speed * PM_FRICTION * 0.016) / speed;
        assert_eq!(pml.velocity, Vec3f::new(200.0 * scale, 0.0, 0.0));
        assert!((pml.velocity.x - 180.8).abs() < 1e-3);
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

        // C PM_Accelerate: accelspeed = accel * frametime * wishspeed (48),
        // below addspeed (300).
        let accelspeed = PM_ACCELERATE * 0.016 * 300.0;
        assert_eq!(pml.velocity, Vec3f::new(accelspeed, 0.0, 0.0));
        assert!((accelspeed - 48.0).abs() < 1e-4);
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
        assert_eq!(pm.s.origin[0], 80); // 10.0 * 8
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
        // PM_CheckDuck always ducks a dead player.
        assert_ne!(pm.s.pm_flags & PMF_DUCKED, 0);
        assert_eq!(pm.maxs.z, 4.0);
    }

    // -----------------------------------------------------------------------
    // Branch coverage, values from pmove.c
    // -----------------------------------------------------------------------

    fn pml_facing_x(frametime: f32) -> PmLocal {
        PmLocal {
            forward: Vec3f::X,
            right: Vec3f::NEG_Y,
            frametime,
            ..PmLocal::default()
        }
    }

    /// Open-air trace that records nothing; panics if a test path must not trace.
    fn no_trace() -> TraceFn {
        Box::new(|_, _, _, _| panic!("unexpected trace"))
    }

    #[test]
    fn water_move_without_input_drifts_down_at_half_of_60() {
        let mut pm = make_test_pmove();
        let mut pml = pml_facing_x(0.1);
        pm_water_move(&mut pm, &mut pml);
        // wishvel z = -60, wishspeed 60 * 0.5 = 30; accelspeed 10*0.1*30 = 30.
        assert_eq!(pml.velocity, Vec3f::new(0.0, 0.0, -30.0));
    }

    #[test]
    fn water_move_with_input_caps_wishspeed_at_maxspeed_then_halves_it() {
        let mut pm = make_test_pmove();
        pm.cmd.forwardmove = 400;
        let mut pml = pml_facing_x(0.1);
        pm_water_move(&mut pm, &mut pml);
        // 400 -> PM_MAXSPEED 300 -> * 0.5 = 150; accelspeed 10*0.1*150 = 150.
        assert_eq!(pml.velocity, Vec3f::new(150.0, 0.0, 0.0));
    }

    #[test]
    fn water_currents_push_at_waterspeed_halved_when_wading_on_ground() {
        let mut pm = make_test_pmove();
        pm.watertype = CONTENTS_WATER | CONTENTS_CURRENT_0 | CONTENTS_CURRENT_UP;
        pm.waterlevel = 1;
        let pml = PmLocal::default();
        let mut wishvel = Vec3f::ZERO;
        pm_add_currents(&pm, &pml, &mut wishvel);
        assert_eq!(wishvel, Vec3f::new(200.0, 0.0, 200.0));

        pm.waterlevel = 2;
        pm.watertype = CONTENTS_CURRENT_180 | CONTENTS_CURRENT_270 | CONTENTS_CURRENT_DOWN;
        let mut wishvel = Vec3f::ZERO;
        pm_add_currents(&pm, &pml, &mut wishvel);
        assert_eq!(wishvel, Vec3f::new(-400.0, -400.0, -400.0));
    }

    #[test]
    fn conveyor_ground_contents_push_at_100_only_on_ground() {
        let mut pm = make_test_pmove();
        let pml = PmLocal {
            ground_contents: CONTENTS_CURRENT_90 | CONTENTS_CURRENT_DOWN,
            ..PmLocal::default()
        };
        let mut wishvel = Vec3f::ZERO;
        pm_add_currents(&pm, &pml, &mut wishvel);
        assert_eq!(wishvel, Vec3f::new(0.0, 100.0, -100.0));

        pm.ground_entity = None;
        let mut wishvel = Vec3f::ZERO;
        pm_add_currents(&pm, &pml, &mut wishvel);
        assert_eq!(wishvel, Vec3f::ZERO);
    }

    #[test]
    fn ladder_sets_climb_speed_from_pitch_or_upmove_and_clamps_sideways() {
        let cases = [
            (-20.0, 400, 0, 200.0),
            (20.0, 400, 0, -200.0),
            (0.0, 0, 10, 200.0),
            (0.0, 0, -10, -200.0),
            (0.0, 400, 0, 0.0),
        ];
        for (pitch, forward, up, climb) in cases {
            let mut pm = make_test_pmove();
            pm.viewangles.x = pitch;
            pm.cmd.forwardmove = forward;
            pm.cmd.upmove = up;
            let pml = PmLocal {
                ladder: true,
                ..PmLocal::default()
            };
            let mut wishvel = Vec3f::new(300.0, -300.0, 0.0);
            pm_add_currents(&pm, &pml, &mut wishvel);
            assert_eq!(
                wishvel,
                Vec3f::new(25.0, -25.0, climb),
                "pitch {pitch} up {up}"
            );
        }
        // Moving faster than 200 vertically: the ladder leaves wishvel alone.
        let pm = make_test_pmove();
        let pml = PmLocal {
            ladder: true,
            velocity: Vec3f::new(0.0, 0.0, 250.0),
            ..PmLocal::default()
        };
        let mut wishvel = Vec3f::new(300.0, 0.0, 0.0);
        pm_add_currents(&pm, &pml, &mut wishvel);
        assert_eq!(wishvel, Vec3f::new(300.0, 0.0, 0.0));
    }

    #[test]
    fn special_movement_finds_a_ladder_in_front() {
        let mut pm = make_test_pmove();
        pm.trace = Box::new(|start, _, _, end| Trace {
            fraction: 0.5,
            endpos: start + (end - start) * 0.5,
            contents: CONTENTS_LADDER,
            ..Trace::default()
        });
        let mut pml = pml_facing_x(0.1);
        pm_check_special_movement(&mut pm, &mut pml);
        assert!(pml.ladder);

        // pm_time set: C returns before touching pml.ladder.
        pm.s.pm_time = 5;
        pm.trace = no_trace();
        pm_check_special_movement(&mut pm, &mut pml);
        assert!(pml.ladder);
    }

    #[test]
    fn special_movement_waterjumps_onto_a_ledge_with_room_above() {
        let mut pm = make_test_pmove();
        pm.waterlevel = 2;
        // Solid 30 units ahead at z+4, empty at z+20.
        pm.pointcontents = Box::new(|p| if p.z < 10.0 { CONTENTS_SOLID } else { 0 });
        let mut pml = pml_facing_x(0.1);
        pm_check_special_movement(&mut pm, &mut pml);
        assert_eq!(pml.velocity, Vec3f::new(50.0, 0.0, 350.0));
        assert_ne!(pm.s.pm_flags & PMF_TIME_WATERJUMP, 0);
        assert_eq!(pm.s.pm_time, 255);

        // Blocked above the ledge: no jump.
        let mut pm = make_test_pmove();
        pm.waterlevel = 2;
        pm.pointcontents = Box::new(|_| CONTENTS_SOLID);
        let mut pml = pml_facing_x(0.1);
        pm_check_special_movement(&mut pm, &mut pml);
        assert_eq!(pml.velocity, Vec3f::ZERO);
        assert_eq!(pm.s.pm_flags & PMF_TIME_WATERJUMP, 0);
    }

    #[test]
    fn waterjump_frame_falls_with_gravity_and_ends_once_moving_down() {
        for (vz_fixed, ends) in [(8i16, true), (3200, false)] {
            let mut pm = make_test_pmove();
            pm.ground_entity = None;
            pm.s.pm_flags = PMF_TIME_WATERJUMP;
            pm.s.pm_time = 255;
            pm.s.velocity = [0, 0, vz_fixed];
            pm.cmd.msec = 16;
            pm.cmd.forwardmove = 400; // ignored: no control during a waterjump
            pm.run();
            // pm_time drops by msec >> 3 = 2 unless the jump ended.
            assert_eq!(pm.s.pm_flags & PMF_TIME_WATERJUMP == 0, ends);
            assert_eq!(pm.s.pm_time, if ends { 0 } else { 253 });
            // vz - gravity * frametime, snapped to eighths.
            let vz = f32::from(vz_fixed) * 0.125 - 800.0 * 0.016;
            assert_eq!(pm.s.velocity, [0, 0, (vz * 8.0) as i16]);
        }
    }

    #[test]
    fn dead_move_slides_with_20_units_of_extra_friction() {
        let pm = make_test_pmove();
        let mut pml = PmLocal {
            velocity: Vec3f::new(30.0, 40.0, 0.0),
            ..PmLocal::default()
        };
        pm_dead_move(&pm, &mut pml);
        // C: VectorNormalize (multiply by 1/len), then scale by len - 20.
        let ilength = 1.0f32 / 50.0;
        assert_eq!(
            pml.velocity,
            Vec3f::new(30.0 * ilength * 30.0, 40.0 * ilength * 30.0, 0.0)
        );

        // In the air nothing changes.
        let mut pm = make_test_pmove();
        pm.ground_entity = None;
        let mut pml = PmLocal {
            velocity: Vec3f::new(30.0, 40.0, 0.0),
            ..PmLocal::default()
        };
        pm_dead_move(&pm, &mut pml);
        assert_eq!(pml.velocity, Vec3f::new(30.0, 40.0, 0.0));
    }

    #[test]
    fn gib_uses_the_small_hull_and_low_view() {
        let mut pm = make_test_pmove();
        pm.s.pm_type = PmType::Gib;
        pm.trace = no_trace();
        pm_check_duck(&mut pm, &PmLocal::default());
        assert_eq!((pm.mins.z, pm.maxs.z, pm.viewheight), (0.0, 16.0, 8.0));
        assert_eq!((pm.mins.x, pm.maxs.y), (-16.0, 16.0));
    }

    #[test]
    fn freeze_returns_before_any_movement_or_trace() {
        let mut pm = make_test_pmove();
        pm.s.pm_type = PmType::Freeze;
        pm.s.origin = [80, 160, 240];
        pm.s.velocity = [800, 0, 0];
        pm.cmd.forwardmove = 400;
        pm.cmd.msec = 16;
        pm.trace = no_trace();
        pm.run();
        assert_eq!(pm.s.origin, [80, 160, 240]);
        assert_eq!(pm.s.velocity, [800, 0, 0]);
        assert_eq!(pm.cmd.forwardmove, 0);
    }

    #[test]
    fn initial_snap_failure_leaves_the_last_candidate_like_c() {
        let mut pm = make_test_pmove();
        pm.s.origin = [80, 160, 240];
        pm.trace = Box::new(|_, _, _, end| Trace {
            allsolid: true,
            startsolid: true,
            fraction: 0.0,
            endpos: end,
            ..Trace::default()
        });
        let mut pml = PmLocal {
            origin: Vec3f::new(1.0, 2.0, 3.0),
            ..PmLocal::default()
        };
        pm_initial_snap_position(&mut pm, &mut pml);
        // All 27 candidates fail; the loop's last write was offsets (1, 1, 1).
        assert_eq!(pm.s.origin, [81, 161, 241]);
        assert_eq!(pml.origin, Vec3f::new(1.0, 2.0, 3.0));
    }

    // -----------------------------------------------------------------------
    // C parity: float paths
    // -----------------------------------------------------------------------

    /// C AngleVectors (shared.c): `deg * (M_PI*2/360)` in double stored to a
    /// float, then `(float)sin((double)angle)`; products in float.
    fn c_angle_vectors(angles: Vec3f) -> (Vec3f, Vec3f, Vec3f) {
        let trig = |deg: f32| {
            let angle = (f64::from(deg) * (std::f64::consts::PI * 2.0 / 360.0)) as f32;
            (f64::from(angle).sin() as f32, f64::from(angle).cos() as f32)
        };
        let (sy, cy) = trig(angles.y);
        let (sp, cp) = trig(angles.x);
        let (sr, cr) = trig(angles.z);
        (
            Vec3f::new(cp * cy, cp * sy, -sp),
            Vec3f::new(
                -sr * sp * cy + -cr * -sy,
                -sr * sp * sy + -cr * cy,
                -sr * cp,
            ),
            Vec3f::new(cr * sp * cy + -sr * -sy, cr * sp * sy + -sr * cy, cr * cp),
        )
    }

    fn bits(v: Vec3f) -> [u32; 3] {
        [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
    }

    #[test]
    fn angle_vectors_match_c_double_path_for_all_short_angles() {
        let mut mismatches = 0;
        for s in i16::MIN..=i16::MAX {
            let deg = short2angle(s);
            // Full view angles, and the pitch/3 path used by Pmove::run.
            for angles in [Vec3f::new(deg, deg, deg), Vec3f::new(deg / 3.0, deg, 0.0)] {
                let (f, r, u) = angle_vectors(angles);
                let (cf, cr, cu) = c_angle_vectors(angles);
                if bits(f) != bits(cf) || bits(r) != bits(cr) || bits(u) != bits(cu) {
                    mismatches += 1;
                }
            }
        }
        assert_eq!(mismatches, 0);
    }

    #[test]
    fn vector_normalize_multiplies_by_reciprocal_like_c() {
        let mut v = Vec3f::new(3.0, 7.0, 0.0);
        let len = vector_normalize(&mut v);
        let expected_len = (3.0f32 * 3.0 + 7.0 * 7.0 + 0.0 * 0.0).sqrt();
        let il = 1.0 / expected_len;
        assert_eq!(len.to_bits(), expected_len.to_bits());
        assert_eq!(bits(v), bits(Vec3f::new(3.0 * il, 7.0 * il, 0.0)));

        let mut zero = Vec3f::ZERO;
        assert_eq!(vector_normalize(&mut zero), 0.0);
        assert_eq!(zero, Vec3f::ZERO);
    }

    #[test]
    fn ground_accelerate_wishdir_uses_reciprocal_normalize() {
        // Find a move where `v / len` and `v * (1/len)` round differently;
        // C VectorNormalize multiplies by the reciprocal.
        let frametime = 0.016f32;
        let expected_velocity = |f: f32, s: f32| {
            let len = (f * f + s * s).sqrt();
            let il = 1.0 / len;
            let accelspeed = PM_ACCELERATE * frametime * len.min(PM_MAXSPEED);
            Vec3f::new(f * il * accelspeed, s * il * accelspeed, 0.0)
        };
        let divided_velocity = |f: f32, s: f32| {
            let len = (f * f + s * s).sqrt();
            let accelspeed = PM_ACCELERATE * frametime * len.min(PM_MAXSPEED);
            Vec3f::new(f / len * accelspeed, s / len * accelspeed, 0.0)
        };
        let (fmove, smove) = (1..200i16)
            .flat_map(|f| (1..200i16).map(move |s| (f, s)))
            .find(|&(f, s)| {
                let (f, s) = (f32::from(f), -f32::from(s));
                bits(expected_velocity(f, s)) != bits(divided_velocity(f, s))
            })
            .expect("a divergent move exists");

        let mut pm = make_test_pmove();
        pm.cmd.forwardmove = fmove;
        pm.cmd.sidemove = smove;
        let mut pml = PmLocal {
            forward: Vec3f::new(1.0, 0.0, 0.0),
            right: Vec3f::new(0.0, -1.0, 0.0),
            frametime,
            ..PmLocal::default()
        };
        pm_air_move(&mut pm, &mut pml);
        let expected = expected_velocity(f32::from(fmove), -f32::from(smove));
        assert_eq!(
            bits(pml.velocity),
            bits(expected),
            "move ({fmove}, {smove})"
        );
    }

    /// Ground trace that always hits a plane with the given normal z.
    fn ground_trace(normal_z: f32) -> TraceFn {
        let normal_y = (1.0 - normal_z * normal_z).sqrt();
        Box::new(move |start, _mins, _maxs, _end| Trace {
            allsolid: false,
            startsolid: false,
            fraction: 0.0,
            endpos: start,
            plane: Plane {
                normal: Vec3f::new(0.0, normal_y, normal_z),
                dist: 0.0,
                plane_type: 3,
                sign_bits: 0,
            },
            surface: None,
            contents: CONTENTS_SOLID,
            ent_index: Some(0),
        })
    }

    #[test]
    fn plane_normal_z_exactly_0_7f_is_not_ground() {
        // C compares `normal[2] < 0.7` in double: 0.7f32 (0.69999998) < 0.7.
        let mut pm = make_test_pmove();
        pm.trace = ground_trace(0.7);
        let mut pml = PmLocal::default();
        pm_categorize_position(&mut pm, &mut pml);
        assert!(pm.ground_entity.is_none());
        assert_eq!(pm.s.pm_flags & PMF_ON_GROUND, 0);

        pm.trace = ground_trace(0.7000001);
        pm_categorize_position(&mut pm, &mut pml);
        assert!(pm.ground_entity.is_some());
    }

    #[test]
    fn step_up_onto_plane_with_normal_z_0_7f_is_rejected() {
        // Blocked at floor level, free once raised; the push-down lands on a
        // plane whose normal z is 0.7f32, which C treats as too steep.
        let mut pm = make_test_pmove();
        pm.trace = Box::new(|start, _mins, _maxs, end| {
            let mut t = Trace {
                fraction: 1.0,
                endpos: end,
                ent_index: Some(0),
                ..Trace::default()
            };
            if start == end {
                return t;
            }
            if end.z < start.z {
                t.fraction = 0.5;
                t.endpos = start + (end - start) * 0.5;
                t.plane.normal = Vec3f::new(0.0, (1.0f32 - 0.49).sqrt(), 0.7);
            } else if start.z < 1.0 {
                t.fraction = 0.0;
                t.endpos = start;
                t.plane.normal = Vec3f::new(-1.0, 0.0, 0.0);
            }
            t
        });
        let mut pml = PmLocal {
            velocity: Vec3f::new(100.0, 0.0, 0.0),
            frametime: 0.1,
            ..PmLocal::default()
        };
        pm_step_slide_move(&mut pm, &mut pml);
        assert_eq!(pml.origin, Vec3f::ZERO);
    }

    // -----------------------------------------------------------------------
    // C parity: control flow and integer wrapping
    // -----------------------------------------------------------------------

    #[test]
    fn spectator_without_input_does_not_coast() {
        // C PM_FlyMove returns before moving when addspeed <= 0.
        let mut pm = make_test_pmove();
        pm.s.pm_type = PmType::Spectator;
        pm.s.velocity = [200 * 8, 0, 0];
        pm.cmd.msec = 16;
        pm.run();
        assert_eq!(pm.s.origin, [0, 0, 0]);
        // Friction only: 200 - 200 * 9 * 0.016 = 171.2 u/s.
        assert_eq!(pm.s.velocity[0], (171.2f32 * 8.0) as i16);
    }

    #[test]
    fn snap_velocity_and_origin_wrap_like_c_short() {
        let mut pm = make_test_pmove();
        let pml = PmLocal {
            origin: Vec3f::new(4096.0, 0.0, 0.0),
            velocity: Vec3f::new(0.0, 0.0, -5000.0),
            ..PmLocal::default()
        };
        pm_snap_position(&mut pm, &pml);
        assert_eq!(pm.s.velocity[2], (-40000i32) as i16);
        assert_eq!(pm.s.origin[0], i16::MIN);
    }

    #[test]
    fn snap_jitter_at_i16_max_wraps_instead_of_panicking() {
        let mut pm = make_test_pmove();
        // Only the wrapped position is free, so the +1 jitter must be tried.
        pm.trace = Box::new(|start, _mins, _maxs, end| Trace {
            allsolid: start.x != -4096.0,
            endpos: end,
            ..Trace::default()
        });
        let pml = PmLocal {
            origin: Vec3f::new(4095.9, 0.0, 0.0),
            ..PmLocal::default()
        };
        pm_snap_position(&mut pm, &pml);
        assert_eq!(pm.s.origin[0], i16::MIN);
    }

    #[test]
    fn initial_snap_at_i16_max_wraps_instead_of_panicking() {
        let mut pm = make_test_pmove();
        pm.trace = Box::new(|start, _mins, _maxs, end| Trace {
            allsolid: start.x != -4096.0,
            endpos: end,
            ..Trace::default()
        });
        pm.s.origin = [i16::MAX, 0, 0];
        let mut pml = PmLocal::default();
        pm_initial_snap_position(&mut pm, &mut pml);
        assert_eq!(pm.s.origin[0], i16::MIN);
        assert_eq!(pml.origin.x, -4096.0);
    }

    #[test]
    fn snap_falls_back_to_previous_origin_when_all_jitters_are_solid() {
        let mut pm = make_test_pmove();
        pm.trace = Box::new(|_start, _mins, _maxs, end| Trace {
            allsolid: true,
            endpos: end,
            ..Trace::default()
        });
        let pml = PmLocal {
            origin: Vec3f::new(10.06, 20.0, 30.0),
            previous_origin: [1, 2, 3],
            ..PmLocal::default()
        };
        pm_snap_position(&mut pm, &pml);
        assert_eq!(pm.s.origin, [1, 2, 3]);
    }

    #[test]
    fn teleport_yaw_uses_full_int_sum_like_c() {
        let mut pm = make_test_pmove();
        pm.s.pm_flags |= PMF_TIME_TELEPORT;
        pm.cmd.angles[YAW] = 30000;
        pm.s.delta_angles[YAW] = 30000;
        let mut pml = PmLocal::default();
        pm_clamp_angles(&mut pm, &mut pml);
        assert_eq!(pm.viewangles.y, 60000.0 * (360.0 / 65536.0));
        assert_eq!(pm.viewangles.x, 0.0);
    }

    #[test]
    fn clamp_angles_limits_downward_pitch_to_89() {
        let mut pm = make_test_pmove();
        let mut pml = PmLocal::default();
        pm.cmd.angles[PITCH] = (100.0f32 * 65536.0 / 360.0) as i16;
        pm_clamp_angles(&mut pm, &mut pml);
        assert_eq!(pm.viewangles.x, 89.0);
        // A short angle is never >= 180 degrees, so C leaves upward pitch
        // (negative) alone; the 271 branch is unreachable from a usercmd.
        pm.cmd.angles[PITCH] = -18204;
        pm_clamp_angles(&mut pm, &mut pml);
        assert_eq!(pm.viewangles.x, short2angle(-18204));
    }

    #[test]
    fn air_accelerate_uses_pm_accelerate_when_enabled() {
        // C PM_AirMove: `if (pm_airaccelerate) PM_AirAccelerate(wishdir,
        // wishspeed, pm_accelerate)`; wishspd is capped at 30.
        let mut pm = make_test_pmove();
        pm.ground_entity = None;
        pm.airaccelerate = 1.0;
        pm.cmd.forwardmove = 300;
        let mut pml = PmLocal {
            forward: Vec3f::new(1.0, 0.0, 0.0),
            right: Vec3f::new(0.0, -1.0, 0.0),
            frametime: 0.016,
            ..PmLocal::default()
        };
        pm_air_move(&mut pm, &mut pml);
        // min(addspeed 30, 10 * 300 * 0.016 = 48) = 30
        assert_eq!(pml.velocity.x, 30.0);
    }

    #[test]
    fn air_move_without_airaccelerate_uses_accel_1() {
        let mut pm = make_test_pmove();
        pm.ground_entity = None;
        pm.cmd.forwardmove = 300;
        let mut pml = PmLocal {
            forward: Vec3f::new(1.0, 0.0, 0.0),
            right: Vec3f::new(0.0, -1.0, 0.0),
            frametime: 0.016,
            ..PmLocal::default()
        };
        pm_air_move(&mut pm, &mut pml);
        assert_eq!(pml.velocity.x, 1.0 * 0.016 * 300.0);
        assert_eq!(pml.velocity.z, -800.0 * 0.016);
    }

    #[test]
    fn pm_time_counts_down_in_eighths_of_msec() {
        let mut pm = make_test_pmove();
        pm.trace = ground_trace(1.0);
        pm.s.pm_flags = PMF_ON_GROUND | PMF_TIME_LAND;
        pm.s.pm_time = 10;
        pm.cmd.msec = 16;
        pm.run();
        assert_eq!(pm.s.pm_time, 8);
        pm.cmd.msec = 4; // 4 >> 3 == 0, C still counts 1
        pm.run();
        assert_eq!(pm.s.pm_time, 7);
        pm.s.pm_time = 1;
        pm.run();
        assert_eq!(pm.s.pm_time, 0);
        assert_eq!(pm.s.pm_flags & PMF_TIME_LAND, 0);
    }

    #[test]
    fn landing_sets_land_time_18_or_25_by_fall_speed() {
        for (vz, expected) in [(-300.0, 18), (-500.0, 25)] {
            let mut pm = make_test_pmove();
            pm.s.pm_flags = 0;
            pm.trace = ground_trace(1.0);
            let mut pml = PmLocal {
                velocity: Vec3f::new(0.0, 0.0, vz),
                ..PmLocal::default()
            };
            pm_categorize_position(&mut pm, &mut pml);
            assert_ne!(pm.s.pm_flags & PMF_TIME_LAND, 0);
            assert_eq!(pm.s.pm_time, expected);
        }
    }

    // -----------------------------------------------------------------------
    // Pmove::run over real BSP collision
    // -----------------------------------------------------------------------

    use crate::collision::tests::build_floor_bsp;
    use crate::collision::CollisionMap;
    use std::cell::RefCell;
    use std::rc::Rc;

    use q2_shared::constants::MASK_PLAYERSOLID;

    /// Resting height on the floor BSP in 1/8 units. A hull exactly touching
    /// the floor (z = 24.0) counts as inside it for C CM_TestBoxInBrush, so
    /// PM_SnapPosition settles one eighth higher.
    const REST_Z: i16 = 24 * 8 + 1;

    /// Pmove on the floor BSP (solid below z=0). World hits report the world
    /// entity via `world_trace`, as C CL_PMTrace does. `step` adds a box
    /// entity, merged the way SV_ClipMoveToEntities merges entity traces.
    fn pmove_on_floor(origin_z: i16, step: Option<(Vec3f, Vec3f)>) -> Pmove {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_floor_bsp()).unwrap();
        let cm = Rc::new(RefCell::new(cm));
        let cm2 = cm.clone();
        let mut pm = make_test_pmove();
        pm.s.pm_flags = 0;
        pm.ground_entity = None;
        pm.s.origin = [0, 0, origin_z];
        pm.trace = Box::new(move |start, mins, maxs, end| {
            let mut cm = cm2.borrow_mut();
            let mut t = cm.world_trace(start, mins, maxs, end, MASK_PLAYERSOLID);
            if let Some((bmins, bmaxs)) = step {
                let h = cm.headnode_for_box(bmins, bmaxs);
                let tb = cm.box_trace(start, end, mins, maxs, h, MASK_PLAYERSOLID);
                if tb.allsolid || tb.fraction < t.fraction {
                    let world_startsolid = t.startsolid;
                    t = tb;
                    t.startsolid |= world_startsolid;
                    t.ent_index = Some(1); // the step entity
                } else if tb.startsolid {
                    t.startsolid = true;
                }
            }
            t
        });
        pm.pointcontents = Box::new(move |p| cm.borrow().point_contents(p, 0));
        pm
    }

    #[test]
    fn run_standing_on_floor_sets_on_ground_and_stays_put() {
        let mut pm = pmove_on_floor(REST_Z, None);
        pm.cmd.msec = 16;
        pm.run();
        assert!(pm.ground_entity.is_some());
        assert_ne!(pm.s.pm_flags & PMF_ON_GROUND, 0);
        assert_eq!(pm.s.origin, [0, 0, REST_Z]);
        assert_eq!(pm.viewheight, 22.0);
    }

    #[test]
    fn run_on_the_floor_reports_the_world_as_ground_entity_and_touch() {
        let mut pm = pmove_on_floor(REST_Z, None);
        pm.cmd.msec = 16;
        pm.run();
        assert_eq!(pm.ground_entity, Some(0));
        assert!(pm.touchents.contains(&0), "{:?}", pm.touchents);
    }

    #[test]
    fn run_standing_on_an_entity_reports_its_number_as_ground_entity() {
        let top = 16.0;
        let rest = ((top + 24.0) * 8.0) as i16 + 1;
        let step = (Vec3f::new(-64.0, -64.0, -8.0), Vec3f::new(64.0, 64.0, top));
        let mut pm = pmove_on_floor(rest, Some(step));
        pm.cmd.msec = 16;
        pm.run();
        assert_eq!(pm.ground_entity, Some(1));
        assert!(pm.touchents.contains(&1));
    }

    #[test]
    fn run_walking_into_an_entity_adds_it_to_the_touch_list_once_per_hit() {
        // A wall entity just ahead in +x, taller than a step.
        let wall = (Vec3f::new(24.0, -64.0, 0.0), Vec3f::new(64.0, 64.0, 128.0));
        let mut pm = pmove_on_floor(REST_Z, Some(wall));
        pm.cmd.msec = 50;
        pm.cmd.forwardmove = 400;
        let mut touched = false;
        for _ in 0..20 {
            pm.run();
            assert!(pm.touchents.len() <= MAXTOUCH);
            touched |= pm.touchents.contains(&1);
        }
        assert!(touched, "the wall entity was touched");
        assert_eq!(pm.ground_entity, Some(0), "still on the world floor");
    }

    #[test]
    fn run_clears_last_frames_touches() {
        let mut pm = pmove_on_floor(REST_Z, None);
        pm.touchents = vec![7; MAXTOUCH];
        pm.cmd.msec = 16;
        pm.run();
        assert!(!pm.touchents.contains(&7));
    }

    #[test]
    fn run_walking_accelerates_toward_maxspeed() {
        let mut pm = pmove_on_floor(REST_Z, None);
        pm.cmd.msec = 16;
        pm.cmd.forwardmove = 400;
        pm.run();
        // First frame from rest: accel = 10 * 0.016 * 300 = 48 u/s.
        assert_eq!(pm.s.velocity[0], 48 * 8);
        for _ in 0..60 {
            pm.run();
        }
        assert_eq!(pm.s.velocity[0], 300 * 8);
        assert!(pm.s.origin[0] > 0);
        assert_eq!(pm.s.origin[2], REST_Z);
    }

    #[test]
    fn run_jump_leaves_ground_with_270_minus_one_frame_of_gravity() {
        let mut pm = pmove_on_floor(REST_Z, None);
        pm.cmd.msec = 16;
        pm.run(); // settle on ground
        pm.cmd.upmove = 200;
        pm.run();
        assert_eq!(pm.s.pm_flags & PMF_ON_GROUND, 0);
        assert_ne!(pm.s.pm_flags & PMF_JUMP_HELD, 0);
        assert_eq!(pm.s.velocity[2], ((270.0f32 - 800.0 * 0.016) * 8.0) as i16);
        // Holding jump does not jump again after landing.
        assert!(pm.ground_entity.is_none());
    }

    #[test]
    fn run_falling_player_lands_on_floor() {
        let mut pm = pmove_on_floor(100 * 8, None);
        pm.cmd.msec = 16;
        for _ in 0..100 {
            pm.run();
        }
        assert_ne!(pm.s.pm_flags & PMF_ON_GROUND, 0);
        assert_eq!(pm.s.origin[2], REST_Z);
        assert_eq!(pm.s.velocity[2], 0);
    }

    #[test]
    fn run_walks_up_a_16_unit_step() {
        let step = (
            Vec3f::new(32.0, -500.0, 0.0),
            Vec3f::new(500.0, 500.0, 16.0),
        );
        let mut pm = pmove_on_floor(REST_Z, Some(step));
        pm.cmd.msec = 16;
        pm.cmd.forwardmove = 400;
        for _ in 0..40 {
            pm.run();
        }
        assert!(pm.s.origin[0] > 48 * 8, "x = {}", pm.s.origin[0]);
        assert_eq!(pm.s.origin[2], REST_Z + 16 * 8);
    }

    #[test]
    fn run_ducking_on_ground_shrinks_hull() {
        let mut pm = pmove_on_floor(REST_Z, None);
        pm.cmd.msec = 16;
        pm.run();
        pm.cmd.upmove = -200;
        pm.run();
        assert_ne!(pm.s.pm_flags & PMF_DUCKED, 0);
        assert_eq!(pm.maxs.z, 4.0);
        assert_eq!(pm.viewheight, -2.0);
        pm.cmd.upmove = 0;
        pm.run();
        assert_eq!(pm.s.pm_flags & PMF_DUCKED, 0);
    }
}
