//! The client particle pool and its generic sprays. C ref:
//! client/cl_particles.c. The effect-specific sprays of cl_effects.c are
//! in `particle_fx`.

use q2_shared::types::Vec3f;

/// C `MAX_PARTICLES` (vid/header/ref.h): the pool, and the most a frame
/// draws.
pub const MAX_PARTICLES: usize = 4096;
/// C `PARTICLE_GRAVITY`.
pub const PARTICLE_GRAVITY: f32 = 40.0;
/// C `INSTANT_PARTICLE`: an `alphavel` that draws a particle for one frame
/// at full alpha, then frees it.
pub const INSTANT_PARTICLE: f32 = -10000.0;

/// A live particle (C `cparticle_t`; `colorvel` is never read in C).
/// Motion is from `time` (client ms) on, so a particle needs no update.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Particle {
    pub time: f32,
    pub org: Vec3f,
    pub vel: Vec3f,
    pub accel: Vec3f,
    /// Palette index (C keeps it as a float).
    pub color: i32,
    pub alpha: f32,
    /// Alpha lost per second.
    pub alphavel: f32,
}

/// A particle to draw this frame (C `V_AddParticle`'s arguments).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawParticle {
    pub origin: Vec3f,
    /// Index into the 256-colour palette.
    pub color: u8,
    pub alpha: f32,
}

/// The `org, dir, color, count` arguments every C particle spray takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spray {
    pub org: Vec3f,
    pub dir: Vec3f,
    /// First palette index.
    pub color: i32,
    pub count: i32,
}

/// C `frandk`: 0..=1 from the low 15 bits, computed in double as C does.
pub fn frandk(randk: &mut impl FnMut() -> u32) -> f32 {
    (f64::from(randk() & 32767) * (1.0 / 32767.0)) as f32
}

/// C `crandk`: -1..=1 from the low 15 bits.
pub fn crandk(randk: &mut impl FnMut() -> u32) -> f32 {
    (f64::from(randk() & 32767) * (2.0 / 32767.0) - 1.0) as f32
}

/// C `(randk() & 7) - 4`: the -4..=3 jitter sprays add per axis.
pub(crate) fn jitter(randk: &mut impl FnMut() -> u32) -> f32 {
    (randk() & 7) as f32 - 4.0
}

/// C `VectorNormalize`: scale by the reciprocal of the length and return
/// the length; a zero vector stays zero.
pub(crate) fn vector_normalize(v: &mut Vec3f) -> f32 {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if length != 0.0 {
        *v *= 1.0 / length;
    }
    length
}

/// The particle pool (C `active_particles` / `free_particles` over
/// `particles[MAX_PARTICLES]`). A full pool refuses new particles until
/// `add_particles` frees faded ones, as C's empty free list does.
#[derive(Debug, Clone, Default)]
pub struct Particles {
    pub(crate) active: Vec<Particle>,
    /// C cl_effects.c's `avelocities`: per-normal spin rates for the fly
    /// and BFG swarms, drawn on first use (while the first is zero).
    pub(crate) avelocities: Vec<Vec3f>,
}

impl Particles {
    /// C `CL_ClearParticles`.
    pub fn clear(&mut self) {
        self.active.clear();
    }

    pub fn len(&self) -> usize {
        self.active.len()
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// Take a particle from the free list, or `None` when the pool is full
    /// (C `if (!free_particles) return;`). Every effect sets all fields.
    pub(crate) fn alloc(&mut self) -> Option<&mut Particle> {
        if self.active.len() >= MAX_PARTICLES {
            return None;
        }
        self.active.push(Particle::default());
        self.active.last_mut()
    }

    /// A puff around `org`, spread along `dir`, in colours `color..+7`,
    /// sinking a little slower than gravity. C ref: `CL_ParticleEffect`.
    pub fn particle_effect(&mut self, spray: Spray, time: i32, randk: &mut impl FnMut() -> u32) {
        for _ in 0..spray.count {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = spray.color + (randk() & 7) as i32;
            let d = (randk() & 31) as f32;
            for j in 0..3 {
                p.org[j] = spray.org[j] + jitter(randk) + d * spray.dir[j];
                p.vel[j] = crandk(randk) * 20.0;
            }
            p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY + 0.2);
            p.alpha = 1.0;
            p.alphavel = (-1.0 / (0.5 + f64::from(frandk(randk)) * 0.3)) as f32;
        }
    }

    /// As `particle_effect`, spread less and under full gravity (laser
    /// sparks). C ref: `CL_ParticleEffect2`.
    pub fn particle_effect2(&mut self, spray: Spray, time: i32, randk: &mut impl FnMut() -> u32) {
        self.short_spray(spray, true, -PARTICLE_GRAVITY, time, randk);
    }

    /// As `particle_effect2` in one colour, rising instead of falling.
    /// C ref: `CL_ParticleEffect3`.
    pub fn particle_effect3(&mut self, spray: Spray, time: i32, randk: &mut impl FnMut() -> u32) {
        self.short_spray(spray, false, PARTICLE_GRAVITY, time, randk);
    }

