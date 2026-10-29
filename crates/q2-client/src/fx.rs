//! Client-side effects the browser draws: temp entities (impacts,
//! explosions, beams, lasers), entity events and the trails and lights of
//! moving entities. C ref: client/cl_tempentities.c, `CL_EntityEvent`
//! (client/cl_effects.c), `CL_FireEntityEvents` (client/cl_parse.c) and
//! the trail part of `CL_AddPacketEntities` (client/cl_entities.c). Base
//! game only.
//!
//! Per frame the browser calls, in C's order: `temp_entity` for each
//! svc_temp_entity, `entity_event` once per entity of each new server
//! frame, `entity_trail` for each drawn entity, then `frame`.

use std::collections::HashMap;
use std::f64::consts::PI;

use q2_common::error::Q2Result;
use q2_common::net_msg::NetMsg;
use q2_shared::constants::*;
use q2_shared::protocol::SvcOp;
use q2_shared::types::{EntityState, Vec3f};

use crate::entities::breaks_lerp;
use crate::particles::{frandk, vector_normalize, DrawParticle, Particles, Spray};
use crate::sound::ATTN_STATIC;

/// C `MAX_EXPLOSIONS`.
pub const MAX_EXPLOSIONS: usize = 64;
/// C `MAX_BEAMS`.
pub const MAX_BEAMS: usize = 64;
/// C `MAX_LASERS`.
pub const MAX_LASERS: usize = 64;

/// shared.h values q2-shared does not carry.
pub const EF_FLIES: u32 = 0x0000_4000;
pub const RF_NOSHADOW: i32 = 8192;
pub const EV_FOOTSTEP: i32 = 2;
pub const EV_FALLSHORT: i32 = 3;
pub const EV_FALL: i32 = 4;
pub const EV_FALLFAR: i32 = 5;
pub const CHAN_AUTO: i32 = 0;
pub const CHAN_WEAPON: i32 = 1;
pub const CHAN_BODY: i32 = 4;
pub const ATTN_NONE: f32 = 0.0;
pub const ATTN_NORM: f32 = 1.0;
pub const ATTN_IDLE: f32 = 2.0;

/// The temp-entity models (C `CL_RegisterTEntModels`, base game).
pub const MOD_EXPLODE: &str = "models/objects/explode/tris.md2";
pub const MOD_SMOKE: &str = "models/objects/smoke/tris.md2";
pub const MOD_FLASH: &str = "models/objects/flash/tris.md2";
pub const MOD_PARASITE_SEGMENT: &str = "models/monsters/parasite/segment/tris.md2";
pub const MOD_EXPLO4: &str = "models/objects/r_explode/tris.md2";
pub const MOD_BFG_EXPLO: &str = "sprites/s_bfg2.sp2";
/// Every model the effects draw, in C `CL_RegisterTEntModels` order.
pub const FX_MODELS: [&str; 6] = [
    MOD_EXPLODE,
    MOD_SMOKE,
    MOD_FLASH,
    MOD_PARASITE_SEGMENT,
    MOD_EXPLO4,
    MOD_BFG_EXPLO,
];

/// TE_SPLASH colours by `SPLASH_*` (C `splash_color`).
const SPLASH_COLOR: [i32; 7] = [0x00, 0xe0, 0xb0, 0x50, 0xd0, 0xe0, 0xe8];
/// C `cl_sfx_footsteps`.
const FOOTSTEPS: [&str; 4] = [
    "player/step1.wav",
    "player/step2.wav",
    "player/step3.wav",
    "player/step4.wav",
];
/// EF_BFG light by server frame (C `bfg_lightramp`).
const BFG_LIGHTRAMP: [f32; 6] = [300.0, 400.0, 600.0, 300.0, 150.0, 75.0];

/// A sound an effect starts (C `S_StartSound`): at a fixed `origin` (temp
/// entities, entity 0), or following `entnum` when `origin` is `None`
/// (entity events). A name starting with `*` is a sexed player sound.
#[derive(Debug, Clone, PartialEq)]
pub struct FxSound {
    pub name: &'static str,
    pub origin: Option<Vec3f>,
    pub entnum: i32,
    pub channel: i32,
    pub volume: f32,
    pub attenuation: f32,
    pub timeofs: f32,
}

impl FxSound {
    fn at(origin: Vec3f, name: &'static str, attenuation: f32) -> Self {
        Self {
            name,
            origin: Some(origin),
            entnum: 0,
            channel: 0,
            volume: 1.0,
            attenuation,
            timeofs: 0.0,
        }
    }

    fn on(entnum: i32, channel: i32, name: &'static str, attenuation: f32) -> Self {
        Self {
            name,
            origin: None,
            entnum,
            channel,
            volume: 1.0,
            attenuation,
            timeofs: 0.0,
        }
    }
}

/// A model to draw (C `entity_t`). A laser has no model: it is an
/// `RF_BEAM` from `origin` to `oldorigin`, `frame` wide, in palette colour
/// `skinnum`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FxEntity {
    pub model: Option<&'static str>,
    pub origin: Vec3f,
    pub oldorigin: Vec3f,
    pub angles: Vec3f,
    pub frame: i32,
    pub oldframe: i32,
    /// 0 draws `frame`, 1 draws `oldframe`.
    pub backlerp: f32,
    pub alpha: f32,
    /// `RF_*` flags.
    pub flags: i32,
    pub skinnum: i32,
}

/// A light for this frame only (C `V_AddLight`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxLight {
    pub origin: Vec3f,
    pub intensity: f32,
    pub color: [f32; 3],
}

/// What the effects draw this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FxFrame {
    pub particles: Vec<DrawParticle>,
    pub entities: Vec<FxEntity>,
    pub lights: Vec<FxLight>,
}

/// The trail state C keeps per entity (`centity_t.trailcount` and
/// `fly_stoptime`). Start a fresh one whenever the entity was not in the
/// previous server frame, as C `CL_DeltaEntity` resets `trailcount`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrailState {
    pub trailcount: i32,
    pub fly_stoptime: i32,
}

impl Default for TrailState {
    fn default() -> Self {
        Self {
            trailcount: 1024,
            fly_stoptime: 0,
        }
    }
}

/// C `exptype_t`, the kinds the base game makes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ExKind {
    #[default]
    Free,
    Misc,
    Flash,
    Poly,
}

/// C `explosion_t`.
#[derive(Debug, Clone, Copy, Default)]
struct Explosion {
    kind: ExKind,
    ent: FxEntity,
    frames: i32,
    light: f32,
    lightcolor: [f32; 3],
    /// Client ms (C float).
    start: f32,
    baseframe: i32,
}

/// C `beam_t` (no `offset`: the base game's beams never set it).
#[derive(Debug, Clone, Copy, Default)]
struct Beam {
    entity: i32,
    model: Option<&'static str>,
    endtime: i32,
    start: Vec3f,
    end: Vec3f,
}

/// C `laser_t`.
#[derive(Debug, Clone, Copy, Default)]
struct Laser {
    ent: FxEntity,
    endtime: i32,
}

/// The client's effects: particles, explosions, beams and lasers.
#[derive(Debug, Clone)]
pub struct ClientFx {
    pub particles: Particles,
    explosions: [Explosion; MAX_EXPLOSIONS],
    beams: [Beam; MAX_BEAMS],
    lasers: [Laser; MAX_LASERS],
    /// C `cl_footsteps` (default 1).
    pub footsteps: bool,
}

impl Default for ClientFx {
    fn default() -> Self {
        Self {
            particles: Particles::default(),
            explosions: [Explosion::default(); MAX_EXPLOSIONS],
            beams: [Beam::default(); MAX_BEAMS],
            lasers: [Laser::default(); MAX_LASERS],
            footsteps: true,
        }
    }
}

fn read_pos(msg: &mut NetMsg) -> Vec3f {
    let mut pos = Vec3f::ZERO;
    msg.read_position(&mut pos);
    pos
}

