//! Client-side effects the browser draws: temp entities (impacts,
//! explosions, beams, lasers, sustained steam and blasts), entity events
//! and the trails and lights of moving entities. C ref:
//! client/cl_tempentities.c, `CL_EntityEvent` (client/cl_effects.c),
//! `CL_FireEntityEvents` (client/cl_parse.c) and the effect part of
//! `CL_AddPacketEntities` (client/cl_entities.c), for the base game and
//! the mission packs (rogue, xatrix, CTF).
//!
//! Per frame the browser calls, in C's order: `temp_entity` for each
//! svc_temp_entity, `entity_event` once per entity of each new server
//! frame, `EntityTrails::run` for the drawn entities' trails, then
//! `frame_view`.
//!
//! Not ported: C's `ex_explosion`, `ex_mflash` and `ex_poly2` explosion
//! kinds, which nothing in C creates. The lights of TE_FLASHLIGHT and
//! TE_TRACKER_EXPLOSION go to `effects::Effects`' dlights through
//! `take_dlights`, as C's share `cl_dlights`.

use std::collections::HashMap;
use std::f64::consts::PI;

use q2_common::error::Q2Result;
use q2_common::net_msg::NetMsg;
pub use q2_shared::constants::EF_FLIES;
use q2_shared::constants::*;
use q2_shared::protocol::SvcOp;
use q2_shared::types::{EntityState, Vec3f};

use crate::entities::{anglemod, breaks_lerp, EntityView};
use crate::particles::{frandk, vector_normalize, DrawParticle, Particles, Spray};
use crate::sound::{
    ATTN_IDLE, ATTN_NONE, ATTN_NORM, ATTN_STATIC, CHAN_AUTO, CHAN_BODY, CHAN_WEAPON,
};
use q2_common::pmove::angle_vectors;

/// C `MAX_EXPLOSIONS`.
pub const MAX_EXPLOSIONS: usize = 64;
/// C `MAX_BEAMS`, for the beams and again for the player beams.
pub const MAX_BEAMS: usize = 64;
/// C `MAX_LASERS`.
pub const MAX_LASERS: usize = 64;
/// C `MAX_SUSTAINS` (client/header/client.h).
pub const MAX_SUSTAINS: usize = 32;

/// The temp-entity models (C `CL_RegisterTEntModels`).
pub const MOD_EXPLODE: &str = "models/objects/explode/tris.md2";
pub const MOD_SMOKE: &str = "models/objects/smoke/tris.md2";
pub const MOD_FLASH: &str = "models/objects/flash/tris.md2";
pub const MOD_PARASITE_SEGMENT: &str = "models/monsters/parasite/segment/tris.md2";
pub const MOD_GRAPPLE_CABLE: &str = "models/ctf/segment/tris.md2";
/// Registered by C but drawn by nothing.
pub const MOD_PARASITE_TIP: &str = "models/monsters/parasite/tip/tris.md2";
pub const MOD_EXPLO4: &str = "models/objects/r_explode/tris.md2";
pub const MOD_BFG_EXPLO: &str = "sprites/s_bfg2.sp2";
/// The `EF_POWERSCREEN` shell (see `powerscreen_entity`).
pub const MOD_POWERSCREEN: &str = "models/items/armor/effect/tris.md2";
pub const MOD_EXPLO4_BIG: &str = "models/objects/r_explode2/tris.md2";
pub const MOD_LIGHTNING: &str = "models/proj/lightning/tris.md2";
pub const MOD_HEATBEAM: &str = "models/proj/beam/tris.md2";
/// Registered by C, which draws monster heat beams with `MOD_HEATBEAM`.
pub const MOD_MONSTER_HEATBEAM: &str = "models/proj/widowbeam/tris.md2";
/// Every model C registers for the effects, in C `CL_RegisterTEntModels`
/// order (without its precaches of weapon and gib models).
pub const FX_MODELS: [&str; 13] = [
    MOD_EXPLODE,
    MOD_SMOKE,
    MOD_FLASH,
    MOD_PARASITE_SEGMENT,
    MOD_GRAPPLE_CABLE,
    MOD_PARASITE_TIP,
    MOD_EXPLO4,
    MOD_BFG_EXPLO,
    MOD_POWERSCREEN,
    MOD_EXPLO4_BIG,
    MOD_LIGHTNING,
    MOD_HEATBEAM,
    MOD_MONSTER_HEATBEAM,
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

/// A light for this frame only (C `V_AddLight`). A negative colour
/// darkens (the tracker's lights).
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

/// The local view `frame_view` needs to place the local player's beams.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FxView {
    /// The local player's entity (C `cl.playernum + 1`).
    pub player: i32,
    /// C `cl.refdef.vieworg`.
    pub vieworg: Vec3f,
    /// The view weapon's offsets of the previous and current server frame
    /// (C `ops->gunoffset` and `ps->gunoffset`, as `CL_AddViewWeapon`
    /// takes them), lerped by `lerpfrac`.
    pub old_gunoffset: Vec3f,
    pub gunoffset: Vec3f,
    /// C `cl.v_forward`, `cl.v_right` and `cl.v_up`.
    pub forward: Vec3f,
    pub right: Vec3f,
    pub up: Vec3f,
    /// C cvar `hand`: 0 right, 1 left, 2 centre.
    pub hand: i32,
}

impl FxView {
    /// The view of `player` at `vieworg` facing `viewangles` (C
    /// `cl.v_forward` and the rest are `AngleVectors(cl.refdef.viewangles)`),
    /// with the gun offsets of the previous and newest frames.
    pub fn new(
        player: i32,
        vieworg: Vec3f,
        viewangles: Vec3f,
        (old_gunoffset, gunoffset): (Vec3f, Vec3f),
        hand: i32,
    ) -> Self {
        let (forward, right, up) = angle_vectors(viewangles);
        Self {
            player,
            vieworg,
            old_gunoffset,
            gunoffset,
            forward,
            right,
            up,
            hand,
        }
    }
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

/// C `exptype_t`, the kinds C creates.
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

/// C `beam_t`.
#[derive(Debug, Clone, Copy, Default)]
struct Beam {
    entity: i32,
    /// Lightning's target entity; other beams keep the slot's old value.
    dest_entity: i32,
    model: Option<&'static str>,
    endtime: i32,
    offset: Vec3f,
    start: Vec3f,
    end: Vec3f,
}

/// C `laser_t`.
#[derive(Debug, Clone, Copy, Default)]
struct Laser {
    ent: FxEntity,
    endtime: i32,
}

/// C `cl_sustain_t`'s `think`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SustainThink {
    /// C `CL_ParticleSteamEffect2`.
    #[default]
    Steam,
    /// C `CL_Widowbeamout`.
    Widow,
    /// C `CL_Nukeblast`.
    Nuke,
}

/// C `cl_sustain_t`: an effect that thinks each `thinkinterval` ms until
/// `endtime`. `id` 0 is a free slot.
#[derive(Debug, Clone, Copy, Default)]
struct Sustain {
    id: i32,
    endtime: i32,
    nextthink: i32,
    thinkinterval: i32,
    org: Vec3f,
    dir: Vec3f,
    color: i32,
    count: i32,
    magnitude: i32,
    think: SustainThink,
}

/// The client's effects: particles, explosions, beams, lasers and
/// sustains.
#[derive(Debug, Clone)]
pub struct ClientFx {
    pub particles: Particles,
    explosions: [Explosion; MAX_EXPLOSIONS],
    beams: [Beam; MAX_BEAMS],
    /// C `cl_playerbeams`: heat beams.
    player_beams: [Beam; MAX_BEAMS],
    lasers: [Laser; MAX_LASERS],
    sustains: [Sustain; MAX_SUSTAINS],
    /// Lights for `effects::Effects`' `cl_dlights` (C `CL_AllocDlight`).
    dlights: Vec<crate::effects::ClientDLight>,
    /// C `cl_footsteps` (default 1).
    pub footsteps: bool,
    /// Effects C does not draw, off by default (`?cl_fxextra=1`): dry
    /// explosions leave rising smoke (`explosion_smoke`).
    pub extras: bool,
}

impl Default for ClientFx {
    fn default() -> Self {
        Self {
            particles: Particles::default(),
            explosions: [Explosion::default(); MAX_EXPLOSIONS],
            beams: [Beam::default(); MAX_BEAMS],
            player_beams: [Beam::default(); MAX_BEAMS],
            lasers: [Laser::default(); MAX_LASERS],
            sustains: [Sustain::default(); MAX_SUSTAINS],
            dlights: Vec::new(),
            footsteps: true,
            extras: false,
        }
    }
}

fn read_pos(msg: &mut NetMsg) -> Vec3f {
    let mut pos = Vec3f::ZERO;
    msg.read_position(&mut pos);
    pos
}

/// The angles a blaster impact's model faces along surface normal `dir`.
/// C ref: the TE_BLASTER and TE_BLASTER2 cases of `CL_ParseTEnt`.
fn impact_angles(dir: Vec3f) -> Vec3f {
    let pitch = (f64::from(f64::from(dir.z).acos() as f32) / PI * 180.0) as f32;
    let yaw = if dir.x != 0.0 {
        (f64::from(f64::from(dir.y).atan2(f64::from(dir.x)) as f32) / PI * 180.0) as f32
    } else if dir.y > 0.0 {
        90.0
    } else if dir.y < 0.0 {
        270.0
    } else {
        0.0
    };
    Vec3f::new(pitch, yaw, 0.0)
}

/// The first free beam: one without a model or past its time.
fn free_beam(beams: &[Beam], time: i32) -> Option<usize> {
    beams
        .iter()
        .position(|b| b.model.is_none() || b.endtime < time)
}