    /// The shared body of `CL_ParticleEffect2` and `CL_ParticleEffect3`.
    fn short_spray(
        &mut self,
        spray: Spray,
        colour_offset: bool,
        accel_z: f32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..spray.count {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = spray.color
                + if colour_offset {
                    (randk() & 7) as i32
                } else {
                    0
                };
            let d = (randk() & 7) as f32;
            for j in 0..3 {
                p.org[j] = spray.org[j] + jitter(randk) + d * spray.dir[j];
                p.vel[j] = crandk(randk) * 20.0;
            }
            p.accel = Vec3f::new(0.0, 0.0, accel_z);
            p.alpha = 1.0;
            p.alphavel = -1.0 / (0.5 + frandk(randk) * 0.3);
        }
    }

    /// A spray whose colour range (`color + (randk & numcolors)`), spread
    /// (`randk & dirspread`) and fade time are arguments.
    /// C ref: `CL_GenericParticleEffect`.
    pub fn generic_particle_effect(
        &mut self,
        spray: Spray,
        numcolors: i32,
        dirspread: i32,
        alphavel: f32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        for _ in 0..spray.count {
            let Some(p) = self.alloc() else { return };
            p.time = time as f32;
            p.color = spray.color
                + if numcolors > 1 {
                    randk() as i32 & numcolors
                } else {
                    0
                };
            let d = (randk() as i32 & dirspread) as f32;
            for j in 0..3 {
                p.org[j] = spray.org[j] + jitter(randk) + d * spray.dir[j];
                p.vel[j] = crandk(randk) * 20.0;
            }
            p.accel = Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY);
            p.alpha = 1.0;
            p.alphavel = -1.0 / (0.5 + frandk(randk) * alphavel);
        }
    }

    /// Free the particles that faded out by client time `time` (ms) and
    /// return the rest where they are now, newest first as C's active list
    /// runs. C ref: `CL_AddParticles`.
    pub fn add_particles(&mut self, time: i32) -> Vec<DrawParticle> {
        let mut drawn = Vec::with_capacity(self.active.len());
        self.active.retain_mut(|p| {
            let (t, alpha) = if p.alphavel != INSTANT_PARTICLE {
                let t = (f64::from(time as f32 - p.time) * 0.001) as f32;
                let alpha = p.alpha + t * p.alphavel;
                if alpha <= 0.0 {
                    return false; // faded out
                }
                (t, alpha)
            } else {
                (0.0, p.alpha)
            };
            drawn.push(DrawParticle {
                origin: p.org + p.vel * t + p.accel * (t * t),
                color: (p.color & 0xff) as u8,
                alpha: alpha.min(1.0),
            });
            if p.alphavel == INSTANT_PARTICLE {
                p.alphavel = 0.0;
                p.alpha = 0.0;
            }
            true
        });
        drawn.reverse();
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed randk() sequence, repeating.
    fn rng(values: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    fn spray(org: Vec3f, dir: Vec3f, color: i32, count: i32) -> Spray {
        Spray {
            org,
            dir,
            color,
            count,
        }
    }

    #[test]
    fn frandk_and_crandk_scale_the_low_15_bits_like_c() {
        assert_eq!(frandk(&mut rng(&[32767])), 1.0);
        assert_eq!(frandk(&mut rng(&[0x8000])), 0.0, "bit 15 is masked off");
        assert_eq!(crandk(&mut rng(&[0])), -1.0);
        assert_eq!(crandk(&mut rng(&[32767])), 1.0);
    }

    #[test]
    fn particle_effect_reads_randk_in_c_order() {
        let mut particles = Particles::default();
        // colour, d, then per axis an org jitter and a crandk velocity,
        // then the frandk fade.
        let mut r = rng(&[3, 10, 7, 32767, 0, 0, 4, 16383, 32767]);
        let dir = Vec3f::new(0.0, 0.0, 1.0);
        particles.particle_effect(
            spray(Vec3f::new(100.0, 0.0, 0.0), dir, 0xe8, 1),
            500,
            &mut r,
        );
        let p = particles.active[0];
        assert_eq!(p.color, 0xe8 + 3);
        assert_eq!(p.org, Vec3f::new(103.0, -4.0, 10.0));
        assert_eq!(p.vel.x, 20.0);
        assert_eq!(p.vel.y, -20.0);
        assert!(p.vel.z.abs() < 0.01);
        assert_eq!(p.accel, Vec3f::new(0.0, 0.0, -PARTICLE_GRAVITY + 0.2));
        assert_eq!((p.time, p.alpha), (500.0, 1.0));
        assert!((p.alphavel - (-1.0 / 0.8)).abs() < 1e-5);
    }

    #[test]
    fn particle_effect2_falls_and_effect3_rises_with_a_fixed_colour() {
        let mut particles = Particles::default();
        particles.particle_effect2(spray(Vec3f::ZERO, Vec3f::X, 0xd0, 2), 0, &mut rng(&[5]));
        assert_eq!(particles.len(), 2);
        assert_eq!(particles.active[0].color, 0xd5);
        assert_eq!(particles.active[0].accel.z, -PARTICLE_GRAVITY);
        particles.clear();
        particles.particle_effect3(spray(Vec3f::ZERO, Vec3f::X, 0xd0, 1), 0, &mut rng(&[5]));
        assert_eq!(particles.active[0].color, 0xd0);
        assert_eq!(particles.active[0].accel.z, PARTICLE_GRAVITY);
    }

    #[test]
    fn generic_effect_masks_colour_and_spread_with_its_arguments() {
        let mut particles = Particles::default();
        let mut r = rng(&[0xff]);
        particles.generic_particle_effect(
            spray(Vec3f::ZERO, Vec3f::Z, 0x40, 1),
            3,
            15,
            0.5,
            0,
            &mut r,
        );
        let p = particles.active[0];
        assert_eq!(p.color, 0x43);
        // d = 0xff & 15, plus the (0xff & 7) - 4 jitter.
        assert_eq!(p.org.z, 15.0 + 3.0);
        particles.clear();
        particles.generic_particle_effect(
            spray(Vec3f::ZERO, Vec3f::Z, 0x40, 1),
            1,
            15,
            0.5,
            0,
            &mut r,
        );
        assert_eq!(particles.active[0].color, 0x40, "one colour is not offset");
    }

    #[test]
    fn the_pool_stops_at_max_particles_until_a_frame_frees_some() {
        let mut particles = Particles::default();
        let mut r = rng(&[0]);
        particles.particle_effect(spray(Vec3f::ZERO, Vec3f::Z, 0, 5000), 0, &mut r);
        assert_eq!(particles.len(), MAX_PARTICLES);
        particles.particle_effect(spray(Vec3f::ZERO, Vec3f::Z, 0, 1), 0, &mut r);
        assert_eq!(particles.len(), MAX_PARTICLES);
        // alphavel is -1 / 0.5 = -2 with randk 0: gone after half a second.
        assert_eq!(particles.add_particles(500).len(), 0);
        assert_eq!(particles.len(), 0);
        particles.particle_effect(spray(Vec3f::ZERO, Vec3f::Z, 0, 1), 500, &mut r);
        assert_eq!(particles.len(), 1);
    }

    fn one(org: Vec3f, vel: Vec3f, accel: Vec3f, alphavel: f32) -> Particle {
        Particle {
            time: 1000.0,
            org,
            vel,
            accel,
            color: 0x1e0,
            alpha: 1.0,
            alphavel,
        }
    }

    #[test]
    fn particles_move_by_velocity_and_accel_times_t_squared() {
        let mut particles = Particles::default();
        particles.active.push(one(
            Vec3f::new(1.0, 2.0, 3.0),
            Vec3f::new(10.0, 0.0, 0.0),
            Vec3f::new(0.0, 0.0, -40.0),
            -0.5,
        ));
        let drawn = particles.add_particles(1500);
        // C adds accel * t * t, without the half.
        assert_eq!(drawn[0].origin, Vec3f::new(6.0, 2.0, -7.0));
        assert_eq!(drawn[0].alpha, 0.75);
        assert_eq!(drawn[0].color, 0xe0, "colour is a palette byte");
    }

    #[test]
    fn particles_fade_and_are_freed_at_zero_alpha_and_brighter_is_clamped() {
        let mut particles = Particles::default();
        let mut bright = one(Vec3f::ZERO, Vec3f::ZERO, Vec3f::ZERO, -1.0);
        bright.alpha = 3.0;
        particles.active.push(bright);
        particles
            .active
            .push(one(Vec3f::ZERO, Vec3f::ZERO, Vec3f::ZERO, -1.0));
        let drawn = particles.add_particles(1000);
        assert_eq!(drawn.len(), 2);
        assert!(drawn.iter().all(|p| p.alpha == 1.0));
        let drawn = particles.add_particles(2000);
        assert_eq!(drawn.len(), 1, "the fainter one faded out");
        assert_eq!(particles.len(), 1);
    }

    #[test]
    fn newest_particles_are_drawn_first_like_cs_active_list() {
        let mut particles = Particles::default();
        particles
            .active
            .push(one(Vec3f::X, Vec3f::ZERO, Vec3f::ZERO, -1.0));
        particles
            .active
            .push(one(Vec3f::Y, Vec3f::ZERO, Vec3f::ZERO, -1.0));
        let drawn = particles.add_particles(1000);
        assert_eq!(drawn[0].origin, Vec3f::Y);
    }

    #[test]
    fn an_instant_particle_draws_once_at_its_start_then_goes() {
        let mut particles = Particles::default();
        particles.active.push(one(
            Vec3f::ZERO,
            Vec3f::X * 100.0,
            Vec3f::ZERO,
            INSTANT_PARTICLE,
        ));
        let drawn = particles.add_particles(5000);
        assert_eq!(drawn[0].origin, Vec3f::ZERO, "time does not move it");
        assert_eq!(drawn[0].alpha, 1.0);
        assert!(particles.add_particles(5000).is_empty());
        assert_eq!(particles.len(), 0);
    }
}