impl ClientFx {
    /// Drop every effect on a map change (C `CL_ClearParticles` and
    /// `CL_ClearTEnts`). Keeps `footsteps` and the swarm rates, which C
    /// keeps in a static.
    pub fn clear(&mut self) {
        self.particles.clear();
        self.explosions = [Explosion::default(); MAX_EXPLOSIONS];
        self.beams = [Beam::default(); MAX_BEAMS];
        self.lasers = [Laser::default(); MAX_LASERS];
    }

    /// A free explosion slot, else the oldest. C ref: `CL_AllocExplosion`.
    fn alloc_explosion(&mut self, time: i32) -> &mut Explosion {
        let index = self
            .explosions
            .iter()
            .position(|ex| ex.kind == ExKind::Free)
            .unwrap_or_else(|| {
                let (mut oldest, mut index) = (time as f32, 0);
                for (i, ex) in self.explosions.iter().enumerate() {
                    if ex.start < oldest {
                        oldest = ex.start;
                        index = i;
                    }
                }
                index
            });
        let ex = &mut self.explosions[index];
        *ex = Explosion::default();
        ex
    }

    /// The puff of smoke and muzzle-like flash of a bullet impact.
    /// C ref: `CL_SmokeAndFlash`.
    fn smoke_and_flash(&mut self, origin: Vec3f, time: i32, servertime: i32) {
        let ex = self.alloc_explosion(time);
        ex.ent.origin = origin;
        ex.kind = ExKind::Misc;
        ex.frames = 4;
        ex.ent.flags = RF_TRANSLUCENT;
        ex.start = servertime as f32 - 100.0;
        ex.ent.model = Some(MOD_SMOKE);
        let ex = self.alloc_explosion(time);
        ex.ent.origin = origin;
        ex.kind = ExKind::Flash;
        ex.ent.flags = RF_FULLBRIGHT;
        ex.frames = 2;
        ex.start = servertime as f32 - 100.0;
        ex.ent.model = Some(MOD_FLASH);
    }

    /// A flash's particles: a monster gun's sparks
    /// (`CL_ParticleEffect(origin, vec3_origin, 0, 40)`) and
    /// `CL_SmokeAndFlash`, or a player's `CL_LogoutEffect`.
    pub fn flash_particles(
        &mut self,
        origin: Vec3f,
        kind: crate::effects::FlashParticles,
        time: i32,
        servertime: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        use crate::effects::FlashParticles;
        match kind {
            FlashParticles::Logout(weapon) => {
                self.particles.logout_effect(origin, weapon, time, randk);
                return;
            }
            FlashParticles::SparksAndSmoke => {
                let spray = Spray {
                    org: origin,
                    dir: Vec3f::ZERO,
                    color: 0,
                    count: 40,
                };
                self.particles.particle_effect(spray, time, randk);
            }
            FlashParticles::Smoke => {}
        }
        self.smoke_and_flash(origin, time, servertime);
    }

    /// A model explosion (C's `ex_poly` cases): `light` in orange unless
    /// the caller recolours it, starting one server frame back.
    fn poly_explosion(
        &mut self,
        origin: Vec3f,
        model: &'static str,
        time: i32,
        servertime: i32,
    ) -> &mut Explosion {
        let ex = self.alloc_explosion(time);
        ex.ent.origin = origin;
        ex.kind = ExKind::Poly;
        ex.ent.flags = RF_FULLBRIGHT | RF_NOSHADOW;
        ex.start = servertime as f32 - 100.0;
        ex.light = 350.0;
        ex.lightcolor = [1.0, 0.5, 0.5];
        ex.ent.model = Some(model);
        ex
    }