impl ClientFx {
    /// Drop every effect on a map change (C `CL_ClearParticles` and
    /// `CL_ClearTEnts`). Keeps `footsteps` and the swarm rates, which C
    /// keeps in a static.
    pub fn clear(&mut self) {
        self.particles.clear();
        self.explosions = [Explosion::default(); MAX_EXPLOSIONS];
        self.beams = [Beam::default(); MAX_BEAMS];
        self.player_beams = [Beam::default(); MAX_BEAMS];
        self.lasers = [Laser::default(); MAX_LASERS];
        self.sustains = [Sustain::default(); MAX_SUSTAINS];
        self.dlights.clear();
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

    /// Not in C (opt-in `extras`): a 64-unit column of grey smoke rising
    /// from a dry explosion, drawn by C's otherwise unused
    /// `CL_SmokeTrail`. ADR-003 records why it is opt-in.
    fn explosion_smoke(&mut self, pos: Vec3f, time: i32, randk: &mut impl FnMut() -> u32) {
        if !self.extras {
            return;
        }
        let style = crate::newfx::SmokeTrail {
            color_start: 4,
            color_run: 8,
            spacing: 4,
        };
        let top = pos + Vec3f::new(0.0, 0.0, 64.0);
        self.particles.smoke_trail(pos, top, style, time, randk);
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

    /// A blaster-style impact model (TE_BLASTER, TE_BLASTER2,
    /// TE_FLECHETTE) facing along `dir`, lit by 150.
    fn impact_explosion(
        &mut self,
        pos: Vec3f,
        dir: Vec3f,
        time: i32,
        servertime: i32,
    ) -> &mut Explosion {
        let ex = self.alloc_explosion(time);
        ex.ent.origin = pos;
        ex.ent.angles = impact_angles(dir);
        ex.kind = ExKind::Misc;
        ex.start = servertime as f32 - 100.0;
        ex.light = 150.0;
        ex.ent.model = Some(MOD_EXPLODE);
        ex.frames = 4;
        ex
    }

    /// Run one svc_temp_entity message (`bytes` starts with the svc op) at
    /// client time `time` and server frame time `servertime` (C
    /// `cl.frame.servertime`; explosions start 100 ms before it). Returns
    /// the sounds to start. Other messages, types C does not handle
    /// (TE_RAILTRAIL2, TE_FLAME) and malformed bodies do nothing; C drops
    /// the connection on a malformed one (`ERR_DROP`), so it is logged.
    /// C ref: `CL_ParseTEnt`.
    pub fn temp_entity(
        &mut self,
        bytes: &[u8],
        time: i32,
        servertime: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Vec<FxSound> {
        if bytes.first() != Some(&(SvcOp::TempEntity as u8)) {
            return Vec::new();
        }
        let mut msg = NetMsg::from_bytes(bytes);
        msg.begin_reading();
        msg.read_byte();
        match self.parse_tent(&mut msg, time, servertime, randk) {
            Ok(sounds) => sounds,
            Err(error) => {
                tracing::warn!("bad svc_temp_entity: {error:?}");
                Vec::new()
            }
        }
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
        let lashit = |pos| FxSound::at(pos, "weapons/lashit.wav", ATTN_NORM);
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
                sounds.push(lashit(pos));
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
            TE_LASER_SPARKS | TE_WELDING_SPARKS => {
                let count = msg.read_byte();
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let color = msg.read_byte();
                self.particles
                    .particle_effect2(spray(pos, dir, color, count), time, randk);
                if kind == TE_WELDING_SPARKS {
                    let ex = self.alloc_explosion(time);
                    ex.ent.origin = pos;
                    ex.kind = ExKind::Flash;
                    ex.ent.flags = RF_BEAM;
                    // C subtracts 0.1 here, not 100.
                    ex.start = servertime as f32 - 0.1;
                    ex.light = 100.0 + (randk() % 75) as f32;
                    ex.lightcolor = [1.0, 1.0, 0.3];
                    ex.ent.model = Some(MOD_FLASH);
                    ex.frames = 2;
                }
            }
            TE_BLUEHYPERBLASTER => {
                // C reads the direction as a position.
                let (pos, dir) = (read_pos(msg), read_pos(msg));
                self.particles.blaster_particles(pos, dir, time, randk);
            }
            TE_BLASTER => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                self.particles.blaster_particles(pos, dir, time, randk);
                let ex = self.impact_explosion(pos, dir, time, servertime);
                ex.lightcolor = [1.0, 1.0, 0.0];
                sounds.push(lashit(pos));
            }
            TE_BLASTER2 | TE_FLECHETTE => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let blaster2 = kind == TE_BLASTER2;
                let color = if blaster2 { 0xd0 } else { 0x6f };
                self.particles
                    .blaster_particles2(pos, dir, color, time, randk);
                let ex = self.impact_explosion(pos, dir, time, servertime);
                ex.ent.flags = RF_FULLBRIGHT | RF_TRANSLUCENT;
                (ex.ent.skinnum, ex.lightcolor) = if blaster2 {
                    (1, [0.0, 1.0, 0.0])
                } else {
                    (2, [0.19, 0.41, 0.75])
                };
                sounds.push(lashit(pos));
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
                if kind != TE_GRENADE_EXPLOSION_WATER {
                    self.explosion_smoke(pos, time, randk);
                }
                let name = if kind == TE_GRENADE_EXPLOSION_WATER {
                    "weapons/xpld_wat.wav"
                } else {
                    "weapons/grenlx1a.wav"
                };
                sounds.push(FxSound::at(pos, name, ATTN_NORM));
            }
            TE_EXPLOSION1
            | TE_ROCKET_EXPLOSION
            | TE_ROCKET_EXPLOSION_WATER
            | TE_EXPLOSION1_BIG
            | TE_EXPLOSION1_NP
            | TE_PLASMA_EXPLOSION
            | TE_PLAIN_EXPLOSION => {
                let pos = read_pos(msg);
                let model = if kind == TE_EXPLOSION1_BIG {
                    MOD_EXPLO4_BIG
                } else {
                    MOD_EXPLO4
                };
                let ex = self.poly_explosion(pos, model, time, servertime);
                ex.ent.angles.y = (randk() % 360) as f32;
                if frandk(randk) < 0.5 {
                    ex.baseframe = 15;
                }
                ex.frames = 15;
                if !matches!(
                    kind,
                    TE_EXPLOSION1_BIG | TE_EXPLOSION1_NP | TE_PLAIN_EXPLOSION
                ) {
                    self.particles.explosion_particles(pos, time, randk);
                }
                if matches!(kind, TE_EXPLOSION1 | TE_ROCKET_EXPLOSION) {
                    self.explosion_smoke(pos, time, randk);
                }
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
                self.parse_beam(msg, MOD_PARASITE_SEGMENT, false, time);
            }
            TE_GRAPPLE_CABLE => self.parse_beam(msg, MOD_GRAPPLE_CABLE, true, time),
            TE_BOSSTPORT => {
                let pos = read_pos(msg);
                self.particles.big_teleport_particles(pos, time, randk);
                sounds.push(FxSound::at(pos, "misc/bigtele.wav", ATTN_NONE));
            }
            TE_GREENBLOOD => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                self.particles
                    .particle_effect2(spray(pos, dir, 0xdf, 30), time, randk);
            }
            TE_TUNNEL_SPARKS => {
                let count = msg.read_byte();
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let color = msg.read_byte();
                self.particles
                    .particle_effect3(spray(pos, dir, color, count), time, randk);
            }
            TE_LIGHTNING => {
                let ent = self.parse_lightning(msg, time);
                sounds.push(FxSound::on(
                    ent,
                    CHAN_WEAPON,
                    "weapons/tesla.wav",
                    ATTN_NORM,
                ));
            }
            TE_DEBUGTRAIL => {
                let (start, end) = (read_pos(msg), read_pos(msg));
                self.particles.debug_trail(start, end, time, randk);
            }
            TE_FLASHLIGHT => {
                let pos = read_pos(msg);
                let ent = msg.read_short();
                self.timed_light(ent, pos, 400.0, [1.0, 1.0, 1.0], time);
            }
            TE_FORCEWALL => {
                let (start, end) = (read_pos(msg), read_pos(msg));
                let color = msg.read_byte();
                self.particles.force_wall(start, end, color, time, randk);
            }
            TE_HEATBEAM => self.parse_player_beam(msg, false, time),
            TE_MONSTER_HEATBEAM => self.parse_player_beam(msg, true, time),
            TE_HEATBEAM_SPARKS | TE_HEATBEAM_STEAM => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                let (color, count) = if kind == TE_HEATBEAM_SPARKS {
                    (8, 50)
                } else {
                    (0xe0, 20)
                };
                self.particles.particle_steam_effect(
                    spray(pos, dir, color, count),
                    60,
                    time,
                    randk,
                );
                sounds.push(lashit(pos));
            }
            TE_STEAM => self.parse_steam(msg, time, randk)?,
            TE_BUBBLETRAIL2 => {
                let (start, end) = (read_pos(msg), read_pos(msg));
                self.particles.bubble_trail2(start, end, 8, time, randk);
                sounds.push(lashit(start));
            }
            TE_MOREBLOOD => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                self.particles
                    .particle_effect(spray(pos, dir, 0xe8, 250), time, randk);
            }
            TE_CHAINFIST_SMOKE => {
                let pos = read_pos(msg);
                self.particles
                    .particle_smoke_effect(spray(pos, Vec3f::Z, 0, 20), 20, time, randk);
            }
            TE_ELECTRIC_SPARKS => {
                let (pos, dir) = (read_pos(msg), msg.read_dir()?);
                self.particles
                    .particle_effect(spray(pos, dir, 0x75, 40), time, randk);
                sounds.push(lashit(pos));
            }
            TE_TRACKER_EXPLOSION => {
                let pos = read_pos(msg);
                self.timed_light(0, pos, 150.0, [-1.0, -1.0, -1.0], time);
                self.particles
                    .color_explosion_particles(pos, 0, 1, time, randk);
                sounds.push(FxSound::at(pos, "weapons/disrupthit.wav", ATTN_NORM));
            }
            TE_TELEPORT_EFFECT | TE_DBALL_GOAL => {
                let pos = read_pos(msg);
                self.particles.teleport_particles(pos, time, randk);
            }
            TE_WIDOWBEAMOUT => {
                let id = msg.read_short();
                let org = read_pos(msg);
                self.start_sustain(id, org, 2100, SustainThink::Widow, time);
            }
            TE_NUKEBLAST => {
                let org = read_pos(msg);
                self.start_sustain(21000, org, 1000, SustainThink::Nuke, time);
            }
            TE_WIDOWSPLASH => {
                let pos = read_pos(msg);
                self.particles.widow_splash(pos, time, randk);
            }
            _ => {}
        }
        Ok(sounds)
    }

    /// A beam from entity `ent` (start, end, and with `with_offset` an
    /// offset added to its start) that lasts 200 ms, replacing that
    /// entity's beam. A full list drops it (C "beam list overflow").
    /// C ref: `CL_ParseBeam`, `CL_ParseBeam2`.
    fn parse_beam(&mut self, msg: &mut NetMsg, model: &'static str, with_offset: bool, time: i32) {
        let entity = msg.read_short();
        let (start, end) = (read_pos(msg), read_pos(msg));
        let offset = if with_offset {
            read_pos(msg)
        } else {
            Vec3f::ZERO
        };
        let slot = self
            .beams
            .iter()
            .position(|b| b.entity == entity)
            .or_else(|| free_beam(&self.beams, time));
        if let Some(b) = slot.map(|slot| &mut self.beams[slot]) {
            *b = Beam {
                entity,
                model: Some(model),
                endtime: time + 200,
                offset,
                start,
                end,
                ..*b
            };
        }
    }

    /// A 200 ms lightning bolt from one entity to another, replacing the
    /// bolt between the same two. Returns the source entity, which the
    /// sound follows. C ref: `CL_ParseLightning`.
    fn parse_lightning(&mut self, msg: &mut NetMsg, time: i32) -> i32 {
        let entity = msg.read_short();
        let dest_entity = msg.read_short();
        let (start, end) = (read_pos(msg), read_pos(msg));
        let slot = self
            .beams
            .iter()
            .position(|b| b.entity == entity && b.dest_entity == dest_entity)
            .or_else(|| free_beam(&self.beams, time));
        if let Some(slot) = slot {
            self.beams[slot] = Beam {
                entity,
                dest_entity,
                model: Some(MOD_LIGHTNING),
                endtime: time + 200,
                offset: Vec3f::ZERO,
                start,
                end,
            };
        }
        entity
    }

    /// A heat beam into the player-beam list: replacing the entity's own
    /// for 200 ms, or new for 100 ms ("to prevent multiple heatbeams").
    /// A player's beam starts at C's fixed gun offset (2, 7, -3); a
    /// monster's (`monster`) has none and draws as a player heat beam.
    /// C's branch reading an offset from the message has no caller.
    /// C ref: `CL_ParsePlayerBeam`.
    fn parse_player_beam(&mut self, msg: &mut NetMsg, monster: bool, time: i32) {
        let entity = msg.read_short();
        let (start, end) = (read_pos(msg), read_pos(msg));
        let offset = if monster {
            Vec3f::ZERO
        } else {
            Vec3f::new(2.0, 7.0, -3.0)
        };
        let (slot, endtime) = match self.player_beams.iter().position(|b| b.entity == entity) {
            Some(slot) => (Some(slot), time + 200),
            None => (free_beam(&self.player_beams, time), time + 100),
        };
        if let Some(b) = slot.map(|slot| &mut self.player_beams[slot]) {
            *b = Beam {
                entity,
                model: Some(MOD_HEATBEAM),
                endtime,
                offset,
                start,
                end,
                ..*b
            };
        }
    }

    /// TE_STEAM: with id -1 one puff now, else a sustain that puffs every
    /// 100 ms for the time the message gives. A full sustain list still
    /// reads the message. C ref: `CL_ParseSteam`.
    fn parse_steam(
        &mut self,
        msg: &mut NetMsg,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Q2Result<()> {
        let id = msg.read_short();
        let count = msg.read_byte();
        let (org, dir) = (read_pos(msg), msg.read_dir()?);
        let color = msg.read_byte() & 0xff;
        let magnitude = msg.read_short();
        let spray = Spray {
            org,
            dir,
            color,
            count,
        };
        if id == -1 {
            self.particles
                .particle_steam_effect(spray, magnitude, time, randk);
            return Ok(());
        }
        let duration = msg.read_long();
        if let Some(s) = self.sustains.iter_mut().find(|s| s.id == 0) {
            *s = Sustain {
                id,
                // The time is off the wire: wrap as C's int add does.
                endtime: time.wrapping_add(duration),
                nextthink: time,
                thinkinterval: 100,
                org,
                dir,
                color,
                count,
                magnitude,
                think: SustainThink::Steam,
            };
        }
        Ok(())
    }

    /// A widow or nuke sustain at `org` for `duration` ms, thinking every
    /// frame. A full list drops it. C ref: `CL_ParseWidow`, `CL_ParseNuke`.
    fn start_sustain(
        &mut self,
        id: i32,
        org: Vec3f,
        duration: i32,
        think: SustainThink,
        time: i32,
    ) {
        if let Some(s) = self.sustains.iter_mut().find(|s| s.id == 0) {
            *s = Sustain {
                id,
                endtime: time + duration,
                nextthink: time,
                thinkinterval: 1,
                org,
                think,
                ..*s
            };
        }
    }

    /// A 100 ms light at `origin` for `cl_dlights` (C `CL_Flashlight`,
    /// `CL_ColorFlash`): minlight 250, keyed by `key` (0 for none).
    fn timed_light(&mut self, key: i32, origin: Vec3f, radius: f32, color: [f32; 3], time: i32) {
        self.dlights.push(crate::effects::ClientDLight {
            key,
            color,
            origin,
            radius,
            die: (time + 100) as f32,
            decay: 0.0,
            minlight: 250.0,
        });
    }

    /// The lights queued since the last call, for `effects::Effects`'
    /// `add_dlight`.
    pub fn take_dlights(&mut self) -> Vec<crate::effects::ClientDLight> {
        std::mem::take(&mut self.dlights)
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
    /// an `EF_TELEPORTER` pad. Returns its sound, if any, which follows the
    /// entity. C ref: `CL_EntityEvent`, `CL_TeleporterParticles`.
    pub fn entity_event(
        &mut self,
        ent: &EntityState,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Option<FxSound> {
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
        sound
    }

    /// The trail `ent`'s effects leave from `lerp_origin` (where it was
    /// drawn last frame, C `cent->lerp_origin`) to `origin` (where it is
    /// drawn now), and the light C adds with `V_AddLight`. Call it each
    /// frame for every drawn entity except the local player (C skips
    /// that and modelindex 0; see `player_light`). An `EF_TRAP` entity's
    /// light, and the next frame's `lerp_origin`, are 32 units up (see
    /// `EntityTrails::run`, which the browser calls). C ref:
    /// `CL_AddPacketEntities`.
    #[cfg(test)]
    pub fn entity_trail(
        &mut self,
        ent: &EntityState,
        lerp_origin: Vec3f,
        origin: Vec3f,
        trail: &mut TrailState,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> Option<FxLight> {
        self.trail_and_light(ent, lerp_origin, origin, trail, time, randk)
            .0
    }

    /// `entity_trail`, also returning the origin C copies into
    /// `cent->lerp_origin` (moved by `EF_TRAP`).
    fn trail_and_light(
        &mut self,
        ent: &EntityState,
        lerp_origin: Vec3f,
        mut origin: Vec3f,
        trail: &mut TrailState,
        time: i32,
        randk: &mut impl FnMut() -> u32,
    ) -> (Option<FxLight>, Vec3f) {
        let effects = ent.effects;
        let light = |origin, intensity, color| {
            Some(FxLight {
                origin,
                intensity,
                color,
            })
        };
        let (yellow, green, dark) = ([1.0, 1.0, 0.0], [0.0, 1.0, 0.0], [-1.0, -1.0, -1.0]);
        let tracker = effects & EF_TRACKER != 0;
        let ps = &mut self.particles;
        // C's order: EF_BLASTER | EF_TRACKER is the blaster2.
        let lit = if effects & !EF_ROTATE == 0 {
            None
        } else if effects & EF_ROCKET != 0 {
            ps.rocket_trail(lerp_origin, origin, &mut trail.trailcount, time, randk);
            // cl_r1q2_lightstyle's default 1 colour.
            light(origin, 200.0, [1.0, 0.23, 0.0])
        } else if effects & EF_BLASTER != 0 {
            if tracker {
                ps.blaster_trail2(lerp_origin, origin, time, randk);
                light(origin, 200.0, green)
            } else {
                ps.blaster_trail(lerp_origin, origin, time, randk);
                light(origin, 200.0, yellow)
            }
        } else if effects & EF_HYPERBLASTER != 0 {
            light(origin, 200.0, if tracker { green } else { yellow })
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
                Some(200.0)
            } else {
                // C reads past the ramp for other frames; no light here.
                usize::try_from(ent.frame)
                    .ok()
                    .and_then(|f| BFG_LIGHTRAMP.get(f))
                    .copied()
            };
            intensity.and_then(|intensity| light(origin, intensity, green))
        } else if effects & EF_TRAP != 0 {
            origin.z += 32.0;
            ps.trap_particles(&mut origin, time, randk);
            let intensity = (randk() % 100 + 100) as f32;
            light(origin, intensity, [1.0, 0.8, 0.1])
        } else if effects & EF_FLAG1 != 0 {
            ps.flag_trail(lerp_origin, origin, 242, time, randk);
            light(origin, 225.0, [1.0, 0.1, 0.1])
        } else if effects & EF_FLAG2 != 0 {
            ps.flag_trail(lerp_origin, origin, 115, time, randk);
            light(origin, 225.0, [0.1, 0.1, 1.0])
        } else if effects & EF_TAGTRAIL != 0 {
            ps.tag_trail(lerp_origin, origin, 220, time, randk);
            light(origin, 225.0, yellow)
        } else if effects & EF_TRACKERTRAIL != 0 {
            if tracker {
                let pulse = f64::from(time as f32 / 500.0).sin() as f32;
                light(origin, 50.0 + 500.0 * (pulse + 1.0), dark)
            } else {
                ps.tracker_shell(lerp_origin, time, randk);
                light(origin, 155.0, dark)
            }
        } else if tracker {
            ps.tracker_trail(lerp_origin, origin, 0, time);
            light(origin, 200.0, dark)
        } else if effects & EF_IONRIPPER != 0 {
            ps.ionripper_trail(lerp_origin, origin, time, randk);
            light(origin, 100.0, [1.0, 0.5, 0.5])
        } else if effects & EF_BLUEHYPERBLASTER != 0 {
            light(origin, 200.0, [0.0, 0.0, 1.0])
        } else if effects & EF_PLASMA != 0 {
            if effects & EF_ANIM_ALLFAST != 0 {
                ps.blaster_trail(lerp_origin, origin, time, randk);
            }
            light(origin, 130.0, [1.0, 0.5, 0.5])
        } else {
            None
        };
        (lit, origin)
    }

    /// Advance everything to client time `time` and return what to draw,
    /// without the local view: the local player's heat beam draws nothing
    /// (use `frame_view`). `player` is C `cl.playernum + 1`; a beam from it
    /// starts 22 units below `vieworg`. For tests; the browser calls
    /// `frame_view`.
    #[cfg(test)]
    pub fn frame(
        &mut self,
        time: i32,
        lerpfrac: f32,
        player: i32,
        vieworg: Vec3f,
        randk: &mut impl FnMut() -> u32,
    ) -> FxFrame {
        let view = FxView {
            player,
            vieworg,
            ..FxView::default()
        };
        self.frame_view(time, lerpfrac, &view, randk)
    }

    /// Advance everything to client time `time` and return what to draw:
    /// beams, player beams, explosions, lasers and sustains as
    /// `CL_AddTEnts` adds them, then particles (`CL_AddParticles`).
    /// `lerpfrac` is C
    /// `cl.lerpfrac`.
    pub fn frame_view(
        &mut self,
        time: i32,
        lerpfrac: f32,
        view: &FxView,
        randk: &mut impl FnMut() -> u32,
    ) -> FxFrame {
        let mut out = FxFrame::default();
        self.add_beams(time, view.player, view.vieworg, randk, &mut out.entities);
        self.add_player_beams(time, lerpfrac, view, randk, &mut out.entities);
        self.add_explosions(time, lerpfrac, &mut out);
        out.entities.extend(
            self.lasers
                .iter()
                .filter(|l| l.endtime >= time)
                .map(|l| l.ent),
        );
        self.process_sustains(time, randk);
        out.particles = self.particles.add_particles(time);
        out
    }

    /// One model per 30 units along each live beam (35 for lightning,
    /// which stops 20 short and faces back), facing along it with a random
    /// roll. Lightning shorter than its model is drawn once at its target
    /// and, as in C, ends the pass: later beams are not drawn this frame.
    /// C ref: `CL_AddBeams`.
    fn add_beams(
        &mut self,
        time: i32,
        player: i32,
        vieworg: Vec3f,
        randk: &mut impl FnMut() -> u32,
        out: &mut Vec<FxEntity>,
    ) {
        for b in self.beams.iter_mut() {
            let Some(model) = b.model.filter(|_| b.endtime >= time) else {
                continue;
            };
            if b.entity == player {
                b.start = vieworg;
                b.start.z -= 22.0; // adjust for view height
            }
            let mut org = b.start + b.offset;
            let mut dist = b.end - org;
            let (pitch, yaw) = beam_angles(dist);
            let mut d = vector_normalize(&mut dist);
            let lightning = model == MOD_LIGHTNING;
            let model_length = if lightning {
                d -= 20.0; // so it doesn't end in the middle of the tesla
                35.0
            } else {
                30.0
            };
            let steps = (d / model_length).ceil();
            let len = (d - model_length) / (steps - 1.0);
            if lightning && d <= model_length {
                out.push(FxEntity {
                    model: Some(model),
                    origin: b.end,
                    flags: RF_FULLBRIGHT,
                    angles: Vec3f::new(pitch, yaw, (randk() % 360) as f32),
                    ..FxEntity::default()
                });
                return;
            }
            while d > 0.0 {
                let roll = (randk() % 360) as f32;
                out.push(if lightning {
                    FxEntity {
                        model: Some(model),
                        origin: org,
                        flags: RF_FULLBRIGHT,
                        angles: Vec3f::new(-pitch, yaw + 180.0, roll),
                        ..FxEntity::default()
                    }
                } else {
                    FxEntity {
                        model: Some(model),
                        origin: org,
                        angles: Vec3f::new(pitch, yaw, roll),
                        ..FxEntity::default()
                    }
                });
                org += dist * len;
                d -= model_length;
            }
        }
    }

    /// The heat beams: 32-unit models facing back along the beam, rolling
    /// with the clock. The local player's leaves the view weapon along the
    /// view, offset by `hand`, with `CL_Heatbeam`'s rings (frame 1);
    /// others' (frame 2) shift by their gun offset, or for a monster
    /// sparkle at the muzzle. Only heat beams reach this list, so C's
    /// other-model branches are not ported. C ref: `CL_AddPlayerBeams`.
    fn add_player_beams(
        &mut self,
        time: i32,
        lerpfrac: f32,
        view: &FxView,
        randk: &mut impl FnMut() -> u32,
        out: &mut Vec<FxEntity>,
    ) {
        const MODEL_LENGTH: f32 = 32.0;
        let hand_multiplier = match view.hand {
            2 => 0.0,
            1 => -1.0,
            _ => 1.0,
        };
        for b in self.player_beams.iter_mut() {
            let Some(model) = b.model.filter(|_| b.endtime >= time) else {
                continue;
            };
            let local = b.entity == view.player;
            let mut org = if local {
                for j in 0..3 {
                    b.start[j] = view.vieworg[j]
                        + view.old_gunoffset[j]
                        + lerpfrac * (view.gunoffset[j] - view.old_gunoffset[j]);
                }
                let mut org = b.start + view.right * (hand_multiplier * b.offset.x);
                org += view.forward * b.offset.y;
                org += view.up * b.offset.z;
                if view.hand == 2 {
                    org += view.up * -1.0;
                }
                org
            } else {
                b.start
            };
            let mut dist = b.end - org;
            if local {
                let len = (dist.x * dist.x + dist.y * dist.y + dist.z * dist.z).sqrt();
                dist = view.forward * len;
                dist += view.right * (hand_multiplier * b.offset.x);
                dist += view.forward * b.offset.y;
                dist += view.up * b.offset.z;
                if view.hand == 2 {
                    org += view.up * -1.0; // C lowers it a second time
                }
            }
            let (pitch, yaw) = beam_angles(dist);
            let framenum = if local {
                self.particles
                    .heatbeam(org, dist, view.right, view.up, time, randk);
                1
            } else {
                let (f, r, u) = angle_vectors(Vec3f::new(-pitch, yaw + 180.0, 0.0));
                if b.offset != Vec3f::ZERO {
                    // A player: their hard-coded gun offset.
                    org += r * (-b.offset.x + 1.0);
                    org += f * -b.offset.y;
                    org += u * (-b.offset.z - 10.0);
                } else {
                    self.particles.monster_plasma_shell(b.start, time, randk);
                }
                2
            };
            let mut d = vector_normalize(&mut dist);
            let steps = (d / MODEL_LENGTH).ceil();
            let len = (d - MODEL_LENGTH) / (steps - 1.0);
            while d > 0.0 {
                out.push(FxEntity {
                    model: Some(model),
                    origin: org,
                    // C's RF_WEAPONMODEL fixes rogue's heat beam at high FOV.
                    flags: RF_FULLBRIGHT | RF_WEAPONMODEL,
                    angles: Vec3f::new(-pitch, yaw + 180.0, (time % 360) as f32),
                    frame: framenum,
                    ..FxEntity::default()
                });
                org += dist * len;
                d -= MODEL_LENGTH;
            }
        }
    }

    /// Run each sustain whose think time has come, and free the ones past
    /// their end. Steam stops advancing its think time when the particle
    /// pool fills, as C's early return skips it. C ref: `CL_ProcessSustain`.
    fn process_sustains(&mut self, time: i32, randk: &mut impl FnMut() -> u32) {
        for s in self.sustains.iter_mut().filter(|s| s.id != 0) {
            if s.endtime >= time && time >= s.nextthink {
                match s.think {
                    SustainThink::Steam => {
                        let spray = Spray {
                            org: s.org,
                            dir: s.dir,
                            color: s.color,
                            count: s.count,
                        };
                        if self
                            .particles
                            .sustain_steam(spray, s.magnitude, time, randk)
                        {
                            s.nextthink += s.thinkinterval;
                        }
                    }
                    SustainThink::Widow => {
                        self.particles.widow_beamout(s.org, s.endtime, time, randk);
                    }
                    SustainThink::Nuke => self.particles.nukeblast(s.org, s.endtime, time, randk),
                }
            } else if s.endtime < time {
                s.id = 0;
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
        crate::newfx::atan2_degrees(dist.y, dist.x)
    } else if dist.y > 0.0 {
        90.0
    } else {
        270.0
    };
    if yaw < 0.0 {
        yaw += 360.0;
    }
    let forward = (dist.x * dist.x + dist.y * dist.y).sqrt();
    // Unlike `vectoangles2`, C multiplies the pitch by -180.0 in double.
    let mut pitch =
        (f64::from(f64::from(dist.z).atan2(f64::from(forward)) as f32) * -180.0 / PI) as f32;
    if pitch < 0.0 {
        pitch += 360.0;
    }
    (pitch, yaw)
}

/// The light the local player's own effects make at `origin` (C adds it
/// for `cl.playernum + 1` instead of a trail): CTF flags, the tag and the
/// tracker's trail. C ref: `CL_AddPacketEntities`.
pub fn player_light(effects: u32, origin: Vec3f) -> Option<FxLight> {
    let color = if effects & EF_FLAG1 != 0 {
        [1.0, 0.1, 0.1]
    } else if effects & EF_FLAG2 != 0 {
        [0.1, 0.1, 1.0]
    } else if effects & EF_TAGTRAIL != 0 {
        [1.0, 1.0, 0.0]
    } else if effects & EF_TRACKERTRAIL != 0 {
        [-1.0, -1.0, -1.0]
    } else {
        return None;
    };
    Some(FxLight {
        origin,
        intensity: 225.0,
        color,
    })
}

/// An `EF_SPINNINGLIGHTS` entity's red light 64 units out along its
/// spinning yaw (`anglemod(time / 2)` plus its own); none for an
/// `EF_ROTATE` one, whose angles C sets first. C adds it for every
/// entity, the local player and invisible ones too, and also draws the
/// model at those angles (pitch 0, roll 180), which is the entity view's
/// part. C ref: `CL_AddPacketEntities`.
pub fn spinning_light(ent: &EntityState, origin: Vec3f, time: i32) -> Option<FxLight> {
    if ent.effects & EF_ROTATE != 0 || ent.effects & EF_SPINNINGLIGHTS == 0 {
        return None;
    }
    let yaw = anglemod((time / 2) as f32) + ent.angles.y;
    let (forward, _, _) = angle_vectors(Vec3f::new(0.0, yaw, 180.0));
    Some(FxLight {
        origin: origin + forward * 64.0,
        intensity: 100.0,
        color: [1.0, 0.0, 0.0],
    })
}

/// The alpha a BFG ball (0.3), plasma (0.6) or translucent sphere (0.6
/// with `EF_TRACKERTRAIL`, else 0.3) draws at; C also sets
/// `RF_TRANSLUCENT` on it. Later flags win, as C assigns in that order.
/// C ref: `CL_AddPacketEntities`.
pub fn effect_alpha(effects: u32) -> Option<f32> {
    let mut alpha = None;
    if effects & EF_BFG != 0 {
        alpha = Some(0.30);
    }
    if effects & EF_PLASMA != 0 {
        alpha = Some(0.6);
    }
    if effects & EF_SPHERETRANS != 0 {
        alpha = Some(if effects & EF_TRACKERTRAIL != 0 {
            0.6
        } else {
            0.3
        });
    }
    alpha
}

/// The extra model an `EF_POWERSCREEN` entity draws over `view`: the
/// translucent green armour shell on frame 0. C ref: `CL_AddPacketEntities`.
pub fn powerscreen_entity(view: &EntityView) -> FxEntity {
    FxEntity {
        model: Some(MOD_POWERSCREEN),
        origin: view.origin,
        oldorigin: view.oldorigin,
        angles: view.angles,
        frame: 0,
        oldframe: 0,
        backlerp: view.backlerp,
        alpha: 0.30,
        flags: RF_TRANSLUCENT | RF_SHELL_GREEN,
        skinnum: 0,
    }
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
    /// lerp from their previous origin. Like C's `cl_entities`, an entry
    /// outlives its entity leaving, and a fresh trail keeps `fly_stoptime`.
    pub fn server_frame(&mut self, current: &[EntityState], previous: &[EntityState]) {
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
                    let fly_stoptime = self.entities.get(&s.number).map_or(0, |e| e.0.fly_stoptime);
                    let trail = TrailState {
                        fly_stoptime,
                        ..TrailState::default()
                    };
                    self.entities.insert(s.number, (trail, start, start));
                }
            }
        }
    }

    /// Each rendered frame (C `CL_AddPacketEntities`): every entity's trail
    /// from where it was drawn last frame to where its model is drawn now,
    /// and the lights they add, spinning lights first. The
    /// local `player` (C `cl.playernum + 1`) only lights its own effects
    /// (`player_light`); entities without a model have no trail.
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
        for s in current {
            let Some((trail, prev_origin, lerp_origin)) = self.entities.get_mut(&s.number) else {
                continue;
            };
            let origin = crate::entities::drawn_origin(*prev_origin, s, lerpfrac);
            lights.extend(spinning_light(s, origin, time));
            if s.number == player {
                lights.extend(player_light(s.effects, origin));
                continue;
            }
            if s.modelindex == 0 {
                continue;
            }
            let (light, drawn) = fx.trail_and_light(s, *lerp_origin, origin, trail, time, randk);
            lights.extend(light);
            *lerp_origin = drawn;
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
    fn with_extras_dry_explosions_leave_a_rising_smoke_column() {
        for kind in [
            TE_ROCKET_EXPLOSION,
            TE_GRENADE_EXPLOSION,
            TE_EXPLOSION1,
            TE_EXPLOSION2,
        ] {
            let plain = {
                let mut fx = ClientFx::default();
                fx.temp_entity(&pos_only(kind, P), 1000, 1000, &mut rng(&[0]));
                fx.particles.len()
            };
            let mut fx = ClientFx {
                extras: true,
                ..ClientFx::default()
            };
            fx.temp_entity(&pos_only(kind, P), 1000, 1000, &mut rng(&[0]));
            assert_eq!(fx.particles.len(), plain + 16, "{kind}");
            let smoke = &fx.particles.active[plain..];
            for p in smoke {
                assert!((4..12).contains(&p.color), "grey: {}", p.color);
                assert!(p.vel.z > 0.0, "rises");
                assert!(p.org.z >= P.z - 3.0 && p.org.z <= P.z + 67.0);
            }
        }
        // Under water there is no smoke.
        for kind in [TE_ROCKET_EXPLOSION_WATER, TE_GRENADE_EXPLOSION_WATER] {
            let count = |extras| {
                let mut fx = ClientFx {
                    extras,
                    ..ClientFx::default()
                };
                fx.temp_entity(&pos_only(kind, P), 1000, 1000, &mut rng(&[0]));
                fx.particles.len()
            };
            assert_eq!(count(true), count(false), "{kind}");
        }
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
            .temp_entity(&pos_only(TE_FLAME, P), 1000, 1000, &mut r)
            .is_empty());
        // TE_RAILTRAIL2 and TE_FLAME are not in C's switch (ERR_DROP).
        assert!(fx
            .temp_entity(&pos_pos(TE_RAILTRAIL2, P, P), 1000, 1000, &mut r)
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
        let sounds =
            Vec::from_iter(fx.entity_event(&event(3, EV_ITEM_RESPAWN), 1000, &mut rng(&[0])));
        assert_eq!(names(&sounds), ["items/respawn1.wav"]);
        let s = &sounds[0];
        assert_eq!(
            (s.origin, s.entnum, s.channel, s.attenuation),
            (None, 3, CHAN_WEAPON, ATTN_IDLE)
        );
        assert_eq!(fx.particles.len(), 64);
        fx.clear();
        let sounds =
            Vec::from_iter(fx.entity_event(&event(3, EV_PLAYER_TELEPORT), 1000, &mut rng(&[0])));
        assert_eq!(names(&sounds), ["misc/tele1.wav"]);
        assert_eq!(fx.particles.len(), 1053);
    }

    #[test]
    fn footsteps_follow_cl_footsteps_and_falls_thud() {
        let mut fx = ClientFx::default();
        let sounds = Vec::from_iter(fx.entity_event(&event(1, EV_FOOTSTEP), 1000, &mut rng(&[2])));
        assert_eq!(names(&sounds), ["player/step3.wav"]);
        assert_eq!(sounds[0].channel, CHAN_BODY);
        fx.footsteps = false;
        assert!(fx
            .entity_event(&event(1, EV_FOOTSTEP), 1000, &mut rng(&[2]))
            .is_none());
        for (ev, name) in [
            (EV_FALLSHORT, "player/land1.wav"),
            (EV_FALL, "*fall2.wav"),
            (EV_FALLFAR, "*fall1.wav"),
        ] {
            let sounds = Vec::from_iter(fx.entity_event(&event(1, ev), 1000, &mut rng(&[0])));
            assert_eq!(names(&sounds), [name]);
            assert_eq!(sounds[0].channel, CHAN_AUTO);
        }
        assert!(fx
            .entity_event(&event(1, EV_OTHER_TELEPORT), 1000, &mut rng(&[0]))
            .is_none());
        assert_eq!(fx.particles.len(), 0);
    }

    #[test]
    fn a_teleporter_pad_sparkles_each_server_frame() {
        let mut fx = ClientFx::default();
        let pad = EntityState {
            effects: EF_TELEPORTER,
            ..event(9, 0)
        };
        assert!(fx.entity_event(&pad, 1000, &mut rng(&[0])).is_none());
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
        let back = [rocket_at(7, FROM, TO)];
        trails.server_frame(&back, &[]);
        assert_eq!(trails.entities[&7], (TrailState::default(), FROM, FROM));
    }

    #[test]
    fn a_fly_swarm_outlasts_the_entity_leaving_like_c_centity_t() {
        // C CL_DeltaEntity resets only trailcount; fly_stoptime stays in
        // cl_entities while the entity is away.
        let mut fx = ClientFx::default();
        let mut trails = EntityTrails::default();
        let corpse = [EntityState {
            effects: EF_FLIES,
            ..rocket_at(7, FROM, FROM)
        }];
        trails.server_frame(&corpse, &[]);
        trails.run(&mut fx, &corpse, 1.0, 1, 1000, &mut rng(&[0]));
        let swarm = trails.entities[&7].0.fly_stoptime;
        assert!(swarm > 1000);
        trails.server_frame(&[], &corpse);
        trails.server_frame(&corpse, &[]);
        assert_eq!(trails.entities[&7].0.fly_stoptime, swarm);
        assert_eq!(trails.entities[&7].0.trailcount, 1024);
    }

    #[test]
    fn frame_lerped_and_beam_entities_trail_from_where_they_are_drawn() {
        // C steps their origin (the frames animate the move), as the model.
        for renderfx in [RF_FRAMELERP, RF_BEAM] {
            let mut fx = ClientFx::default();
            let mut trails = EntityTrails::default();
            let before = [rocket_at(7, FROM, FROM)];
            let after = [EntityState {
                renderfx,
                ..rocket_at(7, FROM, TO)
            }];
            trails.server_frame(&before, &[]);
            trails.server_frame(&after, &before);
            let lights = trails.run(&mut fx, &after, 0.5, 1, 1000, &mut rng(&[0]));
            assert_eq!(lights[0].origin, TO, "{renderfx}");
        }
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

    // Mission packs (rogue, xatrix, CTF).

    fn pos_pos(kind: i32, a: Vec3f, b: Vec3f) -> Vec<u8> {
        tent(kind, |m| {
            m.write_position(a);
            m.write_position(b);
        })
    }

    fn colour_of(fx: &ClientFx) -> i32 {
        fx.particles.active[0].color
    }

    #[test]
    fn mission_pack_impacts_spray_their_colours() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        assert!(fx
            .temp_entity(&pos_dir(TE_GREENBLOOD, P, Vec3f::Z), 0, 0, &mut r)
            .is_empty());
        assert_eq!((fx.particles.len(), colour_of(&fx)), (30, 0xdf));
        fx.clear();
        fx.temp_entity(&pos_dir(TE_MOREBLOOD, P, Vec3f::Z), 0, 0, &mut r);
        assert_eq!((fx.particles.len(), colour_of(&fx)), (250, 0xe8));
        fx.clear();
        let sounds = fx.temp_entity(&pos_dir(TE_ELECTRIC_SPARKS, P, Vec3f::Z), 0, 0, &mut r);
        assert_eq!(names(&sounds), ["weapons/lashit.wav"]);
        assert_eq!((fx.particles.len(), colour_of(&fx)), (40, 0x75));
        fx.clear();
        let tunnel = tent(TE_TUNNEL_SPARKS, |m| {
            m.write_byte(7);
            m.write_position(P);
            m.write_dir(Vec3f::Z);
            m.write_byte(0x40);
        });
        fx.temp_entity(&tunnel, 0, 0, &mut rng(&[3]));
        assert_eq!((fx.particles.len(), colour_of(&fx)), (7, 0x40));
        assert_eq!(fx.particles.active[0].accel.z, 40.0, "effect3 rises");
        fx.clear();
        fx.temp_entity(&pos_only(TE_CHAINFIST_SMOKE, P), 0, 0, &mut r);
        assert_eq!(fx.particles.len(), 20);
        assert_eq!(fx.particles.active[0].accel, Vec3f::ZERO);
        fx.clear();
        for kind in [TE_TELEPORT_EFFECT, TE_DBALL_GOAL] {
            fx.temp_entity(&pos_only(kind, P), 0, 0, &mut r);
        }
        assert_eq!(fx.particles.len(), 2 * 1053);
        fx.clear();
        fx.temp_entity(&pos_only(TE_WIDOWSPLASH, P), 0, 0, &mut r);
        assert_eq!(fx.particles.len(), 256);
    }

    #[test]
    fn heatbeam_sparks_steam_and_bubbles_crackle() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        for (kind, count, color) in [(TE_HEATBEAM_SPARKS, 50, 8), (TE_HEATBEAM_STEAM, 20, 0xe0)] {
            fx.clear();
            let sounds = fx.temp_entity(&pos_dir(kind, P, Vec3f::Z), 0, 0, &mut r);
            assert_eq!(names(&sounds), ["weapons/lashit.wav"]);
            assert_eq!((fx.particles.len(), colour_of(&fx)), (count, color));
        }
        fx.clear();
        let end = P + Vec3f::new(80.0, 0.0, 0.0);
        let sounds = fx.temp_entity(&pos_pos(TE_BUBBLETRAIL2, P, end), 0, 0, &mut r);
        assert_eq!(names(&sounds), ["weapons/lashit.wav"]);
        assert_eq!(sounds[0].origin, Some(P));
        assert_eq!(fx.particles.len(), 10);
        fx.clear();
        fx.temp_entity(&pos_pos(TE_DEBUGTRAIL, P, end), 0, 0, &mut r);
        assert_eq!(fx.particles.len(), 27);
        fx.clear();
        let wall = tent(TE_FORCEWALL, |m| {
            m.write_position(P);
            m.write_position(end);
            m.write_byte(0x74);
        });
        fx.temp_entity(&wall, 0, 0, &mut rng(&[32767]));
        assert_eq!((fx.particles.len(), colour_of(&fx)), (20, 0x74));
    }

    #[test]
    fn a_blue_hyperblaster_impact_reads_its_direction_as_a_position() {
        let mut fx = ClientFx::default();
        let msg = pos_pos(TE_BLUEHYPERBLASTER, P, Vec3f::Z);
        assert!(fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0])).is_empty());
        assert_eq!((fx.particles.len(), colour_of(&fx)), (40, 0xe0));
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert!(frame.entities.is_empty());
    }

    #[test]
    fn blaster2_and_flechette_impacts_flash_green_and_blue() {
        for (kind, color, skin, light) in [
            (TE_BLASTER2, 0xd0, 1, [0.0, 1.0, 0.0]),
            (TE_FLECHETTE, 0x6f, 2, [0.19, 0.41, 0.75]),
        ] {
            let mut fx = ClientFx::default();
            let sounds = fx.temp_entity(&pos_dir(kind, P, Vec3f::Z), 1000, 1000, &mut rng(&[0]));
            assert_eq!(names(&sounds), ["weapons/lashit.wav"]);
            assert_eq!((fx.particles.len(), colour_of(&fx)), (40, color));
            let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
            let ex = frame.entities[0];
            assert_eq!(ex.model, Some(MOD_EXPLODE));
            assert_eq!(
                (ex.flags, ex.skinnum),
                (RF_FULLBRIGHT | RF_TRANSLUCENT, skin)
            );
            assert_eq!(ex.angles.x, 0.0, "facing straight up");
            assert_eq!(frame.lights[0].color, light);
        }
    }

    #[test]
    fn welding_sparks_flash_a_beam_flagged_light_for_one_frame() {
        let mut fx = ClientFx::default();
        let msg = tent(TE_WELDING_SPARKS, |m| {
            m.write_byte(5);
            m.write_position(P);
            m.write_dir(Vec3f::Z);
            m.write_byte(0xe0);
        });
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[10]));
        assert_eq!(fx.particles.len(), 5);
        // C starts it at servertime - 0.1, not - 100.
        let frame = fx.frame(1099, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        let ex = frame.entities[0];
        assert_eq!((ex.model, ex.flags), (Some(MOD_FLASH), RF_BEAM));
        let light = frame.lights[0];
        assert_eq!((light.intensity, light.color), (110.0, [1.0, 1.0, 0.3]));
        assert!(fx
            .frame(1100, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]))
            .entities
            .is_empty());
    }

    #[test]
    fn welding_and_trap_lights_stay_in_c_ranges_for_any_random_number() {
        let mut fx = ClientFx::default();
        let msg = tent(TE_WELDING_SPARKS, |m| {
            m.write_byte(1);
            m.write_position(P);
            m.write_dir(Vec3f::Z);
            m.write_byte(0xe0);
        });
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[u32::MAX]));
        let light = fx.frame(1099, 1.0, 1, Vec3f::ZERO, &mut rng(&[0])).lights[0];
        assert!(
            (100.0..175.0).contains(&light.intensity),
            "{}",
            light.intensity
        );
        let (light, _) = trail_light(EF_TRAP, &[u32::MAX]);
        let intensity = light.unwrap().intensity;
        assert!((100.0..200.0).contains(&intensity), "{intensity}");
    }

    #[test]
    fn plasma_big_np_and_plain_explosions_pick_model_and_particles() {
        for (kind, model, particles) in [
            (TE_PLASMA_EXPLOSION, MOD_EXPLO4, 256),
            (TE_EXPLOSION1_BIG, MOD_EXPLO4_BIG, 0),
            (TE_EXPLOSION1_NP, MOD_EXPLO4, 0),
            (TE_PLAIN_EXPLOSION, MOD_EXPLO4, 0),
        ] {
            let mut fx = ClientFx::default();
            let sounds = fx.temp_entity(&pos_only(kind, P), 1000, 1000, &mut rng(&[0]));
            assert_eq!(names(&sounds), ["weapons/rocklx1a.wav"]);
            assert_eq!(fx.particles.len(), particles);
            let ex = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0])).entities[0];
            assert_eq!((ex.model, ex.frame), (Some(model), 17), "frandk 0 < 0.5");
        }
    }

    fn lightning(src: i32, dest: i32, start: Vec3f, end: Vec3f) -> Vec<u8> {
        tent(TE_LIGHTNING, |m| {
            m.write_short(src);
            m.write_short(dest);
            m.write_position(start);
            m.write_position(end);
        })
    }

    #[test]
    fn lightning_sounds_on_its_source_and_draws_reversed_segments() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(100.0, 0.0, 0.0);
        let msg = lightning(4, 9, Vec3f::ZERO, end);
        let sounds = fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        assert_eq!(names(&sounds), ["weapons/tesla.wav"]);
        let s = &sounds[0];
        assert_eq!((s.origin, s.entnum, s.channel), (None, 4, CHAN_WEAPON));
        // 100 - 20 units in 35-unit models: three, 22.5 apart.
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        let xs: Vec<f32> = frame.entities.iter().map(|e| e.origin.x).collect();
        assert_eq!(xs, [0.0, 22.5, 45.0]);
        let seg = frame.entities[0];
        assert_eq!((seg.model, seg.flags), (Some(MOD_LIGHTNING), RF_FULLBRIGHT));
        assert_eq!((seg.angles.x, seg.angles.y), (0.0, 180.0));
        // Same source and target replace it; another target adds one.
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(frame.entities.len(), 3);
        let other = lightning(4, 10, Vec3f::ZERO, end);
        fx.temp_entity(&other, 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert_eq!(frame.entities.len(), 6);
    }

    #[test]
    fn short_lightning_draws_once_at_its_target_and_hides_later_beams() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(40.0, 0.0, 0.0);
        let msg = lightning(4, 9, Vec3f::ZERO, end);
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        let parasite = beam(TE_PARASITE_ATTACK, 5, Vec3f::ZERO, end);
        fx.temp_entity(&parasite, 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        // C returns from CL_AddBeams here: the parasite beam is not drawn.
        assert_eq!(models(&frame), [Some(MOD_LIGHTNING)]);
        assert_eq!(frame.entities[0].origin, end);
        assert_eq!(frame.entities[0].angles.y, 0.0, "not reversed");
    }

    #[test]
    fn a_grapple_cable_starts_at_its_offset() {
        let mut fx = ClientFx::default();
        let msg = tent(TE_GRAPPLE_CABLE, |m| {
            m.write_short(5);
            m.write_position(Vec3f::ZERO);
            m.write_position(Vec3f::new(90.0, 0.0, 0.0));
            m.write_position(Vec3f::new(30.0, 0.0, 0.0));
        });
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        let xs: Vec<f32> = frame.entities.iter().map(|e| e.origin.x).collect();
        assert_eq!(xs, [30.0, 60.0]);
        assert_eq!(frame.entities[0].model, Some(MOD_GRAPPLE_CABLE));
    }

    /// The view of player 1 at the origin looking down +x.
    fn view(hand: i32) -> FxView {
        FxView {
            player: 1,
            vieworg: Vec3f::ZERO,
            old_gunoffset: Vec3f::ZERO,
            gunoffset: Vec3f::ZERO,
            forward: Vec3f::X,
            right: Vec3f::new(0.0, -1.0, 0.0),
            up: Vec3f::Z,
            hand,
        }
    }

    fn near(a: Vec3f, b: Vec3f) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn another_players_heatbeam_shifts_to_their_gun_and_shows_frame_2() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(64.0, 0.0, 0.0);
        let msg = beam(TE_HEATBEAM, 5, Vec3f::ZERO, end);
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame_view(1000, 1.0, &view(0), &mut rng(&[0]));
        assert_eq!(frame.entities.len(), 2);
        let seg = frame.entities[0];
        assert_eq!(seg.model, Some(MOD_HEATBEAM));
        assert_eq!((seg.flags, seg.frame), (RF_FULLBRIGHT | RF_WEAPONMODEL, 2));
        // The roll is cl.time % 360.
        assert_eq!(
            (seg.angles.x, seg.angles.y, seg.angles.z),
            (0.0, 180.0, 280.0)
        );
        // Offset (2, 7, -3) back along the reversed beam's axes.
        assert!(
            near(seg.origin, Vec3f::new(7.0, -1.0, -7.0)),
            "{:?}",
            seg.origin
        );
        assert!(near(frame.entities[1].origin, Vec3f::new(39.0, -1.0, -7.0)));
        assert!(frame.particles.is_empty());
        // A new player beam lasts 100 ms.
        assert!(fx
            .frame_view(1101, 1.0, &view(0), &mut rng(&[0]))
            .entities
            .is_empty());
    }

    #[test]
    fn a_monster_heatbeam_has_no_offset_and_a_muzzle_shell() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(64.0, 0.0, 0.0);
        let msg = beam(TE_MONSTER_HEATBEAM, 5, Vec3f::ZERO, end);
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        // Replacing the same entity's beam makes it last 200 ms.
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame_view(1200, 1.0, &view(0), &mut rng(&[32767]));
        assert_eq!(models(&frame), [Some(MOD_HEATBEAM); 2]);
        assert_eq!(frame.entities[0].origin, Vec3f::ZERO);
        assert_eq!(frame.particles.len(), 40);
    }

    #[test]
    fn the_local_heatbeam_leaves_the_gun_along_the_view_with_rings() {
        let mut fx = ClientFx::default();
        let end = Vec3f::new(107.0, -2.0, -3.0);
        let msg = beam(TE_HEATBEAM, 1, Vec3f::ZERO, end);
        fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        let frame = fx.frame_view(1000, 1.0, &view(0), &mut rng(&[0]));
        // Right 2, forward 7, up -3 from the gun.
        let seg = frame.entities[0];
        assert_eq!(seg.origin, Vec3f::new(7.0, -2.0, -3.0));
        assert_eq!(seg.frame, 1);
        assert_eq!(frame.entities.len(), 4);
        assert_eq!(frame.particles.len(), 6 * 20);
        // Left-handed mirrors the offset; centre-handed drops it and
        // lowers the beam a unit (C does it twice).
        let frame = fx.frame_view(1000, 1.0, &view(1), &mut rng(&[0]));
        assert_eq!(frame.entities[0].origin, Vec3f::new(7.0, 2.0, -3.0));
        let frame = fx.frame_view(1000, 1.0, &view(2), &mut rng(&[0]));
        assert_eq!(frame.entities[0].origin, Vec3f::new(7.0, 0.0, -5.0));
        // Without the view's axes there is nothing to aim it along.
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert!(frame.entities.is_empty());
    }

    fn steam(id: i32, count: i32, magnitude: i32, ms: i32) -> Vec<u8> {
        tent(TE_STEAM, |m| {
            m.write_short(id);
            m.write_byte(count);
            m.write_position(P);
            m.write_dir(Vec3f::Z);
            m.write_byte(0xe0);
            m.write_short(magnitude);
            if id != -1 {
                m.write_long(ms);
            }
        })
    }

    fn live_sustains(fx: &ClientFx) -> usize {
        fx.sustains.iter().filter(|s| s.id != 0).count()
    }

    #[test]
    fn a_steam_sustain_puffs_every_100_ms_until_it_ends() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        fx.temp_entity(&steam(5, 4, 60, 300), 1000, 1000, &mut r);
        assert_eq!(fx.particles.len(), 0, "a sustain puffs from the frame");
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut r);
        assert_eq!(frame.particles.len(), 4, "drawn the frame it puffs");
        fx.frame(1050, 1.0, 1, Vec3f::ZERO, &mut r);
        assert_eq!(fx.particles.len(), 4);
        fx.frame(1100, 1.0, 1, Vec3f::ZERO, &mut r);
        assert_eq!(fx.particles.len(), 8);
        fx.frame(1301, 1.0, 1, Vec3f::ZERO, &mut r);
        assert_eq!(live_sustains(&fx), 0);
        // An id of -1 puffs once, now.
        fx.clear();
        fx.temp_entity(&steam(-1, 3, 60, 0), 2000, 2000, &mut r);
        assert_eq!((fx.particles.len(), live_sustains(&fx)), (3, 0));
    }

    #[test]
    fn a_steam_time_past_the_clock_wraps_like_cs_int_add() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        fx.temp_entity(&steam(5, 4, 60, i32::MAX), 1000, 1000, &mut r);
        // 1000 + i32::MAX wraps negative: already over.
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut r);
        assert!(frame.particles.is_empty());
        assert_eq!(live_sustains(&fx), 0);
    }

    #[test]
    fn the_sustain_list_holds_32_and_drops_the_rest() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        for id in 1..=40 {
            fx.temp_entity(&steam(id, 1, 60, 1000), 1000, 1000, &mut r);
        }
        assert_eq!(live_sustains(&fx), MAX_SUSTAINS);
        fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut r);
        assert_eq!(fx.particles.len(), MAX_SUSTAINS);
    }

    #[test]
    fn widow_and_nuke_sustains_think_every_frame_for_their_time() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        let widow = tent(TE_WIDOWBEAMOUT, |m| {
            m.write_short(7);
            m.write_position(P);
        });
        fx.temp_entity(&widow, 1000, 1000, &mut r);
        let mut drawn = |fx: &mut ClientFx, time| fx.frame(time, 1.0, 1, Vec3f::ZERO, &mut r);
        assert_eq!(drawn(&mut fx, 1000).particles.len(), 300);
        assert_eq!(drawn(&mut fx, 1001).particles.len(), 300);
        assert_eq!(drawn(&mut fx, 3101).particles.len(), 0);
        fx.temp_entity(&pos_only(TE_NUKEBLAST, P), 4000, 4000, &mut rng(&[0]));
        let mut drawn = |fx: &mut ClientFx, time| fx.frame(time, 1.0, 1, Vec3f::ZERO, &mut r);
        assert_eq!(drawn(&mut fx, 4000).particles.len(), 700);
        assert_eq!(fx.sustains[0].id, 21000);
        assert_eq!(drawn(&mut fx, 5001).particles.len(), 0);
    }

    #[test]
    fn a_flashlight_queues_c_s_white_dlight_for_its_entity() {
        let mut fx = ClientFx::default();
        let flashlight = tent(TE_FLASHLIGHT, |m| {
            m.write_position(P);
            m.write_short(3);
        });
        fx.temp_entity(&flashlight, 1000, 1000, &mut rng(&[0]));
        // C CL_Flashlight: a cl_dlights slot keyed by the entity.
        assert_eq!(
            fx.take_dlights(),
            [crate::effects::ClientDLight {
                key: 3,
                color: [1.0, 1.0, 1.0],
                origin: P,
                radius: 400.0,
                die: 1100.0,
                decay: 0.0,
                minlight: 250.0,
            }]
        );
        assert!(fx.take_dlights().is_empty(), "taken once");
        let frame = fx.frame(1000, 1.0, 1, Vec3f::ZERO, &mut rng(&[0]));
        assert!(frame.lights.is_empty(), "lit through cl_dlights, not here");
    }

    #[test]
    fn a_tracker_explosion_darkens_and_bursts() {
        let mut fx = ClientFx::default();
        let msg = pos_only(TE_TRACKER_EXPLOSION, P);
        let sounds = fx.temp_entity(&msg, 1000, 1000, &mut rng(&[0]));
        assert_eq!(names(&sounds), ["weapons/disrupthit.wav"]);
        assert_eq!((fx.particles.len(), colour_of(&fx)), (128, 0));
        let lights = fx.take_dlights();
        assert_eq!((lights[0].key, lights[0].radius), (0, 150.0));
        assert_eq!(lights[0].color, [-1.0, -1.0, -1.0]);
        assert_eq!(lights[0].minlight, 250.0);
    }

    #[test]
    fn clearing_drops_sustains_player_beams_and_timed_lights() {
        let mut fx = ClientFx::default();
        let mut r = rng(&[0]);
        fx.temp_entity(&steam(5, 4, 60, 300), 1000, 1000, &mut r);
        fx.temp_entity(&pos_only(TE_TRACKER_EXPLOSION, P), 1000, 1000, &mut r);
        let end = Vec3f::new(64.0, 0.0, 0.0);
        fx.temp_entity(&beam(TE_HEATBEAM, 5, Vec3f::ZERO, end), 1000, 1000, &mut r);
        fx.clear();
        assert!(fx.take_dlights().is_empty());
        assert_eq!(
            fx.frame_view(1000, 1.0, &view(0), &mut r),
            FxFrame::default()
        );
    }

    fn trail_light(effects: u32, r: &[u32]) -> (Option<FxLight>, ClientFx) {
        let mut fx = ClientFx::default();
        let mut trail = TrailState::default();
        let light = fx.entity_trail(&mover(effects), FROM, TO, &mut trail, 0, &mut rng(r));
        (light, fx)
    }

    #[test]
    fn tracker_blasters_trail_and_light_green() {
        let green = Some(FxLight {
            origin: TO,
            intensity: 200.0,
            color: [0.0, 1.0, 0.0],
        });
        let (light, fx) = trail_light(EF_BLASTER | EF_TRACKER, &[0]);
        assert_eq!(light, green);
        assert_eq!((fx.particles.len(), colour_of(&fx)), (4, 0xd0));
        let (light, fx) = trail_light(EF_HYPERBLASTER | EF_TRACKER, &[0]);
        assert_eq!((light, fx.particles.len()), (green, 0));
    }

    #[test]
    fn ctf_flags_and_the_tag_trail_and_light_their_colours() {
        for (effects, color, light, count) in [
            (EF_FLAG1, 242, [1.0, 0.1, 0.1], 4),
            (EF_FLAG2, 115, [0.1, 0.1, 1.0], 4),
            (EF_TAGTRAIL, 220, [1.0, 1.0, 0.0], 5),
        ] {
            let (lit, fx) = trail_light(effects, &[0]);
            let lit = lit.unwrap();
            assert_eq!((lit.intensity, lit.color), (225.0, light));
            assert_eq!((fx.particles.len(), colour_of(&fx)), (count, color));
        }
    }

    #[test]
    fn trackers_trail_dark_and_their_targets_wear_a_dark_shell() {
        let (light, fx) = trail_light(EF_TRACKER, &[0]);
        let light = light.unwrap();
        assert_eq!((light.intensity, light.color), (200.0, [-1.0, -1.0, -1.0]));
        assert_eq!(fx.particles.len(), 7);
        let (light, fx) = trail_light(EF_TRACKERTRAIL, &[32767]);
        assert_eq!(light.unwrap().intensity, 155.0);
        assert_eq!(fx.particles.len(), 300);
        let p = fx.particles.active[0].org;
        assert!((p.length() - 40.0).abs() < 1e-4, "round the lerp origin");
        // With the tracker the light pulses: 50 + 500 (sin(t / 500) + 1).
        let (light, fx) = trail_light(EF_TRACKERTRAIL | EF_TRACKER, &[0]);
        assert_eq!((light.unwrap().intensity, fx.particles.len()), (550.0, 0));
    }

    #[test]
    fn ionrippers_plasma_and_blue_hyperblasters_light_up() {
        let (light, fx) = trail_light(EF_IONRIPPER, &[0]);
        let light = light.unwrap();
        assert_eq!((light.intensity, light.color), (100.0, [1.0, 0.5, 0.5]));
        assert_eq!((fx.particles.len(), colour_of(&fx)), (4, 0xe4));
        let (light, fx) = trail_light(EF_BLUEHYPERBLASTER, &[0]);
        assert_eq!(light.unwrap().color, [0.0, 0.0, 1.0]);
        assert!(fx.particles.is_empty());
        let (light, fx) = trail_light(EF_PLASMA, &[0]);
        assert_eq!(light.unwrap().intensity, 130.0);
        assert!(fx.particles.is_empty());
        let (_, fx) = trail_light(EF_PLASMA | EF_ANIM_ALLFAST, &[0]);
        assert_eq!((fx.particles.len(), colour_of(&fx)), (4, 0xe0));
    }

    #[test]
    fn a_trap_glows_32_units_up_and_trails_from_there() {
        let (light, fx) = trail_light(EF_TRAP, &[150]);
        let light = light.unwrap();
        assert_eq!(light.origin, TO + Vec3f::new(0.0, 0.0, 32.0));
        assert_eq!((light.intensity, light.color), (150.0, [1.0, 0.8, 0.1]));
        assert_eq!(fx.particles.len(), 13 + 8);
        let mut fx = ClientFx::default();
        let mut trails = EntityTrails::default();
        let trap = [EntityState {
            effects: EF_TRAP,
            ..rocket_at(7, FROM, TO)
        }];
        trails.server_frame(&trap, &[]);
        trails.run(&mut fx, &trap, 1.0, 1, 0, &mut rng(&[0]));
        assert_eq!(trails.entities[&7].2, TO + Vec3f::new(0.0, 0.0, 32.0));
    }

    #[test]
    fn the_local_player_only_lights_its_flag_and_tracker_effects() {
        let mut fx = ClientFx::default();
        let mut trails = EntityTrails::default();
        let player = [EntityState {
            effects: EF_FLAG1 | EF_ROCKET,
            modelindex: 0,
            ..rocket_at(1, FROM, TO)
        }];
        trails.server_frame(&player, &[]);
        let lights = trails.run(&mut fx, &player, 1.0, 1, 1000, &mut rng(&[0]));
        let red = FxLight {
            origin: TO,
            intensity: 225.0,
            color: [1.0, 0.1, 0.1],
        };
        assert_eq!(lights, [red]);
        assert_eq!(fx.particles.len(), 0);
        assert_eq!(
            trails.entities[&1].2, FROM,
            "C does not move its lerp origin"
        );
        assert_eq!(player_light(EF_TRACKERTRAIL, TO).unwrap().color, [-1.0; 3]);
        assert_eq!(player_light(EF_ROCKET, TO), None);
    }

    #[test]
    fn spinning_lights_shine_red_64_units_ahead() {
        let mut fx = ClientFx::default();
        let mut trails = EntityTrails::default();
        let lamp = [EntityState {
            number: 7,
            effects: EF_SPINNINGLIGHTS,
            modelindex: 0,
            angles: Vec3f::new(0.0, 90.0, 0.0),
            ..Default::default()
        }];
        trails.server_frame(&lamp, &[]);
        // At time 0 the spin is 0: the light is along the entity's yaw.
        let lights = trails.run(&mut fx, &lamp, 1.0, 1, 0, &mut rng(&[0]));
        assert_eq!(lights.len(), 1);
        assert!(near(lights[0].origin, Vec3f::new(0.0, 64.0, 0.0)));
        assert_eq!(
            (lights[0].intensity, lights[0].color),
            (100.0, [1.0, 0.0, 0.0])
        );
        let rotating = EntityState {
            effects: EF_SPINNINGLIGHTS | EF_ROTATE,
            ..lamp[0].clone()
        };
        assert_eq!(spinning_light(&rotating, Vec3f::ZERO, 0), None);
    }

    #[test]
    fn bfg_plasma_and_spheres_draw_translucent() {
        assert_eq!(effect_alpha(EF_ROCKET), None);
        assert_eq!(effect_alpha(EF_BFG), Some(0.30));
        assert_eq!(effect_alpha(EF_BFG | EF_PLASMA), Some(0.6));
        assert_eq!(effect_alpha(EF_SPHERETRANS), Some(0.3));
        assert_eq!(effect_alpha(EF_SPHERETRANS | EF_TRACKERTRAIL), Some(0.6));
    }

    #[test]
    fn a_power_screen_is_a_green_shell_on_frame_0() {
        let view = crate::entities::EntityView {
            number: 0,
            effects: 0,
            modelindex: 3,
            frame: 4,
            oldframe: 3,
            backlerp: 0.25,
            origin: P,
            oldorigin: P,
            angles: Vec3f::new(0.0, 45.0, 0.0),
            skinnum: 2,
            flags: RF_FULLBRIGHT,
            alpha: 1.0,
        };
        let screen = powerscreen_entity(&view);
        assert_eq!(screen.model, Some(MOD_POWERSCREEN));
        assert_eq!((screen.frame, screen.oldframe, screen.skinnum), (0, 0, 0));
        assert_eq!(screen.flags, RF_TRANSLUCENT | RF_SHELL_GREEN);
        assert_eq!((screen.alpha, screen.backlerp), (0.30, 0.25));
        assert_eq!((screen.origin, screen.angles), (P, view.angles));
    }

    #[test]
    fn fx_models_follow_c_registration_order() {
        assert_eq!(
            FX_MODELS,
            [
                MOD_EXPLODE,
                MOD_SMOKE,
                MOD_FLASH,
                MOD_PARASITE_SEGMENT,
                MOD_GRAPPLE_CABLE,
                MOD_PARASITE_TIP,
                MOD_EXPLO4,
                MOD_BFG_EXPLO,
                MOD_POWERSCREEN,
                MOD_EXPLO4_BIG,
                MOD_LIGHTNING,
                MOD_HEATBEAM,
                MOD_MONSTER_HEATBEAM,
            ]
        );
    }

    #[test]
    fn a_view_takes_its_axes_from_the_view_angles() {
        let view = FxView::new(
            1,
            Vec3f::ONE,
            Vec3f::new(0.0, 90.0, 0.0),
            (Vec3f::X, Vec3f::Y),
            0,
        );
        assert!(
            (view.forward - Vec3f::Y).length() < 1e-6,
            "{:?}",
            view.forward
        );
        assert!((view.right - Vec3f::X).length() < 1e-6, "{:?}", view.right);
        assert!((view.up - Vec3f::Z).length() < 1e-6, "{:?}", view.up);
        assert_eq!((view.player, view.vieworg, view.hand), (1, Vec3f::ONE, 0));
        assert_eq!((view.old_gunoffset, view.gunoffset), (Vec3f::X, Vec3f::Y));
    }
}
