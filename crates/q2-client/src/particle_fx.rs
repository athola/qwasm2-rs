//! The base game's particle effects: impact sprays, explosions, teleports
//! and the trails behind moving entities. C ref: client/cl_effects.c.

use std::f64::consts::PI;
use std::sync::OnceLock;

use q2_shared::constants::{EF_GIB, EF_GREENGIB, EF_ROCKET, MZ_LOGIN, MZ_LOGOUT};
use q2_shared::types::Vec3f;

use crate::particles::{
    crandk, frandk, vector_normalize, Particles, MAX_PARTICLES, PARTICLE_GRAVITY,
};

/// C `NUMVERTEXNORMALS`.
const NUM_VERTEX_NORMALS: usize = 162;

/// C `bytedirs` (anorms.h), read back through q2-common's `read_dir` so
/// the table is the one the protocol uses.
fn bytedirs() -> &'static [Vec3f; NUM_VERTEX_NORMALS] {
    static TABLE: OnceLock<[Vec3f; NUM_VERTEX_NORMALS]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let mut msg = q2_common::net_msg::NetMsg::from_bytes(&[i as u8]);
            msg.begin_reading();
            msg.read_dir().unwrap_or_default()
        })
    })
}

/// C `(int)VectorNormalize(vec)`: the unit direction and the truncated
/// length the trail loops count down.
pub(crate) fn trail_dir(start: Vec3f, end: Vec3f) -> (Vec3f, i32) {
    let mut vec = end - start;
    let len = vector_normalize(&mut vec) as i32;
    (vec, len)
}

/// C `MakeNormalVectors`: two unit vectors square to `forward`.
pub(crate) fn make_normal_vectors(forward: Vec3f) -> (Vec3f, Vec3f) {
    // This rotate and negate guarantees a vector not colinear with forward.
    let mut right = Vec3f::new(forward.z, -forward.x, forward.y);
    let d = right.dot(forward);
    right += forward * -d;
    vector_normalize(&mut right);
    (right, right.cross(forward))
}