    /// Run one svc_temp_entity message (`bytes` starts with the svc op) at
    /// client time `time` and server frame time `servertime` (C
    /// `cl.frame.servertime`; explosions start 100 ms before it). Returns
    /// the sounds to start. Other messages, non-base types and malformed
    /// bodies (C `ERR_DROP`) do nothing. C ref: `CL_ParseTEnt`.
    pub fn temp_entity(
        &mut self,
        bytes: &[u8],
        time: i32,
        servertime: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Vec<FxSound> {
        let mut msg = NetMsg::from_bytes(bytes);
        msg.begin_reading();
        if SvcOp::try_from(msg.read_byte() as u8) != Ok(SvcOp::TempEntity) {
            return Vec::new();
        }
        self.parse_tent(&mut msg, time, servertime, randk)
            .unwrap_or_default()
    }

    fn parse_tent(
        &mut self,
        msg: &mut NetMsg,
        time: i32,
        servertime: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Q2Result<Vec<FxSound>> {
        let kind = msg.read_byte();
        let mut sounds = Vec::new();
        let spray = |org, dir, color, count| Spray {
            org,
            dir,
            color,
            count,
        };
        match kind {
            TE_BLOOD => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                self.particles
                    .particle_effect(spray(pos, dir, 0xe8, 60), time, randk);
            }
            TE_GUNSHOT | TE_SPARKS | TE_BULLET_SPARKS => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let (color, count) = if kind == TE_GUNSHOT {
                    (0, 40)
                } else {
                    (0xe0, 6)
                };
                self.particles
                    .particle_effect(spray(pos, dir, color, count), time, randk);
                if kind != TE_SPARKS {
                    self.smoke_and_flash(pos, time, servertime);
                    let ricochet = match randk() & 15 {
                        1 => Some("world/ric1.wav"),
                        2 => Some("world/ric2.wav"),
                        3 => Some("world/ric3.wav"),
                        _ => None,
                    };
                    sounds.extend(ricochet.map(|name| FxSound::at(pos, name, ATTN_NORM)));
                }
            }
            TE_SCREEN_SPARKS | TE_SHIELD_SPARKS => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let color = if kind == TE_SCREEN_SPARKS { 0xd0 } else { 0xb0 };
                self.particles
                    .particle_effect(spray(pos, dir, color, 40), time, randk);
                // cl_limitsparksounds defaults to 0: every spark is heard.
                sounds.push(FxSound::at(pos, "weapons/lashit.wav", ATTN_NORM));
            }
            TE_SHOTGUN => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                self.particles
                    .particle_effect(spray(pos, dir, 0, 20), time, randk);
                self.smoke_and_flash(pos, time, servertime);
            }
            TE_SPLASH => {
                let count = msg.read_byte();
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let r = msg.read_byte();
                // C indexes past the table for a negative r; colour 0 here.
                let color = usize::try_from(r)
                    .ok()
                    .and_then(|r| SPLASH_COLOR.get(r))
                    .copied()
                    .unwrap_or(0);
                self.particles
                    .particle_effect(spray(pos, dir, color, count), time, randk);
                if r == SPLASH_SPARKS {
                    let name = match randk() & 3 {
                        0 => "world/spark5.wav",
                        1 => "world/spark6.wav",
                        _ => "world/spark7.wav",
                    };
                    sounds.push(FxSound::at(pos, name, ATTN_STATIC));
                }
            }
            TE_LASER_SPARKS => {
                let count = msg.read_byte();
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let color = msg.read_byte();
                self.particles
                    .particle_effect2(spray(pos, dir, color, count), time, randk);
            }
            TE_BLASTER => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                self.particles.blaster_particles(pos, dir, time, randk);
                let ex = self.alloc_explosion(time);
                ex.ent.origin = pos;
                ex.ent.angles.x = (f64::from(f64::from(dir.z).acos() as f32) / PI * 180.0) as f32;
                ex.ent.angles.y = if dir.x != 0.0 {
                    (f64::from(f64::from(dir.y).atan2(f64::from(dir.x)) as f32) / PI * 180.0) as f32
                } else if dir.y > 0.0 {
                    90.0
                } else if dir.y < 0.0 {
                    270.0
                } else {
                    0.0
                };
                ex.kind = ExKind::Misc;
                ex.ent.flags = 0;
                ex.start = servertime as f32 - 100.0;
                ex.light = 150.0;
                ex.lightcolor = [1.0, 1.0, 0.0];
                ex.ent.model = Some(MOD_EXPLODE);
                ex.frames = 4;
                sounds.push(FxSound::at(pos, "weapons/lashit.wav", ATTN_NORM));
            }
            TE_RAILTRAIL => {
                let (start, end) = (read_pos(msg), read_pos(msg));
                self.particles.rail_trail(start, end, time, randk);
                sounds.push(FxSound::at(end, "weapons/railgf1a.wav", ATTN_NORM));
            }
            TE_EXPLOSION2 | TE_GRENADE_EXPLOSION | TE_GRENADE_EXPLOSION_WATER => {
                let pos = read_pos(msg);
                let ex = self.poly_explosion(pos, MOD_EXPLO4, time, servertime);
                ex.frames = 19;
                ex.baseframe = 30;
                ex.ent.angles.y = (randk() % 360) as f32;
                self.particles.explosion_particles(pos, time, randk);
                let name = if kind == TE_GRENADE_EXPLOSION_WATER {
                    "weapons/xpld_wat.wav"
                } else {
                    "weapons/grenlx1a.wav"
                };
                sounds.push(FxSound::at(pos, name, ATTN_NORM));
            }
            TE_EXPLOSION1 | TE_ROCKET_EXPLOSION | TE_ROCKET_EXPLOSION_WATER => {
                let pos = read_pos(msg);
                let ex = self.poly_explosion(pos, MOD_EXPLO4, time, servertime);
                ex.ent.angles.y = (randk() % 360) as f32;
                if frandk(randk) < 0.5 {
                    ex.baseframe = 15;
                }
                ex.frames = 15;
                self.particles.explosion_particles(pos, time, randk);
                let name = if kind == TE_ROCKET_EXPLOSION_WATER {
                    "weapons/xpld_wat.wav"
                } else {
                    "weapons/rocklx1a.wav"
                };
                sounds.push(FxSound::at(pos, name, ATTN_NORM));
            }
            TE_BFG_EXPLOSION => {
                let pos = read_pos(msg);
                let ex = self.poly_explosion(pos, MOD_BFG_EXPLO, time, servertime);
                ex.lightcolor = [0.0, 1.0, 0.0];
                ex.ent.flags |= RF_TRANSLUCENT;
                ex.ent.alpha = 0.30;
                ex.frames = 4;
            }
            TE_BFG_BIGEXPLOSION => {
                let pos = read_pos(msg);
                self.particles.bfg_explosion_particles(pos, time, randk);
            }
            TE_BFG_LASER => self.parse_laser(msg, 0xd0d1_d2d3, time, randk),
            TE_BUBBLETRAIL => {
                let (start, end) = (read_pos(msg), read_pos(msg));
                self.particles.bubble_trail(start, end, time, randk);
            }
            TE_PARASITE_ATTACK | TE_MEDIC_CABLE_ATTACK => {
                self.parse_beam(msg, MOD_PARASITE_SEGMENT, time);
            }
            TE_BOSSTPORT => {
                let pos = read_pos(msg);
                self.particles.big_teleport_particles(pos, time, randk);
                sounds.push(FxSound::at(pos, "misc/bigtele.wav", ATTN_NONE));
            }
            _ => {}
        }
        Ok(sounds)
    }

    /// A beam from entity `ent` (start, end) that lasts 200 ms, replacing
    /// that entity's beam. A full list drops it (C "beam list overflow").
    /// C ref: `CL_ParseBeam`.
    fn parse_beam(&mut self, msg: &mut NetMsg, model: &'static str, time: i32) {
        let entity = msg.read_short();
        let (start, end) = (read_pos(msg), read_pos(msg));
        let slot = self
            .beams
            .iter()
            .position(|b| b.entity == entity)
            .or_else(|| {
                self.beams
                    .iter()
                    .position(|b| b.model.is_none() || b.endtime < time)
            });
        if let Some(slot) = slot {
            self.beams[slot] = Beam {
                entity,
                model: Some(model),
                endtime: time + 200,
                start,
                end,
            };
        }
    }

    /// A 100 ms translucent laser in one of the four palette bytes of
    /// `colors`. C ref: `CL_ParseLaser`.
    fn parse_laser(
        &mut self,
        msg: &mut NetMsg,
        colors: u32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) {
        let (start, end) = (read_pos(msg), read_pos(msg));
        if let Some(laser) = self.lasers.iter_mut().find(|l| l.endtime < time) {
            laser.ent = FxEntity {
                flags: RF_TRANSLUCENT | RF_BEAM,
                origin: start,
                oldorigin: end,
                alpha: 0.30,
                skinnum: ((colors >> ((randk() % 4) * 8)) & 0xff) as i32,
                model: None,
                frame: 4,
                ..laser.ent
            };
            laser.endtime = time + 100;
        }
    }

    /// An entity's event, once per server frame it arrives in (C
    /// `CL_FireEntityEvents`): its sound and particles, plus the sparks of
    /// an `EF_TELEPORTER` pad. Returns the sounds, which follow the
    /// entity. C ref: `CL_EntityEvent`, `CL_TeleporterParticles`.
    pub fn entity_event(
        &mut self,
        ent: &EntityState,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Vec<FxSound> {
        let n = ent.number;
        let sound = match ent.event {
            EV_ITEM_RESPAWN => {
                self.particles
                    .item_respawn_particles(ent.origin, time, randk);
                Some(FxSound::on(n, CHAN_WEAPON, "items/respawn1.wav", ATTN_IDLE))
            }
            EV_PLAYER_TELEPORT => {
                self.particles.teleport_particles(ent.origin, time, randk);
                Some(FxSound::on(n, CHAN_WEAPON, "misc/tele1.wav", ATTN_IDLE))
            }
            EV_FOOTSTEP if self.footsteps => {
                let step = FOOTSTEPS[(randk() & 3) as usize];
                Some(FxSound::on(n, CHAN_BODY, step, ATTN_NORM))
            }
            EV_FALLSHORT => Some(FxSound::on(n, CHAN_AUTO, "player/land1.wav", ATTN_NORM)),
            EV_FALL => Some(FxSound::on(n, CHAN_AUTO, "*fall2.wav", ATTN_NORM)),
            EV_FALLFAR => Some(FxSound::on(n, CHAN_AUTO, "*fall1.wav", ATTN_NORM)),
            _ => None,
        };
        if ent.effects & EF_TELEPORTER != 0 {
            self.particles.teleporter_particles(ent.origin, time, randk);
        }
        sound.into_iter().collect()
    }

    /// The trail `ent`'s effects leave from `lerp_origin` (where it was
    /// drawn last frame, C `cent->lerp_origin`) to `origin` (where it is
    /// drawn now), and the light C adds with `V_AddLight`. Call it each
    /// frame for every drawn entity except the local player (C skips
    /// that and modelindex 0). C ref: `CL_AddPacketEntities`.
    pub fn entity_trail(
        &mut self,
        ent: &EntityState,
        lerp_origin: Vec3f,
        origin: Vec3f,
        trail: &mut TrailState,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Option<FxLight> {
        let effects = ent.effects;
        let light = |intensity, color| {
            Some(FxLight {
                origin,
                intensity,
                color,
            })
        };
        let ps = &mut self.particles;
        if effects & !EF_ROTATE == 0 {
            None
        } else if effects & EF_ROCKET != 0 {
            ps.rocket_trail(lerp_origin, origin, &mut trail.trailcount, time, randk);
            // cl_r1q2_lightstyle's default 1 colour.
            light(200.0, [1.0, 0.23, 0.0])
        } else if effects & EF_BLASTER != 0 {
            ps.blaster_trail(lerp_origin, origin, time, randk);
            light(200.0, [1.0, 1.0, 0.0])
        } else if effects & EF_HYPERBLASTER != 0 {
            light(200.0, [1.0, 1.0, 0.0])
        } else if effects & (EF_GIB | EF_GRENADE) != 0 {
            ps.diminishing_trail(
                lerp_origin,
                origin,
                &mut trail.trailcount,
                effects,
                time,
                randk,
            );
            None
        } else if effects & EF_FLIES != 0 {
            ps.fly_effect(origin, &mut trail.fly_stoptime, time, randk);
            None
        } else if effects & EF_BFG != 0 {
            let intensity = if effects & EF_ANIM_ALLFAST != 0 {
                ps.bfg_particles(origin, time, randk);
                200.0
            } else {
                // C reads past the ramp for other frames; no light here.
                *usize::try_from(ent.frame)
                    .ok()
                    .and_then(|f| BFG_LIGHTRAMP.get(f))?
            };
            light(intensity, [0.0, 1.0, 0.0])
        } else {
            None
        }
    }

    /// Advance everything to client time `time` and return what to draw:
    /// beams, explosions and lasers, then particles, as `CL_AddTEnts` and
    /// `CL_AddParticles` add them. `lerpfrac` is C `cl.lerpfrac`; a beam
    /// from entity `player` (C `cl.playernum + 1`) starts 22 units below
    /// `vieworg`.
    pub fn frame(
        &mut self,
        time: i32,
        lerpfrac: f32,
        player: i32,
        vieworg: Vec3f,
        randk: &mut impl FnMut() -> u32,
    ) -> FxFrame {
        let mut out = FxFrame::default();
        self.add_beams(time, player, vieworg, randk, &mut out.entities);
        self.add_explosions(time, lerpfrac, &mut out);
        out.entities.extend(
            self.lasers
                .iter()
                .filter(|l| l.endtime >= time)
                .map(|l| l.ent),
        );
        out.particles = self.particles.add_particles(time);
        out
    }

    /// One model per 30 units along each live beam, facing along it with
    /// a random roll. C ref: `CL_AddBeams`.
    fn add_beams(
        &mut self,
        time: i32,
        player: i32,
        vieworg: Vec3f,
        randk: &mut impl FnMut() -> u32,
        out: &mut Vec<FxEntity>,
    ) {
        const MODEL_LENGTH: f32 = 30.0;
        for b in self.beams.iter_mut() {
            let Some(model) = b.model.filter(|_| b.endtime >= time) else {
                continue;
            };
            if b.entity == player {
                b.start = vieworg;
                b.start.z -= 22.0; // adjust for view height
            }
            let mut org = b.start;
            let mut dist = b.end - org;
            let (pitch, yaw) = beam_angles(dist);
            let mut d = vector_normalize(&mut dist);
            let steps = (d / MODEL_LENGTH).ceil();
            let len = (d - MODEL_LENGTH) / (steps - 1.0);
            while d > 0.0 {
                out.push(FxEntity {
                    model: Some(model),
                    origin: org,
                    angles: Vec3f::new(pitch, yaw, (randk() % 360) as f32),
                    ..FxEntity::default()
                });
                org += dist * len;
                d -= MODEL_LENGTH;
            }
        }
    }

    /// Animate and fade the explosions, freeing finished ones, with their
    /// lights. C ref: `CL_AddExplosions`.
    fn add_explosions(&mut self, time: i32, lerpfrac: f32, out: &mut FxFrame) {
        for ex in self.explosions.iter_mut() {
            if ex.kind == ExKind::Free {
                continue;
            }
            let frac = (f64::from(time as f32 - ex.start) / 100.0) as f32;
            let f = frac.floor() as i32;
            let ent = &mut ex.ent;
            match ex.kind {
                ExKind::Misc if f >= ex.frames - 1 => ex.kind = ExKind::Free,
                ExKind::Misc => ent.alpha = 1.0 - frac / (ex.frames - 1) as f32,
                ExKind::Flash if f >= 1 => ex.kind = ExKind::Free,
                ExKind::Flash => ent.alpha = 1.0,
                ExKind::Poly if f >= ex.frames - 1 => ex.kind = ExKind::Free,
                ExKind::Poly => {
                    ent.alpha = (16.0 - f as f32) / 16.0;
                    if f < 10 {
                        ent.skinnum = (f >> 1).max(0);
                    } else {
                        ent.flags |= RF_TRANSLUCENT;
                        ent.skinnum = if f < 13 { 5 } else { 6 };
                    }
                }
                ExKind::Free => {}
            }
            if ex.kind == ExKind::Free {
                continue;
            }
            if ex.light != 0.0 {
                out.lights.push(FxLight {
                    origin: ent.origin,
                    intensity: ex.light * ent.alpha,
                    color: ex.lightcolor,
                });
            }
            ent.oldorigin = ent.origin;
            let f = f.max(0);
            ent.frame = ex.baseframe + f + 1;
            ent.oldframe = ex.baseframe + f;
            ent.backlerp = 1.0 - lerpfrac;
            out.entities.push(*ent);
        }
    }
}

/// The pitch and yaw (degrees, 0..360) of a beam along `dist`, as
/// `CL_AddBeams` computes them.
fn beam_angles(dist: Vec3f) -> (f32, f32) {
    if dist.y == 0.0 && dist.x == 0.0 {
        return (if dist.z > 0.0 { 90.0 } else { 270.0 }, 0.0);
    }
    let mut yaw = if dist.x != 0.0 {
        (f64::from(f64::from(dist.y).atan2(f64::from(dist.x)) as f32 * 180.0) / PI) as f32
    } else if dist.y > 0.0 {
        90.0
    } else {
        270.0
    };
    if yaw < 0.0 {
        yaw += 360.0;
    }
    let forward = (dist.x * dist.x + dist.y * dist.y).sqrt();
    let mut pitch =
        (f64::from(f64::from(dist.z).atan2(f64::from(forward)) as f32) * -180.0 / PI) as f32;
    if pitch < 0.0 {
        pitch += 360.0;
    }
    (pitch, yaw)
}

/// The per-entity state C keeps in `centity_t` for trails: the
/// `TrailState`, the origin the entity lerps from (`prev.origin`) and
/// where it was drawn last frame (`lerp_origin`), by entity number.
#[derive(Debug, Clone, Default)]
pub struct EntityTrails {
    entities: HashMap<i32, (TrailState, Vec3f, Vec3f)>,
}

impl EntityTrails {
    /// Take a new server frame (C `CL_DeltaEntity`): an entity that is new
    /// or does not lerp from `previous` starts a fresh trail at its
    /// `old_origin`, or at its origin after `EV_OTHER_TELEPORT`; the rest
    /// lerp from their previous origin. Entities gone from `current` are
    /// forgotten.
    pub fn server_frame(&mut self, current: &[EntityState], previous: &[EntityState]) {
        self.entities
            .retain(|n, _| current.binary_search_by_key(n, |s| s.number).is_ok());
        for s in current {
            let prev = previous
                .binary_search_by_key(&s.number, |p| p.number)
                .ok()
                .map(|i| &previous[i])
                .filter(|prev| !breaks_lerp(prev, s));
            match (prev, self.entities.get_mut(&s.number)) {
                (Some(prev), Some(entry)) => entry.1 = prev.origin,
                _ => {
                    let start = if s.event == EV_OTHER_TELEPORT {
                        s.origin
                    } else {
                        s.old_origin
                    };
                    self.entities
                        .insert(s.number, (TrailState::default(), start, start));
                }
            }
        }
    }

    /// Each rendered frame (C `CL_AddPacketEntities`): every entity's trail
    /// from where it was drawn last frame to where it is drawn now, lerped
    /// by `lerpfrac`, and the lights they add. The local `player` (C
    /// `cl.playernum + 1`) and entities without a model have none.
    pub fn run(
        &mut self,
        fx: &mut ClientFx,
        current: &[EntityState],
        lerpfrac: f32,
        player: i32,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Vec<FxLight> {
        let mut lights = Vec::new();
        for s in current
            .iter()
            .filter(|s| s.modelindex != 0 && s.number != player)
        {
            let Some((trail, prev_origin, lerp_origin)) = self.entities.get_mut(&s.number) else {
                continue;
            };
            let origin = *prev_origin + (s.origin - *prev_origin) * lerpfrac;
            lights.extend(fx.entity_trail(s, *lerp_origin, origin, trail, time, randk));
            *lerp_origin = origin;
        }
        lights
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2_common::net_msg::NetMsg;
    use q2_shared::protocol::SvcOp;

    fn rng(values: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    /// An svc_temp_entity message: the type, then `fill` writes its body.
    fn tent(kind: i32, fill: impl FnOnce(&mut NetMsg)) -> Vec<u8> {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::TempEntity as i32);
        msg.write_byte(kind);
        fill(&mut msg);
        msg.data().to_vec()
    }

    fn pos_dir(kind: i32, pos: Vec3f, dir: Vec3f) -> Vec<u8> {
        tent(kind, |m| {
            m.write_position(pos);
            m.write_dir(dir);
        })
    }

    fn pos_only(kind: i32, pos: Vec3f) -> Vec<u8> {
        tent(kind, |m| m.write_position(pos))
    }

    const P: Vec3f = Vec3f::new(64.0, 32.0, 8.0);

    fn names(sounds: &[FxSound]) -> Vec<&str> {
        sounds.iter().map(|s| s.name).collect()
    }

    fn models(frame: &FxFrame) -> Vec<Option<&str>> {
        frame.entities.iter().map(|e| e.model).collect()
    }

    #[test]
    fn a_gunshot_puffs_smokes_flashes_and_may_ricochet() {
        let mut fx = ClientFx::default();
        // Every randk() is 1: the impact sound draw (& 15) picks ric1.
        let sounds = fx.temp_entity(
            &pos_dir(TE_GUNSHOT, P, Vec3f::Z),
            1000,
            1000,
            &mut rng(&[1]),
        );
        assert_eq!(fx.particles.len(), 40);
        assert_eq!(names(&sounds), ["world/ric1.wav"]);
        let s = &sounds[0];
        assert_eq!((s.origin, s.entnum, s.attenuation), (Some(P), 0, ATTN_NORM));
        // The client clock runs up to one frame behind servertime; the
        // flash lasts the first 100 ms from servertime - 100.
        let frame = fx.frame(950, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(models(&frame), [Some(MOD_SMOKE), Some(MOD_FLASH)]);
        assert_eq!(frame.entities[0].flags, RF_TRANSLUCENT);
        assert_eq!(frame.entities[1].flags, RF_FULLBRIGHT);
        assert!(frame.lights.is_empty());
    }

    #[test]
    fn most_impacts_are_silent_and_sparks_leave_no_smoke() {
        let mut fx = ClientFx::default();
        let sounds = fx.temp_entity(
            &pos_dir(TE_GUNSHOT, P, Vec3f::Z),
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert!(sounds.is_empty(), "randk & 15 == 0 has no ricochet");
        fx.clear();
        let sounds = fx.temp_entity(&pos_dir(TE_SPARKS, P, Vec3f::Z), 1000, 1000, &mut rng(&[1]));
        assert!(sounds.is_empty());
        assert_eq!(fx.particles.len(), 6);
        assert_eq!(fx.particles.active[0].color, 0xe1);
        assert!(fx
            .frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .is_empty());
        fx.clear();
        let sounds = fx.temp_entity(
            &pos_dir(TE_BULLET_SPARKS, P, Vec3f::Z),
            1000,
            1000,
            &mut rng(&[2]),
        );
        assert_eq!(names(&sounds), ["world/ric2.wav"]);
        assert_eq!(
            fx.frame(950, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
                .entities
                .len(),
            2
        );
    }

    #[test]
    fn blood_shotgun_and_screen_sparks_spray_their_colours() {
        let mut fx = ClientFx::default();
        assert!(fx
            .temp_entity(&pos_dir(TE_BLOOD, P, Vec3f::Z), 1000, 1000, &mut rng(&[0]))
            .is_empty());
        assert_eq!(
            (fx.particles.len(), fx.particles.active[0].color),
            (60, 0xe8)
        );
        fx.clear();
        let sounds = fx.temp_entity(
            &pos_dir(TE_SHOTGUN, P, Vec3f::Z),
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert!(sounds.is_empty());
        assert_eq!(fx.particles.len(), 20);
        assert_eq!(
            fx.frame(950, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
                .entities
                .len(),
            2
        );
        for (kind, color) in [(TE_SCREEN_SPARKS, 0xd0), (TE_SHIELD_SPARKS, 0xb0)] {
            fx.clear();
            let sounds = fx.temp_entity(&pos_dir(kind, P, Vec3f::Z), 1000, 1000, &mut rng(&[0]));
            assert_eq!(names(&sounds), ["weapons/lashit.wav"]);
            assert_eq!(
                (fx.particles.len(), fx.particles.active[0].color),
                (40, color)
            );
        }
    }

    #[test]
    fn a_blaster_impact_sprays_flashes_a_light_and_faces_the_normal() {
        let mut fx = ClientFx::default();
        let dir = Vec3f::new(0.0, 1.0, 0.0);
        let sounds = fx.temp_entity(&pos_dir(TE_BLASTER, P, dir), 1000, 1000, &mut rng(&[0]));
        assert_eq!(names(&sounds), ["weapons/lashit.wav"]);
        assert_eq!(fx.particles.len(), 40);
        // Starts one server frame back: 100 ms in, frame 1 of 4.
        let frame = fx.frame(1000, 0.25, 1, Vec3f::ZERO, &mut rng(&[0]));
        let ex = &frame.entities[0];
        assert_eq!(ex.model, Some(MOD_EXPLODE));
        assert_eq!(ex.angles, Vec3f::new(90.0, 90.0, 0.0));
        assert_eq!((ex.frame, ex.oldframe, ex.backlerp), (2, 1, 0.75));
        assert_eq!(ex.oldorigin, P);
        let alpha = 1.0 - 1.0 / 3.0;
        assert!((ex.alpha - alpha).abs() < 1e-6);
        let light = frame.lights[0];
        assert_eq!((light.origin, light.color), (P, [1.0, 1.0, 0.0]));
        assert!((light.intensity - 150.0 * alpha).abs() < 1e-4);
    }

    #[test]
    fn explosions_age_out_after_their_frames() {
        let mut fx = ClientFx::default();
        fx.temp_entity(
            &pos_dir(TE_BLASTER, P, Vec3f::Z),
            1000,
            1000,
            &mut rng(&[0]),
        );
        // ex_misc lives while floor(frac) < frames - 1 = 3: until 1200 ms.
        assert_eq!(
            fx.frame(1199, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
                .entities
                .len(),
            1
        );
        assert!(fx
            .frame(1200, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .is_empty());
        // The smoke and flash of a gunshot: the flash goes after 100 ms.
        fx.temp_entity(
            &pos_dir(TE_SHOTGUN, P, Vec3f::Z),
            2000,
            2000,
            &mut rng(&[0]),
        );
        let frame = fx.frame(2050, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(models(&frame), [Some(MOD_SMOKE)]);
    }

    #[test]
    fn a_rocket_explosion_burns_through_its_skins_and_lights_orange() {
        let mut fx = ClientFx::default();
        // randk 0: no yaw, frandk 0 < 0.5 picks the second animation.
        let sounds = fx.temp_entity(
            &pos_only(TE_ROCKET_EXPLOSION, P),
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert_eq!(names(&sounds), ["weapons/rocklx1a.wav"]);
        assert_eq!(fx.particles.len(), 256);
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        let ex = frame.entities[0];
        assert_eq!(ex.model, Some(MOD_EXPLO4));
        assert_eq!((ex.frame, ex.oldframe, ex.skinnum), (17, 16, 0));
        assert_eq!(ex.flags, RF_FULLBRIGHT | RF_NOSHADOW);
        assert_eq!(ex.alpha, 15.0 / 16.0);
        assert_eq!(frame.lights[0].color, [1.0, 0.5, 0.5]);
        assert_eq!(frame.lights[0].intensity, 350.0 * 15.0 / 16.0);
        let late = fx.frame(2100, 1.0, 1, Vec3f::ZERO, &mut rng(&[0])).entities[0];
        assert_eq!(late.skinnum, 5);
        assert_ne!(
            late.flags & RF_TRANSLUCENT,
            0,
            "fades translucent from frame 10"
        );
        let last = fx.frame(2200, 1.0, 1, Vec3f::ZERO, &mut rng(&[0])).entities[0];
        assert_eq!(last.skinnum, 6);
        assert!(fx
            .frame(2500, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .is_empty());
        let water = fx.temp_entity(
            &pos_only(TE_ROCKET_EXPLOSION_WATER, P),
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert_eq!(names(&water), ["weapons/xpld_wat.wav"]);
    }

    #[test]
    fn grenade_explosions_play_the_long_animation() {
        let mut fx = ClientFx::default();
        // randk 40000: yaw 40000 % 360 = 40.
        let sounds = fx.temp_entity(
            &pos_only(TE_GRENADE_EXPLOSION, P),
            1000,
            1000,
            &mut rng(&[40000]),
        );
        assert_eq!(names(&sounds), ["weapons/grenlx1a.wav"]);
        let ex = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0])).entities[0];
        assert_eq!((ex.frame, ex.angles.y), (32, 40.0));
        let water = fx.temp_entity(
            &pos_only(TE_GRENADE_EXPLOSION_WATER, P),
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert_eq!(names(&water), ["weapons/xpld_wat.wav"]);
        let two = fx.temp_entity(&pos_only(TE_EXPLOSION2, P), 1000, 1000, &mut rng(&[0]));
        assert_eq!(names(&two), ["weapons/grenlx1a.wav"]);
    }

    #[test]
    fn bfg_explosions_show_a_green_sprite_and_a_big_burst() {
        let mut fx = ClientFx::default();
        assert!(fx
            .temp_entity(&pos_only(TE_BFG_EXPLOSION, P), 1000, 1000, &mut rng(&[0]))
            .is_empty());
        assert_eq!(fx.particles.len(), 0);
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        let ex = frame.entities[0];
        assert_eq!(ex.model, Some(MOD_BFG_EXPLO));
        assert_ne!(ex.flags & RF_TRANSLUCENT, 0);
        assert_eq!(frame.lights[0].color, [0.0, 1.0, 0.0]);
        fx.temp_entity(
            &pos_only(TE_BFG_BIGEXPLOSION, P),
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert_eq!(fx.particles.len(), 256);
        assert_eq!(fx.particles.active[0].color, 0xd0);
    }

    #[test]
    fn the_explosion_pool_replaces_the_oldest_when_full() {
        let mut fx = ClientFx::default();
        for i in 0..MAX_EXPLOSIONS as i32 {
            let at = Vec3f::new(i as f32, 0.0, 0.0);
            fx.temp_entity(
                &pos_only(TE_BFG_EXPLOSION, at),
                1000,
                1000 + i,
                &mut rng(&[0]),
            );
        }
        let newest = Vec3f::new(-1.0, 0.0, 0.0);
        fx.temp_entity(
            &pos_only(TE_BFG_EXPLOSION, newest),
            1100,
            1100,
            &mut rng(&[0]),
        );
        let origins: Vec<Vec3f> = fx
            .frame(1100, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .iter()
            .map(|e| e.origin)
            .collect();
        assert_eq!(origins.len(), MAX_EXPLOSIONS);
        assert_eq!(origins[0], newest, "slot 0 started first");
    }

    #[test]
    fn a_rail_trail_sounds_at_its_far_end() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(64.0, 0.0, 0.0);
        let msg = tent(TE_RAILTRAIL, |m| {
            m.write_position(Vec3f::ZERO);
            m.write_position(end);
        });
        let sounds = fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        assert_eq!(names(&sounds), ["weapons/railgf1a.wav"]);
        assert_eq!(sounds[0].origin, Some(end));
        assert_eq!(fx.particles.len(), 128);
    }

    #[test]
    fn splashes_take_their_colour_and_sparks_crackle() {
        let mut fx = ClientFx::default();
        let splash = |count: i32, kind: i32| {
            tent(TE_SPLASH, |m| {
                m.write_byte(count);
                m.write_position(P);
                m.write_dir(Vec3f::Z);
                m.write_byte(kind);
            })
        };
        let sounds = fx.temp_entity(&splash(8, SPLASH_BLUE_WATER), 1000, 1000, &mut rng(&[0]));
        assert!(sounds.is_empty());
        assert_eq!(
            (fx.particles.len(), fx.particles.active[0].color),
            (8, 0xb0)
        );
        fx.clear();
        let sounds = fx.temp_entity(&splash(4, SPLASH_SPARKS), 1000, 1000, &mut rng(&[1]));
        assert_eq!(names(&sounds), ["world/spark6.wav"]);
        assert_eq!(sounds[0].attenuation, crate::sound::ATTN_STATIC);
        fx.clear();
        fx.temp_entity(&splash(4, 9), 1000, 1000, &mut rng(&[0]));
        assert_eq!(
            fx.particles.active[0].color, 0,
            "past the table is colour 0"
        );
    }

    #[test]
    fn laser_sparks_and_bubble_trails_spray_as_sent() {
        let mut fx = ClientFx::default();
        let msg = tent(TE_LASER_SPARKS, |m| {
            m.write_byte(10);
            m.write_position(P);
            m.write_dir(Vec3f::Z);
            m.write_byte(0x70);
        });
        assert!(fx.temp_entity(&msg, 1000, 1000, &mut rng(&[2])).is_empty());
        assert_eq!(
            (fx.particles.len(), fx.particles.active[0].color),
            (10, 0x72)
        );
        fx.clear();
        let msg = tent(TE_BUBBLETRAIL, |m| {
            m.write_position(Vec3f::ZERO);
            m.write_position(Vec3f::new(96.0, 0.0, 0.0));
        });
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        assert_eq!(fx.particles.len(), 3);
    }

    #[test]
    fn a_boss_teleport_is_heard_everywhere() {
        let mut fx = ClientFx::default();
        let sounds = fx.temp_entity(&pos_only(TE_BOSSTPORT, P), 1000, 1000, &mut rng(&[0]));
        assert_eq!(names(&sounds), ["misc/bigtele.wav"]);
        assert_eq!(sounds[0].attenuation, ATTN_NONE);
        assert_eq!(fx.particles.len(), 4096);
    }

    #[test]
    fn a_bfg_laser_is_a_translucent_beam_for_100_ms() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(0.0, 0.0, 256.0);
        let msg = tent(TE_BFG_LASER, |m| {
            m.write_position(P);
            m.write_position(end);
        });
        // randk 1: colour byte 1 of 0xd0d1d2d3.
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[1]));
        let frame = fx.frame(1100, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        let laser = frame.entities[0];
        assert_eq!(laser.model, None);
        assert_eq!(laser.flags, RF_TRANSLUCENT | RF_BEAM);
        assert_eq!((laser.origin, laser.oldorigin), (P, end));
        assert_eq!((laser.alpha, laser.frame, laser.skinnum), (0.3, 4, 0xd2));
        assert!(fx
            .frame(1101, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .is_empty());
    }

    fn beam(kind: i32, ent: i32, start: Vec3f, end: Vec3f) -> Vec<u8> {
        tent(kind, |m| {
            m.write_short(ent);
            m.write_position(start);
            m.write_position(end);
        })
    }

    #[test]
    fn a_parasite_beam_lays_segments_along_its_length() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(90.0, 0.0, 0.0);
        fx.temp_entity(
            &beam(TE_PARASITE_ATTACK, 5, Vec3f::ZERO, end),
            1000,
            1000,
            &mut rng(&[0]),
        );
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[370]));
        let xs: Vec<f32> = frame.entities.iter().map(|e| e.origin.x).collect();
        assert_eq!(xs, [0.0, 30.0, 60.0]);
        let seg = frame.entities[0];
        assert_eq!(seg.model, Some(MOD_PARASITE_SEGMENT));
        assert_eq!(
            seg.angles,
            Vec3f::new(0.0, 0.0, 10.0),
            "roll is randk % 360"
        );
        // A short beam is one segment; a straight-up beam pitches 90.
        let up = Vec3f::new(0.0, 0.0, 20.0);
        fx.temp_entity(
            &beam(TE_MEDIC_CABLE_ATTACK, 5, Vec3f::ZERO, up),
            1000,
            1000,
            &mut rng(&[0]),
        );
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(frame.entities.len(), 1, "entity 5's beam was replaced");
        assert_eq!(frame.entities[0].angles.x, 90.0);
        assert!(fx
            .frame(1201, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .is_empty());
    }

    #[test]
    fn the_players_own_beam_starts_below_the_view() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(100.0, 0.0, 78.0);
        fx.temp_entity(
            &beam(TE_PARASITE_ATTACK, 1, Vec3f::ZERO, end),
            1000,
            1000,
            &mut rng(&[0]),
        );
        let vieworg = Vec3f::new(10.0, 0.0, 100.0);
        let frame = fx.frame(1000, 1.0, 1, vieworg, &mut rng(&[0]));
        assert_eq!(frame.entities[0].origin, Vec3f::new(10.0, 0.0, 78.0));
        assert_eq!(frame.entities.len(), 3);
    }

    #[test]
    fn unknown_truncated_and_other_messages_do_nothing() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        assert!(fx
            .temp_entity(&pos_only(TE_FLECHETTE, P), 1000, 1000, &mut r)
            .is_empty());
        assert!(fx
            .temp_entity(&pos_only(200, P), 1000, 1000, &mut r)
            .is_empty());
        let bad_dir = tent(TE_BLOOD, |m| {
            m.write_position(P);
            m.write_byte(200);
        });
        assert!(fx.temp_entity(&bad_dir, 1000, 1000, &mut r).is_empty());
        assert!(fx
            .temp_entity(
                &[SvcOp::TempEntity as u8, TE_BLASTER as u8],
                1000,
                1000,
                &mut r
            )
            .is_empty());
        assert!(fx
            .temp_entity(&[SvcOp::TempEntity as u8], 1000, 1000, &mut r)
            .is_empty());
        assert!(fx.temp_entity(&[], 1000, 1000, &mut r).is_empty());
        let flash = [SvcOp::MuzzleFlash as u8, 1, 0, 0];
        assert!(fx.temp_entity(&flash, 1000, 1000, &mut r).is_empty());
        assert_eq!(fx.particles.len(), 0);
        let truncated_splash = tent(TE_SPLASH, |m| {
            m.write_byte(2);
            m.write_position(P);
            m.write_dir(Vec3f::Z);
        });
        fx.temp_entity(&truncated_splash, 1000, 1000, &mut r);
        assert_eq!(
            fx.particles.active[0].color, 0,
            "a missing splash type reads -1"
        );
    }

    fn event(number: i32, event: i32) -> EntityState {
        EntityState {
            number,
            event,
            origin: P,
            ..Default::default()
        }
    }

    #[test]
    fn item_respawns_and_teleports_sound_on_the_entity_and_sparkle() {
        let mut fx = ClientFx::default();
        let sounds = fx.entity_event(&event(3, EV_ITEM_RESPAWN), 1000, &mut rng(&[0]));
        assert_eq!(names(&sounds), ["items/respawn1.wav"]);
        let s = &sounds[0];
        assert_eq!(
            (s.origin, s.entnum, s.channel, s.attenuation),
            (None, 3, CHAN_WEAPON, ATTN_IDLE)
        );
        assert_eq!(fx.particles.len(), 64);
        fx.clear();
        let sounds = fx.entity_event(&event(3, EV_PLAYER_TELEPORT), 1000, &mut rng(&[0]));
        assert_eq!(names(&sounds), ["misc/tele1.wav"]);
        assert_eq!(fx.particles.len(), 1053);
    }

    #[test]
    fn footsteps_follow_cl_footsteps_and_falls_thud() {
        let mut fx = ClientFx::default();
        let sounds = fx.entity_event(&event(1, EV_FOOTSTEP), 1000, &mut rng(&[2]));
        assert_eq!(names(&sounds), ["player/step3.wav"]);
        assert_eq!(sounds[0].channel, CHAN_BODY);
        fx.footsteps = false;
        assert!(fx
            .entity_event(&event(1, EV_FOOTSTEP), 1000, &mut rng(&[2]))
            .is_empty());
        for (ev, name) in [
            (EV_FALLSHORT, "player/land1.wav"),
            (EV_FALL, "*fall2.wav"),
            (EV_FALLFAR, "*fall1.wav"),
        ] {
            let sounds = fx.entity_event(&event(1, ev), 1000, &mut rng(&[0]));
            assert_eq!(names(&sounds), [name]);
            assert_eq!(sounds[0].channel, CHAN_AUTO);
        }
        assert!(fx
            .entity_event(&event(1, EV_OTHER_TELEPORT), 1000, &mut rng(&[0]))
            .is_empty());
        assert_eq!(fx.particles.len(), 0);
    }

    #[test]
    fn a_teleporter_pad_sparkles_each_server_frame() {
        let mut fx = ClientFx::default();
        let pad = EntityState {
            effects: EF_TELEPORTER,
            ..event(9, 0)
        };
        assert!(fx.entity_event(&pad, 1000, &mut rng(&[0])).is_empty());
        assert_eq!(fx.particles.len(), 8);
    }

    fn mover(effects: u32) -> EntityState {
        EntityState {
            number: 7,
            effects,
            modelindex: 1,
            ..Default::default()
        }
    }

    const FROM: Vec3f = Vec3f::new(0.0, 0.0, 0.0);
    const TO: Vec3f = Vec3f::new(20.0, 0.0, 0.0);

    #[test]
    fn rockets_trail_smoke_and_fire_and_light_orange() {
        let mut fx = ClientFx::default();
        let mut trail = TrailState::default();
        let light = fx.entity_trail(
            &mover(EF_ROCKET),
            FROM,
            TO,
            &mut trail,
            1000,
            &mut rng(&[0]),
        );
        assert_eq!(
            light,
            Some(FxLight {
                origin: TO,
                intensity: 200.0,
                color: [1.0, 0.23, 0.0]
            })
        );
        assert_eq!(trail.trailcount, 1024 - 5 * 20);
        assert_eq!(fx.particles.len(), 40);
    }

    #[test]
    fn blasters_trail_and_hyperblasters_only_light() {
        let mut fx = ClientFx::default();
        let mut trail = TrailState::default();
        let yellow = Some(FxLight {
            origin: TO,
            intensity: 200.0,
            color: [1.0, 1.0, 0.0],
        });
        assert_eq!(
            fx.entity_trail(&mover(EF_BLASTER), FROM, TO, &mut trail, 0, &mut rng(&[0])),
            yellow
        );
        assert_eq!(fx.particles.len(), 4);
        fx.clear();
        assert_eq!(
            fx.entity_trail(
                &mover(EF_HYPERBLASTER),
                FROM,
                TO,
                &mut trail,
                0,
                &mut rng(&[0])
            ),
            yellow
        );
        assert_eq!(fx.particles.len(), 0);
    }

    #[test]
    fn gibs_and_grenades_trail_without_light() {
        let mut fx = ClientFx::default();
        for (effects, color) in [(EF_GIB, 0xe8), (EF_GRENADE, 4)] {
            fx.clear();
            let mut trail = TrailState::default();
            assert_eq!(
                fx.entity_trail(&mover(effects), FROM, TO, &mut trail, 0, &mut rng(&[0])),
                None
            );
            assert_eq!(
                (fx.particles.len(), fx.particles.active[0].color),
                (20, color)
            );
        }
    }

    #[test]
    fn flies_and_bfg_balls_swarm_and_bfg_lights_ramp_with_the_frame() {
        let mut fx = ClientFx::default();
        let mut trail = TrailState::default();
        assert_eq!(
            fx.entity_trail(&mover(EF_FLIES), FROM, TO, &mut trail, 1000, &mut rng(&[1])),
            None
        );
        assert_eq!(trail.fly_stoptime, 61000);
        let ball = mover(EF_BFG | EF_ANIM_ALLFAST);
        let light = fx
            .entity_trail(&ball, FROM, TO, &mut trail, 0, &mut rng(&[1]))
            .unwrap();
        assert_eq!((light.intensity, light.color), (200.0, [0.0, 1.0, 0.0]));
        assert_eq!(fx.particles.len(), 162);
        let blast = EntityState {
            frame: 2,
            ..mover(EF_BFG)
        };
        let light = fx
            .entity_trail(&blast, FROM, TO, &mut trail, 0, &mut rng(&[1]))
            .unwrap();
        assert_eq!(light.intensity, 600.0);
        let past = EntityState {
            frame: 6,
            ..mover(EF_BFG)
        };
        assert_eq!(
            fx.entity_trail(&past, FROM, TO, &mut trail, 0, &mut rng(&[1])),
            None
        );
    }

    #[test]
    fn plain_and_rotating_entities_have_no_trail() {
        let mut fx = ClientFx::default();
        let mut trail = TrailState::default();
        assert_eq!(
            fx.entity_trail(&mover(0), FROM, TO, &mut trail, 0, &mut rng(&[0])),
            None
        );
        assert_eq!(
            fx.entity_trail(&mover(EF_ROTATE), FROM, TO, &mut trail, 0, &mut rng(&[0])),
            None
        );
        assert_eq!(fx.particles.len(), 0);
    }

    #[test]
    fn clearing_for_a_new_map_keeps_the_swarm_rates_and_footsteps() {
        let mut fx = ClientFx {
            footsteps: false,
            ..ClientFx::default()
        };
        fx.particles.bfg_particles(Vec3f::ZERO, 0, &mut rng(&[1]));
        fx.temp_entity(&pos_only(TE_BFG_EXPLOSION, P), 1000, 1000, &mut rng(&[0]));
        let rates = fx.particles.avelocities.clone();
        fx.clear();
        assert_eq!(fx.particles.len(), 0);
        assert!(fx
            .frame(950, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .is_empty());
        // C's avelocities is a file static no clear touches.
        assert_eq!(fx.particles.avelocities, rates);
        assert!(!fx.footsteps);
    }

    #[test]
    fn a_frame_draws_new_particles_and_drops_faded_ones() {
        let mut fx = ClientFx::default();
        fx.temp_entity(&pos_dir(TE_BLOOD, P, Vec3f::Z), 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(frame.particles.len(), 60);
        assert!(frame.particles.iter().all(|p| p.alpha == 1.0));
        assert!(fx
            .frame(1500, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .particles
            .is_empty());
        assert_eq!(fx.particles.len(), 0);
    }

    fn rocket_at(number: i32, old_origin: Vec3f, origin: Vec3f) -> EntityState {
        EntityState {
            number,
            effects: EF_ROCKET,
            modelindex: 1,
            old_origin,
            origin,
            ..Default::default()
        }
    }

    #[test]
    fn a_new_entity_trails_from_its_old_origin_then_from_where_it_was_drawn() {
        let mut fx = ClientFx::default();
        let mut trails = EntityTrails::default();
        let first = [rocket_at(7, FROM, TO)];
        trails.server_frame(&first, &[]);
        // New: C lerps from old_origin, so half way is 10 units on.
        let lights = trails.run(&mut fx, &first, 0.5, 1, 1000, &mut rng(&[0]));
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].origin, Vec3f::new(10.0, 0.0, 0.0));
        assert_eq!(fx.particles.len(), 20, "10 units of rocket trail");
        // The next render frame trails only the 10 units since.
        trails.run(&mut fx, &first, 1.0, 1, 1010, &mut rng(&[0]));
        assert_eq!(fx.particles.len(), 40);
        // The next server frame lerps from the last one's origin.
        let second = [rocket_at(7, TO, Vec3f::new(40.0, 0.0, 0.0))];
        trails.server_frame(&second, &first);
        let lights = trails.run(&mut fx, &second, 0.5, 1, 1050, &mut rng(&[0]));
        assert_eq!(lights[0].origin, Vec3f::new(30.0, 0.0, 0.0));
    }

    #[test]
    fn a_trail_restarts_when_the_entity_teleports_or_reappears() {
        let mut fx = ClientFx::default();
        let mut trails = EntityTrails::default();
        let first = [rocket_at(7, FROM, TO)];
        trails.server_frame(&first, &[]);
        trails.run(&mut fx, &first, 1.0, 1, 1000, &mut rng(&[0]));
        let before = trails.entities[&7].0.trailcount;
        assert!(before < 1024);
        // EV_OTHER_TELEPORT starts from the new origin: no trail across.
        let far = Vec3f::new(2000.0, 0.0, 0.0);
        let teleported = [EntityState {
            event: EV_OTHER_TELEPORT,
            ..rocket_at(7, TO, far)
        }];
        trails.server_frame(&teleported, &first);
        assert_eq!(trails.entities[&7], (TrailState::default(), far, far));
        // Gone for a frame, then back: a fresh trail from its old_origin.
        trails.server_frame(&[], &teleported);
        assert!(trails.entities.is_empty());
        let back = [rocket_at(7, FROM, TO)];
        trails.server_frame(&back, &[]);
        assert_eq!(trails.entities[&7], (TrailState::default(), FROM, FROM));
    }

    #[test]
    fn the_local_player_and_modelless_entities_leave_no_trail() {
        let mut fx = ClientFx::default();
        let mut trails = EntityTrails::default();
        let frame = [
            rocket_at(1, FROM, TO),
            EntityState {
                modelindex: 0,
                ..rocket_at(2, FROM, TO)
            },
        ];
        trails.server_frame(&frame, &[]);
        let lights = trails.run(&mut fx, &frame, 1.0, 1, 1000, &mut rng(&[0]));
        assert!(lights.is_empty());
        assert_eq!(fx.particles.len(), 0);
    }

    #[test]
    fn a_login_flash_makes_the_logout_effect_at_the_player() {
        use crate::effects::FlashParticles;
        let mut fx = ClientFx::default();
        fx.flash_particles(
            P,
            FlashParticles::Logout(MZ_LOGIN),
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert_eq!(fx.particles.len(), 500);
    }

    #[test]
    fn monster_machineguns_spark_and_smoke_and_shotguns_only_smoke() {
        use crate::effects::FlashParticles;
        let mut fx = ClientFx::default();
        fx.flash_particles(
            P,
            FlashParticles::SparksAndSmoke,
            1000,
            1000,
            &mut rng(&[0]),
        );
        assert_eq!(fx.particles.len(), 40);
        // Both start a server frame back; the flash lasts that frame.
        let frame = fx.frame(950, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(models(&frame), [Some(MOD_SMOKE), Some(MOD_FLASH)]);
        let mut fx = ClientFx::default();
        fx.flash_particles(P, FlashParticles::Smoke, 1000, 1000, &mut rng(&[0]));
        assert_eq!(fx.particles.len(), 0);
        // Both start a server frame back; the flash lasts that frame.
        let frame = fx.frame(950, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(models(&frame), [Some(MOD_SMOKE), Some(MOD_FLASH)]);
    }
}
