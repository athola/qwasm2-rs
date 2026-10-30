//! The mission packs' particle effects (rogue, xatrix, CTF): their trails,
//! steam and smoke, the heat beam's rings and the tracker's dark shells.
//! C ref: the cl_newfx.c part of client/cl_effects.c, plus `CL_FlagTrail`,
//! `CL_IonripperTrail`, `CL_TrapParticles` and `CL_SmokeTrail`, which
//! nothing in C calls but the opt-in explosion smoke does. C's
//! `CL_QuadTrail` and `CL_Tracker_Explode`, which nothing calls (id
//! commented out the tracker one), are left out; ADR-003 says why.
//!
//! Where C leaves a particle field unset it keeps whatever the particle
//! held when it was last freed; here such a field is 0 (each function
//! says which).

use std::f64::consts::PI;

use q2_shared::types::Vec3f;

use crate::particle_fx::{make_normal_vectors, trail_dir};
use crate::particles::{
    crandk, frandk, jitter, vector_normalize, Particles, Spray, INSTANT_PARTICLE, MAX_PARTICLES,
    PARTICLE_GRAVITY,
};

/// C `colortable` of `CL_Widowbeamout` and `CL_WidowSplash`.
const WIDOW_COLORS: [i32; 4] = [2 * 8, 13 * 8, 21 * 8, 18 * 8];
/// C `colortable` of `CL_Nukeblast`.
const NUKE_COLORS: [i32; 4] = [110, 112, 114, 116];

use q2_common::pmove::angle_vectors;

/// C `(float)atan2(y, x) * 180 / M_PI`: degrees, the product in float and
/// the division in double.
pub(crate) fn atan2_degrees(y: f32, x: f32) -> f32 {
    (f64::from(f64::from(y).atan2(f64::from(x)) as f32 * 180.0) / PI) as f32
}

/// The angles (pitch negated, no roll) that look along `v`.
/// C ref: `AngleVectors2` (common/shared/shared.c).
pub(crate) fn angle_vectors2(v: Vec3f) -> Vec3f {
    let (pitch, yaw) = if v.y == 0.0 && v.x == 0.0 {
        (if v.z > 0.0 { 90.0 } else { 270.0 }, 0.0)
    } else {
        let mut yaw = if v.x != 0.0 {
            atan2_degrees(v.y, v.x)
        } else if v.y > 0.0 {
            90.0
        } else {
            270.0
        };
        if yaw < 0.0 {
            yaw += 360.0;
        }
        let forward = (v.x * v.x + v.y * v.y).sqrt();
        let mut pitch = atan2_degrees(v.z, forward);
        if pitch < 0.0 {
            pitch += 360.0;
        }
        (pitch, yaw)
    };
    Vec3f::new(-pitch, yaw, 0.0)
}

/// C's three `crandk()` normalised: a random unit direction.
fn random_dir(randk: &mut impl FnMut() -> u32) -> Vec3f {
    let mut dir = Vec3f::ZERO;
    for j in 0..3 {
        dir[j] = crandk(randk);
    }
    vector_normalize(&mut dir);
    dir
}

/// The look of a smoke trail: colours `color_start + randk % color_run`,
/// one particle every `spacing` units. C ref: `CL_SmokeTrail`'s
/// `colorStart`, `colorRun` and `spacing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmokeTrail {
    pub color_start: i32,
    pub color_run: i32,
    pub spacing: i32,
}