impl Particles {
    /// Eight sparks rising off a teleporter pad, once per server frame.
    /// C ref: `CL_TeleporterParticles`.
    pub fn teleporter_particles(
        &mut self,
        origin: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..8 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = 0xdb;
            for j in 0..2 {
                p.org[j] = origin[j] - 16.0 + (randk() & 31) as f32;
                p.vel[j] = crandk(randk) * 14.0;
            }
            p.org.z = origin.z - 8.0 + (randk() & 7) as f32;
            p.vel.z = 80.0 + (randk() & 7) as f32;
            p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
            p.alpha = 1.0;
            p.alphavel = -0.5;
        }
    }

    /// A player logging in (green), out (red) or respawning (yellow):
    /// `kind` is the `MZ_*` flash. C ref: `CL_LogoutEffect`.
    pub fn logout_effect(
        &mut self,
        org: Vec3f,
        kind: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..500 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            let base = match kind {
                MZ_LOGIN => 0xd0,
                MZ_LOGOUT => 0x40,
                _ => 0xe0,
            };
            p.color = base + (randk() & 7) as i32;
            p.org.x = org.x - 16.0 + frandk(randk) * 32.0;
            p.org.y = org.y - 16.0 + frandk(randk) * 32.0;
            p.org.z = org.z - 24.0 + frandk(randk) * 56.0;
            for j in 0..3 {
                p.vel[j] = crandk(randk) * 20.0;
            }
            p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
            p.alpha = 1.0;
            p.alphavel = (-1.0 / (1.0 + f64::from(frandk(randk)) * 0.3)) as f32;
        }
    }

    /// C ref: `CL_ItemRespawnParticles`.
    pub fn item_respawn_particles(
        &mut self,
        org: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..64 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = 0xd4 + (randk() & 3) as i32;
            for j in 0..3 {
                p.org[j] = org[j] + crandk(randk) * 8.0;
            }
            for j in 0..3 {
                p.vel[j] = crandk(randk) * 8.0;
            }
            p.accel = Vec3f::new(0.0, 0.0, (-f64::from(PARTICLE_GRAVITY) * 0.2) as f32);
            p.alpha = 1.0;
            p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.3);
        }
    }

    /// C ref: `CL_ExplosionParticles`.
    pub fn explosion_particles(&mut self, org: Vec3f, time: i32, randk: &mut impl FnMut() -> u32) {
        self.burst(org, 0xe0, time, randk);
    }

    /// C ref: `CL_BFGExplosionParticles`.
    pub fn bfg_explosion_particles(
        &mut self,
        org: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        self.burst(org, 0xd0, time, randk);
    }

    /// The 256-particle blast shared by `CL_ExplosionParticles` and
    /// `CL_BFGExplosionParticles`, which differ only in colour.
    fn burst(&mut self, org: Vec3f, color: i32, time: i32, randk: &mut impl FnMut() -> u32) {
        for _ in 0..256 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = color + (randk() & 7) as i32;
            for j in 0..3 {
                p.org[j] = org[j] + ((randk() % 32) as i32 - 16) as f32;
                p.vel[j] = ((randk() % 384) as i32 - 192) as f32;
            }
            p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
            p.alpha = 1.0;
            p.alphavel = -0.8 / (0.5 + frandk(randk) * 0.3);
        }
    }

    /// A boss teleporting: a ring of 4096 that swirls in and rises.
    /// C ref: `CL_BigTeleportParticles`.
    pub fn big_teleport_particles(
        &mut self,
        org: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        const COLORTABLE: [i32; 4] = [2 * 8, 13 * 8, 21 * 8, 18 * 8];
        for _ in 0..4096 {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = COLORTABLE[(randk() & 3) as usize];
            let angle = (PI * 2.0 * f64::from(randk() & 1023) / 1023.0) as f32;
            let dist = (randk() & 31) as f32;
            let (sin, cos) = (f64::from(angle).sin() as f32, f64::from(angle).cos() as f32);
            p.org.x = org.x + cos * dist;
            p.vel.x = cos * (70 + (randk() & 63)) as f32;
            p.accel.x = -cos * 100.0;
            p.org.y = org.y + sin * dist;
            p.vel.y = sin * (70 + (randk() & 63)) as f32;
            p.accel.y = -sin * 100.0;
            p.org.z = org.z + 8.0 + (randk() % 90) as f32;
            p.vel.z = -100.0 + (randk() & 31) as f32;
            p.accel.z = PARTICLE_GRAVITY * 4.0;
            p.alpha = 1.0;
            p.alphavel = -0.3 / (0.5 + frandk(randk) * 0.3);
        }
    }

    /// A blaster bolt's wall impact: `blaster_particles2` in 0xe0.
    /// C ref: `CL_BlasterParticles`.
    pub fn blaster_particles(
        &mut self,
        org: Vec3f,
        dir: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        self.blaster_particles2(org, dir, 0xe0, time, randk);
    }

    /// C ref: `CL_BlasterTrail`.
    pub fn blaster_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let (vec, mut len) = trail_dir(start, end);
        let (mut mv, vec) = (start, vec * 5.0);
        while len > 0 {
            len -= 5;
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (0.3 + frandk(randk) * 0.2);
            p.color = 0xe0;
            for j in 0..3 {
                p.org[j] = mv[j] + crandk(randk);
                p.vel[j] = crandk(randk) * 5.0;
            }
            mv += vec;
        }
    }

    /// Blood behind a gib (`EF_GIB` in `flags`, green for `EF_GREENGIB`)
    /// or smoke behind a grenade or rocket, thinning as `trailcount` (C
    /// `centity_t.trailcount`) runs down. C's int `len -= 0.5` drops a whole unit per half-unit
    /// step; kept. C ref: `CL_DiminishingTrail`.
    pub fn diminishing_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        trailcount: &mut i32,
        flags: u32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        const DEC: f32 = 0.5;
        let (vec, mut len) = trail_dir(start, end);
        let (mut mv, vec) = (start, vec * DEC);
        let (orgscale, velscale) = match *trailcount {
            c if c > 900 => (4.0, 15.0),
            c if c > 800 => (2.0, 10.0),
            _ => (1.0, 5.0),
        };
        while len > 0 {
            len = (len as f32 - DEC) as i32;
            if self.active.len() >= MAX_PARTICLES {
                return;
            }
            // Drop fewer particles as it flies.
            if ((randk() & 1023) as i32) < *trailcount {
                let Some(p) = self.alloc() else { return };
                p.accel = Vec3f::ZERO;
                p.time = time as f32;
                p.alpha = 1.0;
                if flags & EF_GIB != 0 {
                    p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.4);
                    p.color = 0xe8 + (randk() & 7) as i32;
                    for j in 0..3 {
                        p.org[j] = mv[j] + crandk(randk) * orgscale;
                        p.vel[j] = crandk(randk) * velscale;
                    }
                    p.vel.z -= PARTICLE_GRAVITY;
                } else if flags & EF_GREENGIB != 0 {
                    p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.4);
                    p.color = 0xdb + (randk() & 7) as i32;
                    for j in 0..3 {
                        p.org[j] = mv[j] + crandk(randk) * orgscale;
                        p.vel[j] = crandk(randk) * velscale;
                    }
                    p.vel.z -= PARTICLE_GRAVITY;
                } else {
                    p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.2);
                    p.color = 4 + (randk() & 7) as i32;
                    for j in 0..3 {
                        p.org[j] = mv[j] + crandk(randk) * orgscale;
                        p.vel[j] = crandk(randk) * velscale;
                    }
                    p.accel.z = 20.0;
                }
            }
            *trailcount = (*trailcount - 5).max(100);
            mv += vec;
        }
    }

    /// Smoke (a diminishing trail) plus sparse fire. C ref: `CL_RocketTrail`.
    pub fn rocket_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        trailcount: &mut i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        self.diminishing_trail(start, end, trailcount, EF_ROCKET, time, randk);
        let (vec, mut len) = trail_dir(start, end);
        let mut mv = start;
        while len > 0 {
            len -= 1;
            if self.active.len() >= MAX_PARTICLES {
                return;
            }
            if randk() & 7 == 0 {
                let Some(p) = self.alloc() else { return };
                p.accel = Vec3f::ZERO;
                p.time = time as f32;
                p.alpha = 1.0;
                p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.2);
                p.color = 0xdc + (randk() & 3) as i32;
                for j in 0..3 {
                    p.org[j] = mv[j] + crandk(randk) * 5.0;
                    p.vel[j] = crandk(randk) * 20.0;
                }
                p.accel.z = -PARTICLE_GRAVITY;
            }
            mv += vec;
        }
    }

    /// A railgun shot: a blue spiral, one particle per unit, round a grey
    /// core. C ref: `CL_RailTrail`.
    pub fn rail_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let (vec, mut len) = trail_dir(start, end);
        let (right, up) = make_normal_vectors(vec);
        let mut mv = start;
        for i in 0..len {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.accel = Vec3f::ZERO;
            let d = i as f32 * 0.1;
            let (s, c) = (f64::from(d).sin() as f32, f64::from(d).cos() as f32);
            let dir = right * c + up * s;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.2);
            p.color = 0x74 + (randk() & 7) as i32;
            p.org = mv + dir * 3.0;
            p.vel = dir * 6.0;
            mv += vec;
        }
        const DEC: f32 = 0.75;
        let vec = vec * DEC;
        mv = start;
        while len > 0 {
            len = (len as f32 - DEC) as i32;
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.accel = Vec3f::ZERO;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (0.6 + frandk(randk) * 0.2);
            p.color = (randk() & 15) as i32;
            for j in 0..3 {
                p.org[j] = mv[j] + crandk(randk) * 3.0;
                p.vel[j] = crandk(randk) * 3.0;
            }
            mv += vec;
        }
    }

    /// A bubble every 32 units, drifting up. C ref: `CL_BubbleTrail`.
    pub fn bubble_trail(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let (vec, len) = trail_dir(start, end);
        let (mut mv, vec) = (start, vec * 32.0);
        for _ in (0..len).step_by(32) {
            let Some(p) = self.alloc() else { return };
            p.accel = Vec3f::ZERO;
            p.time = time as f32;
            p.alpha = 1.0;
            p.alphavel = -1.0 / (1.0 + frandk(randk) * 0.2);
            p.color = 4 + (randk() & 7) as i32;
            for j in 0..3 {
                p.org[j] = mv[j] + crandk(randk) * 2.0;
                p.vel[j] = crandk(randk) * 5.0;
            }
            p.vel.z += 6.0;
            mv += vec;
        }
    }

    /// C's `avelocities`, drawn the first time a swarm needs them (and
    /// again while the first rate came out zero, as C tests `[0][0]`).
    fn swarm_rates(&mut self, randk: &mut impl FnMut() -> u32) {
        if self.avelocities.first().is_some_and(|v| v.x != 0.0) {
            return;
        }
        self.avelocities = (0..NUM_VERTEX_NORMALS)
            .map(|_| {
                let mut rate = || (randk() & 255) as f32 * 0.01;
                Vec3f::new(rate(), rate(), rate())
            })
            .collect();
    }

    /// Where swarm particle `i` sits at `ltime` seconds, relative to the
    /// centre: a spinning point 16 out plus a pulse along normal `i`.
    fn swarm_offset(&self, i: usize, ltime: f32) -> Vec3f {
        let rates = self.avelocities[i];
        let angle = ltime * rates.x;
        let (sy, cy) = (f64::from(angle).sin() as f32, f64::from(angle).cos() as f32);
        let angle = ltime * rates.y;
        let (sp, cp) = (f64::from(angle).sin() as f32, f64::from(angle).cos() as f32);
        let forward = Vec3f::new(cp * cy, cp * sy, -sp);
        let dist = f64::from(ltime + i as f32).sin() as f32 * 64.0;
        bytedirs()[i] * dist + forward * 16.0
    }

    /// Up to `count` flies (every other normal) buzzing round `origin`
    /// for one frame. C ref: `CL_FlyParticles`.
    pub fn fly_particles(
        &mut self,
        origin: Vec3f,
        count: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let count = count.min(NUM_VERTEX_NORMALS as i32);
        self.swarm_rates(randk);
        let ltime = time as f32 / 1000.0;
        for i in (0..count.max(0) as usize).step_by(2) {
            let offset = self.swarm_offset(i, ltime);
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.org = origin + offset;
            p.vel = Vec3f::ZERO;
            p.accel = Vec3f::ZERO;
            p.color = 0;
            p.alpha = 1.0;
            p.alphavel = -100.0;
        }
    }

    /// Flies that gather over 20 s, stay, and leave over the last 20 s of a
    /// minute; `fly_stoptime` is C `centity_t.fly_stoptime`.
    /// C ref: `CL_FlyEffect`.
    pub fn fly_effect(
        &mut self,
        origin: Vec3f,
        fly_stoptime: &mut i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let starttime = if *fly_stoptime < time {
            *fly_stoptime = time + 60000;
            time
        } else {
            *fly_stoptime - 60000
        };
        // C: (int)n * 162 / 20000.0, an int product divided in double.
        let ramp = |n: i32| (f64::from(n * 162) / 20000.0) as i32;
        let n = time - starttime;
        let count = if n < 20000 {
            ramp(n)
        } else {
            let n = *fly_stoptime - time;
            if n < 20000 {
                ramp(n)
            } else {
                162
            }
        };
        self.fly_particles(origin, count, time, randk);
    }

    /// The glow round a flying BFG ball, greener and fainter further out,
    /// for one frame. C ref: `CL_BfgParticles`.
    pub fn bfg_particles(&mut self, origin: Vec3f, time: i32, randk: &mut impl FnMut() -> u32) {
        self.swarm_rates(randk);
        let ltime = (f64::from(time as f32) / 1000.0) as f32;
        for i in 0..NUM_VERTEX_NORMALS {
            let offset = self.swarm_offset(i, ltime);
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.org = origin + offset;
            p.vel = Vec3f::ZERO;
            p.accel = Vec3f::ZERO;
            let dist = (p.org - origin).length() / 90.0;
            p.color = (0xd0 as f32 + dist * 7.0).floor() as i32;
            p.alpha = 1.0 - dist;
            p.alphavel = -100.0;
        }
    }

    /// A player teleporting in: a 33x33x49 box of particles bursting out.
    /// C ref: `CL_TeleportParticles`.
    pub fn teleport_particles(&mut self, org: Vec3f, time: i32, randk: &mut impl FnMut() -> u32) {
        for i in (-16..=16).step_by(4) {
            for j in (-16..=16).step_by(4) {
                for k in (-16..=32).step_by(4) {
                    let Some(p) = self.alloc() else { return };
                    p.time = time as f32;
                    p.color = 7 + (randk() & 7) as i32;
                    p.alpha = 1.0;
                    p.alphavel = -1.0 / (0.3 + (randk() & 7) as f32 * 0.02);
                    p.org.x = org.x + i as f32 + (randk() & 3) as f32;
                    p.org.y = org.y + j as f32 + (randk() & 3) as f32;
                    p.org.z = org.z + k as f32 + (randk() & 3) as f32;
                    // C crosses i and j here.
                    let mut dir = Vec3f::new(j as f32 * 8.0, i as f32 * 8.0, k as f32 * 8.0);
                    vector_normalize(&mut dir);
                    p.vel = dir * (50 + (randk() & 63)) as f32;
                    p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::particles::Particles;
    use q2_shared::constants::{EF_GIB, EF_GREENGIB, EF_GRENADE, MZ_LOGIN, MZ_LOGOUT, MZ_RESPAWN};

    fn rng(values: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    fn colours(particles: &Particles) -> Vec<i32> {
        particles.active.iter().map(|p| p.color).collect()
    }

    #[test]
    fn teleporter_pads_rise_eight_green_sparks_a_frame() {
        let mut ps = Particles::default();
        ps.teleporter_particles(Vec3f::new(0.0, 0.0, 100.0), 0, &mut rng(&[7]));
        assert_eq!(ps.len(), 8);
        let p = ps.active[0];
        assert_eq!(p.color, 0xdb);
        assert_eq!(p.org, Vec3f::new(-9.0, -9.0, 99.0));
        assert_eq!((p.vel.z, p.alphavel), (87.0, -0.5));
    }

    #[test]
    fn login_logout_and_respawn_burst_in_their_colours() {
        let mut ps = Particles::default();
        for (kind, base) in [(MZ_LOGIN, 0xd0), (MZ_LOGOUT, 0x40), (MZ_RESPAWN, 0xe0)] {
            ps.clear();
            ps.logout_effect(Vec3f::ZERO, kind, 0, &mut rng(&[1]));
            assert_eq!(ps.len(), 500);
            assert_eq!(ps.active[0].color, base + 1);
        }
    }

    #[test]
    fn an_item_respawn_is_64_slow_falling_green_sparks() {
        let mut ps = Particles::default();
        ps.item_respawn_particles(Vec3f::ZERO, 0, &mut rng(&[2]));
        assert_eq!(ps.len(), 64);
        assert_eq!(ps.active[0].color, 0xd6);
        assert_eq!(ps.active[0].accel.z, -PARTICLE_GRAVITY * 0.2);
    }

    #[test]
    fn explosions_throw_256_particles_with_integer_spread() {
        let mut ps = Particles::default();
        ps.explosion_particles(Vec3f::ZERO, 0, &mut rng(&[40]));
        assert_eq!(ps.len(), 256);
        let p = ps.active[0];
        assert_eq!(p.color, 0xe0);
        // (40 % 32) - 16 and (40 % 384) - 192.
        assert_eq!((p.org.x, p.vel.x), (-8.0, -152.0));
        ps.clear();
        ps.bfg_explosion_particles(Vec3f::ZERO, 0, &mut rng(&[40]));
        assert_eq!((ps.len(), ps.active[0].color), (256, 0xd0));
    }

    #[test]
    fn a_big_teleport_fills_the_pool_from_its_colour_table() {
        let mut ps = Particles::default();
        ps.big_teleport_particles(Vec3f::ZERO, 0, &mut rng(&[1, 0]));
        assert_eq!(ps.len(), 4096);
        assert!(colours(&ps).iter().all(|c| [16, 104, 168, 144].contains(c)));
        assert_eq!(ps.active[0].accel.z, PARTICLE_GRAVITY * 4.0);
    }

    #[test]
    fn blaster_impacts_spray_40_along_the_surface_normal() {
        let mut ps = Particles::default();
        ps.blaster_particles(Vec3f::ZERO, Vec3f::Z, 0, &mut rng(&[16383]));
        assert_eq!(ps.len(), 40);
        // crandk(16383) is just under 0: the velocity is dir * 30.
        assert!((ps.active[0].vel.z - 30.0).abs() < 0.01);
    }

    #[test]
    fn a_blaster_trail_drops_a_particle_every_five_units() {
        let mut ps = Particles::default();
        ps.blaster_trail(
            Vec3f::ZERO,
            Vec3f::new(20.0, 0.0, 0.0),
            0,
            &mut rng(&[16383]),
        );
        assert_eq!(ps.len(), 4);
        let xs: Vec<f32> = ps.active.iter().map(|p| p.org.x.round()).collect();
        assert_eq!(xs, [0.0, 5.0, 10.0, 15.0]);
        assert_eq!(colours(&ps), [0xe0; 4]);
    }

    #[test]
    fn a_diminishing_trail_thins_out_by_five_per_half_unit() {
        let mut ps = Particles::default();
        let mut trailcount = 1024;
        // randk 1000 & 1023 = 1000: kept while trailcount > 1000.
        let end = Vec3f::new(10.0, 0.0, 0.0);
        ps.diminishing_trail(
            Vec3f::ZERO,
            end,
            &mut trailcount,
            EF_GRENADE,
            0,
            &mut rng(&[1000]),
        );
        // The int length drops by one per step: 10 steps of half a unit.
        assert_eq!(trailcount, 974);
        assert_eq!(ps.len(), 5, "kept for trailcount 1024..=1004");
        assert_eq!(ps.active[0].color, 4, "4 + (1000 & 7)");
        assert_eq!(ps.active[0].accel.z, 20.0, "smoke rises");
        // orgscale 4 while trailcount was over 900 at the start.
        assert_eq!(ps.active[4].org.x, 2.0 + crandk(&mut rng(&[1000])) * 4.0);
    }

    #[test]
    fn gib_trails_bleed_and_trailcount_bottoms_out_at_100() {
        let mut ps = Particles::default();
        let mut trailcount = 102;
        let end = Vec3f::new(3.0, 0.0, 0.0);
        ps.diminishing_trail(Vec3f::ZERO, end, &mut trailcount, EF_GIB, 0, &mut rng(&[0]));
        assert_eq!(trailcount, 100);
        assert_eq!(ps.len(), 3);
        let p = ps.active[0];
        assert_eq!(p.color, 0xe8);
        assert_eq!(p.accel, Vec3f::ZERO);
        // velscale 5: crandk(0) * 5 - gravity.
        assert_eq!(p.vel.z, -5.0 - PARTICLE_GRAVITY);
    }

    #[test]
    fn green_gibs_bleed_green_unless_they_are_also_red_gibs() {
        let mut ps = Particles::default();
        let end = Vec3f::new(3.0, 0.0, 0.0);
        let mut trailcount = 1024;
        let green = EF_GRENADE | EF_GREENGIB;
        ps.diminishing_trail(Vec3f::ZERO, end, &mut trailcount, green, 0, &mut rng(&[0]));
        let p = ps.active[0];
        assert_eq!((p.color, p.accel), (0xdb, Vec3f::ZERO));
        assert_eq!(p.vel.z, -15.0 - PARTICLE_GRAVITY);
        ps.clear();
        let both = EF_GIB | EF_GREENGIB;
        ps.diminishing_trail(Vec3f::ZERO, end, &mut 1024, both, 0, &mut rng(&[0]));
        assert_eq!(ps.active[0].color, 0xe8);
    }

    #[test]
    fn a_rocket_trail_adds_fire_to_its_smoke() {
        let mut ps = Particles::default();
        let mut trailcount = 1024;
        let end = Vec3f::new(4.0, 0.0, 0.0);
        ps.rocket_trail(Vec3f::ZERO, end, &mut trailcount, 0, &mut rng(&[8]));
        // Smoke: 4 steps, all kept (8 < trailcount). Fire: 4 steps, each
        // kept as 8 & 7 == 0.
        assert_eq!(ps.len(), 8);
        assert_eq!(colours(&ps)[4..], [0xdc; 4]);
    }

    #[test]
    fn a_rail_trail_is_a_spiral_and_a_core_a_particle_per_unit() {
        let mut ps = Particles::default();
        ps.rail_trail(Vec3f::ZERO, Vec3f::new(10.0, 0.0, 0.0), 0, &mut rng(&[3]));
        assert_eq!(ps.len(), 20);
        let spiral = ps.active[0];
        assert_eq!(spiral.color, 0x74 + 3);
        // Along +x the spiral starts on `right` (MakeNormalVectors), 3 out.
        assert!((spiral.org.length() - 3.0).abs() < 1e-5);
        assert_eq!(spiral.org.x, 0.0);
        assert_eq!(ps.active[10].color, 3);
        let core_jitter = crandk(&mut rng(&[3])) * 3.0;
        assert_eq!(ps.active[11].org.x, 0.75 + core_jitter, "core steps 0.75");
    }

    #[test]
    fn bubbles_trail_every_32_units_and_rise() {
        let mut ps = Particles::default();
        ps.bubble_trail(
            Vec3f::ZERO,
            Vec3f::new(65.0, 0.0, 0.0),
            0,
            &mut rng(&[16383]),
        );
        assert_eq!(ps.len(), 3);
        assert!((ps.active[2].org.x - 64.0).abs() < 2.1);
        assert!((ps.active[0].vel.z - 6.0).abs() < 0.01);
    }

    #[test]
    fn fly_particles_take_every_other_normal_and_last_one_frame() {
        let mut ps = Particles::default();
        ps.fly_particles(Vec3f::ZERO, 500, 0, &mut rng(&[1]));
        assert_eq!(ps.len(), 81, "count is capped at 162");
        assert_eq!(ps.active[0].alphavel, -100.0);
        // At time 0 the swarm sits 16 out along forward (1, 0, 0).
        assert_eq!(ps.active[0].org, Vec3f::new(16.0, 0.0, 0.0));
        assert_eq!(ps.avelocities.len(), 162);
    }

    #[test]
    fn flies_swarm_in_over_20_seconds_and_out_over_the_last_20() {
        let mut ps = Particles::default();
        let mut stop = 0;
        ps.fly_effect(Vec3f::ZERO, &mut stop, 1000, &mut rng(&[1]));
        assert_eq!((stop, ps.len()), (61000, 0), "a new swarm starts empty");
        ps.clear();
        ps.fly_effect(Vec3f::ZERO, &mut stop, 11000, &mut rng(&[1]));
        assert_eq!(ps.len(), 41, "half of 162, every other one");
        ps.clear();
        ps.fly_effect(Vec3f::ZERO, &mut stop, 31000, &mut rng(&[1]));
        assert_eq!(ps.len(), 81);
        ps.clear();
        ps.fly_effect(Vec3f::ZERO, &mut stop, 51000, &mut rng(&[1]));
        assert_eq!(ps.len(), 41);
    }

    #[test]
    fn a_bfg_ball_glows_greener_and_fainter_further_out() {
        let mut ps = Particles::default();
        ps.bfg_particles(Vec3f::ZERO, 0, &mut rng(&[1]));
        assert_eq!(ps.len(), 162);
        // At time 0 every particle is 16 out: dist 16 / 90.
        let p = ps.active[0];
        let dist = 16.0f32 / 90.0;
        assert_eq!(p.color, (0xd0 as f32 + dist * 7.0).floor() as i32);
        assert!((p.alpha - (1.0 - dist)).abs() < 1e-6);
    }

    #[test]
    fn a_teleport_is_a_box_of_1053_particles_bursting_outwards() {
        let mut ps = Particles::default();
        ps.teleport_particles(Vec3f::ZERO, 0, &mut rng(&[0]));
        assert_eq!(ps.len(), 9 * 9 * 13);
        let p = ps.active[0];
        assert_eq!((p.color, p.org), (7, Vec3f::new(-16.0, -16.0, -16.0)));
        assert!((p.vel.length() - 50.0).abs() < 1e-4);
    }
}