impl Particles {
    /// Rising smoke along a line. C leaves the x and y velocity unset (0
    /// here); a zero colour run divides by zero in C and makes nothing
    /// here. C ref: `CL_SmokeTrail`.
    pub fn smoke_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        style: SmokeTrail,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        if style.color_run == 0 {
            return;
        }
        let spacing = style.spacing as f32;
        let mut vec = end - start;
        let mut len = vector_normalize(&mut vec);
        let (mut mv, vec) = (start, vec * spacing);
        while len > 0.0 {
            len -= spacing;
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.5);
            // C's randk is never negative.
            p.color = style.color_start + (randk() % style.color_run.unsigned_abs()) as i32;
            for j in 0..3 {
                p.org[j] = mv[j] + crandk(randk) * 3.0;
            }
            p.vel = Vec3f::new(0.0, 0.0, 20.0 + crandk(randk) * 5.0);
            mv += vec;
        }
    }

    /// A CTF flag carrier's trail in `color` (242 red, 115 blue), wide and
    /// slow to fade. C ref: `CL_FlagTrail`.
    pub fn flag_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        color: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let (vec, mut len) = trail_dir(start, end);
        let (mut mv, vec) = (start, vec * 5.0);
        while len > 0 {
            len -= 5;
            if !self.flag_particle(mv, color, time, randk) {
                return;
            }
            mv += vec;
        }
    }

    /// One wide, slow-fading flag or tag trail particle at `mv`; false
    /// when the pool is full.
    fn flag_particle(
        &mut self,
        mv: Vec3f,
        color: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> bool {
        let Some(p) = self.alloc() else {
            return false;
        };
        p.accel = Vec3f::ZERO;
        p.time = time as f32;
        p.alpha = 1.0;
        p.alphavel = -1.0 / (0.8 + frandk(randk) * 0.2);
        p.color = color;
        for j in 0..3 {
            p.org[j] = mv[j] + crandk(randk) * 16.0;
            p.vel[j] = crandk(randk) * 5.0;
        }
        true
    }

    /// The "tag" carrier's trail (rogue deathmatch): `flag_trail` with a
    /// float length that also drops a particle at the end, as C loops
    /// while `len >= 0`. C ref: `CL_TagTrail`.
    pub fn tag_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        color: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let mut vec = end - start;
        let mut len = vector_normalize(&mut vec);
        let (mut mv, vec) = (start, vec * 5.0);
        while len >= 0.0 {
            len -= 5.0;
            if !self.flag_particle(mv, color, time, randk) {
                return;
            }
            mv += vec;
        }
    }

    /// The ion ripper's orange trail, its particles pushed alternately
    /// left and right along x. C ref: `CL_IonripperTrail`.
    pub fn ionripper_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let (vec, mut len) = trail_dir(start, end);
        let (mut mv, vec) = (start, vec * 5.0);
        let mut left = false;
        while len > 0 {
            len -= 5;
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 0.5;
            p.alphavel = -1.0 / (0.3 + frandk(randk) * 0.2);
            p.color = 0xe4 + (randk() & 3) as i32;
            p.org = mv;
            p.vel = Vec3f::new(if left { 10.0 } else { -10.0 }, 0.0, 0.0);
            left = !left;
            mv += vec;
        }
    }

    /// A trap (xatrix): a rising column of sparks from 14 below `origin`
    /// and a burst of eight. `origin` is C's `ent->origin`, which C moves
    /// down 14 and back up, and leaves 14 down when the pool fills during
    /// the column; both are kept, since the caller lights and trails from
    /// it. C ref: `CL_TrapParticles`.
    pub fn trap_particles(
        &mut self,
        origin: &mut Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        origin.z -= 14.0;
        let start = *origin;
        let mut end = *origin;
        end.z += 64.0;
        let (vec, mut len) = trail_dir(start, end);
        let (mut mv, vec) = (start, vec * 5.0);
        while len > 0 {
            len -= 5;
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (0.3 + frandk(randk) * 0.2);
            p.color = 0xe0;
            for j in 0..3 {
                p.org[j] = mv[j] + crandk(randk);
                p.vel[j] = crandk(randk) * 15.0;
            }
            p.accel = Vec3f::new(0.0, 0.0, PARTICLE_GRAVITY);
            mv += vec;
        }
        origin.z += 14.0;
        let org = *origin;
        for i in [-2, 2] {
            for j in [-2, 2] {
                for k in [-2, 2] {
                    let Some(p) = self.alloc() else { return };
                    p.time = time as f32;
                    p.color = 0xe0 + (randk() & 3) as i32;
                    p.alpha = 1.0;
                    p.alphavel = -1.0 / (0.3 + (randk() & 7) as f32 * 0.02);
                    // C leaves the order of randk() and crandk() in each
                    // product to the compiler; left to right here.
                    for (axis, offset) in [i, j, k].into_iter().enumerate() {
                        let spread = (randk() & 23) as f32;
                        p.org[axis] = org[axis] + offset as f32 + spread * crandk(randk);
                    }
                    // C crosses i and j here, as in `CL_TeleportParticles`.
                    let mut dir = Vec3f::new(j as f32 * 8.0, i as f32 * 8.0, k as f32 * 8.0);
                    vector_normalize(&mut dir);
                    p.vel = dir * (50 + (randk() & 63)) as f32;
                    p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
                }
            }
        }
    }

    /// A still line of particles every 3 units, fading over ten seconds.
    /// C ref: `CL_DebugTrail`.
    pub fn debug_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let mut vec = end - start;
        let mut len = vector_normalize(&mut vec);
        let (mut mv, vec) = (start, vec * 3.0);
        while len > 0.0 {
            len -= 3.0;
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.accel = Vec3f::ZERO;
            p.vel = Vec3f::ZERO;
            p.alpha = 1.0;
            p.alphavel = -0.1;
            p.color = 0x74 + (randk() & 7) as i32;
            p.org = mv;
            mv += vec;
        }
    }

    /// A force wall (rogue): slow-falling sparks in `color8` every 4 units,
    /// seven in ten of them. C ref: `CL_ForceWall`.
    pub fn force_wall(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        color8: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let mut vec = end - start;
        let mut len = vector_normalize(&mut vec);
        let (mut mv, vec) = (start, vec * 4.0);
        while len > 0.0 {
            len -= 4.0;
            if self.active.len() >= MAX_PARTICLES {
                return;
            }
            if f64::from(frandk(randk)) > 0.3 {
                let Some(p) = self.alloc() else { return };
                p.accel = Vec3f::ZERO;
                p.time = time as f32;
                p.alpha = 1.0;
                p.alphavel = (-1.0 / (3.0 + f64::from(frandk(randk) * 0.5))) as f32;
                p.color = color8;
                for j in 0..3 {
                    p.org[j] = mv[j] + crandk(randk) * 3.0;
                }
                p.vel = Vec3f::new(0.0, 0.0, -40.0 - crandk(randk) * 10.0);
            }
            mv += vec;
        }
    }

    /// Bubbles every `dist` units, rising faster than `bubble_trail`'s.
    /// C ref: `CL_BubbleTrail2`.
    pub fn bubble_trail2(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        dist: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let mut vec = end - start;
        let len = vector_normalize(&mut vec);
        let (mut mv, vec) = (start, vec * dist as f32);
        let mut i = 0i32;
        while (i as f32) < len {
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.1);
            p.color = 4 + (randk() & 7) as i32;
            for j in 0..3 {
                p.org[j] = mv[j] + crandk(randk) * 2.0;
                p.vel[j] = crandk(randk) * 10.0;
            }
            p.org.z -= 4.0;
            p.vel.z += 20.0;
            mv += vec;
            i = i.wrapping_add(dist);
        }
    }

    /// The player's heat beam (rogue): up to six rings of 20 round the
    /// beam, every 32 units from `start` along `forward`, sliding out with
    /// the clock. `right` and `up` are C `cl.v_right` and `cl.v_up`.
    /// C ref: `CL_Heatbeam`.
    pub fn heatbeam(
        &mut self,
        start: Vec3f,
        forward: Vec3f,
        right: Vec3f,
        up: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        const STEP: f32 = 32.0;
        let end = start + forward * 4096.0;
        let mut vec = end - start;
        let len = vector_normalize(&mut vec);
        let mut mv = start + right * -0.5;
        mv += up * -0.5;
        let ltime = time as f32 / 1000.0;
        let start_pt = (f64::from(ltime * 96.0) % f64::from(STEP)) as f32;
        mv += vec * start_pt;
        let vec = vec * STEP;
        let rstep = (PI / 10.0) as f32;
        let mut i = start_pt;
        while i < len {
            if i > STEP * 5.0 {
                break; // don't bother after the 5th ring
            }
            let mut rot = 0.0f32;
            while f64::from(rot) < PI * 2.0 {
                let Some(p) = self.alloc() else { return };
                p.time = time as f32;
                p.accel = Vec3f::ZERO;
                let c = f64::from(rot).cos() as f32 * 0.5;
                let s = f64::from(rot).sin() as f32 * 0.5;
                // Trim it so it looks like it starts at the origin.
                let dir = if i < 10.0 {
                    right * (c * (i / 10.0)) + up * (s * (i / 10.0))
                } else {
                    right * c + up * s
                };
                p.alpha = 0.5;
                p.alphavel = -1000.0;
                p.color = 223 - (randk() & 7) as i32;
                p.org = mv + dir * 3.0;
                p.vel = Vec3f::ZERO;
                rot += rstep;
            }
            mv += vec;
            i += STEP;
        }
    }

    /// Steam puffing along `spray.dir` at `magnitude` units a second.
    /// C ref: `CL_ParticleSteamEffect`.
    pub fn particle_steam_effect(
        &mut self,
        spray: Spray,
        magnitude: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        self.steam(
            spray,
            magnitude,
            -PARTICLE_GRAVITY / 2.0,
            false,
            time,
            randk,
        );
    }

    /// A steam sustain's puff: `particle_steam_effect` with its arithmetic
    /// in double. False when the pool filled before the last particle (C
    /// then returns before scheduling the next puff).
    /// C ref: `CL_ParticleSteamEffect2`.
    pub(crate) fn sustain_steam(
        &mut self,
        spray: Spray,
        magnitude: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> bool {
        self.steam(spray, magnitude, -PARTICLE_GRAVITY / 2.0, true, time, randk)
    }

    /// Steam without gravity (the chainfist's smoke).
    /// C ref: `CL_ParticleSmokeEffect`.
    pub fn particle_smoke_effect(
        &mut self,
        spray: Spray,
        magnitude: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        self.steam(spray, magnitude, 0.0, false, time, randk);
    }

    /// The body of the steam and smoke sprays; `c_double` is
    /// `CL_ParticleSteamEffect2`'s double-precision origin and fade.
    /// False when the pool filled first.
    fn steam(
        &mut self,
        spray: Spray,
        magnitude: i32,
        accel_z: f32,
        c_double: bool,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> bool {
        let (r, u) = make_normal_vectors(spray.dir);
        let m = magnitude as f32;
        for _ in 0..spray.count {
            let Some(p) = self.alloc() else { return false };
            p.time = time as f32;
            p.color = spray.color + (randk() & 7) as i32;
            for j in 0..3 {
                p.org[j] = if c_double {
                    (f64::from(spray.org[j])
                        + f64::from(magnitude) * 0.1 * f64::from(crandk(randk)))
                        as f32
                } else {
                    spray.org[j] + m * 0.1 * crandk(randk)
                };
            }
            p.vel = spray.dir * m;
            p.vel += r * (crandk(randk) * m / 3.0);
            p.vel += u * (crandk(randk) * m / 3.0);
            p.accel = Vec3f::new(0.0, 0.0, accel_z);
            p.alpha = 1.0;
            p.alphavel = if c_double {
                (-1.0 / (0.5 + f64::from(frandk(randk)) * 0.3)) as f32
            } else {
                -1.0 / (0.5 + frandk(randk) * 0.3)
            };
        }
        true
    }

    /// A tracker round's dark trail, waving round its path by
    /// `8 cos(distance along it)`. C ref: `CL_TrackerTrail`.
    pub fn tracker_trail(&mut self, start: Vec3f, end: Vec3f, color: i32, time: i32) {
        let mut vec = end - start;
        let mut len = vector_normalize(&mut vec);
        let (forward, _, up) = angle_vectors(angle_vectors2(vec));
        let (mut mv, vec) = (start, vec * 3.0);
        while len > 0.0 {
            len -= 3.0;
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = -2.0;
            p.color = color;
            let dist = mv.x * forward.x + mv.y * forward.y + mv.z * forward.z;
            p.org = mv + up * (8.0 * f64::from(dist).cos()) as f32;
            p.vel = Vec3f::new(0.0, 0.0, 5.0);
            mv += vec;
        }
    }

    /// One frame of `count` particles in `color` on a sphere of `radius`
    /// round `origin`: the tracker's and the monster heat beam's shells.
    fn shell(
        &mut self,
        origin: Vec3f,
        count: i32,
        color: i32,
        radius: f32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..count {
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = INSTANT_PARTICLE;
            p.color = color;
            p.org = origin + random_dir(randk) * radius;
        }
    }

    /// A dark shell of 300 round a tracker-hit entity, for one frame. C
    /// leaves the velocity unset (0 here; an instant particle never moves).
    /// C ref: `CL_Tracker_Shell`.
    pub fn tracker_shell(&mut self, origin: Vec3f, time: i32, randk: &mut impl FnMut() -> u32) {
        self.shell(origin, 300, 0, 40.0, time, randk);
    }

    /// 40 sparks round a monster's heat-beam muzzle, for one frame.
    /// C ref: `CL_MonsterPlasma_Shell`.
    pub fn monster_plasma_shell(
        &mut self,
        origin: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        self.shell(origin, 40, 0xe0, 10.0, time, randk);
    }

    /// The widow's beam-out sustain: 300 for one frame on a sphere growing
    /// to 45 units as client time `time` reaches `endtime`.
    /// C ref: `CL_Widowbeamout`.
    pub fn widow_beamout(
        &mut self,
        org: Vec3f,
        endtime: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let ratio = 1.0 - (endtime as f32 - time as f32) / 2100.0;
        self.growing_shell(org, 300, WIDOW_COLORS, 45.0 * f64::from(ratio), time, randk);
    }

    /// The nuke's sustain: 700 for one frame on a sphere growing to 200
    /// units as `time` reaches `endtime`. C ref: `CL_Nukeblast`.
    pub fn nukeblast(
        &mut self,
        org: Vec3f,
        endtime: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let ratio = 1.0 - (endtime as f32 - time as f32) / 1000.0;
        self.growing_shell(org, 700, NUKE_COLORS, 200.0 * f64::from(ratio), time, randk);
    }

    /// The body of `widow_beamout` and `nukeblast`: `count` instant
    /// particles from `colors` at `radius` (C's double, passed as float).
    fn growing_shell(
        &mut self,
        org: Vec3f,
        count: i32,
        colors: [i32; 4],
        radius: f64,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..count {
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = INSTANT_PARTICLE;
            p.color = colors[(randk() & 3) as usize];
            p.org = org + random_dir(randk) * radius as f32;
        }
    }

    /// The widow's splash: 256 thrown out from a 45-unit sphere. C leaves
    /// the z acceleration unset (0 here). C ref: `CL_WidowSplash`.
    pub fn widow_splash(&mut self, org: Vec3f, time: i32, randk: &mut impl FnMut() -> u32) {
        for _ in 0..256 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = WIDOW_COLORS[(randk() & 3) as usize];
            let dir = random_dir(randk);
            p.org = org + dir * 45.0;
            p.vel = dir * 40.0;
            p.accel = Vec3f::ZERO;
            p.alpha = 1.0;
            p.alphavel = -0.8 / (0.5 + frandk(randk) * 0.3);
        }
    }

    /// 128 particles in colours `color + randk % run` bursting from `org`.
    /// A `run` of 0 divides by zero in C and makes nothing here.
    /// C ref: `CL_ColorExplosionParticles`.
    pub fn color_explosion_particles(
        &mut self,
        org: Vec3f,
        color: i32,
        run: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        if run == 0 {
            return;
        }
        for _ in 0..128 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            // `run` is positive here; C's randk is never negative.
            p.color = color + (randk() % run.unsigned_abs()) as i32;
            for j in 0..3 {
                p.org[j] = org[j] + ((randk() % 32) as i32 - 16) as f32;
                p.vel[j] = ((randk() % 256) as i32 - 128) as f32;
            }
            p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
            p.alpha = 1.0;
            p.alphavel = -0.4 / (0.6 + frandk(randk) * 0.2);
        }
    }

    /// `blaster_particles` in colours `color..+7` (0xd0 for the blaster2,
    /// 0x6f for flechettes). C ref: `CL_BlasterParticles2`.
    pub fn blaster_particles2(
        &mut self,
        org: Vec3f,
        dir: Vec3f,
        color: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..40 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = color + (randk() & 7) as i32;
            let d = (randk() & 15) as f32;
            for j in 0..3 {
                p.org[j] = org[j] + jitter(randk) + d * dir[j];
                p.vel[j] = dir[j] * 30.0 + crandk(randk) * 40.0;
            }
            p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
            p.alpha = 1.0;
            p.alphavel = -1.0 / (0.5 + frandk(randk) * 0.3);
        }
    }

    /// The green blaster bolt's trail. yquake2 never sets the colour (a
    /// freed particle's colour stays); id's cl_newfx.c sets 0xd0, used
    /// here. C ref: `CL_BlasterTrail2`.
    pub fn blaster_trail2(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let mut vec = end - start;
        let mut len = vector_normalize(&mut vec);
        let (mut mv, vec) = (start, vec * 5.0);
        while len > 0.0 {
            len -= 5.0;
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (0.3 + frandk(randk) * 0.2);
            p.color = 0xd0;
            for j in 0..3 {
                p.org[j] = mv[j] + crandk(randk);
                p.vel[j] = crandk(randk) * 5.0;
            }
            mv += vec;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::particles::{Particles, Spray, INSTANT_PARTICLE, PARTICLE_GRAVITY};
    use q2_shared::types::Vec3f;

    fn rng(values: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    const X100: Vec3f = Vec3f::new(100.0, 0.0, 0.0);

    #[test]
    fn a_colour_explosion_stays_in_c_ranges_for_any_random_number() {
        // C randk is never negative; the high bit must not flip signs.
        let mut ps = Particles::default();
        ps.color_explosion_particles(Vec3f::ZERO, 0x10, 4, 0, &mut rng(&[u32::MAX, 0x8000_0001]));
        for p in &ps.active {
            assert!((0x10..0x14).contains(&p.color), "{}", p.color);
            for j in 0..3 {
                assert!((-16.0..16.0).contains(&p.org[j]), "{:?}", p.org);
                assert!((-128.0..128.0).contains(&p.vel[j]), "{:?}", p.vel);
            }
        }
    }

    #[test]
    fn a_smoke_trail_rises_in_its_colour_run() {
        let mut ps = Particles::default();
        let style = SmokeTrail {
            color_start: 0x10,
            color_run: 4,
            spacing: 10,
        };
        ps.smoke_trail(Vec3f::ZERO, X100, style, 0, &mut rng(&[6]));
        assert_eq!(ps.len(), 10);
        let p = ps.active[0];
        assert_eq!(p.color, 0x12, "0x10 + 6 % 4");
        assert_eq!((p.vel.x, p.vel.y), (0.0, 0.0));
        // C randk is never negative: a high bit stays in the run.
        ps.clear();
        ps.smoke_trail(Vec3f::ZERO, X100, style, 0, &mut rng(&[u32::MAX]));
        assert!(ps.active.iter().all(|p| (0x10..0x14).contains(&p.color)));
        // C divides by zero for an empty colour run.
        ps.clear();
        let style = SmokeTrail {
            color_run: 0,
            ..style
        };
        ps.smoke_trail(Vec3f::ZERO, X100, style, 0, &mut rng(&[6]));
        assert!(ps.is_empty());
    }

    #[test]
    fn a_flag_trail_drops_a_wide_particle_every_five_units() {
        let mut ps = Particles::default();
        ps.flag_trail(Vec3f::ZERO, X100, 242, 0, &mut rng(&[0]));
        assert_eq!(ps.len(), 20);
        let p = ps.active[0];
        // crandk 0 is -1: org -16 per axis, vel -5.
        assert_eq!(
            (p.color, p.org, p.vel),
            (242, Vec3f::splat(-16.0), Vec3f::splat(-5.0))
        );
        assert_eq!(p.alphavel, -1.0 / 0.8);
    }

    #[test]
    fn a_tag_trail_also_drops_one_at_the_end() {
        let mut ps = Particles::default();
        // C loops while len >= 0: 100 units make 21.
        ps.tag_trail(Vec3f::ZERO, X100, 220, 0, &mut rng(&[0]));
        assert_eq!((ps.len(), ps.active[0].color), (21, 220));
        ps.clear();
        ps.tag_trail(Vec3f::ZERO, Vec3f::ZERO, 220, 0, &mut rng(&[0]));
        assert_eq!(ps.len(), 1, "a still entity still drops one");
    }

    #[test]
    fn an_ionripper_trail_wobbles_left_and_right() {
        let mut ps = Particles::default();
        ps.ionripper_trail(Vec3f::ZERO, Vec3f::new(20.0, 0.0, 0.0), 0, &mut rng(&[1]));
        let vx: Vec<f32> = ps.active.iter().map(|p| p.vel.x).collect();
        assert_eq!(vx, [-10.0, 10.0, -10.0, 10.0]);
        let p = ps.active[1];
        assert_eq!(
            (p.color, p.alpha, p.org),
            (0xe5, 0.5, Vec3f::new(5.0, 0.0, 0.0))
        );
    }

    #[test]
    fn a_trap_rises_a_column_and_bursts_eight_and_keeps_its_origin() {
        let mut ps = Particles::default();
        let mut origin = Vec3f::new(0.0, 0.0, 100.0);
        ps.trap_particles(&mut origin, 0, &mut rng(&[0]));
        // 64 units of column at one per 5 (13), then 2 x 2 x 2.
        assert_eq!(ps.len(), 13 + 8);
        let column = ps.active[0];
        assert_eq!(column.org, Vec3f::new(-1.0, -1.0, 85.0), "starts 14 down");
        assert_eq!(column.accel.z, PARTICLE_GRAVITY);
        let burst = ps.active[13];
        assert_eq!(burst.accel.z, -PARTICLE_GRAVITY);
        assert_eq!(burst.org, Vec3f::new(-2.0, -2.0, 98.0), "randk & 23 is 0");
        assert_eq!(origin, Vec3f::new(0.0, 0.0, 100.0));
        // A full pool stops in the column, with the origin still 14 down.
        let mut full = Particles::default();
        full.particle_effect(
            Spray {
                org: Vec3f::ZERO,
                dir: Vec3f::ZERO,
                color: 0,
                count: 4096,
            },
            0,
            &mut rng(&[0]),
        );
        let mut origin = Vec3f::new(0.0, 0.0, 100.0);
        full.trap_particles(&mut origin, 0, &mut rng(&[0]));
        assert_eq!(origin.z, 86.0);
    }

    #[test]
    fn a_debug_trail_is_a_still_line_every_three_units() {
        let mut ps = Particles::default();
        ps.debug_trail(Vec3f::ZERO, Vec3f::new(9.0, 0.0, 0.0), 0, &mut rng(&[3]));
        let xs: Vec<f32> = ps.active.iter().map(|p| p.org.x).collect();
        assert_eq!(xs, [0.0, 3.0, 6.0]);
        let p = ps.active[0];
        assert_eq!((p.color, p.vel, p.alphavel), (0x77, Vec3f::ZERO, -0.1));
    }

    #[test]
    fn a_force_wall_drops_seven_tenths_of_its_slow_falling_sparks() {
        let mut ps = Particles::default();
        // frandk 0 is not > 0.3: nothing.
        ps.force_wall(Vec3f::ZERO, X100, 0x74, 0, &mut rng(&[0]));
        assert!(ps.is_empty());
        ps.force_wall(Vec3f::ZERO, X100, 0x74, 0, &mut rng(&[32767]));
        assert_eq!(ps.len(), 25);
        let p = ps.active[0];
        assert_eq!((p.color, p.vel), (0x74, Vec3f::new(0.0, 0.0, -50.0)));
        assert_eq!(p.alphavel, (-1.0 / 3.5f64) as f32);
    }

    #[test]
    fn bubble_trail2_spaces_its_bubbles_by_the_distance_given() {
        let mut ps = Particles::default();
        ps.bubble_trail2(Vec3f::ZERO, X100, 8, 0, &mut rng(&[0]));
        assert_eq!(ps.len(), 13);
        let p = ps.active[0];
        assert_eq!(p.org, Vec3f::new(-2.0, -2.0, -6.0));
        assert_eq!(p.vel, Vec3f::new(-10.0, -10.0, 10.0));
    }

    #[test]
    fn a_heatbeam_is_five_rings_of_twenty_moving_with_the_clock() {
        let mut ps = Particles::default();
        let (right, up) = (Vec3f::new(0.0, -1.0, 0.0), Vec3f::Z);
        ps.heatbeam(Vec3f::ZERO, Vec3f::X, right, up, 0, &mut rng(&[0]));
        // Rings at 0, 32 .. 160 (C breaks only past the 5th, at i > 160).
        assert_eq!(ps.len(), 6 * 20);
        let p = ps.active[0];
        assert_eq!((p.color, p.alpha, p.alphavel), (223, 0.5, -1000.0));
        // The first ring is trimmed to the axis: i / 10 is 0.
        assert_eq!(p.org, Vec3f::new(0.0, 0.5, -0.5));
        let ring2 = ps.active[20];
        assert_eq!(ring2.org.x, 32.0);
        // 250 ms in, the rings start 24 units further out.
        ps.clear();
        ps.heatbeam(Vec3f::ZERO, Vec3f::X, right, up, 250, &mut rng(&[0]));
        assert_eq!(ps.active[0].org.x, 24.0);
        assert_eq!(ps.len(), 5 * 20);
    }

    #[test]
    fn steam_puffs_along_its_direction_and_sinks_at_half_gravity() {
        let mut ps = Particles::default();
        let spray = Spray {
            org: Vec3f::ZERO,
            dir: Vec3f::Z,
            color: 0xe0,
            count: 3,
        };
        ps.particle_steam_effect(spray, 60, 0, &mut rng(&[32767]));
        assert_eq!(ps.len(), 3);
        let p = ps.active[0];
        assert_eq!(p.color, 0xe7);
        assert_eq!(p.org, Vec3f::splat(6.0));
        // dir * 60 plus 20 along each of the two normals.
        assert_eq!(p.vel.z, 60.0);
        assert!((p.vel.length() - (60.0f32 * 60.0 + 800.0).sqrt()).abs() < 1e-3);
        assert_eq!(p.accel, Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY / 2.0));
        ps.clear();
        ps.particle_smoke_effect(spray, 20, 0, &mut rng(&[0]));
        assert_eq!(ps.active[0].accel, Vec3f::ZERO, "smoke floats");
    }

    #[test]
    fn a_tracker_trail_is_a_dark_wave_rising_slowly() {
        let mut ps = Particles::default();
        ps.tracker_trail(Vec3f::ZERO, Vec3f::new(9.0, 0.0, 0.0), 0, 0);
        assert_eq!(ps.len(), 3);
        let p = ps.active[0];
        // At the start dot(move, forward) is 0: 8 * cos 0 along up.
        assert_eq!(p.org, Vec3f::new(0.0, 0.0, 8.0));
        assert_eq!((p.vel, p.alphavel), (Vec3f::new(0.0, 0.0, 5.0), -2.0));
        let wave = ps.active[1].org.z;
        assert_eq!(wave, (8.0 * 3.0f64.cos()) as f32);
    }

    #[test]
    fn tracker_and_monster_plasma_shells_last_one_frame() {
        let mut ps = Particles::default();
        ps.tracker_shell(Vec3f::ZERO, 0, &mut rng(&[32767]));
        assert_eq!(ps.len(), 300);
        let p = ps.active[0];
        assert_eq!((p.color, p.alphavel), (0, INSTANT_PARTICLE));
        assert!((p.org.length() - 40.0).abs() < 1e-4);
        ps.clear();
        ps.monster_plasma_shell(Vec3f::ZERO, 0, &mut rng(&[32767]));
        assert_eq!((ps.len(), ps.active[0].color), (40, 0xe0));
        assert!((ps.active[0].org.length() - 10.0).abs() < 1e-4);
    }

    #[test]
    fn widow_beams_and_nukes_grow_to_their_radius_by_their_end() {
        let mut ps = Particles::default();
        // Half way through the widow's 2100 ms.
        ps.widow_beamout(Vec3f::ZERO, 2050, 1000, &mut rng(&[32767]));
        assert_eq!(ps.len(), 300);
        assert!((ps.active[0].org.length() - 22.5).abs() < 1e-3);
        assert_eq!(ps.active[0].color, 18 * 8);
        ps.clear();
        ps.nukeblast(Vec3f::ZERO, 1000, 1000, &mut rng(&[2]));
        assert_eq!(ps.len(), 700);
        assert_eq!(ps.active[0].color, 114);
        assert!((ps.active[0].org.length() - 200.0).abs() < 1e-3);
    }

    #[test]
    fn a_widow_splash_bursts_256_outwards() {
        let mut ps = Particles::default();
        ps.widow_splash(Vec3f::ZERO, 0, &mut rng(&[32767]));
        assert_eq!(ps.len(), 256);
        let p = ps.active[0];
        assert!((p.org.length() - 45.0).abs() < 1e-3);
        assert!((p.vel.length() - 40.0).abs() < 1e-3);
        assert_eq!(p.accel, Vec3f::ZERO);
    }

    #[test]
    fn a_colour_explosion_bursts_128_particles() {
        let mut ps = Particles::default();
        ps.color_explosion_particles(Vec3f::ZERO, 0, 1, 0, &mut rng(&[300]));
        assert_eq!(ps.len(), 128);
        let p = ps.active[0];
        // 300 % 32 - 16 and 300 % 256 - 128.
        assert_eq!((p.color, p.org.x, p.vel.x), (0, -4.0, -84.0));
        ps.clear();
        ps.color_explosion_particles(Vec3f::ZERO, 0, 0, 0, &mut rng(&[300]));
        assert!(ps.is_empty(), "C divides by zero for run 0");
    }

    #[test]
    fn blaster2_impacts_and_trails_are_green() {
        let mut ps = Particles::default();
        ps.blaster_particles2(Vec3f::ZERO, Vec3f::Z, 0x6f, 0, &mut rng(&[1]));
        assert_eq!((ps.len(), ps.active[0].color), (40, 0x70));
        ps.clear();
        ps.blaster_trail2(Vec3f::ZERO, Vec3f::new(20.0, 0.0, 0.0), 0, &mut rng(&[0]));
        assert_eq!((ps.len(), ps.active[0].color), (4, 0xd0));
    }

    #[test]
    fn angle_vectors2_points_along_the_vector() {
        assert_eq!(angle_vectors2(Vec3f::X), Vec3f::ZERO);
        assert_eq!(angle_vectors2(Vec3f::Z), Vec3f::new(-90.0, 0.0, 0.0));
        assert_eq!(angle_vectors2(Vec3f::new(0.0, -1.0, 0.0)).y, 270.0);
        let (forward, right, up) = angle_vectors(Vec3f::new(0.0, 90.0, 0.0));
        assert!((forward - Vec3f::Y).length() < 1e-6);
        assert!((right - Vec3f::X).length() < 1e-6);
        assert!((up - Vec3f::Z).length() < 1e-6);
    }
}
